#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#include <libavcodec/avcodec.h>
#include <libavfilter/buffersink.h>
#include <libavfilter/buffersrc.h>
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

#include "error.h"
#include "log.h"

#ifdef HAS_VAAPI
#include <unistd.h>

#include <libavutil/hwcontext_drm.h>
#include <libavutil/hwcontext_vaapi.h>
#include <va/va.h>
#endif

const AVRational TIME_BASE = (AVRational){1, 1000};

typedef struct ScaleContext
{
	AVFilterGraph* filter_graph_scale;
	AVFilterContext* buffersink_scale_ctx;
	AVFilterContext* buffersrc_scale_ctx;
	AVFrame* frame_in;
	AVFrame* frame_out;
} ScaleContext;

typedef struct Scalers
{
	ScaleContext bgr0;
	ScaleContext rgb0;
	ScaleContext rgb;
	AVBufferRef* hw_frames_ctx;
	AVFrame* frame_out;
} Scalers;

typedef struct VideoContext
{
	AVFormatContext* oc;
	AVCodecContext* c;

	// pointer to the frame to be encoded, one of frame_out in scalers.bgr0/rgb0/rgb
	AVFrame* frame;

	Scalers scalers;

	AVBufferRef* hw_device_ctx;

	AVPacket* pkt;
	AVStream* st;
	int width_out;
	int height_out;
	int width_in;
	int height_in;
	void* buf;
	void* rust_ctx;
	int pts;
	int initialized;
	int frame_allocated;
	int try_vaapi;
	int try_nvenc;
	int try_videotoolbox;
	int try_mediafoundation;

	// Zero-copy dmabuf input (VAAPI only).
	int input_dmabuf;            // 1 when frames arrive as DRM-PRIME dmabufs
	AVBufferRef* drm_device_ctx; // DRM device for wrapping the incoming fd
	AVBufferRef* drm_frames_ctx; // DRM_PRIME frames pool (format descriptor)
	AVFilterGraph* dmabuf_graph;
	AVFilterContext* dmabuf_src;  // buffersrc (DRM_PRIME)
	AVFilterContext* dmabuf_sink; // buffersink (VAAPI NV12)
	AVFrame* dmabuf_out;          // reused output frame
} VideoContext;

// this is a rust function and lives in src/video.rs
int write_video_packet(void* rust_ctx, const uint8_t* buf, int buf_size);

#if defined(__clang__) || defined(__GNUC__)
void log_callback(__attribute__((unused)) void* _ptr, int level, const char* fmt_orig, va_list args)
#else
void log_callback(void* _ptr, int level, const char* fmt_orig, va_list args)
#endif
{
	char fmt[256] = {0};
	strncpy(fmt, fmt_orig, sizeof(fmt) - 1);
	int done = 0;
	// strip whitespaces from end
	for (int i = sizeof(fmt) - 1; i >= 0 && !done; --i)
		switch (fmt[i])
		{
		case ' ':
		case '\n':
		case '\t':
		case '\r':
			fmt[i] = '\0';
			break;
		case '\0':
			break;
		default:
			done = 1;
		}
	char buf[2048];
	vsnprintf(buf, sizeof(buf), fmt, args);
	switch (level)
	{
	case AV_LOG_FATAL:
	case AV_LOG_ERROR:
	case AV_LOG_PANIC:
		log_error("%s", buf);
		break;
	case AV_LOG_INFO:
		log_info("%s", buf);
		break;
	case AV_LOG_WARNING:
		log_warn("%s", buf);
		break;
	case AV_LOG_QUIET:
		break;
	case AV_LOG_VERBOSE:
		log_debug("%s", buf);
		break;
	case AV_LOG_DEBUG:
		log_trace("%s", buf);
		break;
	}
}

// called in src/log.rs
void init_ffmpeg_logger() { av_log_set_callback(log_callback); }

// Startup probe: 1 if a VAAPI device opens AND the h264_vaapi encoder exists,
// else 0. Gates the zero-copy dmabuf path so it never has to serve a software
// encoder. Called from src/video.rs.
int vaapi_encoder_available(void)
{
#ifdef HAS_VAAPI
	const char* dev = getenv("WEYLUS_VAAPI_DEVICE");
	AVBufferRef* d = NULL;
	if (av_hwdevice_ctx_create(&d, AV_HWDEVICE_TYPE_VAAPI, dev, NULL, 0) != 0)
		return 0;
	av_buffer_unref(&d);
	return avcodec_find_encoder_by_name("h264_vaapi") ? 1 : 0;
#else
	return 0;
#endif
}

void set_codec_params(VideoContext* ctx)
{
	/* resolution must be a multiple of two */
	ctx->c->width = ctx->width_out;
	ctx->c->height = ctx->height_out;
	ctx->c->time_base = TIME_BASE;
	ctx->c->framerate = (AVRational){0, 1};

	ctx->c->gop_size = 12;
	// no B-frames to reduce latency
	ctx->c->max_b_frames = 0;
	if (ctx->oc->oformat->flags & AVFMT_GLOBALHEADER)
		ctx->c->flags |= AV_CODEC_FLAG_GLOBAL_HEADER;
}

void destroy_scale_ctx(ScaleContext* ctx)
{
	avfilter_graph_free(&ctx->filter_graph_scale);
	if (ctx->frame_in)
		av_frame_free(&ctx->frame_in);
}

void init_scaler(
	ScaleContext* ctx,
	int width_in,
	int height_in,
	int width_out,
	int height_out,
	enum AVPixelFormat pix_fmt_in,
	enum AVPixelFormat pix_fmt_out,
	AVBufferRef* hw_device_ctx,
	enum AVPixelFormat pix_fmt_sw_out,
	AVFrame* frame_out,
	Error* err)
{
	int ret = 0;

	ctx->frame_in = av_frame_alloc();
	if (!ctx->frame_in)
		ERROR(err, 1, "Failed to allocate frame_in for scale filter!");

	ctx->frame_out = frame_out;

	ctx->frame_in->format = pix_fmt_in;
	ctx->frame_in->width = width_in;
	ctx->frame_in->height = height_in;
	ret = av_frame_get_buffer(ctx->frame_in, 0);
	if (ret)
	{
		destroy_scale_ctx(ctx);
		ERROR(
			err,
			1,
			"Failed to allocate buffer for frame_in for scale filter: %s!",
			av_err2str(ret));
	}

	char args[512];
	const AVFilter* buffersrc = avfilter_get_by_name("buffer");
	const AVFilter* buffersink = avfilter_get_by_name("buffersink");
	AVFilterInOut* outputs = avfilter_inout_alloc();
	AVFilterInOut* inputs = avfilter_inout_alloc();

	ctx->filter_graph_scale = avfilter_graph_alloc();
	if (!outputs || !inputs || !ctx->filter_graph_scale)
	{
		ret = AVERROR(ENOMEM);
		goto end;
	}

	avfilter_graph_set_auto_convert(ctx->filter_graph_scale, AVFILTER_AUTO_CONVERT_NONE);

	/* buffer video source: the decoded frames from the decoder will be inserted here. */
	snprintf(
		args,
		sizeof(args),
		"video_size=%dx%d:pix_fmt=%d:time_base=%d/%d:pixel_aspect=%d/%d",
		width_in,
		height_in,
		pix_fmt_in,
		TIME_BASE.num,
		TIME_BASE.den,
		1,
		1);

	ret = avfilter_graph_create_filter(
		&ctx->buffersrc_scale_ctx, buffersrc, "in", args, NULL, ctx->filter_graph_scale);
	if (ret < 0)
	{
		log_warn("Cannot create buffer source");
		goto end;
	}

	/* buffer video sink: to terminate the filter chain. */
	ctx->buffersink_scale_ctx =
		avfilter_graph_alloc_filter(ctx->filter_graph_scale, buffersink, "out");

	if (ctx->buffersink_scale_ctx == NULL)
	{
		log_warn("Cannot allocate buffer sink");
		goto end;
	}

	ret = av_opt_set_array(
		ctx->buffersink_scale_ctx,
		"pixel_formats",
		AV_OPT_SEARCH_CHILDREN,
		0,
		1,
		AV_OPT_TYPE_PIXEL_FMT,
		&pix_fmt_out);
	if (ret < 0)
	{
		log_warn("Cannot set output pixel format: %s", av_err2str(ret));
		goto end;
	}

	ret = avfilter_init_dict(ctx->buffersink_scale_ctx, NULL);
	if (ret < 0)
	{
		log_warn("Cannot init buffer sink");
		goto end;
	}

	outputs->name = av_strdup("in");
	outputs->filter_ctx = ctx->buffersrc_scale_ctx;
	outputs->pad_idx = 0;
	outputs->next = NULL;

	inputs->name = av_strdup("out");
	inputs->filter_ctx = ctx->buffersink_scale_ctx;
	inputs->pad_idx = 0;
	inputs->next = NULL;

	int hwupload_created = 0;
	AVFilterContext* hwupload_ctx = NULL;

	switch (pix_fmt_out)
	{
	case AV_PIX_FMT_CUDA:
		if (pix_fmt_in == AV_PIX_FMT_RGB24)
		{
			snprintf(
				args,
				sizeof(args),
				"scale=w=%d:h=%d:flags=fast_bilinear,hwupload_cuda",
				width_out,
				height_out);
		}
		else
		{
			snprintf(
				args,
				sizeof(args),
#ifdef HAS_LIBNPP
				"scale,format=nv12,hwupload_cuda,scale_npp=w=%d:h=%d:format=%s:interp_algo=nn",
#else
				"hwupload_cuda,scale_cuda=w=%d:h=%d:format=%s:interp_algo=nearest",
#endif
				width_out,
				height_out,
				av_get_pix_fmt_name(pix_fmt_sw_out));
		}
		break;
	case AV_PIX_FMT_VAAPI:
	{
		// Create hwupload filter manually so hw_device_ctx is set
		// before the filter's init() is called.
		const AVFilter* hwupload_filter = avfilter_get_by_name("hwupload");
		if (!hwupload_filter)
		{
			ret = AVERROR(ENOSYS);
			log_warn("hwupload filter not found");
			goto end;
		}
		hwupload_ctx = avfilter_graph_alloc_filter(
			ctx->filter_graph_scale, hwupload_filter, "hwupload");
		if (!hwupload_ctx)
		{
			ret = AVERROR(ENOMEM);
			log_warn("Cannot allocate hwupload filter");
			goto end;
		}
		hwupload_ctx->hw_device_ctx = av_buffer_ref(hw_device_ctx);
		ret = avfilter_init_dict(hwupload_ctx, NULL);
		if (ret < 0)
		{
			log_warn("Cannot init hwupload filter: %s", av_err2str(ret));
			goto end;
		}
		hwupload_created = 1;

		if (pix_fmt_in == AV_PIX_FMT_RGB24)
		{
			// Chain: buffersrc -> scale -> hwupload -> buffersink
			// Parsed "scale" connects to hwupload; we link
			// hwupload -> buffersink manually below.
			inputs->filter_ctx = hwupload_ctx;
			snprintf(
				args,
				sizeof(args),
				"scale=w=%d:h=%d:flags=fast_bilinear",
				width_out,
				height_out);
		}
		else
		{
			// Chain: buffersrc -> hwupload -> scale_vaapi -> buffersink
			// buffersrc -> hwupload is linked manually below.
			outputs->filter_ctx = hwupload_ctx;
			snprintf(
				args,
				sizeof(args),
				"scale_vaapi=w=%d:h=%d:format=%s:mode=fast",
				width_out,
				height_out,
				av_get_pix_fmt_name(pix_fmt_sw_out));
		}
		break;
	}
	default:
		snprintf(args, sizeof(args), "scale=w=%d:h=%d:flags=fast_bilinear", width_out, height_out);
	}

	if ((ret = avfilter_graph_parse_ptr(ctx->filter_graph_scale, args, &inputs, &outputs, NULL)) <
		0)
	{
		log_warn("Failed to parse filter");
		goto end;
	}

	// For VAAPI with the hwupload filter created manually, complete the links.
	if (hwupload_created)
	{
		if (pix_fmt_in == AV_PIX_FMT_RGB24)
		{
			// buffersink receives from hwupload instead of directly
			// from the parsed chain.
			ret = avfilter_link(hwupload_ctx, 0, ctx->buffersink_scale_ctx, 0);
			if (ret < 0)
			{
				log_warn("Cannot link hwupload to buffersink");
				goto end;
			}
		}
		else
		{
			// buffersrc feeds into hwupload.
			ret = avfilter_link(
				ctx->buffersrc_scale_ctx, 0, hwupload_ctx, 0);
			if (ret < 0)
			{
				log_warn("Cannot link buffersrc to hwupload");
				goto end;
			}
		}
	}

	if ((ret = avfilter_graph_config(ctx->filter_graph_scale, NULL)) < 0)
	{
		log_warn("Failed to configure filter graph");
		goto end;
	}

end:
	avfilter_inout_free(&inputs);
	avfilter_inout_free(&outputs);

	if (ret != 0)
	{
		destroy_scale_ctx(ctx);
		ERROR(
			err,
			1,
			"Setting up scale filter %s -> %s (sw: %s) failed!",
			av_get_pix_fmt_name(pix_fmt_in),
			av_get_pix_fmt_name(pix_fmt_out),
			av_get_pix_fmt_name(pix_fmt_sw_out));
	}
	else
	{
		log_debug(
			"Scale filter set %s -> %s (sw: %s) up!",
			av_get_pix_fmt_name(pix_fmt_in),
			av_get_pix_fmt_name(pix_fmt_out),
			av_get_pix_fmt_name(pix_fmt_sw_out));
	}
}

void destroy_scalers(Scalers* s)
{
	destroy_scale_ctx(&s->bgr0);
	destroy_scale_ctx(&s->rgb0);
	destroy_scale_ctx(&s->rgb);
	if (s->frame_out)
		av_frame_free(&s->frame_out);
}

void init_scalers(
	Scalers* ctx,
	int width_in,
	int height_in,
	int width_out,
	int height_out,
	enum AVPixelFormat pix_fmt_out,
	enum AVPixelFormat pix_fmt_sw_out,
	AVBufferRef* hw_device_ctx,
	Error* err)
{
	int ret;
	ctx->frame_out = av_frame_alloc();
	if (!ctx->frame_out)
	{
		destroy_scalers(ctx);
		ERROR(err, 1, "Failed to allocate frame_out for scale filter!");
	}

	if (hw_device_ctx != NULL)
	{

		AVBufferRef* hw_frames_ref;
		AVHWFramesContext* frames_ctx = NULL;
		if (!(hw_frames_ref = av_hwframe_ctx_alloc(hw_device_ctx)))
		{
			destroy_scalers(ctx);
			ERROR(err, 1, "Failed to create HW frame context.");
		}
		frames_ctx = (AVHWFramesContext*)(hw_frames_ref->data);
		frames_ctx->format = pix_fmt_out;
		frames_ctx->sw_format = pix_fmt_sw_out;
		frames_ctx->width = width_out;
		frames_ctx->height = height_out;
		frames_ctx->initial_pool_size = 20;
		if ((ret = av_hwframe_ctx_init(hw_frames_ref)) < 0)
		{
			av_buffer_unref(&hw_frames_ref);
			destroy_scalers(ctx);
			ERROR(
				err,
				1,
				"Failed to initialize HW frame context."
				"Error code: %s",
				av_err2str(ret));
		}

		ctx->hw_frames_ctx = av_buffer_ref(hw_frames_ref);
		ret = av_hwframe_get_buffer(ctx->hw_frames_ctx, ctx->frame_out, 0);
		if (ret < 0)
		{
			av_buffer_unref(&hw_frames_ref);
			destroy_scalers(ctx);
			ERROR(
				err,
				1,
				"Could not allocate video hardware frame data for scaling: %s",
				av_err2str(ret));
		}
		av_buffer_unref(&hw_frames_ref);
	}

	enum AVPixelFormat pix_fmts[] = {AV_PIX_FMT_BGR0, AV_PIX_FMT_RGB0, AV_PIX_FMT_RGB24};
	ScaleContext* scalers[] = {&ctx->bgr0, &ctx->rgb0, &ctx->rgb};
	for (int i = 0; i < 3; i++)
	{
		init_scaler(
			scalers[i],
			width_in,
			height_in,
			width_out,
			height_out,
			pix_fmts[i],
			pix_fmt_out,
			hw_device_ctx,
			pix_fmt_sw_out,
			ctx->frame_out,
			err);
		OK_OR_ABORT(err);
	}
}

void scale_frame(ScaleContext* ctx, Error* err)
{
	int ret;
	if ((ret = av_buffersrc_add_frame_flags(
				   ctx->buffersrc_scale_ctx, ctx->frame_in, AV_BUFFERSRC_FLAG_KEEP_REF) < 0))
	{
		ERROR(err, ret, "Error adding frame to buffer source: %s.", av_err2str(ret));
	}

	av_frame_unref(ctx->frame_out);

	while (1)
	{
		int ret = av_buffersink_get_frame(ctx->buffersink_scale_ctx, ctx->frame_out);
		if (ret == AVERROR(EAGAIN) || ret == AVERROR_EOF)
			break;
		if (ret < 0)
		{
			ERROR(err, ret, "Error reading frame from buffer sink: %s.", av_err2str(ret));
		}
	}
}

#ifdef HAS_VAAPI
// Build the zero-copy import graph:
//   buffer(DRM_PRIME) -> hwmap=derive_device=vaapi -> scale_vaapi=nv12 -> buffersink
// Proven end-to-end in lib/vaapi_import_probe.c on branch debug/vaapi-zerocopy-probe.
// The graph is configured once here; fill_dmabuf() pushes each frame through it.
static int build_dmabuf_graph(VideoContext* ctx)
{
	int ret;
	// ffmpeg's DRM backend does open(device), so device MUST be a real node path
	// (passing NULL yields open(NULL) -> EFAULT).
	const char* drm_node = getenv("WEYLUS_VAAPI_DRM_NODE");
	if (!drm_node || !drm_node[0])
		drm_node = "/dev/dri/renderD128";
	if ((ret = av_hwdevice_ctx_create(
			 &ctx->drm_device_ctx, AV_HWDEVICE_TYPE_DRM, drm_node, NULL, 0)) < 0)
		return ret;

	ctx->drm_frames_ctx = av_hwframe_ctx_alloc(ctx->drm_device_ctx);
	if (!ctx->drm_frames_ctx)
		return AVERROR(ENOMEM);
	AVHWFramesContext* fc = (AVHWFramesContext*)ctx->drm_frames_ctx->data;
	fc->format = AV_PIX_FMT_DRM_PRIME;
	fc->sw_format = AV_PIX_FMT_BGR0; // XR24 == XRGB8888 little-endian == BGR0
	fc->width = ctx->width_in;
	fc->height = ctx->height_in;
	if ((ret = av_hwframe_ctx_init(ctx->drm_frames_ctx)) < 0)
		return ret;

	ctx->dmabuf_graph = avfilter_graph_alloc();
	if (!ctx->dmabuf_graph)
		return AVERROR(ENOMEM);

	// A hardware pix_fmt cannot be passed via the args string; alloc the filter,
	// set parameters (incl. hw_frames_ctx), then init with NULL.
	ctx->dmabuf_src =
		avfilter_graph_alloc_filter(ctx->dmabuf_graph, avfilter_get_by_name("buffer"), "in");
	if (!ctx->dmabuf_src)
		return AVERROR(ENOMEM);
	AVBufferSrcParameters* p = av_buffersrc_parameters_alloc();
	p->format = AV_PIX_FMT_DRM_PRIME;
	p->width = ctx->width_in;
	p->height = ctx->height_in;
	p->time_base = TIME_BASE;
	p->frame_rate = (AVRational){30, 1};
	p->hw_frames_ctx = av_buffer_ref(ctx->drm_frames_ctx);
	ret = av_buffersrc_parameters_set(ctx->dmabuf_src, p);
	av_free(p);
	if (ret < 0)
		return ret;
	if ((ret = avfilter_init_str(ctx->dmabuf_src, NULL)) < 0)
		return ret;

	AVFilterContext* map_ctx;
	if ((ret = avfilter_graph_create_filter(
			 &map_ctx,
			 avfilter_get_by_name("hwmap"),
			 "map",
			 "derive_device=vaapi:mode=read",
			 NULL,
			 ctx->dmabuf_graph)) < 0)
		return ret;

	// scale_vaapi also clamps to the VAAPI H.264 4096x4096 limit via width_out/height_out.
	char scale_args[128];
	snprintf(
		scale_args, sizeof(scale_args), "w=%d:h=%d:format=nv12", ctx->width_out, ctx->height_out);
	AVFilterContext* scale_ctx;
	if ((ret = avfilter_graph_create_filter(
			 &scale_ctx,
			 avfilter_get_by_name("scale_vaapi"),
			 "scale",
			 scale_args,
			 NULL,
			 ctx->dmabuf_graph)) < 0)
		return ret;

	ctx->dmabuf_sink =
		avfilter_graph_alloc_filter(ctx->dmabuf_graph, avfilter_get_by_name("buffersink"), "out");
	if (!ctx->dmabuf_sink)
		return AVERROR(ENOMEM);
	if ((ret = avfilter_init_str(ctx->dmabuf_sink, NULL)) < 0)
		return ret;

	if ((ret = avfilter_link(ctx->dmabuf_src, 0, map_ctx, 0)) < 0)
		return ret;
	if ((ret = avfilter_link(map_ctx, 0, scale_ctx, 0)) < 0)
		return ret;
	if ((ret = avfilter_link(scale_ctx, 0, ctx->dmabuf_sink, 0)) < 0)
		return ret;
	if ((ret = avfilter_graph_config(ctx->dmabuf_graph, NULL)) < 0)
		return ret;

	ctx->dmabuf_out = av_frame_alloc();
	if (!ctx->dmabuf_out)
		return AVERROR(ENOMEM);
	return 0;
}
#endif

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
	// Zero-copy path: frames arrive as DRM-PRIME dmabufs. Import them straight into
	// VAAPI (hwmap) and open h264_vaapi from the graph's sink hw_frames_ctx — no
	// GPU->CPU->GPU round trip, no init_scalers.
	if (ctx->try_vaapi && ctx->input_dmabuf)
	{
		ret = build_dmabuf_graph(ctx);
		if (ret < 0)
			ERROR(err, 1, "dmabuf VAAPI graph setup failed: %s", av_err2str(ret));
		AVBufferRef* enc_frames = av_buffersink_get_hw_frames_ctx(ctx->dmabuf_sink);
		if (!enc_frames)
			ERROR(err, 1, "dmabuf graph produced no hw_frames_ctx");
		codec = avcodec_find_encoder_by_name("h264_vaapi");
		if (!codec)
			ERROR(err, 1, "no h264_vaapi encoder");
		ctx->c = avcodec_alloc_context3(codec);
		if (!ctx->c)
			ERROR(err, 1, "Could not allocate h264_vaapi context");
		ctx->c->pix_fmt = AV_PIX_FMT_VAAPI;
		ctx->c->hw_frames_ctx = av_buffer_ref(enc_frames);
		av_opt_set(ctx->c->priv_data, "quality", "7", 0);
		av_opt_set(ctx->c->priv_data, "qp", "23", 0);
		set_codec_params(ctx);
		if ((ret = avcodec_open2(ctx->c, codec, NULL)) < 0)
			ERROR(err, 1, "open h264_vaapi (dmabuf): %s", av_err2str(ret));
		using_hw = 1;
	}

	char* vaapi_device = getenv("WEYLUS_VAAPI_DEVICE");

	if (ctx->try_vaapi && !ctx->input_dmabuf && !using_hw &&
		av_hwdevice_ctx_create(
			&ctx->hw_device_ctx, AV_HWDEVICE_TYPE_VAAPI, vaapi_device, NULL, 0) == 0)
	{

		if (ctx->hw_device_ctx)
		{
			AVHWFramesConstraints* cst =
				av_hwdevice_get_hwframe_constraints(ctx->hw_device_ctx, NULL);
			if (cst)
			{
				for (enum AVPixelFormat* fmt = cst->valid_sw_formats; *fmt != AV_PIX_FMT_NONE;
					 ++fmt)
				{
					log_debug("VAAPI: valid pix_fmt: %s", av_get_pix_fmt_name(*fmt));
				}
				av_hwframe_constraints_free(&cst);
			}
		}

		codec = avcodec_find_encoder_by_name("h264_vaapi");
		if (codec)
		{
			ctx->c = avcodec_alloc_context3(codec);
			if (ctx->c)
			{
				Error err = {0};
				init_scalers(
					&ctx->scalers,
					ctx->width_in,
					ctx->height_in,
					ctx->width_out,
					ctx->height_out,
					AV_PIX_FMT_VAAPI,
					AV_PIX_FMT_NV12,
					ctx->hw_device_ctx,
					&err);
				if (err.code)
				{
					log_warn("Failed to initialize scaler: %s", err.error_str);
					avcodec_free_context(&ctx->c);
				}
				else
				{
					ctx->c->pix_fmt = AV_PIX_FMT_VAAPI;
					ctx->c->hw_frames_ctx = ctx->scalers.hw_frames_ctx;
					av_opt_set(ctx->c->priv_data, "quality", "7", 0);
					av_opt_set(ctx->c->priv_data, "qp", "23", 0);
					set_codec_params(ctx);

					if ((ret = avcodec_open2(ctx->c, codec, NULL) == 0))
						using_hw = 1;
					else
					{
						log_debug("Could not open codec: %s!", av_err2str(ret));
						avcodec_free_context(&ctx->c);
						av_buffer_unref(&ctx->hw_device_ctx);
						destroy_scalers(&ctx->scalers);
					}
				}
			}
		}
		else
			av_buffer_unref(&ctx->hw_device_ctx);
	}
#endif

#ifdef HAS_MEDIAFOUNDATION
	if (ctx->try_mediafoundation && !using_hw)
	{
		codec = avcodec_find_encoder_by_name("h264_mf");
		if (codec)
		{
			ctx->c = avcodec_alloc_context3(codec);
			if (ctx->c)
			{
				Error err = {0};
				init_scalers(
					&ctx->scalers,
					ctx->width_in,
					ctx->height_in,
					ctx->width_out,
					ctx->height_out,
					AV_PIX_FMT_NV12,
					AV_PIX_FMT_NV12,
					NULL,
					&err);
				if (err.code)
				{
					log_warn("Failed to initialize scaler: %s", err.error_str);
					avcodec_free_context(&ctx->c);
				}
				else
				{
					ctx->c->pix_fmt = AV_PIX_FMT_NV12;
					av_opt_set(ctx->c->priv_data, "rate_control", "ld_vbr", 0);
					av_opt_set(ctx->c->priv_data, "scenario", "display_remoting", 0);
					av_opt_set(ctx->c->priv_data, "quality", "100", 0);
					set_codec_params(ctx);
					int ret = avcodec_open2(ctx->c, codec, NULL);
					if (ret == 0)
						using_hw = 1;
					else
					{
						log_debug("Could not open codec: %s!", av_err2str(ret));
						avcodec_free_context(&ctx->c);
						destroy_scalers(&ctx->scalers);
					}
				}
			}
			else
				log_debug("Could not allocate video codec context for 'h264_mf'!");
		}
		else
			log_debug("Codec 'h264_mf' not found!");
	}
#endif

#ifdef HAS_NVENC
	if (ctx->try_nvenc && !using_hw &&
		av_hwdevice_ctx_create(&ctx->hw_device_ctx, AV_HWDEVICE_TYPE_CUDA, NULL, NULL, 0) == 0)
	{
		codec = avcodec_find_encoder_by_name("h264_nvenc");
		if (codec)
		{
			ctx->c = avcodec_alloc_context3(codec);
			if (ctx->c)
			{
				Error err = {0};
				init_scalers(
					&ctx->scalers,
					ctx->width_in,
					ctx->height_in,
					ctx->width_out,
					ctx->height_out,
					AV_PIX_FMT_CUDA,
#ifdef HAS_LIBNPP
					AV_PIX_FMT_NV12,
#else
					AV_PIX_FMT_BGR0,
#endif
					ctx->hw_device_ctx,
					&err);
				if (err.code)
				{
					log_warn("Failed to initialize scaler: %s", err.error_str);
					avcodec_free_context(&ctx->c);
				}
				else
				{
					ctx->c->pix_fmt = AV_PIX_FMT_CUDA;
					ctx->c->hw_frames_ctx = ctx->scalers.hw_frames_ctx;
					av_opt_set(ctx->c->priv_data, "preset", "p1", 0);
					av_opt_set(ctx->c->priv_data, "zerolatency", "1", 0);
					av_opt_set(ctx->c->priv_data, "tune", "ull", 0);
					av_opt_set(ctx->c->priv_data, "rc", "cbr", 0);
					av_opt_set(ctx->c->priv_data, "cq", "21", 0);
					av_opt_set(ctx->c->priv_data, "delay", "0", 0);
					set_codec_params(ctx);

					int ret = avcodec_open2(ctx->c, codec, NULL);
					if (ret == 0)
						using_hw = 1;
					else
					{
						log_debug("Could not open codec: %s!", av_err2str(ret));
						avcodec_free_context(&ctx->c);
						destroy_scalers(&ctx->scalers);
					}
				}
			}
			else
				log_debug("Could not allocate video codec context for 'h264_nvenc'!");
		}
		else
			log_debug("Codec 'h264_nvenc' not found!");
	}
#endif

#ifdef HAS_VIDEOTOOLBOX
	if (ctx->try_videotoolbox && !using_hw)
	{
		codec = avcodec_find_encoder_by_name("h264_videotoolbox");
		if (codec)
		{
			ctx->c = avcodec_alloc_context3(codec);
			if (ctx->c)
			{
				Error err = {0};
				init_scalers(
					&ctx->scalers,
					ctx->width_in,
					ctx->height_in,
					ctx->width_out,
					ctx->height_out,
					AV_PIX_FMT_YUV420P,
					AV_PIX_FMT_YUV420P,
					ctx->hw_device_ctx,
					&err);
				if (err.code)
				{
					log_warn("Failed to initialize scaler: %s", err.error_str);
					avcodec_free_context(&ctx->c);
				}
				else
				{
					ctx->c->pix_fmt = AV_PIX_FMT_YUV420P;
					av_opt_set(ctx->c->priv_data, "realtime", "true", 0);
					av_opt_set(ctx->c->priv_data, "allow_sw", "true", 0);
					av_opt_set(ctx->c->priv_data, "profile", "extended", 0);
					av_opt_set(ctx->c->priv_data, "level", "5.2", 0);
					set_codec_params(ctx);
					if (avcodec_open2(ctx->c, codec, NULL) == 0)
						using_hw = 1;
					else
					{
						log_debug("Could not open codec: %s!", av_err2str(ret));
						avcodec_free_context(&ctx->c);
						destroy_scalers(&ctx->scalers);
					}
				}
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

		init_scalers(
			&ctx->scalers,
			ctx->width_in,
			ctx->height_in,
			ctx->width_out,
			ctx->height_out,
			AV_PIX_FMT_YUV420P,
			AV_PIX_FMT_YUV420P,
			NULL,
			err);
		if (err->code)
		{
			avcodec_free_context(&ctx->c);
			return;
		}

		ctx->c->pix_fmt = AV_PIX_FMT_YUV420P;
		av_opt_set(ctx->c->priv_data, "preset", "ultrafast", 0);
		av_opt_set(ctx->c->priv_data, "tune", "zerolatency", 0);
		av_opt_set(ctx->c->priv_data, "crf", "23", 0);
		set_codec_params(ctx);

		ret = avcodec_open2(ctx->c, codec, NULL);
		if (ret < 0)
		{
			avcodec_free_context(&ctx->c);
			ERROR(err, 1, "Could not open codec: %s", av_err2str(ret));
		}
	}

	ctx->st = avformat_new_stream(ctx->oc, NULL);
	avcodec_parameters_from_context(ctx->st->codecpar, ctx->c);

	ctx->pkt = av_packet_alloc();
	if (!ctx->pkt)
		ERROR(err, 1, "Failed to allocate packet");

	int buf_size = 1024 * 1024;
	ctx->buf = av_malloc(buf_size);
	ctx->oc->pb = avio_alloc_context(
		ctx->buf, buf_size, AVIO_FLAG_WRITE, ctx->rust_ctx, NULL, write_video_packet, NULL);
	if (!ctx->oc->pb)
		ERROR(err, 1, "Failed to allocate avio context");

	AVDictionary* opt = NULL;

	// enable writing fragmented mp4
	av_dict_set(&opt, "movflags", "frag_custom+empty_moov+default_base_moof", 0);
	ret = avformat_write_header(ctx->oc, &opt);
	if (ret < 0)
		log_warn("Video: failed to write header!");
	av_dict_free(&opt);

	if (av_pix_fmt_desc_get(ctx->c->pix_fmt)->flags & AV_PIX_FMT_FLAG_HWACCEL &&
		ctx->c->hw_frames_ctx)
	{
		const char* pix_fmt_sw =
			av_get_pix_fmt_name(((AVHWFramesContext*)ctx->c->hw_frames_ctx->data)->sw_format);
		log_info(
			"Video: %dx%d@%s pix_fmt: %s (%s)",
			ctx->width_out,
			ctx->height_out,
			ctx->c->codec->name,
			av_get_pix_fmt_name(ctx->c->pix_fmt),
			pix_fmt_sw);
	}
	else
		log_info(
			"Video: %dx%d@%s pix_fmt: %s",
			ctx->width_out,
			ctx->height_out,
			ctx->c->codec->name,
			av_get_pix_fmt_name(ctx->c->pix_fmt));

	ctx->initialized = 1;
}

void destroy_video_encoder(VideoContext* ctx)
{
	if (ctx->initialized)
	{
		av_write_trailer(ctx->oc);
		avio_context_free(&ctx->oc->pb);
		avformat_free_context(ctx->oc);
		avcodec_free_context(&ctx->c);
		av_packet_free(&ctx->pkt);
		av_free(ctx->buf);
		destroy_scalers(&ctx->scalers);
	}
	if (ctx->hw_device_ctx)
		av_buffer_unref(&ctx->hw_device_ctx);
	if (ctx->dmabuf_out)
		av_frame_free(&ctx->dmabuf_out);
	if (ctx->dmabuf_graph)
		avfilter_graph_free(&ctx->dmabuf_graph);
	if (ctx->drm_frames_ctx)
		av_buffer_unref(&ctx->drm_frames_ctx);
	if (ctx->drm_device_ctx)
		av_buffer_unref(&ctx->drm_device_ctx);
	free(ctx);
}

void encode_video_frame(VideoContext* ctx, int millis, Error* err)
{
	int ret;
	AVFrame* frame = ctx->frame;
	if (!frame)
		ERROR(err, 1, "Frame not initialized!");

	frame->pts = millis;

	ret = avcodec_send_frame(ctx->c, frame);
	if (ret < 0)
		ERROR(err, 1, "Error sending a frame for encoding: %s", av_err2str(ret));

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
	int try_nvenc,
	int try_videotoolbox,
	int try_mediafoundation,
	int input_dmabuf)
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
	ctx->try_vaapi = try_vaapi;
	ctx->try_nvenc = try_nvenc;
	ctx->try_videotoolbox = try_videotoolbox;
	ctx->try_mediafoundation = try_mediafoundation;
	ctx->hw_device_ctx = NULL;
	ctx->input_dmabuf = input_dmabuf;
	ctx->drm_device_ctx = NULL;
	ctx->drm_frames_ctx = NULL;
	ctx->dmabuf_graph = NULL;
	ctx->dmabuf_src = NULL;
	ctx->dmabuf_sink = NULL;
	ctx->dmabuf_out = NULL;

	// make sure all scalers are zero initialized so that destroy can always be called
	memset(&ctx->scalers, 0, sizeof(Scalers));
	return ctx;
}

void fill_bgr0(VideoContext* ctx, const void* data, int stride, Error* err)
{
	ctx->frame = NULL;
	ScaleContext* scaler = &ctx->scalers.bgr0;
	scaler->frame_in->data[0] = (uint8_t*)data;
	scaler->frame_in->linesize[0] = stride;

	scale_frame(scaler, err);
	OK_OR_ABORT(err)
	ctx->frame = scaler->frame_out;
}

void fill_rgb(VideoContext* ctx, const void* data, Error* err)
{
	ctx->frame = NULL;
	ScaleContext* scaler = &ctx->scalers.rgb;
	ctx->frame = NULL;
	scaler->frame_in->data[0] = (uint8_t*)data;
	scaler->frame_in->linesize[0] = ctx->width_in * 3;

	scale_frame(scaler, err);
	OK_OR_ABORT(err)
	ctx->frame = scaler->frame_out;
}

void fill_rgb0(VideoContext* ctx, const void* data, Error* err)
{
	ctx->frame = NULL;
	ScaleContext* scaler = &ctx->scalers.rgb0;
	scaler->frame_in->data[0] = (uint8_t*)data;
	scaler->frame_in->linesize[0] = ctx->width_in * 4;

	scale_frame(scaler, err);
	OK_OR_ABORT(err)
	ctx->frame = scaler->frame_out;
}

// Zero-copy input: wrap the incoming dmabuf fd as a single-plane DRM-PRIME frame,
// push it through the import graph, and hand the mapped VAAPI NV12 frame to the
// encoder via ctx->frame. Adapted from lib/vaapi_import_probe.c.
#ifdef HAS_VAAPI
// AVBufferRef free callback: frees the AVDRMFrameDescriptor backing the frame.
// A dedicated wrapper (rather than casting av_free) keeps the signature exact
// and avoids -Wcast-function-type.
static void free_drm_descriptor(void* opaque, uint8_t* data)
{
	(void)opaque;
	av_free(data);
}

void fill_dmabuf(VideoContext* ctx, int fd, unsigned int fourcc,
	unsigned long long modifier, unsigned int stride, unsigned int offset, Error* err)
{
	ctx->frame = NULL;
	// True dmabuf size: the compositor may report a bogus GstMemory size, so measure the fd.
	off_t sz = lseek(fd, 0, SEEK_END);
	if (sz <= 0)
		sz = (off_t)stride * ctx->height_in;

	AVFrame* drm = av_frame_alloc();
	if (!drm)
		ERROR(err, 1, "dmabuf: av_frame_alloc failed");
	drm->format = AV_PIX_FMT_DRM_PRIME;
	drm->width = ctx->width_in;
	drm->height = ctx->height_in;
	AVDRMFrameDescriptor* d = av_mallocz(sizeof(*d));
	d->nb_objects = 1;
	d->objects[0].fd = fd;
	d->objects[0].size = sz;
	d->objects[0].format_modifier = modifier;
	d->nb_layers = 1;
	d->layers[0].format = fourcc;
	d->layers[0].nb_planes = 1;
	d->layers[0].planes[0].object_index = 0;
	d->layers[0].planes[0].offset = offset;
	d->layers[0].planes[0].pitch = stride;
	drm->data[0] = (uint8_t*)d;
	drm->buf[0] = av_buffer_create((uint8_t*)d, sizeof(*d), free_drm_descriptor, NULL, 0);
	drm->hw_frames_ctx = av_buffer_ref(ctx->drm_frames_ctx);

	int ret = av_buffersrc_add_frame(ctx->dmabuf_src, drm);
	av_frame_free(&drm);
	if (ret < 0)
		ERROR(err, 1, "dmabuf buffersrc_add_frame: %s", av_err2str(ret));

	av_frame_unref(ctx->dmabuf_out);
	ret = av_buffersink_get_frame(ctx->dmabuf_sink, ctx->dmabuf_out);
	if (ret < 0)
		ERROR(err, 1, "dmabuf buffersink_get_frame: %s", av_err2str(ret));
	ctx->frame = ctx->dmabuf_out;
}
#else
void fill_dmabuf(VideoContext* ctx, int fd, unsigned int fourcc,
	unsigned long long modifier, unsigned int stride, unsigned int offset, Error* err)
{
	(void)ctx;
	(void)fd;
	(void)fourcc;
	(void)modifier;
	(void)stride;
	(void)offset;
	ERROR(err, 1, "dmabuf capture requires a VAAPI-enabled build");
}
#endif
