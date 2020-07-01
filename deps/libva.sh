#!/usr/bin/env bash

set -ex

cd libva

# required to make ffmpeg's configure work
sed -i -e "s/-lva$/-lva -ldrm -ldl/" pkgconfig/libva.pc.in
sed -i -e 's/-lva-\${display}$/-lva-\${display} -lX11 -lXext -lXfixes -ldrm/' pkgconfig/libva-x11.pc.in
sed -i -e 's/-lva-\${display}$/-lva-\${display} -ldrm/' pkgconfig/libva-drm.pc.in

./autogen.sh --prefix=$(readlink -f ../dist) \
    --enable-static=yes \
    --enable-drm \
    --enable-x11 \
    --enable-glx \
    --enable-shared=no \
    --with-drivers-path="/usr/lib/dri"

make -j$(nproc)
make install
