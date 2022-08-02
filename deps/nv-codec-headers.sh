#!/usr/bin/env bash

set -ex

cd nv-codec-headers
make PREFIX="$DIST"
make install PREFIX="$DIST"
