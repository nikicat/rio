#!/bin/sh
# Catch macOS-only compile/lint breakage from a Linux box, before it reaches
# CI's `macos-latest` runner (the only job that compiles cfg(macos) code, so
# failures there otherwise surface ~20 min late).
#
# This works without a macOS SDK or osxcross because the macOS bindings here
# (rio-notifier -> objc2) are pure Rust: rustc type-checks the cfg(macos) path
# when targeting apple-darwin, and type/trait errors (the common breakage) are
# caught before any linking. Scoped to rio-notifier on purpose -- rio-window
# can't be cross-checked this way (its build.rs bindgens the macOS `dispatch`
# SDK headers, which aren't present on Linux).
#
# Mirrors CI's lint gate (`clippy --all-targets --all-features -- -D warnings`).
# Skips cleanly when the target toolchain isn't installed.

set -eu

TARGET=aarch64-apple-darwin

if ! command -v rustup >/dev/null 2>&1; then
    echo "skip: rustup not found"
    exit 0
fi

if ! rustup target list --installed 2>/dev/null | grep -qx "$TARGET"; then
    echo "skip: $TARGET not installed (run: rustup target add $TARGET)"
    exit 0
fi

exec cargo clippy -p rio-notifier --target "$TARGET" \
    --all-targets --all-features -- -D warnings
