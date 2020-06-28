#!/usr/bin/env bash

set -ex

cd libva

# required to make ffmpeg's configure work
sed -i -e "s/-lva$/-lva -ldl/" pkgconfig/libva.pc.in

./autogen.sh --prefix=$(readlink -f ../dist) \
    --enable-static=yes \
    --enable-drm \
    --enable-x11 \
    --enable-wayland \
    --enable-glx \
    --enable-shared=no \
    --with-drivers-path="/usr/lib/dri"

make -j$(nproc)
make install
