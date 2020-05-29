#include <X11/X.h>
#include <X11/Xlib.h>
#include <X11/Xutil.h>

#include <X11/extensions/XShm.h>
#include <stdlib.h>
#include <sys/ipc.h>
#include <sys/shm.h>

#include <stdint.h>
#include <stdio.h>

#include "../error.h"
#include "xwindows.h"

struct CaptureContext
{
	WindowInfo winfo;
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

void* init_capture(WindowInfo* winfo, CaptureContext* ctx, Error* err)
{
	if (!ctx)
		ctx = malloc(sizeof(CaptureContext));
	ctx->winfo = *winfo;
	XWindowAttributes window_attributes;
	if (!XGetWindowAttributes(winfo->disp, winfo->win, &window_attributes))
	{
		fill_error(err, 1, "Failed to get window attributes for window: 0x%.8lx", ctx->winfo.win);
		return NULL;
	}

	int x, y;
	unsigned int width, height;
	get_window_geometry(winfo, &x, &y, &width, &height, err);
	Screen* screen = window_attributes.screen;
	ctx->ximg = XShmCreateImage(
		winfo->disp,
		DefaultVisualOfScreen(screen),
		DefaultDepthOfScreen(screen),
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
	if (!XShmAttach(winfo->disp, &ctx->shminfo))
	{
		fill_error(err, 1, "XShmAttach() failed");
		return NULL;
	}

	return ctx;
}

void destroy_capture(CaptureContext* ctx, Error* err)
{
	XShmDetach(ctx->winfo.disp, &ctx->shminfo);
	XDestroyImage(ctx->ximg);
	if (shmdt(ctx->shminfo.shmaddr) != 0)
	{
		fill_error(err, 1, "Failed to detach shared memory!");
	}
	free(ctx);
}

void capture_sceen(CaptureContext* ctx, struct Image* img, Error* err)
{
	Window root;
	int junkx = 0, junky = 0;
	unsigned int width, height;
	unsigned int bw, depth;
	if (!XGetGeometry(
			ctx->winfo.disp, ctx->winfo.win, &root, &junkx, &junky, &width, &height, &bw, &depth))
	{
		ERROR(err, 1, "Failed to get window geometry!");
	}
	// if window resized, create new capture...
	if (width != (unsigned int)ctx->ximg->width || height != (unsigned int)ctx->ximg->height)
	{
		XShmDetach(ctx->winfo.disp, &ctx->shminfo);
		XDestroyImage(ctx->ximg);
		shmdt(ctx->shminfo.shmaddr);
		CaptureContext* new_ctx = init_capture(&ctx->winfo, ctx, err);
		if (!new_ctx)
		{
			return;
		}
	}

	Window junkwin;
	int x = 0, y = 0;
	Window* active_window = 0;
	unsigned long size;

	active_window = (Window*)get_property(
		ctx->winfo.disp,
		DefaultRootWindow(ctx->winfo.disp),
		XA_WINDOW,
		"_NET_ACTIVE_WINDOW",
		&size,
		err);
	if (*active_window == ctx->winfo.win)
	{
		// capture window within its root so menus are visible as strictly speaking menus do not
		// belong to the window itself ...
		XTranslateCoordinates(ctx->winfo.disp, ctx->winfo.win, root, 0, 0, &x, &y, &junkwin);
		XShmGetImage(ctx->winfo.disp, root, ctx->ximg, x, y, 0x00ffffff);
	}
	else
	{
		// ... but only if it is the active window as we might be recording the wrong thing
		// otherwise. If it is not active just record the window itself.
		XShmGetImage(ctx->winfo.disp, ctx->winfo.win, ctx->ximg, 0, 0, 0x00ffffff);
	}
	free(active_window);
	img->width = ctx->ximg->width;
	img->height = ctx->ximg->height;
	img->data = ctx->ximg->data;
}
