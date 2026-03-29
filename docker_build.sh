#!/usr/bin/env sh

set -ex

# install JS build dependencies (esbuild for TypeScript compilation)
npm install

# build linux release
cargo build --release

# build .deb package
cargo deb --package rylus-server

# check if installing works
sudo dpkg -i target/debian/Rylus*.deb

mkdir -p packages

PKGDIR="$PWD/packages"

# package linux
(
  cp target/debian/Rylus*.deb "$PKGDIR/"
  cp packaging/rylus.desktop target/release/
  cd target/release/
  zip rylus-linux.zip rylus rylus.desktop
  mv rylus-linux.zip "$PKGDIR/"
)
