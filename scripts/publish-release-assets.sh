#!/bin/bash
# Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
# SPDX-License-Identifier: Apache-2.0
#
# Publish the per-platform engine binaries as GitHub release assets.
#
# The VSIX and the npm package both bundle all four platform binaries, because
# both ecosystems allow it. The JetBrains Marketplace does not: it serves one
# artifact to every platform, so a bundled plugin would be a ~120 MB download
# for every user to obtain the ~30 MB they can actually run. Publishing the
# binaries individually lets that client - and anyone scripting an install -
# fetch only what they need.
#
# This does not build anything. It uploads what ./scripts/package-*.sh already
# expect to find in vscode/bin/, so it slots in after the existing
# cross-platform build (see cross-platform-builds.md).
#
# Usage:
#   ./scripts/publish-release-assets.sh              # stage + verify only
#   ./scripts/publish-release-assets.sh --publish    # upload to GitHub
#
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# Overridable so CI (and the refusal path's own test) can point at a different
# staging location without editing this script.
VSCODE_BIN="${CODEGRAPH_BIN_DIR:-$REPO_ROOT/vscode/bin}"
STAGE_DIR="$REPO_ROOT/target/release-assets"
REPO="codegraph-ai/CodeGraph"

# The engine's own version is the one that matters here - these are engine
# binaries, and the JetBrains client asks for them by engine version.
VERSION="$(grep -m1 '^version' "$REPO_ROOT/Cargo.toml" | sed 's/.*"\(.*\)".*/\1/')"
TAG="v${VERSION}"

BINARIES=(
  "codegraph-server-darwin-arm64"
  "codegraph-server-darwin-x64"
  "codegraph-server-linux-x64"
  "codegraph-server-win32-x64.exe"
)

# The Windows engine loads this at runtime. Shipping the exe without it gives
# users a download that succeeds and then fails at startup, which is a worse
# outcome than no download at all - so it is treated as required, not optional.
WINDOWS_SIDECAR="onnxruntime.dll"

echo "CodeGraph release assets"
echo "  version: $VERSION"
echo "  tag:     $TAG"
echo "  repo:    $REPO"
echo

# ---------------------------------------------------------------- verify
missing=0
for bin in "${BINARIES[@]}" "$WINDOWS_SIDECAR"; do
  if [ -f "$VSCODE_BIN/$bin" ]; then
    printf '  ✓ %-36s %s\n' "$bin" "$(du -h "$VSCODE_BIN/$bin" | cut -f1)"
  else
    printf '  ✗ %-36s MISSING\n' "$bin"
    missing=1
  fi
done

if [ "$missing" -ne 0 ]; then
  cat >&2 <<EOF

ERROR: not every platform artifact is present in vscode/bin/.

A partial release is worse than none: a client that resolves its own platform
and finds nothing has no way to tell "not built yet" from "never supported".
Build the missing platforms first - see cross-platform-builds.md for the
per-platform hosts - then re-run.
EOF
  exit 1
fi

# ---------------------------------------------------------------- checksums
# An engine binary runs on the user's machine with their permissions, so the
# client verifies what it downloaded. TLS alone does not cover a mirror, a
# proxy, or a truncated transfer.
echo
echo "Staging with checksums in ${STAGE_DIR#"$REPO_ROOT"/} ..."
rm -rf "$STAGE_DIR"
mkdir -p "$STAGE_DIR"

for bin in "${BINARIES[@]}" "$WINDOWS_SIDECAR"; do
  cp "$VSCODE_BIN/$bin" "$STAGE_DIR/$bin"
  # `shasum -a 256` and `sha256sum` produce the same "<digest>  <name>" format;
  # the clients read the first field.
  ( cd "$STAGE_DIR" && shasum -a 256 "$bin" > "$bin.sha256" )
  printf '  %s  %s\n' "$(cut -c1-16 < "$STAGE_DIR/$bin.sha256")" "$bin"
done

if [ "${1:-}" != "--publish" ]; then
  echo
  echo "Staged only. Re-run with --publish to upload to $REPO."
  exit 0
fi

# ---------------------------------------------------------------- publish
if ! command -v gh >/dev/null 2>&1; then
  echo "ERROR: the GitHub CLI (gh) is required to publish." >&2
  exit 1
fi

echo
if gh release view "$TAG" --repo "$REPO" >/dev/null 2>&1; then
  echo "Release $TAG exists; uploading assets (--clobber replaces same-named files)."
else
  echo "Creating release $TAG."
  gh release create "$TAG" \
    --repo "$REPO" \
    --title "CodeGraph $VERSION" \
    --notes "Engine binaries for CodeGraph $VERSION.

Each binary has a matching \`.sha256\`. Clients that download an engine are
expected to verify it before running.

Windows additionally requires \`onnxruntime.dll\` alongside the executable."
fi

gh release upload "$TAG" --repo "$REPO" --clobber "$STAGE_DIR"/*

echo
echo "Published $TAG:"
gh release view "$TAG" --repo "$REPO" --json assets \
  --jq '.assets[] | "  \(.name)  \(.size) bytes"'
