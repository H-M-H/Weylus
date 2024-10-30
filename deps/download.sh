#!/usr/bin/env bash

set -ex

test -d x264 || git clone --depth 1 -b stable https://code.videolan.org/videolan/x264.git x264
test -d ffmpeg || git clone --depth 1 -b n7.1 https://git.ffmpeg.org/ffmpeg.git ffmpeg
if [ "$TARGET_OS" == "linux" ]; then
    test -d nv-codec-headers || git clone --depth 1 https://git.videolan.org/git/ffmpeg/nv-codec-headers.git
    test -d libva || git clone --depth 1 -b 2.22.0 https://github.com/intel/libva
fi
if [ "$TARGET_OS" == "windows" ]; then
    test -d nv-codec-headers || git clone --depth 1 https://git.videolan.org/git/ffmpeg/nv-codec-headers.git
fi

if [ "$TARGET_OS" == "windows" ] && [ "$HOST_OS" == "windows" ]; then
    cd ffmpeg
    git apply ../command_limit.patch
    git apply ../awk.patch
fi
