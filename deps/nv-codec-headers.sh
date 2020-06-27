#!/usr/bin/env bash

set -ex

cd nv-codec-headers
make
make install PREFIX=../dist
