#!/usr/bin/env bash

set -ex

rm -rf x264 ffmpeg
git clone --depth 1 -b stable https://code.videolan.org/videolan/x264.git x264
git clone --depth 1 -b n4.4 https://git.ffmpeg.org/ffmpeg.git ffmpeg
if [ "$RUNNER_OS" == "Linux" ]; then
    git clone --depth 1 https://git.videolan.org/git/ffmpeg/nv-codec-headers.git
    git clone --depth 1 -b 2.11.0 https://github.com/intel/libva
fi
if [ "$RUNNER_OS" == "Windows" ]; then
    git clone --depth 1 https://git.videolan.org/git/ffmpeg/nv-codec-headers.git
fi
cd ffmpeg

if [ "$RUNNER_OS" == "Windows" ]; then
    git apply ../command_limit.patch
    git apply ../awk.patch
    git apply ../ffmpeg-x264-static.patch
fi
