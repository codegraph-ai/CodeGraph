#!/bin/bash
# Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
# SPDX-License-Identifier: Apache-2.0
#
# Package the npm MCP server distribution.
# Run from the repo root after all platform binaries are built.
#
# Usage:
#   ./scripts/package-npm.sh          # copy from vscode/bin/
#   ./scripts/package-npm.sh --publish # also publish to npmjs.com

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PKG_DIR="$REPO_ROOT/mcp-package"
BIN_DIR="$PKG_DIR/bin"
VSCODE_BIN="$REPO_ROOT/vscode/bin"

BINARIES=(
  "codegraph-server-darwin-arm64"
  "codegraph-server-darwin-x64"
  "codegraph-server-linux-x64"
  "codegraph-server-win32-x64.exe"
)

echo "=== CodeGraph npm package builder ==="
echo ""

# Step 1: Check that source binaries exist
MISSING=0
for bin in "${BINARIES[@]}"; do
  if [ ! -f "$VSCODE_BIN/$bin" ]; then
    echo "  ✗ Missing: vscode/bin/$bin"
    MISSING=1
  else
    SIZE=$(du -h "$VSCODE_BIN/$bin" | cut -f1)
    echo "  ✓ Found: vscode/bin/$bin ($SIZE)"
  fi
done

if [ "$MISSING" -eq 1 ]; then
  echo ""
  echo "ERROR: Not all platform binaries are present in vscode/bin/"
  echo "Build missing platforms first. See: scripts/build-all.sh or ~/.claude/cross-platform-builds.md"
  exit 1
fi

# Step 2: Copy binaries to mcp-package/bin/
echo ""
echo "Copying binaries to mcp-package/bin/..."
mkdir -p "$BIN_DIR"

for bin in "${BINARIES[@]}"; do
  cp "$VSCODE_BIN/$bin" "$BIN_DIR/$bin"
  # Set executable on Unix binaries
  if [[ "$bin" != *.exe ]]; then
    chmod +x "$BIN_DIR/$bin"
  fi
done

# Copy Windows ONNX runtime DLL (required for Windows binary)
if [ -f "$VSCODE_BIN/onnxruntime.dll" ]; then
  cp "$VSCODE_BIN/onnxruntime.dll" "$BIN_DIR/"
  echo "  ✓ Copied onnxruntime.dll"
elif [ -f "$BIN_DIR/codegraph-server-win32-x64.exe" ]; then
  echo "  ⚠ WARNING: Windows binary present but onnxruntime.dll missing!"
  echo "    Windows users will fail at runtime without this DLL."
  echo "    Copy from Windows build host: C:\\Users\\Administrator\\projects\\codegraph\\target\\release\\onnxruntime.dll"
fi

# Ensure launcher scripts are executable
chmod +x "$BIN_DIR/codegraph-mcp.js"

echo ""
echo "Package contents:"
ls -lh "$BIN_DIR/"

# Step 3: Verify version consistency
PKG_VERSION=$(node -e "console.log(require('$PKG_DIR/package.json').version)")
SERVER_VERSION=$(node -e "console.log(require('$PKG_DIR/server.json').version)")
echo ""
echo "package.json version: $PKG_VERSION"
echo "server.json version:  $SERVER_VERSION"

if [ "$PKG_VERSION" != "$SERVER_VERSION" ]; then
  echo "WARNING: version mismatch between package.json and server.json"
fi

# Step 4: Pack
echo ""
echo "Packing..."
cd "$PKG_DIR"
npm pack 2>&1

TARBALL=$(ls -t *.tgz 2>/dev/null | head -1)
if [ -n "$TARBALL" ]; then
  SIZE=$(du -h "$TARBALL" | cut -f1)
  echo ""
  echo "✓ Created: mcp-package/$TARBALL ($SIZE)"
fi

# Step 5: Publish if requested
if [ "${1:-}" = "--publish" ]; then
  echo ""
  echo "Publishing to npmjs.com..."
  npm publish --access public
  echo "✓ Published @astudioplus/codegraph-mcp@$PKG_VERSION"
else
  echo ""
  echo "To publish: cd mcp-package && npm publish --access public"
fi
