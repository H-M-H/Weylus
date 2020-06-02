#!/usr/bin/env bash

set -ex

./download.sh

if [ "$RUNNER_OS" == "Windows" ]; then
	export CC=cl
	export FFMPEG_EXTRA_ARGS="--toolchain=msvc"
	export FFMPEG_CFLAGS="-I../dist/include"
	export FFMPEG_LIBRARY_PATH="-LIBPATH:../dist/lib"
else
	export FFMPEG_CFLAGS="-I../dist/include"
	export FFMPEG_LIBRARY_PATH="-L../dist/lib"
fi

./x264.sh
./ffmpeg.sh
