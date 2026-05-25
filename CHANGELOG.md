# Changelog

All notable changes to CodeGraph are documented here. Versions follow
[Semantic Versioning](https://semver.org/). Each release is tagged as
`vscode/vX.Y.Z` in the git history.

## [0.17.0] — 2026-05-25

### Documentation Intelligence (new)

Index your project's design docs and keep them in sync with the codebase — 7 new tools:

- **`codegraph_index_markdown`** — Index a local `.md` file into the persistent docs store.
  Parses the heading hierarchy (`##`/`###`/`####`) into a tree and embeds only leaf sections
  for precise retrieval. Persists across sessions.
- **`codegraph_search_docs`** — Semantic search over indexed docs. Returns matching sections
  with heading-path breadcrumbs (e.g. *Authentication > JWT > Refresh Token Rotation*).
- **`codegraph_verify_design`** — Cross-reference a design doc against the code graph.
  Extracts backtick-wrapped identifiers and checks each against the codebase.
  Supports `direction=forward` (doc→code), `reverse` (code→doc), or `both`.
- **`codegraph_design_gaps`** — Find things described in docs that don't exist in code yet.
  Build TODO lists directly from architecture specs.
- **`codegraph_generate_architecture_doc`** — Auto-generate a structured ARCHITECTURE.md
  from the live code graph: module breakdown, complexity hotspots, hot paths, circular deps.
- **`codegraph_list_doc_sources`** / **`codegraph_remove_doc_source`** — Manage indexed docs.
- **`codegraph_get_ai_context`** now auto-augments responses with a `design_context` section
  when indexed docs mention the file being queried — design context without extra tool calls.

### Tool Profiles

New `--profile` flag (also `CODEGRAPH_TOOL_PROFILE` env var) scopes the MCP tool surface:

| Profile | Tools | Best for |
|---------|-------|----------|
| `all` | all 41 | normal sessions (default) |
| `core` | 8 | chatty agent sessions — search + symbol info + AI context only |
| `graph` | 16 | refactoring — callers/callees/deps/impact/traverse |
| `memory` | 14 | knowledge workflows — memory + docs tools |
| `security` | pro only | security audits |

### Crash Recovery

- **Stale RocksDB LOCK detection** — after a crash, the next server launch probes the LOCK
  file with `fs2::try_lock_exclusive` and clears it if no live process holds it. Verified on
  macOS (arm64), Windows (LockFileEx semantics), and SLES Linux (POSIX fcntl).
- **Panic hook + signal handlers** — SIGINT/SIGTERM/panic all funnel through `process::exit`
  so the OS releases the LOCK at exit. WAL durability covers in-flight writes.
- **Loud error surfacing** — storage-open failures now log `MessageType::ERROR` to the LSP
  client instead of silently falling back to memory-only mode.

### Hardened Workspace Indexing

- Default exclude list expanded from 14 → 47 directories: Python tooling (`.venv`, `.tox`,
  `.pytest_cache`, `.mypy_cache`), Node (`.next`, `.nuxt`, `.parcel-cache`), iOS (`Pods`,
  `xcuserdata`), IaC (`.terraform`, `.serverless`), and sensitive credential dirs (`.aws`,
  `.ssh`, `.gnupg`, `.kube`, `.docker`).
- Secret file extensions blocked from indexing: `*.pem`, `*.key`, `*.p12`, `*.pfx`, `*.crt`,
  `*.gpg`, `*.kdbx`, SSH key filenames (`id_rsa`, `id_ed25519`, `known_hosts`,
  `authorized_keys`), Terraform state (`*.tfstate`).

### Memory Improvements

- **`agentSource`** — optional tag on `codegraph_memory_store` identifying which AI agent
  stored the memory (`"claude"`, `"cursor"`, `"windsurf"`, `"codex"`, `"cline"`). Surfaces
  in `codegraph_memory_list` for cross-agent attribution.

### Other

- **codegraph-rules-for-agents** — [companion repo](https://github.com/codegraph-ai/codegraph-rules-for-agents)
  with per-agent rule files that bias Claude / Cursor / Windsurf / Codex / Cline to use
  CodeGraph tools before grep / multi-file reads.
- `.env` excluded from VSIX packages (PostHog key is baked into `extension.js` at build time).
- Combined (universal) VSIX now generated alongside platform-specific ones.
- 41 community tools (was 34).

---

## [0.16.6] — 2026-05-24

Internal release — version alignment + crash recovery groundwork. Rolled into 0.17.0.

## [0.16.4] — 2026-05-23

### Bug Fixes

- **UTF-8 panic fix** (issue [#3](https://github.com/codegraph-ai/CodeGraph/issues/3)) —
  `&source[..4096]` byte slice in `codegraph-c` panics when a multi-byte character (CJK,
  emoji) straddles the boundary. Added `truncate_at_char_boundary` helper; fixed 3 sites
  across `codegraph-c`, `codegraph-toml`, and `codegraph-harness`.
- Rust workspace version aligned to 0.16.4 (was stuck at 0.15.0 since initial release).

## [0.16.3] — 2026-05-23

### Bug Fixes

- **Windows path quoting** (issue [#2](https://github.com/codegraph-ai/CodeGraph/issues/2)) —
  server binary path with spaces in the Windows username caused EPIPE on launch. Fixed by
  quoting the path in the VS Code extension.
- **TypeScript parser error tolerance** (issue [#1](https://github.com/codegraph-ai/CodeGraph/issues/1)) —
  files with syntax errors were fully rejected. Now tolerates parse errors (tree-sitter's
  ERROR nodes are non-fatal), warns on stderr, and extracts what it can.

## [0.16.0] — 2026-05-18

### Features

- **Anonymous telemetry** — PostHog Cloud integration with three-layer privacy contract
  (gates in reporter wrapper, allowlist redaction, sampling). Events: activation, indexing,
  tool invocations (10% sampled), commands, server health. Respects VS Code's global
  `telemetry.telemetryLevel` setting. `codegraph.telemetry.verbose` flag logs every event
  to the output channel for transparency.
- Per-language file counts + duration in reindex response.
- MCP pre-edit hook installer for Claude Code integration.

---

*For the complete commit history, see the
[GitHub repository](https://github.com/codegraph-ai/CodeGraph).*
