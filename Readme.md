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
Weylus uses the `uinput` interface to simulate input events on Linux. **To enable stylus and
multi-touch support `/dev/uinput` needs to be writable by Weylus.** To make `/dev/uinput`
permanently writable by your user, run:
```sh
sudo groupadd -r uinput
sudo usermod -aG uinput $USER
echo 'KERNEL=="uinput", MODE="0660", GROUP="uinput", OPTIONS+="static_node=uinput"' \
| sudo tee /etc/udev/rules.d/60-weylus.rules
```

Then, either reboot, or run

```sh
sudo udevadm control --reload
sudo udevadm trigger
```

then log out and log in again. To undo this, run:

```sh
sudo rm /etc/udev/rules.d/60-weylus.rules
```

This allows your user to synthesize input events system-wide, even when another user is logged in.
Therefore, untrusted users should not be added to the uinput group.

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
To build Weylus you need to install Rust, Typescript, make, git, a C compiler, nasm and bash. `cargo
build` builds the project. On Linux some additional dependencies are required to build Weylus. On
Debian or Ubuntu they can be installed via:
```sh
apt-get install -y libx11-dev libxext-dev libxft-dev libxinerama-dev libxcursor-dev libxrender-dev
libxfixes-dev libxtst-dev
```
Note that building for the first time may take a while as ffmpeg needs to be build. On windows only
msvc is supported as C compiler.

In case you do not want to build ffmpeg and libx264 via the supplied build script you have to create
the directory `deps/dist` yourself and copy static ffmpeg libraries built with support for libx264
and a static version of libx264 into `deps/dist/lib`. Additional `deps/dist/include` needs to be
filled with ffmpeg's include header files. The build script will only try to build ffmpeg if the
directory `deps/dist` does not exist.

### Docker
It is also possible to build the Linux version inside a docker container. The Dockerfile used is
located at [docker/Dockerfile](docker/Dockerfile). This is also how the official release is built.
Building works like
this:
```console
docker run -it hhmhh/weylus_build bash
root@f02164dbfa18:/# git clone https://github.com/H-M-H/Weylus
Cloning into 'Weylus'...
remote: Enumerating objects: 10, done.
remote: Counting objects: 100% (10/10), done.
remote: Compressing objects: 100% (7/7), done.
remote: Total 827 (delta 1), reused 6 (delta 0), pack-reused 817
Receiving objects: 100% (827/827), 5.38 MiB | 7.12 MiB/s, done.
Resolving deltas: 100% (431/431), done.
root@f02164dbfa18:/# cd Weylus/
root@f02164dbfa18:/Weylus# cargo deb
   Compiling
   ...
```
Once the build is finished you can for example copy the binary from the container to your file
system like this:
```sh
docker cp f02164dbfa18:/Weylus/target/release/weylus ~/some/path/weylus
```
The .deb is located at `/Weylus/target/debian/`.  Please note that the container ID will most likely
not be `f02164dbfa18` if you run this yourself, replace it accordingly.

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
captured are then encoded to a video stream using ffmpeg. Fragmented MP4 is used as container format
to enable browsers to play the stream via the Media Source Extensions API. The video codec used is
H.264 as this is widely supported and allows very fast encoding as opposed to formats like AV1. To
minimize dependencies ffmpeg is statically linked into Weylus.

---

[![Packaging status](
https://repology.org/badge/vertical-allrepos/weylus.svg
)](https://repology.org/project/weylus/versions)
