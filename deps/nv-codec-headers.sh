#!/usr/bin/env bash

set -ex

cd nv-codec-headers
make PREFIX=../dist
make install PREFIX=../dist
