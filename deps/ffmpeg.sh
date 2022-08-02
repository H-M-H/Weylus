#!/usr/bin/env bash

set -ex

cd ffmpeg
PKG_CONFIG_PATH="$DIST/lib/pkgconfig" ./configure \
	--prefix="$DIST" \
	--disable-debug \
	--enable-static \
	--disable-shared \
	--enable-pic \
	--enable-stripping \
	--disable-programs \
	--enable-gpl \
	--enable-libx264 \
	--disable-autodetect \
	--extra-cflags="$FFMPEG_CFLAGS" \
	--extra-ldflags="$FFMPEG_LIBRARY_PATH" \
	$FFMPEG_EXTRA_ARGS

make -j$NPROCS
make install
