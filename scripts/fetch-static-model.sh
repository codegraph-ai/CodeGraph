#!/usr/bin/env bash
# Fetch the distilled static embedding model from the release-independent
# `model` GitHub release (decoupled from versioned releases — bumping the app
# version never requires re-uploading the model).
#
# Populates the server's default resolve path so `--embedding-model static`
# works without any further configuration:
#       scripts/fetch-static-model.sh
#
# The model is no longer staged into the VS Code extension bundle: the VSIX
# excludes bin/** (see vscode/.vscodeignore), so a copy placed there would be
# dropped at package time. IDE users point codegraph.staticModelPath at the
# directory this script writes instead.
#
# Usage: scripts/fetch-static-model.sh [DEST_DIR] [MODEL_NAME]
#   DEST_DIR    default ~/.codegraph/static_models/<MODEL_NAME>
#   MODEL_NAME  default jina-code-static-256
#
# Expects a `<MODEL_NAME>.tar.gz` asset on the `model` release whose contents
# are the model files at the archive root, created with:
#   tar czf jina-code-static-256.tar.gz -C <model-dir> .
set -euo pipefail

MODEL="${2:-jina-code-static-256}"
DEST="${1:-$HOME/.codegraph/static_models/$MODEL}"
URL="https://github.com/codegraph-ai/CodeGraph/releases/download/model/${MODEL}.tar.gz"

mkdir -p "$DEST"
echo "fetching ${MODEL}.tar.gz -> $DEST"
curl -fsSL "$URL" | tar xz -C "$DEST"

if [ ! -f "$DEST/model.safetensors" ]; then
    echo "error: model.safetensors missing after extract — check the '$MODEL' release asset" >&2
    exit 1
fi
echo "static model ready: $(ls "$DEST" | tr '\n' ' ')"
