#!/usr/bin/env bash

set -ex

./download.sh
./x264.sh
./ffmpeg.sh

if [ "$RUNNER_OS" == "Windows" ]; then
    cd dist/lib
    for l in *.a; do
        cp "$l" "${${l#lib}%.a}.lib"
    done
fi
