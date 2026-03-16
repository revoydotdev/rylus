#!/usr/bin/env sh

set -ex

rm -f docker/archive.tar.gz
git ls-files | tar Tczf - docker/archive.tar.gz

podman run --replace -d --name rylus_build hhmhh/weylus_build:latest sleep infinity
podman cp docker/archive.tar.gz rylus_build:/
podman exec rylus_build sh -c "mkdir /rylus && tar xf archive.tar.gz --directory=/rylus && cd rylus && ./docker_build.sh"

podman run --replace -d --name rylus_build_alpine hhmhh/weylus_build_alpine:latest sleep infinity
podman cp docker/archive.tar.gz rylus_build_alpine:/
podman exec rylus_build_alpine sh -c "mkdir /rylus && tar xf archive.tar.gz --directory=/rylus && cd rylus && RUSTFLAGS='-C target-feature=-crt-static' cargo build --release"
