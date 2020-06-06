#include "xwindows.h"
#include "../error.h"
#include <X11/X.h>
#include <X11/Xlib.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

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

size_t get_window_info(Display* disp, WindowInfo* windows, size_t size, Error* err)
{
	Window* client_list;
	unsigned long client_list_size;

	if ((client_list = get_client_list(disp, &client_list_size, err)) == NULL)
	{
		return 0;
	}

	size_t num_screens = XScreenCount(disp);
	size_t i = 0;
	for (; i < num_screens && i < size; i++)
	{
		Screen* s = XScreenOfDisplay(disp, i);
		windows[i].disp = disp;
		windows[i].win = XRootWindowOfScreen(s);
		windows[i].desktop_id = -1;
		snprintf(windows[i].title, sizeof(windows[i].title), "Screen - %lu", i);
		windows[i].should_activate = 0;
	}

	size_t num_windows = client_list_size / sizeof(Window);

	for (; i < num_windows && i < size; i++)
	{
		char* title_utf8 = get_window_title(disp, client_list[i], NULL);
		if (title_utf8 == NULL)
		{
			title_utf8 = malloc(32);
			snprintf(title_utf8, 32, "UNKNOWN %lu", i);
		}
		unsigned long* desktop;

		/* desktop ID */
		if ((desktop = (unsigned long*)get_property(
				 disp, client_list[i], XA_CARDINAL, "_NET_WM_DESKTOP", NULL, NULL)) == NULL)
		{
			desktop = (unsigned long*)get_property(
				disp, client_list[i], XA_CARDINAL, "_WIN_WORKSPACE", NULL, NULL);
		}

		windows[i].disp = disp;
		windows[i].win = client_list[i];
		// special desktop ID -1 means "all desktops"
		// use -2 to indicate that no desktop has been found
		windows[i].desktop_id = desktop ? (signed long)*desktop : -2;
		strncpy(windows[i].title, title_utf8, sizeof(windows->title));
		windows[i].should_activate = 1;
		free(title_utf8);
		free(desktop);
	}
	free(client_list);
	return num_windows;
}

void get_window_geometry(
	WindowInfo* winfo, int* x, int* y, unsigned int* width, unsigned int* height, Error* err)
{
	Window junkroot;
	int junkx, junky;
	unsigned int bw, depth;
	if (!XGetGeometry(
			winfo->disp, winfo->win, &junkroot, &junkx, &junky, width, height, &bw, &depth))
	{
		ERROR(err, 1, "Failed to get window geometry!");
	}
	XTranslateCoordinates(winfo->disp, winfo->win, junkroot, 0, 0, x, y, &junkroot);
}

void get_window_geometry_relative(
	WindowInfo* winfo, float* x, float* y, float* width, float* height, Error* err)
{
	Window junkroot;
	int x_tmp, y_tmp;

	XWindowAttributes window_attributes;
	if (!XGetWindowAttributes(winfo->disp, winfo->win, &window_attributes))
	{
		ERROR(err, 1, "Failed to get window attributes for window: 0x%.8lx", winfo->win);
	}
	if (!XTranslateCoordinates(
			winfo->disp, winfo->win, window_attributes.root, 0, 0, &x_tmp, &y_tmp, &junkroot))
	{
		// we are on a different screen or some error occured
		ERROR(err, 1, "Failed to get window coordinates relative to its root!");
	}
	*x = x_tmp / (float)window_attributes.screen->width;
	*y = y_tmp / (float)window_attributes.screen->height;
	*width = window_attributes.width / (float)window_attributes.screen->width;
	*height = window_attributes.height / (float)window_attributes.screen->height;
}

void get_root_window_info(Display* disp, WindowInfo* winfo)
{
	Window root = DefaultRootWindow(disp);
	char* title_utf8 = malloc(8);
	snprintf(title_utf8, 8, "Desktop");
	winfo->disp = disp;
	strncpy(winfo->title, title_utf8, sizeof(winfo->title) - 1);
	winfo->win = root;
	winfo->desktop_id = -1;
	winfo->should_activate = 0;
	free(title_utf8);
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

void activate_window(WindowInfo* winfo, Error* err)
{
	// do not activate windows like the root window or root windows of a screen
	if (!winfo->should_activate)
		return;

	Window* active_window = 0;
	unsigned long size;

	active_window = (Window*)get_property(
		winfo->disp, DefaultRootWindow(winfo->disp), XA_WINDOW, "_NET_ACTIVE_WINDOW", &size, err);
	if (*active_window == winfo->win)
	{
		// nothing to do window is active already
		free(active_window);
		return;
	}

	unsigned long* desktop;
	/* desktop ID */
	if ((desktop = (unsigned long*)get_property(
			 winfo->disp, winfo->win, XA_CARDINAL, "_NET_WM_DESKTOP", NULL, err)) == NULL)
	{
		if ((desktop = (unsigned long*)get_property(
				 winfo->disp, winfo->win, XA_CARDINAL, "_WIN_WORKSPACE", NULL, err)) == NULL)
		{
			ERROR(err, 1, "Cannot find desktop ID of the window.");
		}
	}
	client_msg(
		winfo->disp,
		DefaultRootWindow(winfo->disp),
		"_NET_CURRENT_DESKTOP",
		*desktop,
		0,
		0,
		0,
		0,
		err);
	free(desktop);
	OK_OR_ABORT(err);

	client_msg(winfo->disp, winfo->win, "_NET_ACTIVE_WINDOW", 0, 0, 0, 0, 0, err);
	OK_OR_ABORT(err);
	XMapRaised(winfo->disp, winfo->win);
}
