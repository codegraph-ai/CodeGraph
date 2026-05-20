# Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
# SPDX-License-Identifier: Apache-2.0
#
# CodeGraph PreToolUse hook (Windows / PowerShell variant) — nudges
# Claude to fetch architectural context before editing source files
# in a codegraph-indexed workspace.
#
# Mirrors hooks/codegraph-pre-edit.sh; same input/output contract.
#
# Hook input  (stdin, JSON):
#   {tool_name, tool_input: {file_path, ...}, ...}
# Hook output (stdout, JSON):
#   {hookSpecificOutput: {hookEventName, permissionDecision, additionalContext}}
# Exit code: always 0 — purely advisory, never blocks.

$ErrorActionPreference = 'Stop'

# Read the entire stdin as a single JSON document.
$raw = [Console]::In.ReadToEnd()
if (-not $raw) { exit 0 }

try {
    $event = $raw | ConvertFrom-Json -ErrorAction Stop
} catch {
    # Malformed JSON — don't block, just exit silently.
    exit 0
}

$toolName = $event.tool_name
$filePath = $event.tool_input.file_path
if (-not $filePath) { exit 0 }

# Only act on source-code edits. Markdown, JSON, config, etc. don't need
# call-graph context.
$sourceExtensions = @(
    '.rs', '.go', '.py', '.ts', '.tsx', '.js', '.jsx', '.mjs', '.cjs',
    '.java', '.kt', '.scala', '.swift', '.m', '.mm',
    '.c', '.cc', '.cpp', '.cxx', '.h', '.hpp', '.hxx',
    '.cs', '.rb', '.php', '.lua', '.pl', '.ex', '.exs', '.erl', '.hrl',
    '.hs', '.ml', '.mli', '.elm', '.dart', '.zig', '.r', '.jl',
    '.tcl', '.f', '.f90', '.cobol', '.cob', '.sol', '.v', '.vh', '.sv'
)

# PowerShell on Windows is case-insensitive by default for paths, but
# extension comparison should still be normalised.
$ext = [System.IO.Path]::GetExtension($filePath).ToLower()
if (-not ($sourceExtensions -contains $ext)) { exit 0 }

# Must be a rooted (absolute) path — Path.IsPathRooted handles both
# Unix-style and Windows-style absolute paths.
if (-not [System.IO.Path]::IsPathRooted($filePath)) { exit 0 }

# Walk up from the file's directory looking for the FIRST project-root
# marker. Stop there — don't keep walking past, otherwise we'll find
# the user's `~/.codegraph/` global MCP state on Unix or
# `%USERPROFILE%\.codegraph` on Windows.
$dir = [System.IO.Path]::GetDirectoryName($filePath)
$workspaceRoot = $null
$rootMarkers = @('Cargo.toml', 'package.json', 'pyproject.toml', 'go.mod',
                 'build.gradle', 'pom.xml')

while ($dir -and -not [string]::IsNullOrEmpty($dir)) {
    $foundMarker = $false
    foreach ($marker in $rootMarkers) {
        if (Test-Path (Join-Path $dir $marker) -PathType Leaf) {
            $foundMarker = $true
            break
        }
    }
    if (-not $foundMarker -and (Test-Path (Join-Path $dir '.git') -PathType Container)) {
        $foundMarker = $true
    }
    if ($foundMarker) {
        $workspaceRoot = $dir
        break
    }
    $parent = [System.IO.Path]::GetDirectoryName($dir)
    if ($parent -eq $dir) { break }   # reached filesystem root
    $dir = $parent
}

# No workspace root found → file is probably ad-hoc, no nudge needed.
if (-not $workspaceRoot) { exit 0 }

# Indexed iff the workspace root has its own `.codegraph/` (per-project
# state dir), NOT the user's home `.codegraph/`.
$indexed = Test-Path (Join-Path $workspaceRoot '.codegraph') -PathType Container

# Build the nudge text.
$baseName = [System.IO.Path]::GetFileName($filePath)
# Convert backslashes to forward slashes for the file:// URI form.
$uriPath = $filePath -replace '\\', '/'
if ($uriPath -notmatch '^/') {
    # Windows drive path → file:///C:/foo/bar.rs
    $uriPath = "/$uriPath"
}

if ($indexed) {
    $nudge = "codegraph: $baseName is in an indexed workspace ($workspaceRoot). Before this edit, consider calling ``codegraph_get_edit_context`` (uri='file://$uriPath') to see callers, callees, and related symbols. If you're changing a function signature, also run ``codegraph_analyze_impact`` to see the blast radius. Skip these if you've already gathered the context this turn."
} else {
    $nudge = "codegraph: $baseName is in a project ($workspaceRoot) that doesn't appear to be indexed by codegraph. If this edit changes a function signature or moves code across modules, consider running ``codegraph_reindex_workspace`` or ``codegraph_index_directory`` first so impact analysis works."
}

# Emit JSON output the harness pipes back to Claude.
$output = @{
    hookSpecificOutput = @{
        hookEventName       = 'PreToolUse'
        permissionDecision  = 'allow'
        additionalContext   = $nudge
    }
} | ConvertTo-Json -Depth 4 -Compress:$false

# Write JSON to stdout, no trailing newline ambiguity.
[Console]::Out.Write($output)
exit 0
