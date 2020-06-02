#!/usr/bin/env bash

set -ex

rm -rf x264 ffmpeg
git clone -b stable https://code.videolan.org/videolan/x264.git x264
git clone -b n4.2.3 https://git.ffmpeg.org/ffmpeg.git ffmpeg
cd ffmpeg

if [ "$RUNNER_OS" == "Windows" ]; then
	git apply ../command_limit.patch
	git apply ../awk.patch
fi
