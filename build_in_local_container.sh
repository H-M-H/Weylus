#!/usr/bin/env sh

set -ex

rm -f docker/archive.tar.gz
git ls-files | tar Tczf - docker/archive.tar.gz

podman run --replace -d --name weylus_build hhmhh/weylus_build:latest sleep infinity
podman cp docker/archive.tar.gz weylus_build:/
podman exec weylus_build sh -c "mkdir /weylus && tar xf archive.tar.gz --directory=/weylus && cd weylus && ./docker_build.sh"

podman run --replace -d --name weylus_build_alpine hhmhh/weylus_build_alpine:latest sleep infinity
podman cp docker/archive.tar.gz weylus_build_alpine:/
podman exec weylus_build_alpine sh -c "mkdir /weylus && tar xf archive.tar.gz --directory=/weylus && cd weylus && RUSTFLAGS='-C target-feature=-crt-static' cargo build --release"
