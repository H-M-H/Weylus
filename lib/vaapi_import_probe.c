#include "vaapi_import_probe.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include <libavcodec/avcodec.h>
#include <libavfilter/avfilter.h>
#include <libavfilter/buffersink.h>
#include <libavfilter/buffersrc.h>
#include <libavutil/hwcontext.h>
#include <libavutil/hwcontext_drm.h>
#include <libavutil/pixdesc.h>

int vaapi_probe_selftest(void) { return 42; }

static void say(char* reason, int n, const char* what, int err)
{
	char e[128] = {0};
	if (err)
		av_strerror(err, e, sizeof(e));
	snprintf(reason, n, "%s%s%s", what, err ? ": " : "", e);
}

int vaapi_probe_encode_dmabuf(
	int dmabuf_fd,
	unsigned int width,
	unsigned int height,
	unsigned int drm_fourcc,
	unsigned long long drm_modifier,
	unsigned int stride,
	unsigned int offset,
	char* reason,
	int reason_len)
{
	int ret = -1;
	int out_bytes = -1;
	AVBufferRef* drm_dev = NULL;
	AVBufferRef* drm_frames = NULL;
	AVFrame* drm_frame = NULL;
	AVFilterGraph* graph = NULL;
	AVFilterContext* src_ctx = NULL;
	AVFilterContext* map_ctx = NULL;
	AVFilterContext* scale_ctx = NULL;
	AVFilterContext* sink_ctx = NULL;
	AVFrame* va_frame = av_frame_alloc();
	AVCodecContext* enc = NULL;
	AVPacket* pkt = av_packet_alloc();

	// True dmabuf size: niri may report a bogus GstMemory size, so measure the fd.
	off_t fd_size = lseek(dmabuf_fd, 0, SEEK_END);
	if (fd_size <= 0)
		fd_size = (off_t)stride * height;

	// The VAAPI H.264 encoder maxes out at 4096x4096; downscale (preserving aspect,
	// keeping even dims) so oversized monitors (e.g. 5120-wide) still encode. The
	// dmabuf is still IMPORTED at native resolution — only the encode is clamped.
	unsigned int out_w = width, out_h = height;
	if (out_w > 4096)
	{
		out_h = (unsigned int)((unsigned long long)height * 4096u / width) & ~1u;
		out_w = 4096;
	}
	if (out_h > 4096)
	{
		out_w = (unsigned int)((unsigned long long)out_w * 4096u / out_h) & ~1u;
		out_h = 4096;
	}

	// 1) DRM device + a DRM_PRIME frames pool describing the incoming dmabuf.
	// ffmpeg's DRM backend does open(device), so device MUST be a real node path
	// (passing NULL yields open(NULL) -> EFAULT "Bad address").
	const char* drm_node = getenv("WEYLUS_PROBE_DRM_NODE");
	if (!drm_node || !drm_node[0])
		drm_node = "/dev/dri/renderD128";
	ret = av_hwdevice_ctx_create(&drm_dev, AV_HWDEVICE_TYPE_DRM, drm_node, NULL, 0);
	if (ret < 0)
	{
		char what[128];
		snprintf(what, sizeof(what), "hwdevice DRM create (%s)", drm_node);
		say(reason, reason_len, what, ret);
		goto done;
	}

	drm_frames = av_hwframe_ctx_alloc(drm_dev);
	if (!drm_frames)
	{
		say(reason, reason_len, "hwframe_ctx_alloc", 0);
		goto done;
	}
	{
		AVHWFramesContext* fc = (AVHWFramesContext*)drm_frames->data;
		fc->format = AV_PIX_FMT_DRM_PRIME;
		fc->sw_format = AV_PIX_FMT_BGR0; // XR24 == XRGB8888 little-endian == BGR0
		fc->width = width;
		fc->height = height;
		ret = av_hwframe_ctx_init(drm_frames);
		if (ret < 0)
		{
			say(reason, reason_len, "drm hwframe_ctx_init", ret);
			goto done;
		}
	}

	// 2) Wrap the incoming fd as a single-plane AVDRMFrameDescriptor.
	drm_frame = av_frame_alloc();
	drm_frame->format = AV_PIX_FMT_DRM_PRIME;
	drm_frame->width = width;
	drm_frame->height = height;
	{
		AVDRMFrameDescriptor* d = av_mallocz(sizeof(*d));
		d->nb_objects = 1;
		d->objects[0].fd = dmabuf_fd;
		d->objects[0].size = fd_size;
		d->objects[0].format_modifier = drm_modifier;
		d->nb_layers = 1;
		d->layers[0].format = drm_fourcc;
		d->layers[0].nb_planes = 1;
		d->layers[0].planes[0].object_index = 0;
		d->layers[0].planes[0].offset = offset;
		d->layers[0].planes[0].pitch = stride;
		drm_frame->data[0] = (uint8_t*)d;
		drm_frame->buf[0] = av_buffer_create(
			(uint8_t*)d, sizeof(*d), (void (*)(void*, uint8_t*))av_free, NULL, 0);
		drm_frame->hw_frames_ctx = av_buffer_ref(drm_frames);
	}

	// 3) Filter graph:
	//      buffer -> hwmap=derive_device=vaapi -> scale_vaapi=format=nv12 -> buffersink
	graph = avfilter_graph_alloc();
	{
		// A hardware pix_fmt cannot be passed via the args string; alloc the
		// filter, set parameters (incl. hw_frames_ctx), then init with NULL.
		src_ctx = avfilter_graph_alloc_filter(graph, avfilter_get_by_name("buffer"), "in");
		if (!src_ctx)
		{
			say(reason, reason_len, "alloc buffersrc", 0);
			goto done;
		}
		AVBufferSrcParameters* p = av_buffersrc_parameters_alloc();
		p->format = AV_PIX_FMT_DRM_PRIME;
		p->width = width;
		p->height = height;
		p->time_base = (AVRational){1, 1000};
		p->frame_rate = (AVRational){30, 1};
		p->hw_frames_ctx = av_buffer_ref(drm_frames);
		ret = av_buffersrc_parameters_set(src_ctx, p);
		av_free(p);
		if (ret < 0)
		{
			say(reason, reason_len, "buffersrc params", ret);
			goto done;
		}
		ret = avfilter_init_str(src_ctx, NULL);
		if (ret < 0)
		{
			say(reason, reason_len, "buffersrc init", ret);
			goto done;
		}

		ret = avfilter_graph_create_filter(
			&map_ctx,
			avfilter_get_by_name("hwmap"),
			"map",
			"derive_device=vaapi:mode=read",
			NULL,
			graph);
		if (ret < 0)
		{
			say(reason, reason_len, "create hwmap", ret);
			goto done;
		}

		char scale_args[128];
		snprintf(scale_args, sizeof(scale_args), "w=%u:h=%u:format=nv12", out_w, out_h);
		ret = avfilter_graph_create_filter(
			&scale_ctx,
			avfilter_get_by_name("scale_vaapi"),
			"scale",
			scale_args,
			NULL,
			graph);
		if (ret < 0)
		{
			say(reason, reason_len, "create scale_vaapi", ret);
			goto done;
		}

		ret = avfilter_graph_create_filter(
			&sink_ctx, avfilter_get_by_name("buffersink"), "out", NULL, NULL, graph);
		if (ret < 0)
		{
			say(reason, reason_len, "create buffersink", ret);
			goto done;
		}

		if ((ret = avfilter_link(src_ctx, 0, map_ctx, 0)) < 0
			|| (ret = avfilter_link(map_ctx, 0, scale_ctx, 0)) < 0
			|| (ret = avfilter_link(scale_ctx, 0, sink_ctx, 0)) < 0)
		{
			say(reason, reason_len, "filter link", ret);
			goto done;
		}
		ret = avfilter_graph_config(graph, NULL);
		if (ret < 0)
		{
			say(reason, reason_len, "graph config (VA import rejected?)", ret);
			goto done;
		}
	}

	// 4) Push the DRM frame, pull the mapped+scaled VAAPI NV12 frame.
	ret = av_buffersrc_add_frame(src_ctx, drm_frame);
	if (ret < 0)
	{
		say(reason, reason_len, "buffersrc_add_frame", ret);
		goto done;
	}
	ret = av_buffersink_get_frame(sink_ctx, va_frame);
	if (ret < 0)
	{
		say(reason, reason_len, "buffersink_get_frame (VA import/convert failed)", ret);
		goto done;
	}

	// 5) h264_vaapi encoder using the mapped frame's hw_frames_ctx.
	{
		const AVCodec* codec = avcodec_find_encoder_by_name("h264_vaapi");
		if (!codec)
		{
			say(reason, reason_len, "no h264_vaapi encoder", 0);
			goto done;
		}
		enc = avcodec_alloc_context3(codec);
		enc->width = out_w;
		enc->height = out_h;
		enc->time_base = (AVRational){1, 1000};
		enc->framerate = (AVRational){30, 1};
		enc->pix_fmt = AV_PIX_FMT_VAAPI;
		enc->max_b_frames = 0;
		if (!va_frame->hw_frames_ctx)
		{
			say(reason, reason_len, "mapped frame has no hw_frames_ctx", 0);
			goto done;
		}
		enc->hw_frames_ctx = av_buffer_ref(va_frame->hw_frames_ctx);
		ret = avcodec_open2(enc, codec, NULL);
		if (ret < 0)
		{
			say(reason, reason_len, "avcodec_open2 h264_vaapi", ret);
			goto done;
		}
		va_frame->pict_type = AV_PICTURE_TYPE_NONE;
		ret = avcodec_send_frame(enc, va_frame);
		if (ret < 0)
		{
			say(reason, reason_len, "send_frame", ret);
			goto done;
		}
		avcodec_send_frame(enc, NULL); // flush
		ret = avcodec_receive_packet(enc, pkt);
		if (ret < 0)
		{
			say(reason, reason_len, "receive_packet", ret);
			goto done;
		}
		out_bytes = pkt->size;
		reason[0] = '\0';
	}

done:
	if (pkt)
		av_packet_free(&pkt);
	if (enc)
		avcodec_free_context(&enc);
	if (va_frame)
		av_frame_free(&va_frame);
	if (graph)
		avfilter_graph_free(&graph);
	if (drm_frame)
		av_frame_free(&drm_frame);
	if (drm_frames)
		av_buffer_unref(&drm_frames);
	if (drm_dev)
		av_buffer_unref(&drm_dev);
	return out_bytes;
}
