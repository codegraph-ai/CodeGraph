# Changelog

All notable changes to CodeGraph are documented here. Versions follow
[Semantic Versioning](https://semver.org/). Releases from 0.18.1 on are tagged
`vX.Y.Z`; earlier ones `vscode/vX.Y.Z`.

## [0.21.1] - 2026-10-10

VS Code extension only; the engine stays 0.21.0.

- The extension now downloads its engine automatically on first start, with retries.
  0.21.0 asked first, and most users who auto-updated from 0.19.1 (which bundled the engine) dismissed or never saw the prompt and were left without a working extension.
  A failed download shows an error with a Retry button.
- An engine older than the extension expects is updated automatically instead of prompting.

## [0.21.0] - 2026-10-05

### Upgrading

Restart any `codegraph-mcp --watch` daemon after upgrading.
A daemon left running from 0.20.1 keeps writing vectors that 0.21 will not load, and sessions attached to it run without semantic search until it restarts.

Each project re-embeds once, in the background, the first time 0.21 opens it.
Keyword search keeps working while that runs.

### Search

- Identifiers whose words run together, like `getUserById`, are now also embedded in word-split form.
  This mostly helps natural-language search in camelCase languages such as TypeScript, Java and C#.
  Pass `--split-identifiers=false` to keep the old behaviour.
- Stored vectors now record the model and settings that built them.
  Switching the embedding model or these settings re-embeds instead of silently mixing incompatible vectors.
- `--full-body-embedding=false` now takes effect; in 0.20.1 the flag was always on.

### Reliability

- A session that started while another CodeGraph process had the shared graph database open could treat it as corrupt and discard every project's saved graph.
  It now waits for the other process to finish, and only a genuinely unreadable database is replaced.
  The stale-`LOCK` cleanup added in 0.17.0 is gone: the OS releases a lock when its holder exits, so a leftover `LOCK` file never blocked an open, and deleting one could let two processes open the same database on Linux.
- After the database was replaced, a project could stay empty until its files changed.
  It is now re-indexed.
- The file watcher honours the workspace's exclude settings and `.codegraphignore`, works on workspaces reached through a symlink, and rescans the graph once per batch of changes instead of once per event (#23).
- `--graph-only` no longer loads the embedding model.
- Two clients starting at the same moment now share one engine, and the engine receives their flags.

### Build

- Release engines are built with one pinned Rust toolchain (`rust-toolchain.toml`).

## [0.20.1] - 2026-08-09

- **linux-arm64 engine.** ARM Linux previously had no engine at all; it is now published and fetched like the other platforms (#15).
- Fixed a startup crash on aarch64 Linux before `main()` ran (#15).
- Indexing a second markdown file no longer overwrites chunks from the first.
  Doc chunk ids are now namespaced by source file (#16).

## [0.20.0] - 2026-08-08

- **Engines are fetched, not bundled.** The VSIX and the npm package no longer carry every platform's binary.
  Each install downloads the one engine it can run from the GitHub release, which took the VSIX from 118 MB to under 1 MB.
- **JetBrains IDE client.** A new plugin connects JetBrains IDEs to the same engine.
- VS Code onboarding: an index that comes back empty now explains why (no folder, no supported languages, misconfigured `indexPaths`, everything excluded) and offers the fix, and the Symbols view offers to index an empty workspace.
- Embedding tools are no longer disabled on healthy Macs where the OS reports 0 MB available memory.
  `CODEGRAPH_SKIP_MEMORY_CHECK=1` bypasses the check (#13).
- npm publishing now refuses to pack until every engine asset is on the release.

## [0.19.1] - 2026-06-30

- `codegraph_pr_context` recognises tests by the language's test marker (for example `#[cfg(test)]` modules in Rust), not only by name, so covered functions are no longer reported as untested.
- PR risk is based on callers of signature-changed functions and the untested ratio, not raw caller count.
  Callers under `examples/` and `benches/` count as coverage.

## [0.19.0] - 2026-06-30

- **Static embedding model.** `--embedding-model static` (VS Code: `codegraph.embeddingModel`) uses a distilled, ONNX-free model.
  It embeds roughly 70x faster and reaches about 90% of the default BGE model's search quality end to end.
  The model ships with the VS Code extension and is fetched by the npm install; BGE stays the default.
- COBOL, Fortran, Perl, Dart, Zig and R parsers moved behind the `extra-languages` build feature, cutting the engine by 25 MB.
  Telemetry showed no indexed workspace using them.

## [0.18.0 - 0.18.6] - 2026-06-01 to 2026-06-17

- **Shared engine.** One engine process can serve several workspaces and clients with a single loaded model (opt-in via `CODEGRAPH_ENGINE`).
- **Crash recovery.** Fixed a Windows crash loop (thread stack size and UTF-8 truncation of embedding text).
  A graph database that crashes the engine on load is detected across restarts and replaced by a fresh one.
- **Memory-safe embedding.** Long embedding runs checkpoint every 20k symbols, back off when RAM runs low, and resume after a kill.
- `codegraph_get_module_summary` no longer returns all zeros on Windows (#5).
- MCP tool results are compact JSON, about 24% smaller; `codegraph_find_entry_points` honours `limit`.

## [0.17.1 - 0.17.11] - 2026-05-25 to 2026-06-01

- **`codegraph_pr_context`.** Analyses a branch's changes in one call: changed symbols, callers, test gaps, related docs.
- **CI PR review.** `--graph-only` skips embeddings, and `--run-tool` runs one tool and exits, which together power a GitHub Action that comments a code-graph review on each PR.
- **Watcher daemon.** `codegraph-server --watch` keeps a workspace's graph fresh in the background for MCP sessions.
- Files are indexed when opened, so hover and go-to-definition work without a prior workspace index.
- The persisted graph is a true snapshot, fixing duplicate symbols that built up across edits and reloads.
- `codegraph_get_ai_context` reports graph statistics, and every tool warns when the graph looks nearly empty.
- VS Code stops auto-restarting the server after 3 crashes in 60 seconds and shows the error instead.
- User exclude settings now add to the default exclude list instead of replacing it.

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
