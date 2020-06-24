#include <X11/X.h>
#include <X11/Xlib.h>
#include <X11/extensions/XInput.h>
#include <X11/extensions/XInput2.h>
#include <X11/extensions/Xrandr.h>
#include <X11/extensions/randr.h>
#include <stdlib.h>
#include <string.h>

#include "../error.h"
#include "xhelper.h"

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
				2,
				"Cannot get client list properties. "
				"_NET_CLIENT_LIST: %s or _WIN_CLIENT_LIST: %s",
				err_net.error_str,
				err_win.error_str);
			return NULL;
		}
	}

	return client_list;
}

int create_capturables(Display* disp, Capturable** capturables, int size, Error* err)
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
			fill_error(err, 2, "Failed to query monitor info via xrandr.");
		}
	}
	else
	{
		fill_error(err, 2, "Xrandr is unsupported on this X server.");
	}

	Window* client_list;
	unsigned long client_list_size;

	size_t num_windows = ((client_list = get_client_list(disp, &client_list_size, err)) == NULL)
							 ? 0
							 : client_list_size / sizeof(Window);

	size_t i = 0;
	Capturable* c = malloc(sizeof(Capturable));
	capturables[i] = c;
	c->disp = disp;
	c->screen = ScreenOfDisplay(disp, screen);
	strncpy(c->name, "Desktop", sizeof(c->name) - 1);
	c->type = WINDOW;
	c->c.winfo.win = root;
	c->c.winfo.is_regular_window = 0;
	++i;

	for (; i < (size_t)num_monitors + 1 && i < (size_t)size; ++i)
	{
		Capturable* c = malloc(sizeof(Capturable));
		capturables[i] = c;
		XRRMonitorInfo* m = &monitors[i - 1];
		c->disp = disp;
		c->screen = ScreenOfDisplay(disp, screen);
		char* name = XGetAtomName(disp, m->name);
		snprintf(c->name, sizeof(c->name) - 1, "Monitor: %s", name);
		XFree(name);
		c->type = RECT;
		c->c.rinfo.x = m->x;
		c->c.rinfo.y = m->y;
		c->c.rinfo.width = m->width;
		c->c.rinfo.height = m->height;
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

		Capturable* c = malloc(sizeof(Capturable));
		capturables[i] = c;
		c->disp = disp;
		c->screen = ScreenOfDisplay(disp, screen);
		c->type = WINDOW;
		strncpy(c->name, title_utf8, sizeof(c->name) - 1);
		c->c.winfo.win = client_list[j];
		c->c.winfo.is_regular_window = 1;
		free(title_utf8);
		free(desktop);
	}
	free(client_list);
	XRRFreeMonitors(monitors);
	return i;
}

void* clone_capturable(Capturable* c)
{
	Capturable* c2 = malloc(sizeof(Capturable));
	*c2 = *c;
	memcpy(c2->name, c->name, sizeof(c2->name));
	return c2;
}

void destroy_capturable(Capturable* c) { free(c); }

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
	Capturable* cap, int* x, int* y, unsigned int* width, unsigned int* height, Error* err)
{
	switch (cap->type)
	{
	case WINDOW:
		get_window_geometry(cap->disp, cap->c.winfo.win, x, y, width, height, err);
		return;
	case RECT:
		*x = cap->c.rinfo.x;
		*y = cap->c.rinfo.y;
		*width = cap->c.rinfo.width;
		*height = cap->c.rinfo.height;
		return;
	}
}

void get_geometry_relative(
	Capturable* cap, float* x, float* y, float* width, float* height, Error* err)
{
	int x_tmp, y_tmp;
	unsigned int width_tmp, height_tmp;
	get_geometry(cap, &x_tmp, &y_tmp, &width_tmp, &height_tmp, err);
	OK_OR_ABORT(err);
	*x = x_tmp / (float)cap->screen->width;
	*y = y_tmp / (float)cap->screen->height;
	*width = width_tmp / (float)cap->screen->width;
	*height = height_tmp / (float)cap->screen->height;
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
	if (!winfo->is_regular_window)
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
	free(active_window);

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

void capturable_before_input(Capturable* cap, Error* err)
{
	switch (cap->type)
	{
	case WINDOW:
		activate_window(cap->disp, &cap->c.winfo, err);
		break;
	case RECT:
		break;
	}
}

const char* get_capturable_name(Capturable* c) { return c->name; }

void map_input_device_to_entire_screen(Display* disp, const char* device_name, int pen, Error* err)
{

	// for some reason a device simualting a stylus does NOT create a single device in
	// XListInputDevices but actually two: One with the original name and the other one with
	// "Pen (0)" appended to it. The problem is that the original device does NOT permit setting
	// "Coordinate Transformation Matrix". This can only be done for the device with "Pen (0)"
	// appended. So this here is a dirty workaround assuming the configurable stylus/pen device is
	// always called original name + "Pen" + whatever.
	char pen_name[256];
	if (pen)
		snprintf(pen_name, sizeof(pen_name), "%s Pen", device_name);
	XID device_id;
	int num_devices = 0;
	XDeviceInfo* devices = XListInputDevices(disp, &num_devices);

	int found = 0;
	for (int i = 0; i < num_devices; ++i)
	{
		if ((!pen && strcmp(device_name, devices[i].name) == 0) ||
			(pen && strncmp(pen_name, devices[i].name, strlen(pen_name)) == 0))
		{
			device_id = devices[i].id;
			found = 1;
			break;
		}
	}
	XFreeDeviceList(devices);

	if (!found)
		ERROR(err, 2, "Device with name: %s not found!", device_name);

	Atom prop_float, prop_matrix;

	union
	{
		unsigned char* c;
		float* f;
	} data;
	int format_return;
	Atom type_return;
	unsigned long nitems;
	unsigned long bytes_after;

	int rc;

	prop_float = XInternAtom(disp, "FLOAT", False);
	prop_matrix = XInternAtom(disp, "Coordinate Transformation Matrix", False);

	if (!prop_float)
	{
		ERROR(err, 1, "Float atom not found. This server is too old.");
	}
	if (!prop_matrix)
	{
		ERROR(
			err,
			1,
			"Coordinate transformation matrix not found. This "
			"server is too old.");
	}

	rc = XIGetProperty(
		disp,
		device_id,
		prop_matrix,
		0,
		9,
		False,
		prop_float,
		&type_return,
		&format_return,
		&nitems,
		&bytes_after,
		&data.c);
	if (rc != Success || prop_float != type_return || format_return != 32 || nitems != 9 ||
		bytes_after != 0)
	{
		ERROR(err, 1, "Failed to retrieve current property values.");
	}

	data.f[0] = 1.0;
	data.f[1] = 0.0;
	data.f[2] = 0.0;
	data.f[3] = 0.0;
	data.f[4] = 1.0;
	data.f[5] = 0.0;
	data.f[6] = 0.0;
	data.f[7] = 0.0;
	data.f[8] = 1.0;

	XIChangeProperty(
		disp, device_id, prop_matrix, prop_float, format_return, PropModeReplace, data.c, nitems);

	XFree(data.c);
}
