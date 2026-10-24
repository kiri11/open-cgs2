#!/bin/bash
# Builds the device binaries (aarch64, static musl) and packs them with the scripts and the screen
# into dist/bundle.tar, the files the installer puts on a device (see installer/build.rs).
# Needs rustup with the aarch64-unknown-linux-musl target; runs on Linux, macOS or WSL.
set -euo pipefail
cd "$(dirname "$0")/.."
export OPEN_CGS2_VERSION=${OPEN_CGS2_VERSION:-dev}
cargo build --release --target aarch64-unknown-linux-musl -p panel -p sensord
(cd adbd-auth && cargo build --release --target aarch64-unknown-linux-musl)
rm -rf dist/device
mkdir -p dist
cp -r device dist/device
cp target/aarch64-unknown-linux-musl/release/panel target/aarch64-unknown-linux-musl/release/sensord dist/device/open-cgs2/
cp adbd-auth/target/aarch64-unknown-linux-musl/release/libadbd_auth.so dist/device/open-cgs2/adbd-auth.so
echo "$OPEN_CGS2_VERSION" > dist/device/open-cgs2/VERSION
tar -C dist/device -cf dist/bundle.tar .
echo "dist/bundle.tar ($OPEN_CGS2_VERSION)"
