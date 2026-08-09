#!/bin/bash
# Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
# SPDX-License-Identifier: Apache-2.0
#
# Package the npm MCP server distribution.
# Run from the repo root after all platform binaries are built.
#
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

# The fetch path is what every install now depends on, and the wrapper's
# argument contract is what the crash loop came down to, so both are checked
# here rather than discovered by the first user to install the package. The
# package's own `npm test` is the single list of what must pass, so a test added
# there is not silently skipped by this gate.
echo ""
echo "Checking the engine fetch and the wrapper arguments..."
if ! test_log="$( cd "$PKG_DIR" && npm test 2>&1 )"; then
  printf '%s\n' "$test_log" >&2
  echo "  ✗ package tests FAILED - not packaging" >&2
  exit 1
fi
echo "  ✓ package tests pass"

# Step 3: Verify version consistency
PKG_VERSION=$(node -e "console.log(require('$PKG_DIR/package.json').version)")
SERVER_VERSION=$(node -e "console.log(require('$PKG_DIR/server.json').version)")
echo ""
echo "package.json version: $PKG_VERSION"
echo "server.json version:  $SERVER_VERSION"

# A mismatch here is fatal rather than a warning. The two files are published to
# two different registries under one version, and a warning scrolls past in the
# npm pack output - leaving npmjs.com and the MCP Registry disagreeing about what
# this release is, which cannot be corrected by republishing the same version.
if [ "$PKG_VERSION" != "$SERVER_VERSION" ]; then
  echo "ERROR: version mismatch between package.json ($PKG_VERSION) and server.json ($SERVER_VERSION)" >&2
  exit 1
fi

# The npm package contains no engine; every install fetches one from the release
# tagged with this version. Publishing before those assets exist produces a
# package that installs cleanly and then has nothing to run.
ENGINE_VERSION=$(node -e "console.log(require('$PKG_DIR/bin/fetch-engine').ENGINE_VERSION)")
echo "engine version:       $ENGINE_VERSION (fetched at install time)"
if ! curl -fsSL -o /dev/null \
  "https://github.com/codegraph-ai/CodeGraph/releases/download/v${ENGINE_VERSION}/codegraph-server-linux-x64.sha256"; then
  echo "ERROR: no published engine assets for v${ENGINE_VERSION}" >&2
  echo "  Run ./scripts/publish-release-assets.sh first, or installs will find no engine." >&2
  exit 1
fi
echo "  ✓ engine assets are published for v${ENGINE_VERSION}"

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
