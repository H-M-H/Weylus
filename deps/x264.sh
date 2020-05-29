#!/usr/bin/env bash

set -ex

cd x264
./configure \
	--prefix=../dist \
	--exec-prefix=../dist \
	--enable-static \
	--enable-pic \
	--enable-strip \
	--disable-cli \
	--disable-opencl

make -j$(nproc)
make install
