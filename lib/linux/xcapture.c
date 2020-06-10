#include <X11/X.h>
#include <X11/Xlib.h>
#include <X11/Xutil.h>

#include <X11/extensions/XShm.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ipc.h>
#include <sys/shm.h>

#include <stdint.h>
#include <stdio.h>

#include "../error.h"
#include "xhelper.h"

struct CaptureContext
{
	Capture capture;
	XImage* ximg;
	XShmSegmentInfo shminfo;
};

typedef struct CaptureContext CaptureContext;

struct Image
{
	char* data;
	unsigned int width;
	unsigned int height;
};

void* start_capture(Capture* capture, CaptureContext* ctx, Error* err)
{
	if (!ctx)
		ctx = malloc(sizeof(CaptureContext));
	ctx->capture = *capture;
	strncpy(ctx->capture.name, capture->name, sizeof(ctx->capture.name));

	int x, y;
	unsigned int width, height;
	get_geometry(capture, &x, &y, &width, &height, err);
	ctx->ximg = XShmCreateImage(
		capture->disp,
		DefaultVisualOfScreen(capture->screen),
		DefaultDepthOfScreen(capture->screen),
		ZPixmap,
		NULL,
		&ctx->shminfo,
		width,
		height);

	ctx->shminfo.shmid =
		shmget(IPC_PRIVATE, ctx->ximg->bytes_per_line * ctx->ximg->height, IPC_CREAT | 0777);
	ctx->shminfo.shmaddr = ctx->ximg->data = (char*)shmat(ctx->shminfo.shmid, 0, 0);
	ctx->shminfo.readOnly = False;
	if (ctx->shminfo.shmid < 0)
	{
		fill_error(err, 1, "Fatal shminfo error!");
		return NULL;
	}
	if (!XShmAttach(capture->disp, &ctx->shminfo))
	{
		fill_error(err, 1, "XShmAttach() failed");
		return NULL;
	}

	return ctx;
}

void stop_capture(CaptureContext* ctx, Error* err)
{
	XShmDetach(ctx->capture.disp, &ctx->shminfo);
	XDestroyImage(ctx->ximg);
	if (shmdt(ctx->shminfo.shmaddr) != 0)
	{
		fill_error(err, 1, "Failed to detach shared memory!");
	}
	free(ctx);
}

void capture_sceen(CaptureContext* ctx, struct Image* img, Error* err)
{
	Window root = DefaultRootWindow(ctx->capture.disp);
	int x, y;
	unsigned int width, height;
	get_geometry(&ctx->capture, &x, &y, &width, &height, err);
	OK_OR_ABORT(err);
	// if window resized, create new capture...
	if (width != (unsigned int)ctx->ximg->width || height != (unsigned int)ctx->ximg->height)
	{
		XShmDetach(ctx->capture.disp, &ctx->shminfo);
		XDestroyImage(ctx->ximg);
		shmdt(ctx->shminfo.shmaddr);
		CaptureContext* new_ctx = start_capture(&ctx->capture, ctx, err);
		if (!new_ctx)
		{
			return;
		}
	}

	switch (ctx->capture.type)
	{
	case WINDOW:
	{
		Window* active_window;
		unsigned long size;

		active_window = (Window*)get_property(
			ctx->capture.disp, root, XA_WINDOW, "_NET_ACTIVE_WINDOW", &size, err);
		if (*active_window == ctx->capture.c.winfo.win)
		{
			// capture window within its root so menus are visible as strictly speaking menus do not
			// belong to the window itself ...
			XShmGetImage(ctx->capture.disp, root, ctx->ximg, x, y, 0x00ffffff);
		}
		else
		{
			// ... but only if it is the active window as we might be recording the wrong thing
			// otherwise. If it is not active just record the window itself.
			XShmGetImage(
				ctx->capture.disp, ctx->capture.c.winfo.win, ctx->ximg, 0, 0, 0x00ffffff);
		}
		free(active_window);
		break;
	}
	case RECT:
		XShmGetImage(ctx->capture.disp, root, ctx->ximg, x, y, 0x00ffffff);
		break;
	}
	img->width = ctx->ximg->width;
	img->height = ctx->ximg->height;
	img->data = ctx->ximg->data;
}
