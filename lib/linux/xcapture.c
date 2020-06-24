#include <X11/X.h>
#include <X11/Xlib.h>
#include <X11/Xutil.h>

#include <X11/extensions/XShm.h>
#include <X11/extensions/Xcomposite.h>
#include <X11/extensions/Xfixes.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ipc.h>
#include <sys/shm.h>

#include <stdint.h>

#include "../error.h"
#include "xhelper.h"

int clamp(int x, int lb, int ub)
{
	if (x < lb)
		return lb;
	if (x > ub)
		return ub;
	return x;
}

struct CaptureContext
{
	Capturable cap;
	XImage* ximg;
	XShmSegmentInfo shminfo;
	int has_xfixes;
	int has_offscreen;
	Bool last_img_return;
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
	if (XShmQueryExtension(cap->disp) != True)
	{
		fill_error(err, 1, "XShmExtension is not available but required!");
		return NULL;
	}

	if (!ctx)
	{
		ctx = malloc(sizeof(CaptureContext));

		int major, minor;
		Bool pixmaps = False;
		XShmQueryVersion(cap->disp, &major, &minor, &pixmaps);
		ctx->has_offscreen = pixmaps == True;
		if (ctx->has_offscreen && cap->type == WINDOW && cap->c.winfo.is_regular_window)
		{
			int event_base, error_base;
			ctx->has_offscreen =
				XCompositeQueryExtension(cap->disp, &event_base, &error_base) == True;
			if (ctx->has_offscreen)
				XCompositeRedirectWindow(cap->disp, cap->c.winfo.win, False);
		}
	}
	ctx->cap = *cap;
	ctx->last_img_return = True;

	if (&ctx->cap != cap)
		strncpy(ctx->cap.name, cap->name, sizeof(ctx->cap.name));

	int event_base, error_base;
	ctx->has_xfixes = XFixesQueryExtension(cap->disp, &event_base, &error_base) == True;

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
		free(ctx);
		return NULL;
	}
	if (!XShmAttach(cap->disp, &ctx->shminfo))
	{
		fill_error(err, 1, "XShmAttach() failed");
		free(ctx);
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
	if (ctx->has_offscreen && ctx->cap.type == WINDOW && ctx->cap.c.winfo.is_regular_window)
		XCompositeUnredirectWindow(ctx->cap.disp, ctx->cap.c.winfo.win, False);
	free(ctx);
}

void capture_sceen(CaptureContext* ctx, struct Image* img, int capture_cursor, Error* err)
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

	Bool get_img_ret = False;

	switch (ctx->cap.type)
	{
	case WINDOW:
	{
		Window* active_window;
		unsigned long size;

		int is_offscreen = ctx->cap.c.winfo.is_regular_window &&
						   (x < 0 || y < 0 || x + (int)width > ctx->cap.screen->width ||
							y + (int)height > ctx->cap.screen->height);

		active_window =
			(Window*)get_property(ctx->cap.disp, root, XA_WINDOW, "_NET_ACTIVE_WINDOW", &size, err);
		if (*active_window == ctx->cap.c.winfo.win && !is_offscreen)
		{
			// cap window within its root so menus are visible as strictly speaking menus do not
			// belong to the window itself ...
			get_img_ret = XShmGetImage(ctx->cap.disp, root, ctx->ximg, x, y, 0x00ffffff);
		}
		else
		{
			// ... but only if it is the active window as we might be recording the wrong thing
			// otherwise. If it is not active just record the window itself.
			// also if pixmaps are supported use those as they support capturing windows even if
			// they are offscreen
			if (is_offscreen)
			{
				if (ctx->has_offscreen)
				{
					Pixmap pm = XCompositeNameWindowPixmap(ctx->cap.disp, ctx->cap.c.winfo.win);
					get_img_ret = XShmGetImage(ctx->cap.disp, pm, ctx->ximg, 0, 0, 0x00ffffff);
					XFreePixmap(ctx->cap.disp, pm);
				}
				else
					ERROR(
						err,
						1,
						"Can not capture window as it is off screen and Xcomposite is "
						"unavailable!");
			}
			else
				get_img_ret =
					XShmGetImage(ctx->cap.disp, ctx->cap.c.winfo.win, ctx->ximg, 0, 0, 0x00ffffff);
		}
		free(active_window);
		break;
	}
	case RECT:
		get_img_ret = XShmGetImage(ctx->cap.disp, root, ctx->ximg, x, y, 0x00ffffff);
		break;
	}

	Bool last_img_return = ctx->last_img_return;
	ctx->last_img_return = get_img_ret;
	// only print an error once and do not repeat this message if consecutive calls to XShmGetImage
	// fail to avoid spamming the logs.
	if (get_img_ret != True)
	{
		if (last_img_return != get_img_ret)
		{
			ERROR(err, 1, "XShmGetImage failed!");
		}
		else
		{
			ERROR(err, 2, "XShmGetImage failed!");
		}
	}

	// capture cursor if requested and if XFixes is available
	if (capture_cursor && ctx->has_xfixes)
	{
		XFixesCursorImage* cursor_img = XFixesGetCursorImage(ctx->cap.disp);
		uint32_t* data = (uint32_t*)ctx->ximg->data;

		// coordinates of cursor inside ximg
		int x0 = cursor_img->x - cursor_img->xhot - x;
		int y0 = cursor_img->y - cursor_img->yhot - y;

		// clamp part of cursor image to draw to the part of the cursor that is inside
		// the captured area
		int i0 = clamp(0, -x0, width - x0);
		int i1 = clamp(cursor_img->width, -x0, width - x0);
		int j0 = clamp(0, -y0, height - y0);
		int j1 = clamp(cursor_img->height, -y0, height - y0);
		// paint cursor image into captured image
		for (int j = j0; j < j1; ++j)
			for (int i = i0; i < i1; ++i)
			{
				uint32_t c_pixel = cursor_img->pixels[j * cursor_img->width + i];
				unsigned char a = (c_pixel & 0xff000000) >> 24;
				if (a)
				{
					uint32_t d_pixel = data[(j + y0) * width + i + x0];

					unsigned char c1 = (c_pixel & 0x00ff0000) >> 16;
					unsigned char c2 = (c_pixel & 0x0000ff00) >> 8;
					unsigned char c3 = (c_pixel & 0x000000ff) >> 0;
					unsigned char d1 = (d_pixel & 0x00ff0000) >> 16;
					unsigned char d2 = (d_pixel & 0x0000ff00) >> 8;
					unsigned char d3 = (d_pixel & 0x000000ff) >> 0;
					// colors from the cursor image are premultiplied with the alpha channel
					unsigned char f1 = c1 + d1 * (255 - a) / 255;
					unsigned char f2 = c2 + d2 * (255 - a) / 255;
					unsigned char f3 = c3 + d3 * (255 - a) / 255;
					data[(j + y0) * width + i + x0] = (f1 << 16) | (f2 << 8) | (f3 << 0);
				}
			}

		XFree(cursor_img);
	}

	img->width = ctx->ximg->width;
	img->height = ctx->ximg->height;
	img->data = ctx->ximg->data;
}
