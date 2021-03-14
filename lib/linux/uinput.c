#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <linux/input-event-codes.h>
#include <linux/input.h>
#include <linux/uinput.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include "../error.h"

#define ABS_MAXVAL 65535

void setup_abs(int fd, int code, int minimum, int maximum, int resolution, Error* err)
{
	if (ioctl(fd, UI_SET_ABSBIT, code) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_ABSBIT, code %#x", code);

	struct uinput_abs_setup abs_setup;
	memset(&abs_setup, 0, sizeof(abs_setup));
	abs_setup.code = code;
	abs_setup.absinfo.value = 0;
	abs_setup.absinfo.minimum = minimum;
	abs_setup.absinfo.maximum = maximum;
	abs_setup.absinfo.fuzz = 0;
	abs_setup.absinfo.flat = 0;
	// units/mm
	abs_setup.absinfo.resolution = resolution;
	if (ioctl(fd, UI_ABS_SETUP, &abs_setup) < 0)
		ERROR(err, 1, "error: UI_ABS_SETUP, code: %#x", code);
}

void setup(int fd, const char* name, Error* err)
{

	struct uinput_setup setup;
	memset(&setup, 0, sizeof(setup));
	strncpy(setup.name, name, UINPUT_MAX_NAME_SIZE - 1);
	setup.id.bustype = BUS_VIRTUAL;
	setup.id.vendor = 0x1701;
	setup.id.product = 0x1701;
	setup.id.version = 0x0001;
	setup.ff_effects_max = 0;
	if (ioctl(fd, UI_DEV_SETUP, &setup) < 0)
		ERROR(err, 1, "error: UI_DEV_SETUP");
}

void init_keyboard(int fd, const char* name, Error* err)
{
	// enable synchronization
	if (ioctl(fd, UI_SET_EVBIT, EV_SYN) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_EVBIT EV_SYN");

	// enable keys
	if (ioctl(fd, UI_SET_EVBIT, EV_KEY) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_EVBIT EV_KEY");

	// enable all the keys!
	for (int keycode = KEY_ESC; keycode <= KEY_MICMUTE; ++keycode)
		if (ioctl(fd, UI_SET_KEYBIT, keycode) < 0)
			ERROR(err, 1, "error: ioctl UI_SET_KEYBIT %x", keycode);

	// TODO: figure if scancodes are needed
	// if (ioctl(fd, UI_SET_EVBIT, EV_MSC) < 0)
	// 	ERROR(err, 1, "error: ioctl UI_SET_EVBIT EV_MSC");
	// if (ioctl(fd, UI_SET_MSCBIT, MSC_SCAN) < 0)
	// 	ERROR(err, 1, "error: ioctl UI_SET_MSCBIT MSC_SCAN");

	setup(fd, name, err);
	OK_OR_ABORT(err);

	if (ioctl(fd, UI_DEV_CREATE) < 0)
		ERROR(err, 1, "error: ioctl");
}

void init_mouse(int fd, const char* name, Error* err)
{
	// enable synchronization
	if (ioctl(fd, UI_SET_EVBIT, EV_SYN) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_EVBIT EV_SYN");

	if (ioctl(fd, UI_SET_PROPBIT, INPUT_PROP_DIRECT) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_PROPBIT INPUT_PROP_DIRECT");

	// enable buttons
	if (ioctl(fd, UI_SET_EVBIT, EV_KEY) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_EVBIT EV_KEY");
	if (ioctl(fd, UI_SET_KEYBIT, BTN_LEFT) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_KEYBIT BTN_LEFT");

	// setup sending timestamps
	if (ioctl(fd, UI_SET_EVBIT, EV_MSC) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_EVBIT EV_MSC");
	if (ioctl(fd, UI_SET_MSCBIT, MSC_TIMESTAMP) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_MSCBIT MSC_TIMESTAMP");

	if (ioctl(fd, UI_SET_EVBIT, EV_ABS) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_EVBIT EV_ABS");

	setup_abs(fd, ABS_X, 0, ABS_MAXVAL, 0, err);
	OK_OR_ABORT(err);
	setup_abs(fd, ABS_Y, 0, ABS_MAXVAL, 0, err);
	OK_OR_ABORT(err);

	setup(fd, name, err);
	OK_OR_ABORT(err);

	if (ioctl(fd, UI_DEV_CREATE) < 0)
		ERROR(err, 1, "error: ioctl");
}

void init_stylus(int fd, const char* name, Error* err)
{
	// enable synchronization
	if (ioctl(fd, UI_SET_EVBIT, EV_SYN) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_EVBIT EV_SYN");

	if (ioctl(fd, UI_SET_PROPBIT, INPUT_PROP_DIRECT) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_PROPBIT INPUT_PROP_DIRECT");

	// enable buttons
	if (ioctl(fd, UI_SET_EVBIT, EV_KEY) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_EVBIT EV_KEY");
	if (ioctl(fd, UI_SET_KEYBIT, BTN_TOOL_PEN) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_KEYBIT BTN_TOOL_PEN");
	if (ioctl(fd, UI_SET_KEYBIT, BTN_TOOL_RUBBER) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_KEYBIT BTN_TOOL_RUBBER");
	if (ioctl(fd, UI_SET_KEYBIT, BTN_TOUCH) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_KEYBIT BTN_TOUCH");

	// setup sending timestamps
	if (ioctl(fd, UI_SET_EVBIT, EV_MSC) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_EVBIT EV_MSC");
	if (ioctl(fd, UI_SET_MSCBIT, MSC_TIMESTAMP) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_MSCBIT MSC_TIMESTAMP");

	if (ioctl(fd, UI_SET_EVBIT, EV_ABS) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_EVBIT EV_ABS");

	setup_abs(fd, ABS_X, 0, ABS_MAXVAL, 12, err);
	OK_OR_ABORT(err);
	setup_abs(fd, ABS_Y, 0, ABS_MAXVAL, 12, err);
	OK_OR_ABORT(err);
	setup_abs(fd, ABS_PRESSURE, 0, ABS_MAXVAL, 12, err);
	OK_OR_ABORT(err);
	setup_abs(fd, ABS_TILT_X, -90, 90, 12, err);
	OK_OR_ABORT(err);
	setup_abs(fd, ABS_TILT_Y, -90, 90, 12, err);
	OK_OR_ABORT(err);

	setup(fd, name, err);
	OK_OR_ABORT(err);

	if (ioctl(fd, UI_DEV_CREATE) < 0)
		ERROR(err, 1, "error: ioctl");
}

void init_touch(int fd, const char* name, Error* err)
{
	// enable synchronization
	if (ioctl(fd, UI_SET_EVBIT, EV_SYN) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_EVBIT EV_SYN");

	if (ioctl(fd, UI_SET_PROPBIT, INPUT_PROP_DIRECT) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_PROPBIT INPUT_PROP_DIRECT");

	if (ioctl(fd, UI_SET_EVBIT, EV_KEY) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_EVBIT EV_KEY");
	if (ioctl(fd, UI_SET_KEYBIT, BTN_TOUCH) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_KEYBIT");
	if (ioctl(fd, UI_SET_KEYBIT, BTN_TOOL_FINGER) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_KEYBIT");
	if (ioctl(fd, UI_SET_KEYBIT, BTN_TOOL_DOUBLETAP) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_KEYBIT");
	if (ioctl(fd, UI_SET_KEYBIT, BTN_TOOL_TRIPLETAP) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_KEYBIT");
	if (ioctl(fd, UI_SET_KEYBIT, BTN_TOOL_QUADTAP) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_KEYBIT");
	if (ioctl(fd, UI_SET_KEYBIT, BTN_TOOL_QUINTTAP) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_KEYBIT");

	// setup sending timestamps
	if (ioctl(fd, UI_SET_EVBIT, EV_MSC) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_EVBIT EV_MSC");
	if (ioctl(fd, UI_SET_MSCBIT, MSC_TIMESTAMP) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_MSCBIT MSC_TIMESTAMP");

	if (ioctl(fd, UI_SET_EVBIT, EV_ABS) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_EVBIT EV_ABS");

	setup_abs(fd, ABS_X, 0, ABS_MAXVAL, 200, err);
	OK_OR_ABORT(err);
	setup_abs(fd, ABS_Y, 0, ABS_MAXVAL, 200, err);
	OK_OR_ABORT(err);

	// 5 fingers 5 multitouch slots.
	setup_abs(fd, ABS_MT_SLOT, 0, 4, 0, err);
	OK_OR_ABORT(err);
	setup_abs(fd, ABS_MT_TRACKING_ID, 0, 4, 0, err);
	OK_OR_ABORT(err);
	setup_abs(fd, ABS_MT_POSITION_X, 0, ABS_MAXVAL, 200, err);
	OK_OR_ABORT(err);
	setup_abs(fd, ABS_MT_POSITION_Y, 0, ABS_MAXVAL, 200, err);
	OK_OR_ABORT(err);
	setup_abs(fd, ABS_MT_PRESSURE, 0, ABS_MAXVAL, 0, err);
	OK_OR_ABORT(err);
	setup_abs(fd, ABS_MT_TOUCH_MAJOR, 0, ABS_MAXVAL, 12, err);
	OK_OR_ABORT(err);
	setup_abs(fd, ABS_MT_TOUCH_MINOR, 0, ABS_MAXVAL, 12, err);
	OK_OR_ABORT(err);
	// PointerEvent only gives partial orientation of the touch ellipse
	setup_abs(fd, ABS_MT_ORIENTATION, 0, 1, 0, err);
	OK_OR_ABORT(err);

	setup(fd, name, err);
	OK_OR_ABORT(err);

	if (ioctl(fd, UI_DEV_CREATE) < 0)
		ERROR(err, 1, "error: ioctl");
}

int init_uinput_keyboard(const char* name, Error* err)
{
	int device;

	if ((device = open("/dev/uinput", O_WRONLY | O_NONBLOCK)) < 0)
		fill_error(err, 101, "error: failed to open /dev/uinput");
	else
	{
		init_keyboard(device, name, err);
	}
	return device;
}

int init_uinput_stylus(const char* name, Error* err)
{
	int device;

	if ((device = open("/dev/uinput", O_WRONLY | O_NONBLOCK)) < 0)
		fill_error(err, 101, "error: failed to open /dev/uinput");
	else
	{
		init_stylus(device, name, err);
	}
	return device;
}

int init_uinput_mouse(const char* name, Error* err)
{
	int device;

	if ((device = open("/dev/uinput", O_WRONLY | O_NONBLOCK)) < 0)
		fill_error(err, 101, "error: failed to open /dev/uinput");
	else
	{
		init_mouse(device, name, err);
	}
	return device;
}

int init_uinput_touch(const char* name, Error* err)
{
	int device;

	if ((device = open("/dev/uinput", O_WRONLY | O_NONBLOCK)) < 0)
		fill_error(err, 101, "error: failed to open /dev/uinput");
	else
	{
		init_touch(device, name, err);
	}
	return device;
}

void destroy_uinput_device(int fd)
{
	ioctl(fd, UI_DEV_DESTROY);
	close(fd);
}

void send_uinput_event(int device, int type, int code, int value, Error* err)
{
	struct input_event ev;
	ev.type = type;
	ev.code = code;
	ev.value = value;
	if (write(device, &ev, sizeof(ev)) < 0)
		ERROR(err, 1, "error writing to device, filedescriptor: %d)", device);
}
