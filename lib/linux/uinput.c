#include <linux/input-event-codes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <signal.h>
#include <unistd.h>
#include <fcntl.h>
#include <errno.h>
#include <limits.h>
#include <linux/input.h>
#include <linux/uinput.h>
#include <stdint.h>

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
	abs_setup.absinfo.resolution = 1;
	if (ioctl(fd, UI_ABS_SETUP, &abs_setup) < 0)
	  ERROR(err, 1, "error: UI_ABS_SETUP, code: %x", code);
}

void setup(int fd, Error* err)
{

	struct uinput_setup setup;
	memset(&setup, 0, sizeof(setup));
	snprintf(setup.name, UINPUT_MAX_NAME_SIZE, "Network Tablet");
	setup.id.bustype = BUS_VIRTUAL;
	setup.id.vendor  = 0x1;
	setup.id.product = 0x1;
	setup.id.version = 2;
	setup.ff_effects_max = 0;
	if (ioctl(fd, UI_DEV_SETUP, &setup) < 0)
	  ERROR(err, 1, "error: UI_DEV_SETUP");
}

void init_device(int fd, Error* err)
{
	// enable synchronization
	if (ioctl(fd, UI_SET_EVBIT, EV_SYN) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_EVBIT EV_SYN");

	// enable 1 button
	if (ioctl(fd, UI_SET_EVBIT, EV_KEY) < 0)
		ERROR(err, 1, "error: ioctl UI_SET_EVBIT EV_KEY");
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
	setup(fd, err);
	OK_OR_ABORT(err);


	if (ioctl(fd, UI_DEV_CREATE) < 0)
	  ERROR(err, 1, "error: ioctl");
}

int init_uinput(Error* err)
{
	int device;

	if ((device = open("/dev/uinput", O_WRONLY | O_NONBLOCK)) < 0)
		fill_error(err, 1, "error: open");
	else
	{
		init_device(device, err);
	}
	return device;
}

void destroy_uinput(int fd)
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


/*void run(void)
{
	int device;

	if ((device = open("/dev/uinput", O_WRONLY | O_NONBLOCK)) < 0)
		ERROR(err, 1, "error: open");

	init_device(device);
    sleep(1);

    int x = 50000;
    int y = 50;
    int pressure = 0;
	send_event(device, EV_ABS, ABS_X, x);
	send_event(device, EV_ABS, ABS_Y, y);
	send_event(device, EV_ABS, ABS_PRESSURE, pressure);

    char button = 2;
    char down = 1;

    for (int i = 0; i < 100; i++)
	switch (1) {
		case 1:
			send_event(device, EV_SYN, SYN_REPORT, 1);
			break;
		case 2:
			// stylus hovering
			if (button == -1)
				send_event(device, EV_KEY, BTN_TOOL_PEN, down);
			// stylus touching
			if (button == 0)
				send_event(device, EV_KEY, BTN_TOUCH, down);
			// button 1
			if (button == 1)
				send_event(device, EV_KEY, BTN_STYLUS, down);
			// button 2
			if (button == 2)
				send_event(device, EV_KEY, BTN_STYLUS2, down);
			printf("sent button: %hhi, %hhu\n", button, down);
			send_event(device, EV_SYN, SYN_REPORT, 1);
    }
    sleep(1);

	printf("Removing network tablet from device list\n");

	printf("Tablet driver shut down gracefully\n");
}*/
