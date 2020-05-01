#pragma once

#include <X11/X.h>
#include <X11/Xatom.h>
#include <X11/Xlib.h>
#include <X11/Xutil.h>

#include <iconv.h>
#include <malloc.h>
#include <string.h>

#include "error.h"

#define MAX_PROPERTY_VALUE_LEN 4096

struct WindowInfo
{
	Display* disp;
	Window win;
	signed long desktop_id;
	char title[MAX_PROPERTY_VALUE_LEN];
};

typedef struct WindowInfo WindowInfo;

int locale_to_utf8(char* src, char* dest, size_t size);

char* get_property(
	Display* disp, Window win, Atom xa_prop_type, char* prop_name, unsigned long* size, Error* err);

char* get_window_title(Display* disp, Window win, Error* err);

Window* get_client_list(Display* disp, unsigned long* size, Error* err);

char* get_window_class(Display* disp, Window win, Error* err);

void free_window_info(WindowInfo* windows, size_t size);

size_t create_window_info(Display* disp, WindowInfo** windows, Error* err);

void get_window_geometry(
	WindowInfo* winfo, int* x, int* y, unsigned int* width, unsigned int* height, Error* err);

