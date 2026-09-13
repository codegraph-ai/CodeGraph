#!/bin/bash
# Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
# SPDX-License-Identifier: Apache-2.0
#
# Package the npm MCP server distribution.
# Run from the repo root after all platform binaries are built.
#
# The engine is not bundled: it is fetched from the GitHub release at install
# time by bin/postinstall.js. Publish the release assets first with
# ./scripts/publish-release-assets.sh - this script refuses to pack until every
# asset for the pinned engine version is on the release, because an install
# without them succeeds and then has nothing to run.
#
# Usage:
#   ./scripts/package-npm.sh           # pack only
#   ./scripts/package-npm.sh --publish # also publish to npmjs.com
#
# --publish checks both registries' credentials before doing any work, so it
# needs a terminal: an expired npm session is renewed with an interactive
# `npm login` (the account has 2FA). The MCP Registry is reached with the token
# from `gh auth token`, which must carry read:org; set CODEGRAPH_MCP_TOKEN to
# use a PAT limited to that scope instead. Packing alone needs no credentials.

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

# ------------------------------------------------------------------ auth
#
# The credentials are checked here, before the tests, the asset probe and the
# pack - not at the point of use. Every one of those has to pass anyway, and
# discovering an unusable credential after them means doing them again. The last
# release failed exactly there: `npm publish` ran after several minutes of work
# and `mcp-publisher` after that, so a stale token surfaced at the end.
#
# What is checked here is deliberately not what is minted here. The npm session
# is long-lived, so logging in now is the whole fix for that half. The MCP
# Registry's token lives 300 seconds - less than the rest of this script takes -
# so it is minted at the publish step instead, and what is checked now is the
# GitHub token it will be minted from, which is the credential that goes stale.
#
# Only for --publish. Packing needs no credentials, and this script also runs as
# a plain build step, where prompting for a login would hang it.
MCP_TOKEN=""
if [ "${1:-}" = "--publish" ]; then
  echo "Checking publish credentials..."

  # npm's own session. Left interactive on purpose: the account has 2FA, so this
  # needs a human and a TTY, and that is better spent now than after the pack.
  if npm_user="$(npm whoami 2>/dev/null)"; then
    echo "  ✓ npm authenticated as $npm_user"
  else
    echo "  npm: not logged in - starting login (2FA expected)"
    npm login || { echo "  ✗ npm login failed - not packaging" >&2; exit 1; }
    npm_user="$(npm whoami 2>/dev/null || echo '<unknown>')"
    echo "  ✓ npm authenticated as $npm_user"
  fi

  # The MCP Registry decides which namespaces a token may publish to by calling
  # GET /user/memberships/orgs and granting io.github.<org>/* for every org the
  # account owns; GitHub gates that call behind read:org. A token without the
  # scope does not fail to log in - GitHub answers 403, the registry reads that
  # as "owns no organisations", and it issues a perfectly valid token scoped to
  # io.github.<user>/* alone. The 403 then arrives at the publish, carrying a
  # message about organisation membership that is not the actual cause.
  #
  # A successful login therefore cannot tell the two cases apart, so the
  # precondition is checked against GitHub directly instead of inferred from one.
  #
  # `gh auth token` already carries read:org. It also carries repo and workflow,
  # which is broader than the registry needs; a PAT limited to read:org can be
  # substituted by setting CODEGRAPH_MCP_TOKEN.
  if command -v mcp-publisher >/dev/null 2>&1; then
    MCP_TOKEN="${CODEGRAPH_MCP_TOKEN:-}"
    if [ -z "$MCP_TOKEN" ] && command -v gh >/dev/null 2>&1; then
      MCP_TOKEN="$(gh auth token 2>/dev/null || true)"
    fi
    if [ -z "$MCP_TOKEN" ]; then
      echo "  ✗ no GitHub token for the MCP Registry - not packaging" >&2
      echo "    Run 'gh auth login', or set CODEGRAPH_MCP_TOKEN to a PAT with read:org." >&2
      exit 1
    fi

    gh_body="$(mktemp)"
    trap 'rm -f "$gh_body"' EXIT
    gh_api() {
      curl -sS -o "$gh_body" -w '%{http_code}' \
        -H "Authorization: Bearer $MCP_TOKEN" \
        -H "Accept: application/vnd.github+json" \
        -H "X-GitHub-Api-Version: 2022-11-28" \
        "https://api.github.com/$1" 2>/dev/null || echo 000
    }
    # GitHub's error bodies carry no trailing newline, which would otherwise run
    # the hint that follows onto the last line of the JSON.
    gh_body_err() { printf '%s\n' "$(sed 's/^/    /' "$gh_body")" >&2; }

    gh_status="$(gh_api user)"
    if [ "$gh_status" != "200" ]; then
      echo "  ✗ GitHub rejected the token (HTTP $gh_status) - not packaging" >&2
      gh_body_err
      echo "    Run 'gh auth login', or set CODEGRAPH_MCP_TOKEN to a live PAT with read:org." >&2
      exit 1
    fi
    gh_login="$(node -e "
      console.log(JSON.parse(require('fs').readFileSync(process.argv[1], 'utf8')).login);
    " "$gh_body")"

    # Read the namespace from server.json rather than naming it here: that file is
    # what the publish is authorised against, so renaming the server must not be
    # able to leave this check passing against the namespace it used to use.
    MCP_NAMESPACE="$(node -e "console.log(require('$PKG_DIR/server.json').name.split('/')[0])")"
    case "$MCP_NAMESPACE" in
      io.github.*) mcp_owner="${MCP_NAMESPACE#io.github.}" ;;
      *) mcp_owner="" ;;
    esac

    if [ -z "$mcp_owner" ]; then
      echo "  ⚠ $MCP_NAMESPACE is not an io.github.* namespace - ownership not checked"
    elif [ "$(printf '%s' "$mcp_owner" | tr '[:upper:]' '[:lower:]')" = "$(printf '%s' "$gh_login" | tr '[:upper:]' '[:lower:]')" ]; then
      echo "  ✓ $MCP_NAMESPACE is the token's own user namespace ($gh_login)"
    else
      gh_status="$(gh_api 'user/memberships/orgs?per_page=100')"
      if [ "$gh_status" = "403" ]; then
        echo "  ✗ the GitHub token cannot read organisation membership - not packaging" >&2
        echo "    GitHub answered 403 for GET /user/memberships/orgs, which needs read:org." >&2
        echo "    Without it the Registry sees no organisations and refuses $MCP_NAMESPACE." >&2
        echo "    Use 'gh auth token', or set CODEGRAPH_MCP_TOKEN to a PAT with read:org." >&2
        exit 1
      fi
      if [ "$gh_status" != "200" ]; then
        echo "  ✗ could not read organisation membership from GitHub (HTTP $gh_status)" >&2
        gh_body_err
        exit 1
      fi
      # The Registry grants io.github.<org>/* to owners only, which this endpoint
      # reports as role "admin". An active plain membership is refused at the
      # publish just as a missing one is, so both are refused here.
      membership="$(node -e "
        const want = process.argv[2].toLowerCase();
        const orgs = JSON.parse(require('fs').readFileSync(process.argv[1], 'utf8'));
        const m = (Array.isArray(orgs) ? orgs : []).find(
          (o) => ((o.organization || {}).login || '').toLowerCase() === want);
        console.log(m ? m.role + '/' + m.state : 'none/none');
      " "$gh_body" "$mcp_owner")"
      if [ "$membership" != "admin/active" ]; then
        echo "  ✗ $gh_login does not own the $mcp_owner organisation - not packaging" >&2
        echo "    GET /user/memberships/orgs reports role/state: $membership" >&2
        echo "    The Registry grants $MCP_NAMESPACE to owners (role admin) only." >&2
        exit 1
      fi
      echo "  ✓ $gh_login owns $mcp_owner - $MCP_NAMESPACE is publishable"
    fi

    rm -f "$gh_body"
    trap - EXIT
  else
    echo "  ⚠ mcp-publisher not on PATH - the MCP Registry step will be skipped"
  fi
  echo ""
fi

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
#
# server.json carries the release version twice: once at the top level, and once
# inside the npm entry of `packages`, which is the field the MCP Registry
# resolves the tarball from. Nothing synchronises the three numbers - they are
# hand-edited - so all of them are compared, not just the top-level one. A
# release that bumped package.json and server.json but missed the nested version
# would register a new server entry pointing at the previous tarball, and the
# Registry would accept it because that older tarball exists and carries the
# right mcpName.
PKG_VERSION=$(node -e "console.log(require('$PKG_DIR/package.json').version)")
SERVER_VERSION=$(node -e "console.log(require('$PKG_DIR/server.json').version)")
SERVER_NPM_VERSION=$(node -e "
  const npm = (require('$PKG_DIR/server.json').packages || [])
    .find((p) => p.registryType === 'npm');
  console.log(npm ? npm.version : '<no npm package entry>');
")
echo ""
echo "package.json version:      $PKG_VERSION"
echo "server.json version:       $SERVER_VERSION"
echo "server.json npm package:   $SERVER_NPM_VERSION"

# A mismatch here is fatal rather than a warning. The two files are published to
# two different registries under one version, and a warning scrolls past in the
# npm pack output - leaving npmjs.com and the MCP Registry disagreeing about what
# this release is, which cannot be corrected by republishing the same version.
if [ "$PKG_VERSION" != "$SERVER_VERSION" ]; then
  echo "ERROR: version mismatch between package.json ($PKG_VERSION) and server.json ($SERVER_VERSION)" >&2
  exit 1
fi
if [ "$PKG_VERSION" != "$SERVER_NPM_VERSION" ]; then
  echo "ERROR: version mismatch between package.json ($PKG_VERSION) and the npm entry in server.json ($SERVER_NPM_VERSION)" >&2
  echo "  The MCP Registry resolves the tarball from packages[].version, so this would" >&2
  echo "  publish a $PKG_VERSION server entry pointing at the $SERVER_NPM_VERSION tarball." >&2
  exit 1
fi

# The npm package contains no engine; every install fetches one from the release
# tagged with the engine version pinned in bin/fetch-engine.js, which is
# deliberately not this package's version (see the ENGINE_VERSION comment there:
# a client-only patch release must not start asking for a tag nobody published).
# Publishing before those assets exist produces a package that installs cleanly
# and then has nothing to run.
#
# Every asset is probed, not just one. publish-release-assets.sh uploads the
# whole staging directory in a single `gh release upload`, so a network drop or
# a rate limit part-way through leaves the release with some platforms attached
# and others missing - and a one-platform probe would wave that through, giving
# users on the missing platforms exactly the empty install this gate exists to
# prevent.
ENGINE_VERSION=$(node -e "console.log(require('$PKG_DIR/bin/fetch-engine').ENGINE_VERSION)")
# Read from fetch-engine.js rather than repeating it here. A copy of this list
# drifts silently: it stays green while probing a set that no longer matches
# what the clients resolve, which is the exact failure this gate exists to
# catch. The Windows sidecar is appended because it is a required asset without
# being an engine, so it is not in PUBLISHED_BINARIES.
# A `while read` loop rather than `mapfile`: this script's shebang is
# /bin/bash, which on macOS is bash 3.2, and mapfile arrived in bash 4.
ENGINE_ASSETS=()
while IFS= read -r asset; do
  [ -n "$asset" ] && ENGINE_ASSETS+=("$asset")
done < <(node -e "
  const f = require('$PKG_DIR/bin/fetch-engine');
  for (const a of f.PUBLISHED_BINARIES) console.log(a);
  console.log(f.WINDOWS_SIDECAR);
")
if [ "${#ENGINE_ASSETS[@]}" -lt 2 ]; then
  echo "ERROR: could not read the published asset list from bin/fetch-engine.js" >&2
  exit 1
fi
RELEASE_BASE="https://github.com/codegraph-ai/CodeGraph/releases/download/v${ENGINE_VERSION}"

echo ""
echo "engine version:            $ENGINE_VERSION (fetched at install time)"
echo "Checking published engine assets for v${ENGINE_VERSION}..."
missing_assets=0
for asset in "${ENGINE_ASSETS[@]}"; do
  # A binary and its checksum are separate assets and the client needs both, so
  # both are probed. The binaries are requested one byte at a time - presence is
  # the question here, and downloading ~120 MB to answer it is not worth it.
  if ! curl -fsSL -o /dev/null -r 0-0 "$RELEASE_BASE/$asset" \
    || ! curl -fsSL -o /dev/null "$RELEASE_BASE/$asset.sha256"; then
    printf '  ✗ %s\n' "$asset"
    missing_assets=1
  else
    printf '  ✓ %s\n' "$asset"
  fi
done

if [ "$missing_assets" -ne 0 ]; then
  echo "ERROR: the release v${ENGINE_VERSION} is missing engine assets (binary or .sha256)." >&2
  echo "  Run ./scripts/publish-release-assets.sh --publish first, or installs on those" >&2
  echo "  platforms will find no engine." >&2
  exit 1
fi
echo "  ✓ every engine asset is published for v${ENGINE_VERSION}"

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
    # The Registry's token is minted here, not in the preflight above: it lives
    # 300 seconds, and the tests, the asset probe, the pack and an interactive
    # npm 2FA prompt all happen in between. The preflight established that this
    # GitHub token can reach the namespace, so this is expected to succeed - it
    # is checked anyway, because the npm publish above is already irreversible.
    if ! login_log="$(mcp-publisher login github -token "${MCP_TOKEN:-}" 2>&1)"; then
      printf '%s\n' "$login_log" >&2
      echo "✗ mcp-publisher login failed - npmjs.com has $PKG_VERSION but the MCP" >&2
      echo "  Registry does not. Nothing else is needed; re-run just that step:" >&2
      echo "  cd mcp-package && mcp-publisher login github -token \"\$(gh auth token)\" \\" >&2
      echo "    && mcp-publisher publish --server-json server.json" >&2
      exit 1
    fi
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
