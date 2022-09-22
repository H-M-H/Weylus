#!/usr/bin/env bash

set -x
for d in ffmpeg x264 nv-codec-headers libva; do
    test -d "$d" || continue
    (cd "$d" && git clean -dfx && git reset --hard HEAD)
done
