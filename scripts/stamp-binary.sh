#!/bin/bash
# Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
# SPDX-License-Identifier: Apache-2.0
#
# Record which version a staged engine binary was built from.
#
# vscode/bin/ is a staging directory that is not cleaned between releases, so a
# binary left over from an earlier version is indistinguishable from a fresh
# one by inspection. Scraping the version out of the image does not work
# reliably across targets - the engine does not store it as a standalone string
# everywhere, and good binaries get reported as stale.
#
# So provenance is recorded at the point where it is actually known: whoever
# stages a binary states what produced it. publish-release-assets.sh refuses to
# publish anything not stamped for the version being released.
#
# Usage:
#   ./scripts/stamp-binary.sh codegraph-server-linux-x64 0.20.0
#
set -euo pipefail

if [ $# -ne 2 ]; then
  echo "usage: $(basename "$0") <binary-name> <version>" >&2
  exit 2
fi

BINARY="$1"
VERSION="$2"

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_DIR="${CODEGRAPH_BIN_DIR:-$REPO_ROOT/vscode/bin}"
MANIFEST="$BIN_DIR/BUILD-MANIFEST"

if [ ! -f "$BIN_DIR/$BINARY" ]; then
  echo "ERROR: $BIN_DIR/$BINARY does not exist - stage the binary first." >&2
  exit 1
fi

touch "$MANIFEST"
# One line per binary: re-stamping replaces the previous entry rather than
# appending, so the manifest can never claim two versions for one file.
tmp="$(mktemp)"
grep -vF "  $BINARY" "$MANIFEST" > "$tmp" 2>/dev/null || true
printf '%s  %s\n' "$VERSION" "$BINARY" >> "$tmp"
sort -k2 "$tmp" > "$MANIFEST"
rm -f "$tmp"

echo "Stamped $BINARY as $VERSION"
cat "$MANIFEST"
