#!/bin/bash
# Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
# SPDX-License-Identifier: Apache-2.0
#
# Package the npm MCP server distribution.
# Run from the repo root after all platform binaries are built.
#
# Usage:
# The engine is not bundled: it is fetched from the GitHub release at install
# time by bin/postinstall.js. Publish the release assets first with
# ./scripts/publish-release-assets.sh, or installs of this version will fail to
# find an engine.
#
# Usage:
#   ./scripts/package-npm.sh           # pack only
#   ./scripts/package-npm.sh --publish # also publish to npmjs.com

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PKG_DIR="$REPO_ROOT/mcp-package"
BIN_DIR="$PKG_DIR/bin"

# The package no longer bundles platform binaries. Shipping all four made it
# 88 MB compressed and 498 MB unpacked so that every user could run exactly one
# of them; the engine is now published once as release assets and fetched by
# bin/postinstall.js for the platform doing the installing.
#
# Any binary left over in mcp-package/bin/ from an older build is removed here,
# so a stale one cannot be published by accident.
echo "=== CodeGraph npm package builder ==="
echo ""

echo "Removing any bundled binaries (the engine is fetched at install time)..."
for stale in "$BIN_DIR"/codegraph-server-* "$BIN_DIR/onnxruntime.dll"; do
  if [ -e "$stale" ]; then
    rm -f "$stale"
    echo "  - removed $(basename "$stale")"
  fi
done

# The fetch path is what every install now depends on, so it is checked here
# rather than discovered by the first user to install the package.
echo ""
echo "Checking the engine fetch..."
( cd "$PKG_DIR" && node test/fetch-engine.test.js >/dev/null ) \
  && echo "  ✓ fetch-engine tests pass" \
  || { echo "  ✗ fetch-engine tests FAILED — not packaging"; exit 1; }

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
  echo ""
  echo "Updating MCP Registry..."
  if command -v mcp-publisher &>/dev/null; then
    mcp-publisher publish --server-json server.json
    echo "✓ MCP Registry updated"
  else
    echo "⚠ mcp-publisher not found — update the MCP Registry manually:"
    echo "  cd mcp-package && mcp-publisher publish --server-json server.json"
  fi
else
  echo ""
  echo "To publish: cd mcp-package && npm publish --access public"
  echo "Then update MCP Registry: mcp-publisher publish --server-json server.json"
fi
