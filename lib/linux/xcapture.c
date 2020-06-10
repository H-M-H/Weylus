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
	Capturable cap;
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

void* start_capture(Capturable* cap, CaptureContext* ctx, Error* err)
{
	if (!ctx)
		ctx = malloc(sizeof(CaptureContext));
	ctx->cap = *cap;
	strncpy(ctx->cap.name, cap->name, sizeof(ctx->cap.name));

	int x, y;
	unsigned int width, height;
	get_geometry(cap, &x, &y, &width, &height, err);
	ctx->ximg = XShmCreateImage(
		cap->disp,
		DefaultVisualOfScreen(cap->screen),
		DefaultDepthOfScreen(cap->screen),
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
	if (!XShmAttach(cap->disp, &ctx->shminfo))
	{
		fill_error(err, 1, "XShmAttach() failed");
		return NULL;
	}

	return ctx;
}

void stop_capture(CaptureContext* ctx, Error* err)
{
	XShmDetach(ctx->cap.disp, &ctx->shminfo);
	XDestroyImage(ctx->ximg);
	if (shmdt(ctx->shminfo.shmaddr) != 0)
	{
		fill_error(err, 1, "Failed to detach shared memory!");
	}
	free(ctx);
}

void capture_sceen(CaptureContext* ctx, struct Image* img, Error* err)
{
	Window root = DefaultRootWindow(ctx->cap.disp);
	int x, y;
	unsigned int width, height;
	get_geometry(&ctx->cap, &x, &y, &width, &height, err);
	OK_OR_ABORT(err);
	// if window resized, create new cap...
	if (width != (unsigned int)ctx->ximg->width || height != (unsigned int)ctx->ximg->height)
	{
		XShmDetach(ctx->cap.disp, &ctx->shminfo);
		XDestroyImage(ctx->ximg);
		shmdt(ctx->shminfo.shmaddr);
		CaptureContext* new_ctx = start_capture(&ctx->cap, ctx, err);
		if (!new_ctx)
		{
			return;
		}
	}

	switch (ctx->cap.type)
	{
	case WINDOW:
	{
		Window* active_window;
		unsigned long size;

		active_window = (Window*)get_property(
			ctx->cap.disp, root, XA_WINDOW, "_NET_ACTIVE_WINDOW", &size, err);
		if (*active_window == ctx->cap.c.winfo.win)
		{
			// cap window within its root so menus are visible as strictly speaking menus do not
			// belong to the window itself ...
			XShmGetImage(ctx->cap.disp, root, ctx->ximg, x, y, 0x00ffffff);
		}
		else
		{
			// ... but only if it is the active window as we might be recording the wrong thing
			// otherwise. If it is not active just record the window itself.
			XShmGetImage(
				ctx->cap.disp, ctx->cap.c.winfo.win, ctx->ximg, 0, 0, 0x00ffffff);
		}
		free(active_window);
		break;
	}
	case RECT:
		XShmGetImage(ctx->cap.disp, root, ctx->ximg, x, y, 0x00ffffff);
		break;
	}
	img->width = ctx->ximg->width;
	img->height = ctx->ximg->height;
	img->data = ctx->ximg->data;
}
