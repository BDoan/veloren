#!/bin/bash
# Compile the Android native library (arm64) with cargo-ndk.
# Needs: the Rust toolchain from rust-toolchain with the aarch64-linux-android
# target, cargo-ndk, and ANDROID_NDK_HOME pointing at NDK r27.
set -euo pipefail
cd "$(dirname "$0")/.."
: "${ANDROID_NDK_HOME:?set ANDROID_NDK_HOME to an NDK r27 install}"
# 16 KB page alignment so the library loads on 16 KB-page Android builds too.
export CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS="-C link-arg=-Wl,-z,max-page-size=16384"
export CARGO_INCREMENTAL=0
cargo ndk -t arm64-v8a -P 30 build --profile android -p veloren-voxygen-android "$@"
