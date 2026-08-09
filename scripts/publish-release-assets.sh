#!/bin/bash
# Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
# SPDX-License-Identifier: Apache-2.0
#
# Publish the per-platform engine binaries as GitHub release assets.
#
# No channel bundles engines any more. Shipping all four platform binaries meant
# a 118 MB VSIX and a 498 MB npm package for the ~30 MB a given user can
# actually run, and the JetBrains Marketplace cannot ship per-platform artifacts
# at all. The binaries are published here once, and each client fetches only
# what it needs - which makes this release the single source of engines, so a
# missing or mistagged one leaves every channel with no engine at all.
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
# binaries, and every client asks for them by the engine version it pins.
VERSION="$(grep -m1 '^version' "$REPO_ROOT/Cargo.toml" | sed 's/.*"\(.*\)".*/\1/' || true)"
if [ -z "$VERSION" ]; then
  echo "ERROR: no 'version = \"...\"' line in $REPO_ROOT/Cargo.toml." >&2
  echo "Every asset and every client pin is keyed on it, so nothing can be" >&2
  echo "published without it." >&2
  exit 1
fi
TAG="v${VERSION}"

# Read the platform list from bin/fetch-engine.js instead of repeating it. That
# module is what every client resolves against, so a copy here can publish a set
# the clients never ask for, or omit one they do - and the failure only shows up
# as an empty install on the platform nobody tested. A `while read` loop rather
# than `mapfile`, which needs bash 4 and is absent from macOS's bash 3.2.
FETCH_ENGINE="$REPO_ROOT/mcp-package/bin/fetch-engine.js"
BINARIES=()
while IFS= read -r asset; do
  [ -n "$asset" ] && BINARIES+=("$asset")
done < <(node -e "
  for (const a of require('$FETCH_ENGINE').PUBLISHED_BINARIES) console.log(a);
" 2>/dev/null)

if [ "${#BINARIES[@]}" -eq 0 ]; then
  echo "ERROR: could not read PUBLISHED_BINARIES from $FETCH_ENGINE." >&2
  echo "That list is what the clients fetch by, so publishing without it would" >&2
  echo "guess at the platform set." >&2
  exit 1
fi

# The Windows engine loads this at runtime. Shipping the exe without it gives
# users a download that succeeds and then fails at startup, which is a worse
# outcome than no download at all - so it is treated as required, not optional.
WINDOWS_SIDECAR="onnxruntime.dll"

echo "CodeGraph release assets"
echo "  version: $VERSION"
echo "  tag:     $TAG"
echo "  repo:    $REPO"
echo

# ---------------------------------------------------------------- pins
# Each client hard-codes the engine release it fetches, so that a client-only
# patch release does not start asking for a tag that was never published. That
# only works while the pins agree with the engine being published here: a pin
# ahead of this tag 404s on every fresh install, and one behind it silently
# installs the previous engine. Checked before anything is uploaded, because
# after the fact the only symptom is "CodeGraph has no engine".
PIN_SOURCES=(
  "mcp-package/bin/fetch-engine.js|npm + VS Code"
  "jetbrains/src/main/kotlin/ai/codegraph/jetbrains/server/CodeGraphServerResolver.kt|JetBrains"
)

pin_mismatch=0
for entry in "${PIN_SOURCES[@]}"; do
  file="${entry%%|*}"
  label="${entry##*|}"
  pin="$(grep -m1 'ENGINE_VERSION = "' "$REPO_ROOT/$file" | sed 's/.*"\(.*\)".*/\1/' || true)"
  if [ "$pin" = "$VERSION" ]; then
    printf '  ✓ %-14s pins engine %s\n' "$label" "$pin"
  else
    printf '  ✗ %-14s pins engine %s, not %s (%s)\n' \
      "$label" "${pin:-<not found>}" "$VERSION" "$file"
    pin_mismatch=1
  fi
done

if [ "$pin_mismatch" -ne 0 ]; then
  cat >&2 <<EOF

ERROR: a client pins an engine version other than $VERSION.

Clients fetch the engine by the version they pin, and no channel bundles one any
more, so a pin naming an unpublished tag means no engine at all on that channel.
Set the ENGINE_VERSION constants to $VERSION, or publish the version they
already name, then re-run.
EOF
  exit 1
fi
echo

# ---------------------------------------------------------------- verify
#
# Presence is not enough. vscode/bin/ is a build staging directory that is not
# cleaned between releases, so a binary from a previous version sits there
# looking exactly as valid as a fresh one - and publishing a stale engine under
# a new tag is worse than publishing nothing, because the checksums are
# perfectly correct for the wrong build.
#
# Provenance is recorded where it is knowable rather than guessed here: each
# binary is stamped into MANIFEST by the build that produced it, on the host
# that could actually run `--info`. Scraping version strings out of a
# cross-platform image was tried and is not reliable - the engine does not
# store its version as a standalone string on every target, so good binaries
# were reported as stale.
MANIFEST="$VSCODE_BIN/BUILD-MANIFEST"

# The binary name is the last field, so a line whose second field is that name
# is one written before the manifest carried a commit at all.
manifest_line() {
  [ -f "$MANIFEST" ] || return 0
  awk -v b="$1" '$NF == b { print; exit }' "$MANIFEST"
}

manifest_version() {
  local line ver
  line="$(manifest_line "$1")"
  if [ -n "$line" ]; then
    read -r ver _ <<< "$line"
    printf '%s\n' "$ver"
  fi
}

manifest_commit() {
  local line ver commit
  line="$(manifest_line "$1")"
  if [ -n "$line" ]; then
    read -r ver commit _ <<< "$line"
    if [ "$commit" != "$1" ]; then
      printf '%s\n' "$commit"
    fi
  fi
}

missing=0
stale=0
COMMITS=()
for bin in "${BINARIES[@]}" "$WINDOWS_SIDECAR"; do
  if [ ! -f "$VSCODE_BIN/$bin" ]; then
    printf '  ✗ %-36s MISSING\n' "$bin"
    missing=1
    continue
  fi

  size="$(du -h "$VSCODE_BIN/$bin" | cut -f1)"

  # The sidecar is a third-party library with no CodeGraph version of its own.
  if [ "$bin" = "$WINDOWS_SIDECAR" ]; then
    printf '  ✓ %-36s %s\n' "$bin" "$size"
    continue
  fi

  recorded_version="$(manifest_version "$bin")"
  recorded_commit="$(manifest_commit "$bin")"

  if [ "$recorded_version" != "$VERSION" ]; then
    printf '  ✗ %-36s %s  NOT STAMPED %s\n' \
      "$bin" "$size" "${recorded_version:+(manifest says: $recorded_version)}"
    stale=1
  elif [ -z "$recorded_commit" ]; then
    printf '  ✗ %-36s %s  NO COMMIT RECORDED\n' "$bin" "$size"
    stale=1
  else
    printf '  ✓ %-36s %s  (%s @ %s)\n' "$bin" "$size" "$VERSION" "$recorded_commit"
    COMMITS+=("$recorded_commit")
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

if [ "$stale" -ne 0 ]; then
  cat >&2 <<EOF

ERROR: at least one binary is not stamped as $VERSION with a source commit in
$VSCODE_BIN/BUILD-MANIFEST.

vscode/bin/ is not cleaned between releases, so an unstamped binary is assumed
to be left over from an earlier one. Rebuild it and record it with:

  ./scripts/stamp-binary.sh <name> <version> [commit]

A binary stamped with no commit was recorded before the manifest carried one;
re-stamp it, from the host that can run it or with the commit stated, so the
set can be checked for agreement.

Publishing an unverified binary would produce a release whose checksums are
perfectly valid for the wrong build - the hardest kind of mistake to notice.
EOF
  exit 1
fi

# ------------------------------------------------------ one commit, all assets
# A shared version number is not a shared source. Each platform is built on its
# own host, so five binaries can all be stamped $VERSION and still come from
# five different trees - and then a bug reported against $VERSION on Linux and
# one reported against $VERSION on macOS describe different software under the
# same name. Compared by prefix: each host's git abbreviates commits to its own
# length, so the same commit legitimately appears as 7 and 12 hex digits.
ref_commit=""
for c in "${COMMITS[@]}"; do
  if [ -z "$ref_commit" ] || [ "${#c}" -lt "${#ref_commit}" ]; then
    ref_commit="$c"
  fi
done

commit_mismatch=0
for c in "${COMMITS[@]}"; do
  case "$c" in "$ref_commit"*) ;; *) commit_mismatch=1 ;; esac
done

if [ "$commit_mismatch" -ne 0 ]; then
  {
    echo
    echo "ERROR: the staged binaries were not all built from the same commit."
    echo
    for bin in "${BINARIES[@]}"; do
      printf '  %-36s %s\n' "$bin" "$(manifest_commit "$bin")"
    done
    cat <<EOF

Everything in one release has to come from one tree, or the tag names a build
that never existed as a whole. Rebuild the assets that disagree from the commit
being released - see cross-platform-builds.md for the per-platform hosts - and
re-stamp them with ./scripts/stamp-binary.sh.
EOF
  } >&2
  exit 1
fi

echo
printf '  all %d engine binaries built from %s\n' "${#COMMITS[@]}" "$ref_commit"

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
