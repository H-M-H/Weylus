#include <X11/X.h>
#include <X11/Xlib.h>
#include <X11/extensions/Xrandr.h>
#include <X11/extensions/randr.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>


#include "xhelper.h"
#include "../error.h"

int locale_to_utf8(char* src, char* dest, size_t size)
{
	iconv_t icd = iconv_open("UTF-8//IGNORE", "");
	size_t src_size = size;
	size_t outbytes_left = MAX_PROPERTY_VALUE_LEN - 1;
	int ret = iconv(icd, &src, &src_size, &dest, &outbytes_left);
	iconv_close(icd);
	if (ret < 0)
	{
		return -1;
	}
	dest[src_size - 1 - outbytes_left] = '\0';
	return 0;
}

char* get_property(
	Display* disp, Window win, Atom xa_prop_type, char* prop_name, unsigned long* size, Error* err)
{
	Atom xa_prop_name;
	Atom xa_ret_type;
	int ret_format;
	unsigned long ret_nitems;
	unsigned long ret_bytes_after;
	unsigned long tmp_size;
	unsigned char* ret_prop;
	char* ret;

	xa_prop_name = XInternAtom(disp, prop_name, False);

	/* MAX_PROPERTY_VALUE_LEN / 4 explanation (XGetWindowProperty manpage):
	 *
	 * long_length = Specifies the length in 32-bit multiples of the
	 *               data to be retrieved.
	 */
	if (XGetWindowProperty(
			disp,
			win,
			xa_prop_name,
			0,
			MAX_PROPERTY_VALUE_LEN / 4,
			False,
			xa_prop_type,
			&xa_ret_type,
			&ret_format,
			&ret_nitems,
			&ret_bytes_after,
			&ret_prop) != Success)
	{
		fill_error(err, 1, "Cannot get %s property.", prop_name);
		return NULL;
	}

	if (xa_ret_type != xa_prop_type)
	{
		fill_error(err, 1, "Invalid type of %s property.", prop_name);
		XFree(ret_prop);
		return NULL;
	}

	/* null terminate the result to make string handling easier */
	tmp_size = (ret_format / 8) * ret_nitems;
	/* Correct 64 Architecture implementation of 32 bit data */
	if (ret_format == 32)
		tmp_size *= sizeof(long) / 4;
	ret = malloc(tmp_size + 1);
	memcpy(ret, ret_prop, tmp_size);
	ret[tmp_size] = '\0';

	if (size)
	{
		*size = tmp_size;
	}

	XFree(ret_prop);
	return ret;
}

char* get_window_title(Display* disp, Window win, Error* err)
{
	char* title_utf8;
	char* wm_name;
	char* net_wm_name;
	Error err_wm;
	Error err_net_wm;

	wm_name = get_property(disp, win, XA_STRING, "WM_NAME", NULL, &err_wm);
	net_wm_name = get_property(
		disp, win, XInternAtom(disp, "UTF8_STRING", False), "_NET_WM_NAME", NULL, &err_net_wm);

	if (net_wm_name)
	{
		title_utf8 = strdup(net_wm_name);
	}
	else
	{
		if (wm_name)
		{
			title_utf8 = malloc(MAX_PROPERTY_VALUE_LEN);
			if (locale_to_utf8(wm_name, title_utf8, MAX_PROPERTY_VALUE_LEN) != 0)
			{
				fill_error(err, 1, "Failed to convert windowname to UTF-8!");
				free(title_utf8);
				title_utf8 = NULL;
			}
		}
		else
		{
			fill_error(
				err,
				1,
				"Could not get window name: (%s) (%s)",
				err_net_wm.error_str,
				err_wm.error_str);
			title_utf8 = NULL;
		}
	}

	free(wm_name);
	free(net_wm_name);

	return title_utf8;
}

Window* get_client_list(Display* disp, unsigned long* size, Error* err)
{
	Window* client_list;
	Error err_net;
	Error err_win;
	if ((client_list = (Window*)get_property(
			 disp, DefaultRootWindow(disp), XA_WINDOW, "_NET_CLIENT_LIST", size, &err_net)) == NULL)
	{
		if ((client_list = (Window*)get_property(
				 disp, DefaultRootWindow(disp), XA_CARDINAL, "_WIN_CLIENT_LIST", size, &err_win)) ==
			NULL)
		{
			fill_error(
				err,
				1,
				"Cannot get client list properties. "
				"_NET_CLIENT_LIST: %s or _WIN_CLIENT_LIST: %s",
				err_net.error_str,
				err_win.error_str);
			return NULL;
		}
	}

	return client_list;
}

int create_capture_infos(Display* disp, Capture** captures, int size, Error* err)
{
	if (size <= 0)
		return 0;

	int screen = DefaultScreen(disp);
	Window root = RootWindow(disp, screen);

	int event_base, error_base, major, minor;
	int num_monitors = 0;
	XRRMonitorInfo* monitors = NULL;
	if (XRRQueryExtension(disp, &event_base, &error_base) && XRRQueryVersion(disp, &major, &minor))
	{
		monitors = XRRGetMonitors(disp, root, True, &num_monitors);
		if (num_monitors < 0)
		{
			num_monitors = 0;
			fill_error(err, 1, "Failed to query monitor info via xrandr.");
		}
	}
	else
	{
		fill_error(err, 1, "Xrandr is unsupported on this X server.");
	}

	Window* client_list;
	unsigned long client_list_size;

	size_t num_windows = ((client_list = get_client_list(disp, &client_list_size, err)) == NULL)
							 ? 0
							 : client_list_size / sizeof(Window);

	size_t i = 0;
	Capture* cs = malloc(sizeof(Capture));
	captures[i] = cs;
	cs->disp = disp;
	cs->screen = ScreenOfDisplay(disp, screen);
	strncpy(cs->name, "Desktop", sizeof(cs->name) - 1);
	cs->type = WINDOW;
	cs->c.winfo.win = root;
	cs->c.winfo.should_activate = 0;
	++i;

	for (; i < (size_t)num_monitors + 1 && i < (size_t)size; ++i)
	{
		Capture* cs = malloc(sizeof(Capture));
		captures[i] = cs;
		XRRMonitorInfo* m = &monitors[i - 1];
		cs->disp = disp;
		cs->screen = ScreenOfDisplay(disp, screen);
		char* name = XGetAtomName(disp, m->name);
		strncpy(cs->name, name, sizeof(cs->name) - 1);
		XFree(name);
		cs->type = RECT;
		cs->c.rinfo.x = m->x;
		cs->c.rinfo.y = m->y;
		cs->c.rinfo.width = m->width;
		cs->c.rinfo.height = m->height;
	}

	for (; i < num_windows + num_monitors + 1 && i < (size_t)size; ++i)
	{
		size_t j = i - num_monitors - 1;
		char* title_utf8 = get_window_title(disp, client_list[j], NULL);
		if (title_utf8 == NULL)
		{
			title_utf8 = malloc(32);
			snprintf(title_utf8, 32, "UNKNOWN %lu", j);
		}
		unsigned long* desktop;

		/* desktop ID */
		if ((desktop = (unsigned long*)get_property(
				 disp, client_list[j], XA_CARDINAL, "_NET_WM_DESKTOP", NULL, NULL)) == NULL)
		{
			desktop = (unsigned long*)get_property(
				disp, client_list[j], XA_CARDINAL, "_WIN_WORKSPACE", NULL, NULL);
		}

		Capture* cs = malloc(sizeof(Capture));
		captures[i] = cs;
		cs->disp = disp;
		cs->type = WINDOW;
		strncpy(cs->name, title_utf8, sizeof(cs->name) - 1);
		cs->c.winfo.win = client_list[j];
		cs->c.winfo.should_activate = 1;
		free(title_utf8);
		free(desktop);
	}
	free(client_list);
	XRRFreeMonitors(monitors);
	return i;
}

void* clone_capture_info(Capture* c)
{
	Capture* c2 = malloc(sizeof(Capture));
	*c2 = *c;
	memcpy(c2->name, c->name, sizeof(c2->name));
	return c2;
}

void destroy_capture_info(Capture* c) { free(c); }

void get_window_geometry(
	Display* disp,
	Window win,
	int* x,
	int* y,
	unsigned int* width,
	unsigned int* height,
	Error* err)
{
	Window junkroot;
	int junkx, junky;
	unsigned int bw, depth;
	if (!XGetGeometry(disp, win, &junkroot, &junkx, &junky, width, height, &bw, &depth))
	{
		ERROR(err, 1, "Failed to get window geometry!");
	}
	XTranslateCoordinates(disp, win, junkroot, 0, 0, x, y, &junkroot);
}

void get_geometry(
	Capture* capture, int* x, int* y, unsigned int* width, unsigned int* height, Error* err)
{
	switch (capture->type)
	{
	case WINDOW:
		get_window_geometry(capture->disp, capture->c.winfo.win, x, y, width, height, err);
		return;
	case RECT:
		*x = capture->c.rinfo.x;
		*y = capture->c.rinfo.y;
		*width = capture->c.rinfo.width;
		*height = capture->c.rinfo.height;
		return;
	}
}

void get_geometry_relative(
	Capture* capture, float* x, float* y, float* width, float* height, Error* err)
{
	int x_tmp, y_tmp;
	unsigned int width_tmp, height_tmp;
	get_geometry(capture, &x_tmp, &y_tmp, &width_tmp, &height_tmp, err);
	OK_OR_ABORT(err);
	*x = x_tmp / (float)capture->screen->width;
	*y = y_tmp / (float)capture->screen->height;
	*width = width_tmp / (float)capture->screen->width;
	*height = height_tmp / (float)capture->screen->height;
}

void client_msg(
	Display* disp,
	Window win,
	char* msg,
	unsigned long data0,
	unsigned long data1,
	unsigned long data2,
	unsigned long data3,
	unsigned long data4,
	Error* err)
{
	XEvent event;
	long mask = SubstructureRedirectMask | SubstructureNotifyMask;

	event.xclient.type = ClientMessage;
	event.xclient.serial = 0;
	event.xclient.send_event = True;
	event.xclient.message_type = XInternAtom(disp, msg, False);
	event.xclient.window = win;
	event.xclient.format = 32;
	event.xclient.data.l[0] = data0;
	event.xclient.data.l[1] = data1;
	event.xclient.data.l[2] = data2;
	event.xclient.data.l[3] = data3;
	event.xclient.data.l[4] = data4;

	if (!XSendEvent(disp, DefaultRootWindow(disp), False, mask, &event))
	{
		ERROR(err, 1, "Cannot send %s event.", msg);
	}
}

void activate_window(Display* disp, WindowInfo* winfo, Error* err)
{
	// do not activate windows like the root window or root windows of a screen
	if (!winfo->should_activate)
		return;

	Window* active_window = 0;
	unsigned long size;

	active_window = (Window*)get_property(
		disp, DefaultRootWindow(disp), XA_WINDOW, "_NET_ACTIVE_WINDOW", &size, err);
	if (*active_window == winfo->win)
	{
		// nothing to do window is active already
		free(active_window);
		return;
	}

	unsigned long* desktop;
	/* desktop ID */
	if ((desktop = (unsigned long*)get_property(
			 disp, winfo->win, XA_CARDINAL, "_NET_WM_DESKTOP", NULL, err)) == NULL)
	{
		if ((desktop = (unsigned long*)get_property(
				 disp, winfo->win, XA_CARDINAL, "_WIN_WORKSPACE", NULL, err)) == NULL)
		{
			ERROR(err, 1, "Cannot find desktop ID of the window.");
		}
	}
	client_msg(disp, DefaultRootWindow(disp), "_NET_CURRENT_DESKTOP", *desktop, 0, 0, 0, 0, err);
	free(desktop);
	OK_OR_ABORT(err);

	client_msg(disp, winfo->win, "_NET_ACTIVE_WINDOW", 0, 0, 0, 0, 0, err);
	OK_OR_ABORT(err);
	XMapRaised(disp, winfo->win);
}

void capture_before_input(Capture* capture, Error* err)
{
	switch (capture->type)
	{
	case WINDOW:
		activate_window(capture->disp, &capture->c.winfo, err);
		break;
	case RECT:
		break;
	}
}

const char* get_capture_name(Capture* c) { return c->name; }
