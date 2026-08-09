#!/bin/bash
# Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
# SPDX-License-Identifier: Apache-2.0
#
# Record which version and which commit a staged engine binary was built from.
#
# vscode/bin/ is a staging directory that is not cleaned between releases, so a
# binary left over from an earlier version is indistinguishable from a fresh
# one by inspection. Scraping the version out of the image does not work
# reliably across targets - the engine does not store it as a standalone string
# everywhere, and good binaries get reported as stale.
#
# The version alone is not provenance. Five binaries built on five different
# hosts can all be 0.20.1 and still come from five different trees, which is a
# release whose assets disagree with each other and with the tag it was cut
# from. So the commit is recorded beside the version, and
# publish-release-assets.sh refuses a set that does not agree on one.
#
# The commit is taken from the binary itself wherever the staging host can run
# it: the engine bakes it in at build time and prints it from `--info`, which
# makes it a measurement rather than a claim. A cross-built asset cannot be
# executed on the host that stages it, so there it has to be stated.
#
# Usage:
#   ./scripts/stamp-binary.sh codegraph-server-darwin-arm64 0.20.1
#   ./scripts/stamp-binary.sh codegraph-server-linux-x64 0.20.1 a1b2c3d4e5f6
#
set -euo pipefail

if [ $# -lt 2 ] || [ $# -gt 3 ]; then
  echo "usage: $(basename "$0") <binary-name> <version> [commit]" >&2
  exit 2
fi

BINARY="$1"
VERSION="$2"
COMMIT="${3:-}"

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_DIR="${CODEGRAPH_BIN_DIR:-$REPO_ROOT/vscode/bin}"
MANIFEST="$BIN_DIR/BUILD-MANIFEST"

if [ ! -f "$BIN_DIR/$BINARY" ]; then
  echo "ERROR: $BIN_DIR/$BINARY does not exist - stage the binary first." >&2
  exit 1
fi

# Two abbreviations of the same commit, made by different git versions on
# different hosts, differ in length rather than content.
same_commit() {
  case "$1" in "$2"*) return 0 ;; esac
  case "$2" in "$1"*) return 0 ;; esac
  return 1
}

# Failure here is the ordinary cross-built case, not an error: a linux binary
# staged on macOS cannot execute, which is exactly why [commit] exists.
SELF_REPORTED=""
if [ -x "$BIN_DIR/$BINARY" ]; then
  info="$("$BIN_DIR/$BINARY" --info 2>/dev/null || true)"
  SELF_REPORTED="$(printf '%s\n' "$info" | sed -n '1s/.*(\(.*\)).*/\1/p')"
fi

if [ -n "$SELF_REPORTED" ] && [ -n "$COMMIT" ] && ! same_commit "$COMMIT" "$SELF_REPORTED"; then
  echo "ERROR: $BINARY reports commit $SELF_REPORTED, but $COMMIT was given." >&2
  echo "The binary is the only witness of what actually built it, so the" >&2
  echo "argument cannot override it. Stage the binary you meant to stage." >&2
  exit 1
fi

# They agree by here, so keep whichever abbreviation names the commit more
# precisely.
if [ "${#SELF_REPORTED}" -gt "${#COMMIT}" ]; then
  COMMIT="$SELF_REPORTED"
fi

if [ -z "$COMMIT" ]; then
  echo "ERROR: no commit recorded for $BINARY." >&2
  echo "This host cannot run it, so the commit it was built from has to be" >&2
  echo "stated. From the checkout it was built on:" >&2
  echo >&2
  echo "  $(basename "$0") $BINARY $VERSION \$(git rev-parse --short=12 HEAD)" >&2
  exit 1
fi

case "$COMMIT" in
  unknown*)
    echo "ERROR: $BINARY reports commit '$COMMIT' - it was built somewhere git" >&2
    echo "could not be read, so nothing can say which source produced it." >&2
    exit 1 ;;
  *-dirty)
    echo "ERROR: $BINARY reports commit '$COMMIT' - it was built from a modified" >&2
    echo "tree and cannot be traced back to a released commit." >&2
    exit 1 ;;
  *[!0-9a-f]*)
    echo "ERROR: '$COMMIT' is not a commit hash." >&2
    exit 1 ;;
esac

if [ "${#COMMIT}" -lt 7 ]; then
  echo "ERROR: commit '$COMMIT' is too short to name one commit; use at least 7" >&2
  echo "hex digits, as \`git rev-parse --short\` produces." >&2
  exit 1
fi

touch "$MANIFEST"
# One line per binary: re-stamping replaces the previous entry rather than
# appending, so the manifest can never claim two versions for one file.
tmp="$(mktemp)"
grep -vF "  $BINARY" "$MANIFEST" > "$tmp" 2>/dev/null || true
printf '%s  %s  %s\n' "$VERSION" "$COMMIT" "$BINARY" >> "$tmp"
sort -k3 "$tmp" > "$MANIFEST"
rm -f "$tmp"

echo "Stamped $BINARY as $VERSION ($COMMIT)"
cat "$MANIFEST"
