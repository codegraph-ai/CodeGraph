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
#                 on stock Ubuntu 20.04, Debian 11, RHEL 8 or Amazon Linux 2 -
#                 not even on its own build host as shipped. The container does
#                 run it, and the check at the end of the recipe relies on
#                 that, only because the toolchain PPA below upgrades the
#                 container's libstdc++6 past GLIBCXX_3.4.29. That floor is
#                 identical to the x64 asset. SLES 15 SP4 works because SUSE
#                 ships an updated libstdc++6 on an old glibc, which is the
#                 whole reason this combination is worth pinning.
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
#
#   CODEGRAPH_ALLOW_DIRTY=1 ./scripts/build-linux-arm64.sh
#       Build from whatever is in the tree, skipping the commit check below.
#       Implies --no-stage: a binary nobody can trace must not reach the
#       staging directory, where it would look exactly like a release build.

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

# Which commit the asset claims is a floor like the other two, and is checked
# like one. build.rs derives that stamp by shelling out to git from inside the
# container, and every way that can fail - a worktree whose gitdir is not
# mounted, git refusing a tree owned by another uid, no git at all - fails
# silently to the literal string "unknown". A modified tree stamps "-dirty".
# Neither is publishable, and stamp-binary.sh records only the version, so
# nothing downstream would notice. So HEAD is captured here and the built
# binary is made to agree with it.
EXPECTED_GIT_SHORT=""
CHECK_PROVENANCE=1
if [ "${CODEGRAPH_ALLOW_DIRTY:-0}" = "1" ]; then
  CHECK_PROVENANCE=0
else
  # An explicit length rather than `--short`, whose default the host user's
  # core.abbrev can change while the container's git has no such setting.
  EXPECTED_GIT_SHORT="$(git -C "$REPO_ROOT" rev-parse --short=12 HEAD 2>/dev/null || true)"
  if [ -z "$EXPECTED_GIT_SHORT" ]; then
    echo "ERROR: $REPO_ROOT is not a git checkout, so the engine cannot say what" >&2
    echo "produced it. Build from a checkout, or set CODEGRAPH_ALLOW_DIRTY=1 for a" >&2
    echo "throwaway build that must never be published." >&2
    exit 1
  fi
  if [ -n "$(git -C "$REPO_ROOT" status --porcelain 2>/dev/null)" ]; then
    echo "ERROR: the working tree is dirty, so this build would be stamped '-dirty'" >&2
    echo "and would disagree with the assets built for the same release. Commit or" >&2
    echo "stash first, or set CODEGRAPH_ALLOW_DIRTY=1 for a throwaway build." >&2
    exit 1
  fi
fi

echo "=== CodeGraph linux-arm64 engine ==="
echo "  version: $VERSION"
echo "  target:  $BUILD_DIR"
if [ "$CHECK_PROVENANCE" -eq 1 ]; then
  echo "  commit:  $EXPECTED_GIT_SHORT"
else
  echo "  commit:  unchecked (CODEGRAPH_ALLOW_DIRTY=1) - do not publish this build"
fi
echo

mkdir -p "$BUILD_DIR"

# -i is required: the recipe below is fed to `bash -s` on stdin, and without it
# docker attaches no stdin and the container runs an empty script successfully.
docker run --rm -i --platform linux/arm64 \
  -v "$REPO_ROOT:/src" \
  -v "$BUILD_DIR:/target" \
  -e MAX_GLIBC="$MAX_GLIBC" \
  -e MAX_GLIBCXX="$MAX_GLIBCXX" \
  -e CHECK_PROVENANCE="$CHECK_PROVENANCE" \
  -e EXPECTED_GIT_SHORT="$EXPECTED_GIT_SHORT" \
  -e HOST_UID="$(id -u)" \
  -e HOST_GID="$(id -g)" \
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

# build.rs reads git provenance from here, and git refuses a tree owned by
# another uid unless told the ownership is expected. Without this the stamp
# degrades to "unknown" with nothing on stderr.
git config --global --add safe.directory /src

# The container is root and both mounts live in the developer's tree. On a
# Linux host there is no uid remapping, so anything written here stays
# root-owned and the host cannot rebuild or clean its own target/ afterwards.
# Runs on every exit path, including a failed build, which is when a
# half-written target/ is most annoying to be locked out of.
restore_ownership() {
  chown -R "$HOST_UID:$HOST_GID" /target 2>/dev/null || true
  chown "$HOST_UID:$HOST_GID" /src/Cargo.lock 2>/dev/null || true
  [ -d /src/.git ] && chown -R "$HOST_UID:$HOST_GID" /src/.git 2>/dev/null
  return 0
}
trap restore_ownership EXIT

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
#
# The class is the field before the symbol name, not the second field: nm omits
# the address column for undefined symbols, so a positional $2 reads the name
# itself there. An absent symbol is an error, not a pass - main.rs defines it
# unconditionally on Linux, so nothing found means nm could not tell us and the
# guard did not run.
NM_OUT=$(nm "$BIN" 2>&1) || { echo "ERROR: nm could not read $BIN, so the #15 guard cannot run:" >&2
  printf '%s\n' "$NM_OUT" >&2; exit 1; }
SHIM_CLASS=$(printf '%s\n' "$NM_OUT" | awk '$NF == "__libc_single_threaded" { print $(NF-1); exit }')
echo "__libc_single_threaded section class: ${SHIM_CLASS:-<absent>}"
case "$SHIM_CLASS" in
  B|D|b|d) ;;
  "") echo "ERROR: __libc_single_threaded is absent from the symbol table, so the" >&2
      echo "issue #15 startup guard could not be checked. Do not ship this binary." >&2; exit 1 ;;
  U) echo "ERROR: __libc_single_threaded is undefined - the shim did not link in, so" >&2
     echo "this binary requires glibc 2.32 and will not start on the supported floor." >&2; exit 1 ;;
  *) echo "ERROR: shim is in section class '$SHIM_CLASS' (not writable) - this binary" >&2
     echo "will SIGSEGV at startup." >&2; exit 1 ;;
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

# --info both proves the binary starts and is the only place the provenance
# build.rs baked in is readable from outside.
INFO=$("$BIN" --info) || { echo "ERROR: the binary does not run." >&2; exit 1; }
printf '%s\n' "$INFO"

if [ "$CHECK_PROVENANCE" = "1" ]; then
  GOT_GIT=$(printf '%s\n' "$INFO" | sed -n '1s/.*(\(.*\)).*/\1/p')
  case "$GOT_GIT" in
    "")
      echo "ERROR: the binary reports no commit at all." >&2; exit 1 ;;
    unknown*)
      echo "ERROR: the binary is stamped '$GOT_GIT' - git told build.rs nothing usable" >&2
      echo "inside the container. If /src is a git worktree, its gitdir is not mounted." >&2
      exit 1 ;;
    *-dirty)
      echo "ERROR: the binary is stamped '$GOT_GIT' - it was built from a modified tree" >&2
      echo "and cannot be traced back to a released commit." >&2
      exit 1 ;;
  esac
  if [ "${#GOT_GIT}" -lt 7 ]; then
    echo "ERROR: the binary's stamp '$GOT_GIT' is too short to name a commit." >&2
    exit 1
  fi
  # By prefix: the container's git and the host's git abbreviate independently,
  # so the same commit can legitimately come back at two different lengths.
  same_commit=0
  case "$EXPECTED_GIT_SHORT" in "$GOT_GIT"*) same_commit=1 ;; esac
  case "$GOT_GIT" in "$EXPECTED_GIT_SHORT"*) same_commit=1 ;; esac
  if [ "$same_commit" -eq 0 ]; then
    echo "ERROR: the binary claims commit $GOT_GIT, but the host is at" >&2
    echo "$EXPECTED_GIT_SHORT. The release assets would disagree on their source." >&2
    exit 1
  fi
  echo "provenance: $GOT_GIT, matching the host checkout"
fi
echo "BUILD OK"
CONTAINER

BUILT="$BUILD_DIR/release/codegraph-server"
[ -f "$BUILT" ] || { echo "ERROR: no binary at $BUILT" >&2; exit 1; }

if [ "${1:-}" = "--no-stage" ] || [ "$CHECK_PROVENANCE" -eq 0 ]; then
  echo
  echo "Built (not staged): $BUILT"
  if [ "$CHECK_PROVENANCE" -eq 0 ]; then
    echo
    echo "Not staged: CODEGRAPH_ALLOW_DIRTY=1 skipped the commit check, and an"
    echo "unverified binary in $STAGE_DIR would be indistinguishable"
    echo "from a release build. Commit the tree and re-run to stage."
  fi
  exit 0
fi

mkdir -p "$STAGE_DIR"
cp "$BUILT" "$STAGE_DIR/$ASSET"
chmod +x "$STAGE_DIR/$ASSET"
echo
echo "Staged: $STAGE_DIR/$ASSET"

# Provenance is recorded where it is known. publish-release-assets.sh treats an
# unstamped binary in the staging directory as stale, because vscode/bin/ is not
# cleaned between releases, and refuses a set of assets whose commits disagree.
# The commit is passed rather than read back from the binary: an x86_64 host
# cannot execute what it just cross-built, and the container already proved this
# binary agrees with this checkout.
"$REPO_ROOT/scripts/stamp-binary.sh" "$ASSET" "$VERSION" "$EXPECTED_GIT_SHORT"
