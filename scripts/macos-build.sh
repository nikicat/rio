#!/usr/bin/env bash
# Cross-build the full macOS `rio` binary from Linux using osxcross.
#
# This produces a real, linked Mach-O executable (not just a type-check) for the
# Apple-Silicon target CI ships -- handy for reproducing a macOS-only build or
# link failure without a Mac. Requires osxcross with a modern SDK installed
# (>= 10.14 for UserNotifications.framework; an arm64 SDK for the default
# target). On this box that's /usr/lib/osxcross with MacOSX26.1.sdk.
#
#   scripts/macos-build.sh                      # debug build, aarch64-apple-darwin
#   scripts/macos-build.sh --release            # release
#   scripts/macos-build.sh -p rio-notifier      # a single crate
#   TARGET=x86_64-apple-darwin scripts/macos-build.sh
#
# Why this is a script and not a committed Cargo change: `rio-window` gates its
# `dispatch` bindgen on the *host* OS (build scripts compile for the host), and
# declares `bindgen` as a `cfg(target_os="macos")` build-dependency -- which
# Cargo also selects by host. Cross-compiling therefore needs two source tweaks
# that we DON'T want in tree (the bindgen build-dep would then compile on every
# Linux/Windows build). So we apply them transiently and always revert, even on
# failure or Ctrl-C.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

OSXCROSS_DIR="${OSXCROSS_DIR:-/usr/lib/osxcross}"
TARGET="${TARGET:-aarch64-apple-darwin}"

# --- locate osxcross toolchain + SDK -----------------------------------------
if [ ! -d "$OSXCROSS_DIR/bin" ]; then
    echo "error: osxcross not found at $OSXCROSS_DIR (set OSXCROSS_DIR)" >&2
    exit 1
fi

SDK="$(ls -d "$OSXCROSS_DIR"/SDK/MacOSX*.sdk 2>/dev/null | sort -V | tail -1)"
if [ -z "$SDK" ]; then
    echo "error: no MacOSX*.sdk under $OSXCROSS_DIR/SDK" >&2
    exit 1
fi

# osxcross names its wrappers <arch>-apple-darwin<ver>-clang; the <ver> tracks
# the SDK, so discover it instead of hard-coding.
case "$TARGET" in
    aarch64-apple-darwin) WRAP_ARCH=aarch64 ;;
    x86_64-apple-darwin)  WRAP_ARCH=x86_64  ;;
    *) echo "error: unsupported TARGET '$TARGET'" >&2; exit 1 ;;
esac
CLANG="$(ls "$OSXCROSS_DIR"/bin/${WRAP_ARCH}-apple-darwin*-clang 2>/dev/null | sort -V | tail -1)"
if [ -z "$CLANG" ]; then
    echo "error: no ${WRAP_ARCH}-apple-darwin*-clang wrapper in $OSXCROSS_DIR/bin" >&2
    exit 1
fi
PREFIX="${CLANG%-clang}"   # e.g. /usr/lib/osxcross/bin/aarch64-apple-darwin25.1

if ! rustup target list --installed 2>/dev/null | grep -qx "$TARGET"; then
    echo "error: rust std for $TARGET missing (run: rustup target add $TARGET)" >&2
    exit 1
fi

# --- transient rio-window patches, reverted no matter how we exit -------------
BUILD_RS=rio-window/build.rs
CARGO_TOML=rio-window/Cargo.toml
BACKUP="$(mktemp -d)"
cp "$BUILD_RS" "$BACKUP/build.rs"
cp "$CARGO_TOML" "$BACKUP/Cargo.toml"
restore() {
    cp "$BACKUP/build.rs" "$BUILD_RS"
    cp "$BACKUP/Cargo.toml" "$CARGO_TOML"
    rm -rf "$BACKUP"
}
trap restore EXIT INT TERM

python3 - "$BUILD_RS" "$CARGO_TOML" <<'PY'
import sys
build_rs, cargo_toml = sys.argv[1], sys.argv[2]

s = open(build_rs).read()
host_gate = '    #[cfg(target_os = "macos")]\n    generate_dispatch_bindings();'
target_gate = ('    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {\n'
               '        generate_dispatch_bindings();\n    }')
assert host_gate in s, "build.rs: host-gated bindgen call not found (upstream changed?)"
s = s.replace(host_gate, target_gate)
s = s.replace('#[cfg(target_os = "macos")]\nfn generate_dispatch_bindings()',
              'fn generate_dispatch_bindings()')
open(build_rs, 'w').write(s)

s = open(cargo_toml).read()
assert '[build-dependencies]\n' in s, "Cargo.toml: [build-dependencies] section not found"
s = s.replace('[build-dependencies]\n', '[build-dependencies]\nbindgen = "0.70.1"\n', 1)
open(cargo_toml, 'w').write(s)
PY

# --- build -------------------------------------------------------------------
TARGET_UPPER="$(echo "$TARGET" | tr 'a-z-' 'A-Z_')"
export PATH="$OSXCROSS_DIR/bin:$PATH"
export "CARGO_TARGET_${TARGET_UPPER}_LINKER=${PREFIX}-clang"
export "CC_${TARGET//-/_}=${PREFIX}-clang"
export "CXX_${TARGET//-/_}=${PREFIX}-clang++"
export "AR_${TARGET//-/_}=${PREFIX}-ar"
# bindgen drives libclang directly, so it needs the SDK sysroot pointed out.
export BINDGEN_EXTRA_CLANG_ARGS="-isysroot $SDK"

# Default to the full frontend binary when no cargo args are given.
if [ "$#" -eq 0 ]; then
    set -- -p rioterm
fi

echo ">> SDK:    $SDK"
echo ">> target: $TARGET  (linker: ${PREFIX}-clang)"
echo ">> cargo build --target $TARGET $*"
# NB: no `exec` -- it would replace this shell and discard the EXIT trap, so the
# transient patches would never be reverted. Run normally and propagate status.
cargo build --target "$TARGET" "$@"
