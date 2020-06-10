#pragma once

#include <X11/X.h>
#include <X11/Xatom.h>
#include <X11/Xlib.h>
#include <X11/Xutil.h>

#include <iconv.h>
#include <malloc.h>
#include <string.h>

#include "../error.h"

#define MAX_PROPERTY_VALUE_LEN 4096

typedef struct WindowInfo
{
	Window win;
	int should_activate;
} WindowInfo;

typedef struct RectInfo
{
	int x;
	int y;
	unsigned int width;
	unsigned int height;
} RectInfo;

typedef enum CaptureType
{
	WINDOW,
	RECT
} CaptureType;

typedef struct Capture
{
	CaptureType type;
	char name[128];
	Display* disp;
	Screen* screen;
	union
	{
		WindowInfo winfo;
		RectInfo rinfo;
	} c;
} Capture;

char* get_property(
	Display* disp, Window win, Atom xa_prop_type, char* prop_name, unsigned long* size, Error* err);

void get_geometry(
	Capture* capture, int* x, int* y, unsigned int* width, unsigned int* height, Error* err);

void get_geometry_relative(
	Capture* capture, float* x, float* y, float* width, float* height, Error* err);

void capture_before_input(Capture* capture, Error* err);
