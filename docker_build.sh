#!/usr/bin/env sh

set -ex

# install JS build dependencies (esbuild for TypeScript compilation)
npm install

# cross compile windows version
cargo build --target x86_64-pc-windows-gnu --release

# cleanup cross compiled windows artifacts
(cd deps && ./clean.sh)

# build linux versions
cargo deb --package rylus-server -- --features=va-static

# check if installing works
dpkg -i target/debian/Rylus*.deb
cp target/release/rylus target/release/rylus_va_static

# build version with dynamic libva
cargo build --release

mkdir packages

PKGDIR="$PWD/packages"

# package windows
(
  cd target/x86_64-pc-windows-gnu/release/
  zip rylus-windows.zip rylus.exe
  mv rylus-windows.zip "$PKGDIR/"
)

# package linux
(
  cp target/debian/Rylus*.deb "$PKGDIR/"
  cp packaging/rylus.desktop target/release/
  cd target/release/
  zip rylus-linux.zip rylus rylus_va_static rylus.desktop
  mv rylus-linux.zip "$PKGDIR/"
)
