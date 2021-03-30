#!/usr/bin/env bash

set -ex
if [ "$OSTYPE" == "linux-gnu" ]; then
    export RUNNER_OS="Linux"
fi

if [[ "$OSTYPE" == "darwin"* ]]; then
    export RUNNER_OS="macOS"
fi

export NPROCS=$(nproc || echo 4)

./download.sh

if [ "$RUNNER_OS" == "Windows" ]; then
    export CC=cl
    export FFMPEG_EXTRA_ARGS="--toolchain=msvc"
    export FFMPEG_CFLAGS="-I../dist/include"
    export FFMPEG_LIBRARY_PATH="-LIBPATH:../dist/lib"
else
    export FFMPEG_CFLAGS="-I../dist/include"
    export FFMPEG_LIBRARY_PATH="-L../dist/lib"
    if [ "$RUNNER_OS" == "Linux" ]; then
        export FFMPEG_EXTRA_ARGS="--enable-nvenc \
            --enable-ffnvcodec \
            --enable-vaapi \
            --enable-libdrm \
            --enable-xlib"
    fi
    if [ "$RUNNER_OS" == "macOS" ]; then
        export FFMPEG_EXTRA_ARGS="--enable-videotoolbox"
    fi
    if [ "$RUNNER_OS" == "Windows" ]; then
        export FFMPEG_EXTRA_ARGS="--enable-nvenc --enable-ffnvcodec"
    fi
fi

./x264.sh
if [ "$RUNNER_OS" == "Linux" ]; then
    ./nv-codec-headers.sh
    ./libva.sh
fi
if [ "$RUNNER_OS" == "Windows" ]; then
    ./nv-codec-headers.sh
fi
./ffmpeg.sh

if [ "$RUNNER_OS" == "Windows" ]; then
    cd dist/lib
    for l in *.a; do
        d=${l#lib}
        cp "$l" "${d%.a}.lib"
    done
    cp libx264.lib x264.lib
fi
