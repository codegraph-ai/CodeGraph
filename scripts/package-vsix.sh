#!/bin/bash
# Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
# SPDX-License-Identifier: Apache-2.0
#
# Package VS Code extension with platform-specific binaries.
# Run from the repo root after all platform binaries are built.
#
# Usage:
#   ./scripts/package-vsix.sh                    # all platforms (universal)
#   ./scripts/package-vsix.sh darwin-arm64       # single platform

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VSCODE_DIR="$REPO_ROOT/vscode"
BIN_DIR="$VSCODE_DIR/bin"
TARGET="${1:-all}"

PLATFORMS=(
  "darwin-arm64:codegraph-server-darwin-arm64"
  "darwin-x64:codegraph-server-darwin-x64"
  "linux-x64:codegraph-server-linux-x64"
  "win32-x64:codegraph-server-win32-x64.exe"
)

echo "=== CodeGraph VSIX builder ==="
echo ""

cd "$VSCODE_DIR"

# Ensure node_modules
if [ ! -d "node_modules" ]; then
  echo "Installing npm dependencies..."
  npm install
fi

# Copy CHANGELOG from repo root so vsce includes it in the VSIX.
# VS Code marketplace renders it as a "Changelog" tab alongside README.
if [ -f "$REPO_ROOT/CHANGELOG.md" ]; then
  cp "$REPO_ROOT/CHANGELOG.md" "$VSCODE_DIR/CHANGELOG.md"
  echo "Copied CHANGELOG.md into vscode/"
fi

# Build TypeScript
echo "Building extension..."
npm run esbuild-base -- --production
echo ""

if [ "$TARGET" = "all" ]; then
  # Build platform-specific VSIX for each available binary
  for entry in "${PLATFORMS[@]}"; do
    PLAT="${entry%%:*}"
    BIN="${entry##*:}"
    if [ -f "$BIN_DIR/$BIN" ]; then
      echo "Packaging for $PLAT..."
      npx @vscode/vsce package --target "$PLAT" 2>&1 | grep -E "DONE|ERROR"
    else
      echo "  ⚠ Skipping $PLAT (binary not found: bin/$BIN)"
    fi
  done

  # Combined VSIX: no --target, includes all 4 platform binaries + the
  # Windows onnxruntime.dll. Useful for manual sideload + as a fallback
  # for marketplace listings that don't yet have platform-targeted
  # distribution wired up.
  echo "Packaging combined (no --target)..."
  npx @vscode/vsce package 2>&1 | grep -E "DONE|ERROR"
else
  # Single platform
  echo "Packaging for $TARGET..."
  npx @vscode/vsce package --target "$TARGET" 2>&1 | grep -E "DONE|ERROR"
fi

echo ""
echo "VSIX packages:"
ls -lh *.vsix 2>/dev/null || echo "  No VSIX files found"
