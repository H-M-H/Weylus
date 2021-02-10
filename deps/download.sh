#!/usr/bin/env bash

set -ex

rm -rf x264 ffmpeg
git clone -b stable https://code.videolan.org/videolan/x264.git x264
git clone -b n4.3 https://git.ffmpeg.org/ffmpeg.git ffmpeg
if [ "$RUNNER_OS" == "Linux" ]; then
    git clone https://git.videolan.org/git/ffmpeg/nv-codec-headers.git
    git clone -b 2.10.0 https://github.com/intel/libva
fi
cd ffmpeg

if [ "$RUNNER_OS" == "Windows" ]; then
    git apply ../command_limit.patch
    git apply ../awk.patch
    git apply ../ffmpeg-x264-static.patch
fi
