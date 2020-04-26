// gcc raw.c -o raw -lX11 -lXext -Ofast -funroll-loops && ./raw
// gcc raw.c -o raw -lX11 -lXext -Ofast -funroll-loops -mfpmath=both -march=native -m64 -mavx2 &&
// ./raw

// The MIT-SHM extension allows for shared-memory XImage objects
// This requires OS support for SYSV (System-V) shared memory, and X support for the MIT-SHM
// extension. Shared memory PIXMAPS can only be supported when the X server can use regular virtual
// memory for pixmap data; if the pixmaps are stored in some magic graphics hardware, you're out of
// luck. Xdpyinfo(1) gives NO information about this!

// Depth 16: each pixel has 16 bits. Red: 5 bits, green: 6 bits, blue: 5 bits. Total: 65536 colors

#include <X11/Xlib.h>
#include <X11/Xutil.h>

#include <X11/extensions/XShm.h>
#include <stdlib.h>
#include <sys/ipc.h>
#include <sys/shm.h>

#include <stdint.h>
#include <stdio.h>

#include "error.h"

struct CaptureContext
{
	Display* display;
	XImage* ximg;
	XShmSegmentInfo shminfo;
	Window window_root;
};

typedef struct CaptureContext CaptureContext;

struct Image
{
	char* data;
	int width;
	int height;
};

void* init_capture(Error* err)
{
	CaptureContext* ctx = malloc(sizeof(CaptureContext));
	ctx->display = XOpenDisplay(NULL);

	int ignore, major, minor;
	Bool pixmaps;
	if (XQueryExtension(ctx->display, "MIT-SHM", &ignore, &ignore, &ignore))
		if (XShmQueryVersion(ctx->display, &major, &minor, &pixmaps))
			printf(
				"XShm extension v%d.%d %s shared pixmaps\n",
				major,
				minor,
				pixmaps ? "with" : "without");
	// Macro to return the root window! It's a simple uint32
	ctx->window_root = DefaultRootWindow(ctx->display);
	XWindowAttributes window_attributes;
	XGetWindowAttributes(ctx->display, ctx->window_root, &window_attributes);
	Screen* screen = window_attributes.screen;
	ctx->ximg = XShmCreateImage(
		ctx->display,
		DefaultVisualOfScreen(screen),
		DefaultDepthOfScreen(screen),
		ZPixmap,
		NULL,
		&ctx->shminfo,
		screen->width,
		screen->height);

	ctx->shminfo.shmid =
		shmget(IPC_PRIVATE, ctx->ximg->bytes_per_line * ctx->ximg->height, IPC_CREAT | 0777);
	ctx->shminfo.shmaddr = ctx->ximg->data = (char*)shmat(ctx->shminfo.shmid, 0, 0);
	ctx->shminfo.readOnly = False;
	if (ctx->shminfo.shmid < 0)
		puts("Fatal shminfo error!");
	;
	Status s1 = XShmAttach(ctx->display, &ctx->shminfo);
	printf("XShmAttach() %s\n", s1 ? "success!" : "failure!");

	return ctx;
}

void capture_sceen(CaptureContext* ctx, struct Image* img, Error* err)
{
	XShmGetImage(ctx->display, ctx->window_root, ctx->ximg, 0, 0, 0x00ffffff);
	img->width = ctx->ximg->width;
	img->height = ctx->ximg->height;
	img->data = ctx->ximg->data;
}

void destroy_capture(CaptureContext* ctx, Error* err)
{
	XShmDetach(ctx->display, &ctx->shminfo);
	XDestroyImage(ctx->ximg);
	shmdt(ctx->shminfo.shmaddr);
	XCloseDisplay(ctx->display);
	free(ctx);
}
