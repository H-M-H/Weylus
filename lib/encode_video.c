#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#include <libavcodec/avcodec.h>
#include <libavformat/avformat.h>
#include <libavformat/avio.h>

#include <libavutil/buffer.h>
#include <libavutil/dict.h>
#include <libavutil/error.h>
#include <libavutil/frame.h>
#include <libavutil/hwcontext.h>
#include <libavutil/imgutils.h>
#include <libavutil/mem.h>
#include <libavutil/opt.h>
#include <libavutil/pixdesc.h>
#include <libavutil/pixfmt.h>

#include <libswscale/swscale.h>

#include "error.h"
#include "log.h"

#ifdef HAS_VAAPI
#include <libavutil/hwcontext_vaapi.h>
#include <va/va.h>
#endif

typedef struct VideoContext
{
	AVFormatContext* oc;
	AVCodecContext* c;
	AVFrame* frame;
	AVFrame* frame_hw;
	AVPacket* pkt;
	AVStream* st;
	AVBufferRef* hw_device_ctx;
	enum AVPixelFormat sw_pix_fmt;
	int width_out;
	int height_out;
	int width_in;
	int height_in;
	size_t buf_size;
	void* buf;
	void* rust_ctx;
	int pts;
	struct SwsContext* sws_rgb;
	struct SwsContext* sws_bgra;
	int initialized;
	int frame_allocated;
	int frame_hw_allocated;
	int using_vaapi;
	int try_vaapi;
	int try_nvenc;
} VideoContext;

// this is a rust function and lives in src/video.rs
int write_video_packet(void* rust_ctx, uint8_t* buf, int buf_size);

void set_codec_params(VideoContext* ctx)
{
	/* resolution must be a multiple of two */
	ctx->c->width = ctx->width_out;
	ctx->c->height = ctx->height_out;
	ctx->c->time_base = (AVRational){1, 1000};
	ctx->c->framerate = (AVRational){0, 1};

	ctx->c->gop_size = 12;
	// no B-frames to reduce latency
	ctx->c->max_b_frames = 0;
	if (ctx->oc->oformat->flags & AVFMT_GLOBALHEADER)
		ctx->c->flags |= AV_CODEC_FLAG_GLOBAL_HEADER;
}

void set_hwframe_ctx(VideoContext* ctx, Error* err)
{
	AVBufferRef* hw_frames_ref;
	AVHWFramesContext* frames_ctx = NULL;
	if (!(hw_frames_ref = av_hwframe_ctx_alloc(ctx->hw_device_ctx)))
		ERROR(err, 1, "Failed to create VAAPI frame context.");
	frames_ctx = (AVHWFramesContext*)(hw_frames_ref->data);
	frames_ctx->format = AV_PIX_FMT_VAAPI;
	frames_ctx->sw_format = AV_PIX_FMT_NV12;
	frames_ctx->width = ctx->width_out;
	frames_ctx->height = ctx->height_out;
	frames_ctx->initial_pool_size = 20;
	int ret;
	if ((ret = av_hwframe_ctx_init(hw_frames_ref)) < 0)
	{
		av_buffer_unref(&hw_frames_ref);
		ERROR(
			err,
			1,
			"Failed to initialize VAAPI frame context."
			"Error code: %s",
			av_err2str(ret));
	}
	ctx->c->hw_frames_ctx = av_buffer_ref(hw_frames_ref);
	if (!ctx->c->hw_frames_ctx)
		ERROR(err, 1, "Out of memory!");
	av_buffer_unref(&hw_frames_ref);
}

void set_frame_params(VideoContext* ctx)
{
	ctx->frame->format = ctx->sw_pix_fmt;
	ctx->frame->width = ctx->c->width;
	ctx->frame->height = ctx->c->height;
}

void open_video(VideoContext* ctx, Error* err)
{
	if (ctx->width_out <= 1 || ctx->height_out <= 1)
		ERROR(
			err,
			1,
			"Invalid size for video: width = %d, height = %d",
			ctx->width_out,
			ctx->height_out);

	const AVCodec* codec;
	int ret;

	avformat_alloc_output_context2(&ctx->oc, NULL, "mp4", NULL);
	if (!ctx->oc)
	{
		ERROR(err, 1, "Could not find output format mp4.");
	}

	int using_hw = 0;

#ifdef HAS_VAAPI
	char* vaapi_device = getenv("WEYLUS_VAAPI_DEVICE");

	if (ctx->try_vaapi &&
		av_hwdevice_ctx_create(
			&ctx->hw_device_ctx, AV_HWDEVICE_TYPE_VAAPI, vaapi_device, NULL, 0) == 0)
	{
		codec = avcodec_find_encoder_by_name("h264_vaapi");
		if (codec)
		{
			ctx->c = avcodec_alloc_context3(codec);
			if (ctx->c)
			{
				ctx->c->pix_fmt = AV_PIX_FMT_VAAPI;
				av_opt_set(ctx->c->priv_data, "quality", "7", 0);
				av_opt_set(ctx->c->priv_data, "qp", "23", 0);
				set_codec_params(ctx);
				Error err = {0};
				set_hwframe_ctx(ctx, &err);

				// Some drivers incorrectly report to support some pixel formats that actually are
				// not supported. Using these formats leads to a crash and that's why the following
				// workaround detects the drivers mentioned and if it finds them forces the pixel
				// format to NV12 as this seems to work so far.
				VADisplay dpy =
					((AVVAAPIDeviceContext*)((AVHWDeviceContext*)ctx->hw_device_ctx->data)->hwctx)
						->display;
				const char* vendor_string = vaQueryVendorString(dpy);
				log_debug("VA-API vendor: %s", vendor_string);
				// currently only some AMD drivers/hardware seem to be affected, this list may need
				// to be refined in the future
				const char* drivers_force_nv12[] = {"Radeon", "AMD RAVEN", NULL};
				int force_nv12 = 0;
				for (const char** pattern = drivers_force_nv12; *pattern; pattern++)
					if (strstr(vendor_string, *pattern) != NULL)
					{
						force_nv12 = 1;
						log_debug("%s is blacklisted and NV12 is forced as pixel format.");
						break;
					}

				AVHWFramesConstraints* cst =
					av_hwdevice_get_hwframe_constraints(ctx->hw_device_ctx, NULL);
				if (!force_nv12 && cst)
				{
					// If bgr0 is supported choose it as this avoids the overhead of calling
					// sws_scale otherwise choose the first supported format.
					int has_bgr0 = 0;
					for (enum AVPixelFormat* fmt = cst->valid_sw_formats; *fmt != AV_PIX_FMT_NONE;
						 ++fmt)
						if (*fmt == AV_PIX_FMT_BGR0)
						{
							has_bgr0 = 1;
							break;
						}
					ctx->sw_pix_fmt = has_bgr0 ? AV_PIX_FMT_BGR0 : cst->valid_sw_formats[0];
				}
				else
					ctx->sw_pix_fmt = AV_PIX_FMT_NV12;

				if (cst)
					av_hwframe_constraints_free(&cst);

				if (err.code == 0 && avcodec_open2(ctx->c, codec, NULL) == 0)
				{
					using_hw = 1;
					ctx->using_vaapi = 1;
				}
				else
				{
					avcodec_free_context(&ctx->c);
					av_buffer_unref(&ctx->hw_device_ctx);
				}
			}
		}
		else
			av_buffer_unref(&ctx->hw_device_ctx);
	}
#endif

#ifdef HAS_NVENC
	if (ctx->try_nvenc && !using_hw)
	{
		codec = avcodec_find_encoder_by_name("h264_nvenc");
		if (codec)
		{
			ctx->c = avcodec_alloc_context3(codec);
			if (ctx->c)
			{
				ctx->sw_pix_fmt = ctx->c->pix_fmt = AV_PIX_FMT_BGR0;
				av_opt_set(ctx->c->priv_data, "preset", "llhq", 0);
				av_opt_set(ctx->c->priv_data, "zerolatency", "1", 0);
				av_opt_set(ctx->c->priv_data, "rc", "vbr_hq", 0);
				av_opt_set(ctx->c->priv_data, "cq", "21", 0);
				set_codec_params(ctx);
				if (avcodec_open2(ctx->c, codec, NULL) == 0)
				{
					using_hw = 1;
				}
				else
					avcodec_free_context(&ctx->c);
			}
		}
	}
#endif

	if (!using_hw)
	{
		codec = avcodec_find_encoder_by_name("libx264");
		if (!codec)
		{
			ERROR(err, 1, "Codec 'libx264' not found");
		}

		ctx->c = avcodec_alloc_context3(codec);
		if (!ctx->c)
		{
			ERROR(err, 1, "Could not allocate video codec context");
		}
		ctx->sw_pix_fmt = ctx->c->pix_fmt = AV_PIX_FMT_YUV420P;
		av_opt_set(ctx->c->priv_data, "preset", "ultrafast", 0);
		av_opt_set(ctx->c->priv_data, "tune", "zerolatency", 0);
		av_opt_set(ctx->c->priv_data, "crf", "23", 0);
		set_codec_params(ctx);

		ret = avcodec_open2(ctx->c, codec, NULL);
		if (ret < 0)
		{
			ERROR(err, 1, "Could not open codec: %s", av_err2str(ret));
		}
	}

	ctx->st = avformat_new_stream(ctx->oc, NULL);
	avcodec_parameters_from_context(ctx->st->codecpar, ctx->c);

	ctx->frame = av_frame_alloc();
	if (!ctx->frame)
		ERROR(err, 1, "Could not allocate video frame");
	if (ctx->using_vaapi)
	{
		ctx->frame_hw = av_frame_alloc();
		if (!ctx->frame_hw)
			ERROR(err, 1, "Could not allocate video hardware frame");
	}
	set_frame_params(ctx);
	ctx->pkt = av_packet_alloc();
	if (!ctx->pkt)
		ERROR(err, 1, "Failed to allocate packet");

	ctx->buf_size = 1024 * 1024;
	ctx->buf = av_malloc(ctx->buf_size);
	ctx->oc->pb = avio_alloc_context(
		ctx->buf, ctx->buf_size, AVIO_FLAG_WRITE, ctx->rust_ctx, NULL, write_video_packet, NULL);
	if (!ctx->oc->pb)
		ERROR(err, 1, "Failed to allocate avio context");

	av_dump_format(ctx->oc, 0, NULL, 1);
	AVDictionary* opt = NULL;
	// enable writing fragmented mp4
	av_dict_set(&opt, "movflags", "frag_custom+empty_moov+default_base_moof", 0);
	ret = avformat_write_header(ctx->oc, &opt);
	if (ret < 0)
		log_warn("Video: failed to write header!");
	av_dict_free(&opt);

	ctx->sws_rgb = sws_getContext(
		ctx->width_in,
		ctx->height_in,
		AV_PIX_FMT_RGB24,
		ctx->width_out,
		ctx->height_out,
		ctx->sw_pix_fmt,
		SWS_FAST_BILINEAR,
		NULL,
		NULL,
		NULL);

	ctx->sws_bgra = sws_getContext(
		ctx->width_in,
		ctx->height_in,
		AV_PIX_FMT_BGRA,
		ctx->width_out,
		ctx->height_out,
		ctx->sw_pix_fmt,
		SWS_FAST_BILINEAR,
		NULL,
		NULL,
		NULL);

	ctx->initialized = 1;
	log_info(
		"Video: %dx%d@%s pix_fmt: %s",
		ctx->width_out,
		ctx->height_out,
		ctx->c->codec->name,
		av_get_pix_fmt_name(ctx->sw_pix_fmt));
}

void destroy_video_encoder(VideoContext* ctx)
{
	if (ctx->initialized)
	{
		av_write_trailer(ctx->oc);
		av_frame_free(&ctx->frame);
		if (ctx->using_vaapi)
			av_frame_free(&ctx->frame_hw);
		avio_context_free(&ctx->oc->pb);
		avformat_free_context(ctx->oc);
		avcodec_free_context(&ctx->c);
		av_packet_free(&ctx->pkt);
		av_free(ctx->buf);
		sws_freeContext(ctx->sws_rgb);
		sws_freeContext(ctx->sws_bgra);
	}
	if (ctx->using_vaapi)
		av_buffer_unref(&ctx->hw_device_ctx);
	free(ctx);
}

void encode_video_frame(VideoContext* ctx, int micros, Error* err)
{
	int ret;
	AVFrame* frame = ctx->using_vaapi ? ctx->frame_hw : ctx->frame;

	frame->pts = micros;

	ret = avcodec_send_frame(ctx->c, frame);
	if (ret < 0)
		ERROR(err, 1, "Error sending a frame for encoding");

	while (ret >= 0)
	{
		ret = avcodec_receive_packet(ctx->c, ctx->pkt);
		if (ret == AVERROR(EAGAIN) || ret == AVERROR_EOF)
			return;
		else if (ret < 0)
		{
			ERROR(err, 1, "Error during encoding");
		}

		av_packet_rescale_ts(ctx->pkt, ctx->c->time_base, ctx->st->time_base);
		av_write_frame(ctx->oc, ctx->pkt);
		av_packet_unref(ctx->pkt);

		// new fragment on every frame for lowest latency
		av_write_frame(ctx->oc, NULL);
	}
}

VideoContext* init_video_encoder(
	void* rust_ctx,
	int width_in,
	int height_in,
	int width_out,
	int height_out,
	int try_vaapi,
	int try_nvenc)
{
	VideoContext* ctx = malloc(sizeof(VideoContext));
	ctx->rust_ctx = rust_ctx;
	ctx->width_out = width_out - width_out % 2;
	ctx->height_out = height_out - height_out % 2;
	ctx->width_in = width_in;
	ctx->height_in = height_in;
	ctx->pts = 0;
	ctx->initialized = 0;
	ctx->frame_allocated = 0;
	ctx->frame_hw_allocated = 0;
	ctx->using_vaapi = 0;
	ctx->try_vaapi = try_vaapi;
	ctx->try_nvenc = try_nvenc;
	return ctx;
}

void alloc_frame_buffer(VideoContext* ctx, Error* err)
{
	int ret = av_frame_get_buffer(ctx->frame, 0);
	if (ret < 0)
		ERROR(err, 1, "Could not allocate video frame data: %s", av_err2str(ret));
	ctx->frame_allocated = 1;
}

void dealloc_frame_buffer(VideoContext* ctx)
{
	av_frame_unref(ctx->frame);
	set_frame_params(ctx);
	ctx->frame_allocated = 0;
}

void alloc_frame_buffer_hw(VideoContext* ctx, Error* err)
{
	int ret = av_hwframe_get_buffer(ctx->c->hw_frames_ctx, ctx->frame_hw, 0);
	if (ret < 0)
		ERROR(err, 1, "Could not allocate video hardware frame data: %s", av_err2str(ret));
	if (!ctx->frame_hw->hw_frames_ctx)
		ERROR(err, 2, "Could not allocate video hardware frame data");
	ctx->frame_hw_allocated = 1;
}

void fill_bgra(VideoContext* ctx, const void* data, Error* err)
{
	if (ctx->frame->format == AV_PIX_FMT_BGR0 && ctx->width_in == ctx->width_out &&
		ctx->height_in == ctx->height_out)
	{
		if (ctx->frame_allocated)
			dealloc_frame_buffer(ctx);
		ctx->frame->data[0] = (uint8_t*)data;
		ctx->frame->linesize[0] = ctx->width_in * 4;
	}
	else
	{
		const uint8_t* const* src = (const uint8_t* const*)&data;
		// 4 colors per pixel
		const int src_stride[] = {ctx->width_in * 4, 0, 0, 0};
		if (!ctx->frame_allocated)
		{
			alloc_frame_buffer(ctx, err);
			OK_OR_ABORT(err);
		}
		av_frame_make_writable(ctx->frame);
		sws_scale(
			ctx->sws_bgra,
			src,
			src_stride,
			0,
			ctx->height_in,
			ctx->frame->data,
			ctx->frame->linesize);
	}
	if (ctx->using_vaapi)
	{
		if (!ctx->frame_hw_allocated)
		{
			alloc_frame_buffer_hw(ctx, err);
			OK_OR_ABORT(err);
		}
		av_frame_make_writable(ctx->frame_hw);
		int ret = av_hwframe_transfer_data(ctx->frame_hw, ctx->frame, 0);
		if (ret < 0)
			ERROR(err, 1, "Could not upload video frame to hardware: %s", av_err2str(ret));
	}
}

void fill_rgb(VideoContext* ctx, const void* data, Error* err)
{
	const uint8_t* const* src = (const uint8_t* const*)&data;
	// 3 colors per pixel
	const int src_stride[] = {ctx->width_in * 3, 0, 0, 0};
	if (!ctx->frame_allocated)
	{
		alloc_frame_buffer(ctx, err);
		OK_OR_ABORT(err);
	}
	av_frame_make_writable(ctx->frame);
	sws_scale(
		ctx->sws_rgb, src, src_stride, 0, ctx->height_in, ctx->frame->data, ctx->frame->linesize);
	if (ctx->using_vaapi)
	{
		if (!ctx->frame_hw_allocated)
		{
			alloc_frame_buffer_hw(ctx, err);
			OK_OR_ABORT(err);
		}
		av_frame_make_writable(ctx->frame_hw);
		int ret = av_hwframe_transfer_data(ctx->frame_hw, ctx->frame, 0);
		if (ret < 0)
			ERROR(err, 1, "Could not upload video frame to hardware: %s", av_err2str(ret));
	}
}
