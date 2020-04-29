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

#include "error.h"

void setup_abs(int fd, int code, int minimum, int maximum, Error* err)
{
	struct uinput_abs_setup abs_setup;

	memset(&abs_setup, 0, sizeof(abs_setup));
	abs_setup.code = code;
	abs_setup.absinfo.value = 0;
	abs_setup.absinfo.minimum = minimum;
	abs_setup.absinfo.maximum = maximum;
	abs_setup.absinfo.fuzz = 0;
	abs_setup.absinfo.flat = 0;
	// units/mm
	abs_setup.absinfo.resolution = 12;
	if (ioctl(fd, UI_ABS_SETUP, &abs_setup) < 0)
		ERROR(err, 1, "error: UI_ABS_SETUP, code: %x", code);
}

void setup(int fd, const char* name, int product, Error* err)
{

	struct uinput_setup setup;
	memset(&setup, 0, sizeof(setup));
	strncpy(setup.name, name, UINPUT_MAX_NAME_SIZE);
	setup.id.bustype = BUS_VIRTUAL;
	setup.id.vendor = 0x1;
	setup.id.product = product;
	setup.id.version = 1;
	setup.ff_effects_max = 0;
	if (ioctl(fd, UI_DEV_SETUP, &setup) < 0)
		ERROR(err, 1, "error: UI_DEV_SETUP");
}

void init_pointer(int fd, Error* err)
{
	// enable synchronization
	if (ioctl(fd, UI_SET_EVBIT, EV_SYN) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_EVBIT EV_SYN");

	// enable 1 button
	if (ioctl(fd, UI_SET_EVBIT, EV_KEY) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_EVBIT EV_KEY");
	if (ioctl(fd, UI_SET_KEYBIT, BTN_LEFT) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_KEYBIT");
	if (ioctl(fd, UI_SET_KEYBIT, BTN_TOUCH) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_KEYBIT");
	if (ioctl(fd, UI_SET_KEYBIT, BTN_TOOL_PEN) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_KEYBIT");
	if (ioctl(fd, UI_SET_KEYBIT, BTN_STYLUS) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_KEYBIT");
	if (ioctl(fd, UI_SET_KEYBIT, BTN_STYLUS2) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_KEYBIT");

	// enable 2 main axes + pressure (absolute positioning)
	if (ioctl(fd, UI_SET_EVBIT, EV_ABS) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_EVBIT EV_ABS");
	if (ioctl(fd, UI_SET_ABSBIT, ABS_X) < 0)
		ERROR(err, 1, "error: ioctl UI_SETEVBIT ABS_X");
	if (ioctl(fd, UI_SET_ABSBIT, ABS_Y) < 0)
		ERROR(err, 1, "error: ioctl UI_SETEVBIT ABS_Y");
	if (ioctl(fd, UI_SET_ABSBIT, ABS_PRESSURE) < 0)
		ERROR(err, 1, "error: ioctl UI_SETEVBIT ABS_PRESSURE");

	setup_abs(fd, ABS_X, 0, UINT16_MAX, err);
	OK_OR_ABORT(err);
	setup_abs(fd, ABS_Y, 0, UINT16_MAX, err);
	OK_OR_ABORT(err);
	setup_abs(fd, ABS_PRESSURE, 0, UINT16_MAX, err);
	OK_OR_ABORT(err);
	setup_abs(fd, ABS_TILT_X, -90, 90, err);
	OK_OR_ABORT(err);
	setup_abs(fd, ABS_TILT_Y, -90, 90, err);
	OK_OR_ABORT(err);

	setup(fd, "WebTabletPointer", 1, err);
	OK_OR_ABORT(err);

	if (ioctl(fd, UI_DEV_CREATE) < 0)
		ERROR(err, 1, "error: ioctl");
}

void init_multitouch(int fd, Error* err)
{
	// enable synchronization
	if (ioctl(fd, UI_SET_EVBIT, EV_SYN) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_EVBIT EV_SYN");

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


	if (ioctl(fd, UI_SET_EVBIT, EV_ABS) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_EVBIT EV_ABS");

	if (ioctl(fd, UI_SET_ABSBIT, ABS_X) < 0)
		ERROR(err, 1, "error: ioctl UI_SETEVBIT ABS_X");
	if (ioctl(fd, UI_SET_ABSBIT, ABS_Y) < 0)
		ERROR(err, 1, "error: ioctl UI_SETEVBIT ABS_Y");
	if (ioctl(fd, UI_SET_ABSBIT, ABS_MT_SLOT) < 0)
		ERROR(err, 1, "error: ioctl UI_SETEVBIT ABS_MT_SLOT");
	if (ioctl(fd, UI_SET_ABSBIT, ABS_MT_TRACKING_ID) < 0)
		ERROR(err, 1, "error: ioctl UI_SETEVBIT ABS_MT_TRACKING_ID");
	if (ioctl(fd, UI_SET_ABSBIT, ABS_MT_POSITION_X) < 0)
		ERROR(err, 1, "error: ioctl UI_SETEVBIT ABS_MT_POSITION_X");
	if (ioctl(fd, UI_SET_ABSBIT, ABS_MT_POSITION_Y) < 0)
		ERROR(err, 1, "error: ioctl UI_SETEVBIT ABS_MT_POSITION_Y");
	if (ioctl(fd, UI_SET_ABSBIT, ABS_MT_PRESSURE) < 0)
		ERROR(err, 1, "error: ioctl UI_SETEVBIT ABS_MT_PRESSURE");
	if (ioctl(fd, UI_SET_ABSBIT, ABS_MT_TOUCH_MAJOR) < 0)
		ERROR(err, 1, "error: ioctl UI_SETEVBIT ABS_MT_TOUCH_MAJOR");
	if (ioctl(fd, UI_SET_ABSBIT, ABS_MT_TOUCH_MINOR) < 0)
		ERROR(err, 1, "error: ioctl UI_SETEVBIT ABS_MT_TOUCH_MINOR");
	if (ioctl(fd, UI_SET_ABSBIT, ABS_MT_ORIENTATION) < 0)
		ERROR(err, 1, "error: ioctl UI_SETEVBIT ABS_MT_ORIENTATION");

	// 5 fingers 5 multitouch slots.
	setup_abs(fd, ABS_MT_SLOT, 0, 4, err);
	OK_OR_ABORT(err);
	setup_abs(fd, ABS_MT_TRACKING_ID, 0, 4, err);
	OK_OR_ABORT(err);
	setup_abs(fd, ABS_MT_POSITION_X, 0, UINT16_MAX, err);
	OK_OR_ABORT(err);
	setup_abs(fd, ABS_MT_POSITION_Y, 0, UINT16_MAX, err);
	OK_OR_ABORT(err);
	setup_abs(fd, ABS_MT_PRESSURE, 0, UINT16_MAX, err);
	OK_OR_ABORT(err);
	setup_abs(fd, ABS_MT_TOUCH_MAJOR, 0, UINT16_MAX, err);
	OK_OR_ABORT(err);
	setup_abs(fd, ABS_MT_TOUCH_MINOR, 0, UINT16_MAX, err);
	OK_OR_ABORT(err);
	// PointerEvent only gives partial orientation of the touch ellipse
	setup_abs(fd, ABS_MT_ORIENTATION, 0, 1, err);
	OK_OR_ABORT(err);

	setup(fd, "WebTabletMultiTouch", 2, err);
	OK_OR_ABORT(err);

	if (ioctl(fd, UI_DEV_CREATE) < 0)
		ERROR(err, 1, "error: ioctl");
}

int init_uinput_pointer(Error* err)
{
	int device;

	if ((device = open("/dev/uinput", O_WRONLY | O_NONBLOCK)) < 0)
		fill_error(err, 1, "error: open");
	else
	{
		init_pointer(device, err);
	}
	return device;
}

int init_uinput_multitouch(Error* err)
{
	int device;

	if ((device = open("/dev/uinput", O_WRONLY | O_NONBLOCK)) < 0)
		fill_error(err, 1, "error: open");
	else
	{
		init_multitouch(device, err);
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
