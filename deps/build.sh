#!/usr/bin/env bash

set -ex

./download.sh
./x264.sh
./ffmpeg.sh
