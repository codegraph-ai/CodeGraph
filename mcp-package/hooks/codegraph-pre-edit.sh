#!/usr/bin/env bash
# Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
# SPDX-License-Identifier: Apache-2.0
#
# CodeGraph PreToolUse hook — nudges Claude to fetch architectural context
# before editing source files in a codegraph-indexed workspace.
#
# Triggered on Edit, Write, MultiEdit when the target file is a known source
# language. Outputs a JSON `additionalContext` reminder that Claude reads
# before deciding the edit. Does NOT block — purely advisory.
#
# Hook input (stdin, JSON):
#   {
#     "session_id": "...",
#     "cwd": "...",
#     "hook_event_name": "PreToolUse",
#     "tool_name": "Edit"|"Write"|"MultiEdit",
#     "tool_input": { "file_path": "/abs/path/to/file.rs", ... }
#   }
#
# Hook output (stdout, JSON):
#   {
#     "hookSpecificOutput": {
#       "hookEventName": "PreToolUse",
#       "permissionDecision": "allow",
#       "additionalContext": "..."
#     }
#   }
#
# Exit 0 always — purely advisory, never blocks.

set -euo pipefail

# Read event JSON from stdin
EVENT="$(cat)"

# jq-extract fields; tolerate missing
TOOL_NAME="$(printf '%s' "$EVENT" | jq -r '.tool_name // empty' 2>/dev/null)"
FILE_PATH="$(printf '%s' "$EVENT" | jq -r '.tool_input.file_path // empty' 2>/dev/null)"

# No file path → nothing to do
if [ -z "${FILE_PATH:-}" ]; then
  exit 0
fi

# Only act on source-code edits. Markdown, JSON, config, etc. don't need
# call-graph context.
case "$FILE_PATH" in
  *.rs|*.go|*.py|*.ts|*.tsx|*.js|*.jsx|*.mjs|*.cjs|\
  *.java|*.kt|*.scala|*.swift|*.m|*.mm|\
  *.c|*.cc|*.cpp|*.cxx|*.h|*.hpp|*.hxx|\
  *.cs|*.rb|*.php|*.lua|*.pl|*.ex|*.exs|*.erl|*.hrl|\
  *.hs|*.ml|*.mli|*.elm|*.dart|*.zig|*.r|*.jl|\
  *.tcl|*.f|*.f90|*.cobol|*.cob|*.sol|*.v|*.vh|*.sv)
    ;;
  *)
    exit 0
    ;;
esac

# Must be an absolute path (the hook is unreliable with cwd-relative paths)
case "$FILE_PATH" in
  /*) ;;
  *) exit 0 ;;
esac

# Find the workspace root: walk up from the file looking for the FIRST
# project-root marker (Cargo.toml, package.json, etc.). Stop there —
# don't keep walking past the project root, otherwise we'll find the
# user's `~/.codegraph/` global MCP state and mistakenly classify every
# file as "indexed."
DIR="$(dirname "$FILE_PATH")"
WORKSPACE_ROOT=""

while [ "$DIR" != "/" ] && [ -n "$DIR" ]; do
  if [ -f "$DIR/Cargo.toml" ] || [ -f "$DIR/package.json" ] || \
     [ -f "$DIR/pyproject.toml" ] || [ -f "$DIR/go.mod" ] || \
     [ -f "$DIR/build.gradle" ] || [ -f "$DIR/pom.xml" ] || \
     [ -d "$DIR/.git" ]; then
    WORKSPACE_ROOT="$DIR"
    break
  fi
  DIR="$(dirname "$DIR")"
done

# No workspace root found → file is probably ad-hoc, no nudge needed
if [ -z "$WORKSPACE_ROOT" ]; then
  exit 0
fi

# Indexed iff the workspace root has its own `.codegraph/` (per-project
# state dir), NOT the user's home `.codegraph/`.
if [ -d "$WORKSPACE_ROOT/.codegraph" ]; then
  INDEXED=1
else
  INDEXED=0
fi

# Build the nudge text. Keep it short — Claude reads this verbatim.
if [ "$INDEXED" = "1" ]; then
  NUDGE="codegraph: ${FILE_PATH##*/} is in an indexed workspace (${WORKSPACE_ROOT}). Before this edit, consider calling \`codegraph_get_edit_context\` (uri='file://${FILE_PATH}') to see callers, callees, and related symbols. If you're changing a function signature, also run \`codegraph_analyze_impact\` to see the blast radius. Skip these if you've already gathered the context this turn."
else
  NUDGE="codegraph: ${FILE_PATH##*/} is in a project (${WORKSPACE_ROOT}) that doesn't appear to be indexed by codegraph. If this edit changes a function signature or moves code across modules, consider running \`codegraph_reindex_workspace\` or \`codegraph_index_directory\` first so impact analysis works."
fi

# Emit JSON output the harness pipes back to Claude
jq -n --arg ctx "$NUDGE" '{
  hookSpecificOutput: {
    hookEventName: "PreToolUse",
    permissionDecision: "allow",
    additionalContext: $ctx
  }
}'
