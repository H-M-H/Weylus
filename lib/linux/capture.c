// gcc raw.c -o raw -lX11 -lXext -Ofast -funroll-loops && ./raw
// gcc raw.c -o raw -lX11 -lXext -Ofast -funroll-loops -mfpmath=both -march=native -m64 -mavx2 &&
// ./raw

// The MIT-SHM extension allows for shared-memory XImage objects
// This requires OS support for SYSV (System-V) shared memory, and X support for the MIT-SHM
// extension. Shared memory PIXMAPS can only be supported when the X server can use regular virtual
// memory for pixmap data; if the pixmaps are stored in some magic graphics hardware, you're out of
// luck. Xdpyinfo(1) gives NO information about this!

// Depth 16: each pixel has 16 bits. Red: 5 bits, green: 6 bits, blue: 5 bits. Total: 65536 colors

#include <X11/X.h>
#include <X11/Xlib.h>
#include <X11/Xutil.h>

#include <X11/extensions/XShm.h>
#include <stdlib.h>
#include <sys/ipc.h>
#include <sys/shm.h>

#include <stdint.h>
#include <stdio.h>

#include "error.h"
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
	int width;
	int height;
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
	shmdt(ctx->shminfo.shmaddr);
	free(ctx);
}

void capture_sceen(CaptureContext* ctx, struct Image* img, Error* err)
{
	Window junkroot;
	int x, y;
	unsigned int width, height;
	unsigned int bw, depth;
	if (!XGetGeometry(
			ctx->winfo.disp, ctx->winfo.win, &junkroot, &x, &y, &width, &height, &bw, &depth))
	{
		ERROR(err, 1, "Failed to get window geometry!");
	}
	// if window resized, create new capture...
	if (width != ctx->ximg->width || height != ctx->ximg->height)
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
	XShmGetImage(ctx->winfo.disp, ctx->winfo.win, ctx->ximg, 0, 0, 0x00ffffff);
	img->width = ctx->ximg->width;
	img->height = ctx->ximg->height;
	img->data = ctx->ximg->data;
}
