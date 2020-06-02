#!/usr/bin/env bash

set -ex

./download.sh
./x264.sh
./ffmpeg.sh

if [ "$RUNNER_OS" == "Windows" ]; then
    cd dist/lib
    for l in *.a; do
        d=${l#lib}
        cp "$l" "${d%.a}.lib"
    done
fi
