# Weylus
![Build](https://github.com/H-M-H/Weylus/workflows/Build/badge.svg)

Weylus turns your tablet or smart phone into a graphic tablet/touch screen for your computer!

Weylus in action with [Xournal++](https://github.com/xournalpp/xournalpp):

![Weylus in action](In_action.gif)

## Features
- Control your mouse with your tablet
- Mirror your screen to your tablet

The above features are available on all Operating Systems but Weylus works best on Linux. Additional
features on Linux are:
- Support for a stylus/pen (supports pressure and tilt)
- Multi-touch: Try it with software that supports multi-touch, like Krita, and see for yourself!
- Capturing specific windows and only drawing to them
- Faster screen mirroring

## Installation
Just grab the latest release for your OS from the
[releases page](https://github.com/H-M-H/Weylus/releases) and install it on your computer. No apps
except a modern browser are required on your tablet.

## Running
Start Weylus, preferably set a password in the password box and press the Start button. This will
start a webserver running on your computer. To control your computer with your tablet you need to
open the url `http://<address of your computer>:<port set in the menu, default is 1701>`, if
possible Weylus will display to you the url you need to open.

### Linux
**To enable stylus and multi-touch support `/dev/uinput` needs to be writable by Weylus!**
To make `/dev/uinput` permanently writable by your user you can do the following:
```sh
sudo useradd -r -U -s /usr/bin/nologin uinput
sudo usermod -aG uinput $USER
echo 'KERNEL=="uinput", MODE="0660", GROUP="uinput", OPTIONS+="static_node=uinput"' \
| sudo tee /etc/udev/rules.d/60-weylus.rules
sudo udevadm control --reload
sudo udevadm trigger
```

### macOS
Weylus needs some permissions to work properly, make sure you enable:
- Incoming connections
- Screen capturing
- Controlling your desktop

### Windows
I am afraid but as of now Weylus has not been tested on Windows.

---

That is it, start drawing!

## Building
To build Weylus you need to install Rust and Typescript. `cargo build` builds the project.
On windows make sure to run the Typescript compiler in the project root first.
On Linux some additional dependencies are required to build Weylus. On Debian or Ubuntu they can be
installed via:
```
apt-get install -y libx11-dev libxext-dev libxft-dev libxinerama-dev libxcursor-dev libxrender-dev
libxfixes-dev libxtst-dev
```

## How does this work?
### Stylus/Touch
Modern browsers expose so called
[PointerEvents](https://developer.mozilla.org/en-US/docs/Web/API/PointerEvent) that can convey not
only mouse but additionally stylus/pen and touch information. Weylus sets up a webserver with the
corresponding javascript code to capture these events. The events are sent back to the server using
websockets.
Weylus then processes these events using either the generic OS independent backend, which only
supports controlling the mouse or on Linux the uinput backend can be used. It makes use of the
uinput Linux kernel module which supports creating a wide range of input devices including mouse,
stylus and touch input devices.

### Screen mirroring & window capturing
Either the generic backend is used which is less efficient and only captures the whole screen or on
Linux xlib is used to connect to the X-server and do the necessary work of getting window
information and capturing the window/screen. To make things fast the "MIT-SHM - The MIT Shared
Memory Extension" is used to create shared memory images using `XShmCreateImage`. The images
captured are then encoded to the PNG format using the library mtpng for greater speed. Finally the
PNG binary data are encoded to base64 and sent back to the connected browser, which then loads them
to an image tag and draws them to a canvas element. The latencies and loads on my machine are
reasonable but I am open to improvements here.
