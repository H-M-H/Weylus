#include <libavcodec/avcodec.h>
#include <libavformat/avformat.h>
#include <libavformat/avio.h>
#include <libavutil/dict.h>
#include <libavutil/frame.h>
#include <libavutil/mem.h>
#include <libavutil/pixfmt.h>

#include <libavutil/imgutils.h>
#include <libavutil/opt.h>

#include <libswscale/swscale.h>
#include <stdint.h>

#include "error.h"

typedef struct VideoContext
{
	AVFormatContext* oc;
	AVCodecContext* c;
	AVFrame* frame;
	AVPacket* pkt;
	AVStream* st;
	int width;
	int height;
	size_t buf_size;
	void* buf;
	void* rust_ctx;
	int pts;
	struct SwsContext* sws;
	int initialized;
} VideoContext;

int write_video_packet(void* rust_ctx, uint8_t* buf, int buf_size);

void open_video(VideoContext* ctx, Error* err)
{
	if (ctx->width <= 1 || ctx->height <= 1)
		ERROR(err, 1, "Invalid size for video: width = %d, height = %d", ctx->width, ctx->height);

	const AVCodec* codec;
	int ret;

	avformat_alloc_output_context2(&ctx->oc, NULL, "mp4", NULL);
	if (!ctx->oc)
	{
		ERROR(err, 1, "Could not find output format mp4.");
	}

	// codec = avcodec_find_encoder_by_name("libx264");
	codec = avcodec_find_encoder_by_name("h264_nvenc");
	if (!codec)
	{
		ERROR(err, 1, "Codec 'libx264' not found");
	}

	ctx->c = avcodec_alloc_context3(codec);
	if (!ctx->c)
	{
		ERROR(err, 1, "Could not allocate video codec context");
	}

	/* resolution must be a multiple of two */
	ctx->c->width = ctx->width;
	ctx->c->height = ctx->height;
	ctx->c->time_base = (AVRational){1, 1000};
	ctx->c->framerate = (AVRational){0, 1};

	ctx->c->gop_size = 12;
	// no B-frames to reduce latency
	ctx->c->max_b_frames = 0;
	ctx->c->pix_fmt = AV_PIX_FMT_YUV420P;
	//ctx->c->pix_fmt = AV_PIX_FMT_BGR0;
	if (ctx->oc->oformat->flags & AVFMT_GLOBALHEADER)
		ctx->c->flags |= AV_CODEC_FLAG_GLOBAL_HEADER;

	/*av_opt_set(ctx->c->priv_data, "preset", "ultrafast", 0);
	av_opt_set(ctx->c->priv_data, "tune", "zerolatency", 0);
	av_opt_set(ctx->c->priv_data, "crf", "23", 0);*/
	av_opt_set(ctx->c->priv_data, "preset", "llhp", 0);
	av_opt_set(ctx->c->priv_data, "zerolatency", "1", 0);
	av_opt_set(ctx->c->priv_data, "rc", "vbr_hq", 0);
	av_opt_set(ctx->c->priv_data, "cq", "23", 0);

	ctx->st = avformat_new_stream(ctx->oc, NULL);

	/* open it */
	ret = avcodec_open2(ctx->c, codec, NULL);
	if (ret < 0)
	{
		ERROR(err, 1, "Could not open codec: %s", av_err2str(ret));
	}

	avcodec_parameters_from_context(ctx->st->codecpar, ctx->c);

	ctx->frame = av_frame_alloc();
	if (!ctx->frame)
	{
		ERROR(err, 1, "Could not allocate video frame");
	}
	ctx->frame->format = ctx->c->pix_fmt;
	ctx->frame->width = ctx->c->width;
	ctx->frame->height = ctx->c->height;

	ret = av_frame_get_buffer(ctx->frame, 32);
	if (ret < 0)
	{
		ERROR(err, 1, "Could not allocate the video frame data");
	}

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
	av_dict_free(&opt);
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
		av_frame_free(&ctx->frame);
		av_packet_free(&ctx->pkt);
		av_free(ctx->buf);
		sws_freeContext(ctx->sws);
	}
	free(ctx);
}

void encode_video_frame(VideoContext* ctx, int micros, Error* err)
{
	int ret;

	ctx->frame->pts = micros;

	ret = avcodec_send_frame(ctx->c, ctx->frame);
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

VideoContext* init_video_encoder(void* rust_ctx, int width, int height)
{
	VideoContext* ctx = malloc(sizeof(VideoContext));
	ctx->rust_ctx = rust_ctx;
	ctx->width = width - width % 2;
	ctx->height = height - height % 2;
	ctx->pts = 0;
	ctx->initialized = 0;
	ctx->sws = sws_getContext(
		width,
		height,
		AV_PIX_FMT_BGRA,
		ctx->width,  // note that this is != width, this is in purpose as this allows proper
		ctx->height, // rescaling if dimensions of provided image data are not even
		AV_PIX_FMT_YUV420P,
		SWS_FAST_BILINEAR,
		NULL,
		NULL,
		NULL);
	return ctx;
}

uint8_t** get_video_frame_data(VideoContext* ctx, int** linesizes)
{
	// make sure the frame data is writable
	av_frame_make_writable(ctx->frame);

	*linesizes = ctx->frame->linesize;
	return ctx->frame->data;
}

void convert_bgra2yuv420p(
	VideoContext* ctx,
	const void* data,
	int width,
	int height,
	uint8_t* const* dst,
	const int* dst_stride)
{
	const uint8_t* const* src = (const uint8_t* const*)&data;
	const int src_stride[] = {width * 4, 0, 0, 0};
	sws_scale(ctx->sws, src, src_stride, 0, height, dst, dst_stride);
}
