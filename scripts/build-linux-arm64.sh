#!/bin/bash
# Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
# SPDX-License-Identifier: Apache-2.0
#
# Build the linux-arm64 engine, in a container, with the same compatibility
# floor as the linux-x64 asset.
#
# The other four platforms are built natively on their own machines. arm64 Linux
# has no such machine, and the floor is easy to get wrong, so it is pinned here
# instead of remembered:
#
#   Ubuntu 20.04  supplies glibc 2.31, matching the SLES 15 SP4 host the x64
#                 asset is built on, so both architectures land on one support
#                 statement instead of two.
#
#                 Note what the floor actually is. glibc is not the binding
#                 constraint - GLIBCXX_3.4.29 is, because ONNX forces GCC 11.
#                 Measured, this binary runs on SLES 15 SP4, Ubuntu 22.04+,
#                 Debian 12+, RHEL 9+ and Amazon Linux 2023, and does not run
#                 on Ubuntu 20.04, Debian 11, RHEL 8 or Amazon Linux 2 - not
#                 even on its own build host. That is identical to the x64
#                 asset. SLES 15 SP4 works because SUSE ships an updated
#                 libstdc++6 on an old glibc, which is the whole reason this
#                 combination is worth pinning.
#
#   gcc-11        is required, and is why the obvious choices do not work.
#                 ONNX Runtime's prebuilt aarch64 static library needs GCC 11's
#                 libstdc++ (`std::__throw_bad_array_new_length`) and newer
#                 libgcc outline atomics (`__aarch64_cas8_sync`). Measured:
#                   Debian 11 (gcc 10)      - fails, both symbols
#                   SLES 15 SP4 BCI + gcc11 - fails, SUSE's gcc11 ships no
#                                             outline atomics
#                   Ubuntu 22.04 (gcc 11)   - links, but floors at glibc 2.34
#                 Ubuntu 20.04 + gcc-11 is the only combination measured to give
#                 both a working link and a 2.30 floor.
#
# Requires Docker. On an arm64 host this runs natively; on x86_64 it runs under
# emulation, which works but is slow.
#
# Usage:
#   ./scripts/build-linux-arm64.sh            # build + verify + stage
#   ./scripts/build-linux-arm64.sh --no-stage # build + verify only

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
STAGE_DIR="${CODEGRAPH_BIN_DIR:-$REPO_ROOT/vscode/bin}"
ASSET="codegraph-server-linux-arm64"
BUILD_DIR="${CODEGRAPH_ARM64_TARGET:-$REPO_ROOT/target/linux-arm64}"

# The floors the x64 asset already has. A build that exceeds either of these
# silently narrows who can run CodeGraph, so they are asserted, not printed.
MAX_GLIBC="2.30"
MAX_GLIBCXX="3.4.29"

command -v docker >/dev/null || { echo "ERROR: docker is required." >&2; exit 1; }

VERSION="$(grep -m1 '^version' "$REPO_ROOT/Cargo.toml" | sed 's/.*"\(.*\)".*/\1/')"
[ -n "$VERSION" ] || { echo "ERROR: no version in Cargo.toml" >&2; exit 1; }

echo "=== CodeGraph linux-arm64 engine ==="
echo "  version: $VERSION"
echo "  target:  $BUILD_DIR"
echo

mkdir -p "$BUILD_DIR"

# -i is required: the recipe below is fed to `bash -s` on stdin, and without it
# docker attaches no stdin and the container runs an empty script successfully.
docker run --rm -i --platform linux/arm64 \
  -v "$REPO_ROOT:/src" \
  -v "$BUILD_DIR:/target" \
  -e MAX_GLIBC="$MAX_GLIBC" \
  -e MAX_GLIBCXX="$MAX_GLIBCXX" \
  ubuntu:20.04 bash -s <<'CONTAINER'
set -uo pipefail
export DEBIAN_FRONTEND=noninteractive
APT_OPTS=(-o Acquire::Retries=8 -o Acquire::http::Timeout=30)
apt_install() {
  for attempt in 1 2 3; do
    apt-get "${APT_OPTS[@]}" install -y -qq --fix-missing "$@" && return 0
    echo "(apt attempt $attempt failed: $*)"; sleep 5
  done
  return 1
}

apt-get "${APT_OPTS[@]}" update -qq || { echo "APT UPDATE FAILED"; exit 1; }
apt_install software-properties-common curl git ca-certificates || exit 1
add-apt-repository -y ppa:ubuntu-toolchain-r/test >/dev/null 2>&1 || { echo "PPA FAILED"; exit 1; }
apt-get "${APT_OPTS[@]}" update -qq
apt_install gcc-11 g++-11 clang libclang-dev cmake pkg-config libssl-dev make binutils || exit 1

if ! command -v cargo >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --default-toolchain stable --profile minimal >/dev/null || exit 1
fi
. "$HOME/.cargo/env"

cd /src
export CARGO_TARGET_DIR=/target
export CC=gcc-11 CXX=g++-11
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=gcc-11
cargo build --release -p codegraph-server 2>&1 | tail -12
[ "${PIPESTATUS[0]}" -eq 0 ] || { echo "BUILD FAILED"; exit 1; }

BIN=/target/release/codegraph-server

# The #15 regression guard. An immutable definition of this symbol lands in
# .rodata, and on aarch64 it is also exported, so glibc's startup write to it
# faults before main(). It must live in writable memory (B or D, never R).
SHIM_CLASS=$(nm "$BIN" 2>/dev/null | awk '/__libc_single_threaded/ {print $2; exit}')
echo "__libc_single_threaded section class: ${SHIM_CLASS:-<absent>}"
case "$SHIM_CLASS" in
  B|D|b|d|"") ;;
  *) echo "ERROR: shim is '$SHIM_CLASS' (read-only) - this binary will SIGSEGV at startup." >&2; exit 1 ;;
esac

highest() { readelf -V "$BIN" 2>/dev/null | grep -o "$1[0-9.]*" | sed "s/$1//" | sort -V | tail -1; }
GOT_GLIBC=$(highest 'GLIBC_')
GOT_GLIBCXX=$(highest 'GLIBCXX_')
echo "glibc floor:   ${GOT_GLIBC:-none} (max allowed $MAX_GLIBC)"
echo "GLIBCXX floor: ${GOT_GLIBCXX:-none} (max allowed $MAX_GLIBCXX)"

newer_than() { [ "$(printf '%s\n%s\n' "$1" "$2" | sort -V | tail -1)" = "$1" ] && [ "$1" != "$2" ]; }
fail=0
if [ -n "$GOT_GLIBC" ] && newer_than "$GOT_GLIBC" "$MAX_GLIBC"; then
  echo "ERROR: glibc floor $GOT_GLIBC exceeds $MAX_GLIBC - this no longer matches" >&2
  echo "the x64 asset, and drops SLES 15 SP4 among others." >&2; fail=1
fi
if [ -n "$GOT_GLIBCXX" ] && newer_than "$GOT_GLIBCXX" "$MAX_GLIBCXX"; then
  echo "ERROR: GLIBCXX floor $GOT_GLIBCXX exceeds $MAX_GLIBCXX." >&2; fail=1
fi
[ "$fail" -eq 0 ] || exit 1

"$BIN" --version || { echo "ERROR: the binary does not run." >&2; exit 1; }
echo "BUILD OK"
CONTAINER

BUILT="$BUILD_DIR/release/codegraph-server"
[ -f "$BUILT" ] || { echo "ERROR: no binary at $BUILT" >&2; exit 1; }

if [ "${1:-}" = "--no-stage" ]; then
  echo
  echo "Built (not staged): $BUILT"
  exit 0
fi

mkdir -p "$STAGE_DIR"
cp "$BUILT" "$STAGE_DIR/$ASSET"
chmod +x "$STAGE_DIR/$ASSET"
echo
echo "Staged: $STAGE_DIR/$ASSET"

# Provenance is recorded where it is known. publish-release-assets.sh treats an
# unstamped binary in the staging directory as stale, because vscode/bin/ is not
# cleaned between releases.
"$REPO_ROOT/scripts/stamp-binary.sh" "$ASSET" "$VERSION"
