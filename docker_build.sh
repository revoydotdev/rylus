#!/usr/bin/env sh

set -ex

# install JS build dependencies (esbuild for TypeScript compilation)
npm install

# build linux release
cargo build --release

# build .deb package
cargo deb --package rylus-server

# check if installing works (skip on non-Debian systems)
if command -v dpkg >/dev/null 2>&1; then
  sudo dpkg -i target/debian/rylus*.deb
fi

mkdir -p packages

PKGDIR="$PWD/packages"

# package linux
(
  cp target/debian/rylus*.deb "$PKGDIR/"
  cp packaging/rylus.desktop target/release/
  cd target/release/
  zip rylus-linux.zip rylus rylus.desktop
  mv rylus-linux.zip "$PKGDIR/"
)
