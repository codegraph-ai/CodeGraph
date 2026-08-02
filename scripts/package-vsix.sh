#!/bin/bash
# Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
# SPDX-License-Identifier: Apache-2.0
#
# Package the VS Code extension.
#
# One VSIX serves every platform: the engine is fetched for the machine the
# extension lands on rather than bundled, so there is nothing platform-specific
# to package and no per-platform argument to pass.
#
# Usage:
#   ./scripts/package-vsix.sh

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VSCODE_DIR="$REPO_ROOT/vscode"

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

# One VSIX for every platform. The extension fetches the engine for the
# machine it lands on (src/engineDownload.ts), so there is nothing
# platform-specific left to package. Building four targeted VSIXs plus a
# combined one previously produced a 118 MB artifact in which any given user
# could run a quarter of the payload.
#
# Publish the release assets first with ./scripts/publish-release-assets.sh, or
# installs of this version will have no engine to fetch.
echo "Packaging (platform-independent; the engine is fetched at first use)..."
npx @vscode/vsce package 2>&1 | grep -E "DONE|ERROR"

echo ""
echo "VSIX packages:"
ls -lh *.vsix 2>/dev/null || echo "  No VSIX files found"
