#!/usr/bin/env bash

set -ex

export TARGET_OS="$CARGO_CFG_TARGET_OS"

if [ "$OSTYPE" == "linux-gnu" ]; then
    export HOST_OS="linux"
fi

if [[ "$OSTYPE" == "darwin"* ]]; then
    export HOST_OS="macos"
fi

if [ "$OS" == "Windows_NT" ]; then
    export HOST_OS="windows"
fi

[ -z "$TARGET_OS" ] && export TARGET_OS="$HOST_OS"

export NPROCS="$(nproc || echo 4)"

./download.sh

if [ "$TARGET_OS" == "windows" ]; then
    if [ "$HOST_OS" == "linux" ]; then
        export CROSS_COMPILE="x86_64-w64-mingw32-"
        export FFMPEG_EXTRA_ARGS="--arch=x86_64 --target-os=mingw64 --cross-prefix=x86_64-w64-mingw32- \
            --enable-nvenc --enable-ffnvcodec --enable-mediafoundation --pkg-config=pkg-config"
        export FFMPEG_CFLAGS="-I../dist/include"
        export FFMPEG_LIBRARY_PATH="-L../dist/lib"
    else
        export CC="cl"
        export FFMPEG_EXTRA_ARGS="--toolchain=msvc --enable-nvenc --enable-ffnvcodec \
            --enable-mediafoundation"
        export FFMPEG_CFLAGS="-I../dist/include"
        export FFMPEG_LIBRARY_PATH="-LIBPATH:../dist/lib"
    fi
else
    export FFMPEG_CFLAGS="-I../dist/include"
    export FFMPEG_LIBRARY_PATH="-L../dist/lib"
    if [ "$TARGET_OS" == "linux" ]; then
        export FFMPEG_EXTRA_ARGS="--enable-nvenc \
            --enable-ffnvcodec \
            --enable-vaapi \
            --enable-libdrm \
            --enable-xlib"
    fi
    if [ "$TARGET_OS" == "macos" ]; then
        export FFMPEG_EXTRA_ARGS="--enable-videotoolbox"
    fi
fi

if [ "$TARGET_OS" == "windows" ] && [ "$HOST_OS" == "linux" ]; then
    export X264_EXTRA_ARGS="--cross-prefix=x86_64-w64-mingw32- --host=x86_64-w64-mingw32"
fi
./x264.sh
if [ "$TARGET_OS" == "linux" ]; then
    ./nv-codec-headers.sh
    ./libva.sh
fi
if [ "$TARGET_OS" == "windows" ]; then
    ./nv-codec-headers.sh
fi
./ffmpeg.sh

if [ "$TARGET_OS" == "windows" ] && [ "$HOST_OS" == "windows" ]; then
    cd dist/lib
    for l in *.a; do
        d=${l#lib}
        cp "$l" "${d%.a}.lib"
    done
    cp libx264.lib x264.lib
fi
