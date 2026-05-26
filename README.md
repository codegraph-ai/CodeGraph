# CodeGraph

**Cross-language code intelligence for AI agents and developers.**

[![License](https://img.shields.io/badge/License-Apache%202.0-green.svg)](LICENSE)

CodeGraph builds a semantic graph of your codebase — functions, classes, imports, call chains — and exposes it through **45 MCP tools**, a **VS Code extension**, and a **persistent memory layer**. Parses **37 languages** via tree-sitter. AI agents get structured code understanding instead of grepping through files.

## Quick Start

### MCP Server (Claude Code, Cursor, any MCP client)

Add to `~/.claude.json` (or your MCP client config):

```json
{
  "mcpServers": {
    "codegraph": {
      "command": "/path/to/codegraph-server",
      "args": ["--mcp"]
    }
  }
}
```

The server indexes the current working directory automatically.

### VS Code Extension

Install the VSIX:

```bash
code --install-extension codegraph-0.14.0.vsix
```

The extension starts the server automatically and registers all tools as Language Model Tools for Copilot.

### Rules for AI agents

Pre-configured rule files that teach AI coding agents (Claude, Cursor,
Windsurf, Codex, Cline) to use CodeGraph MCP tools before falling back
to grep / multi-file reads. Maps natural-language intent to the right
`codegraph_*` tool.

→ **[codegraph-ai/codegraph-rules-for-agents](https://github.com/codegraph-ai/codegraph-rules-for-agents)**

Setup is `cp <agent>/codegraph.md ~/<agent>/` (one line per agent — see
the rules repo's README).

---

## Configuration

### MCP Server flags

| Flag | Default | Description |
|------|---------|-------------|
| `--workspace <path>` | current dir | Directories to index (repeatable for multi-project) |
| `--exclude <dir>` | — | Directories to skip (repeatable) |
| `--embedding-model <model>` | `bge-small` | `bge-small` (384d, fast), `jina-code-v2` (768d, 6× slower), or `granite-97m` (384d, 32K ctx, ~3× slower) |
| `--full-body-embedding` | `true` | Embed full function body (~50 lines) for better semantic search and duplicate detection |
| `--max-files <n>` | 5000 | Maximum files to index |
| `--profile <name>` | `all` | Filter the exposed MCP tool surface to a named subset (see below) |

#### `--profile` — narrow the MCP tool surface

The full 32-tool surface is convenient but inflates the agent's prompt-context cost. A profile exposes only the slice you need (also settable via the `CODEGRAPH_TOOL_PROFILE` env var):

| Profile | Tools | Use when |
|---------|-------|----------|
| `all` *(default)* | every tool (community + pro) | normal sessions |
| `core` | 8 — search + symbol info + AI context | chatty agent sessions where you only need lookups |
| `graph` | 16 — callers/callees/deps/impact/traverse | refactoring + structural analysis |
| `memory` | 7 — `codegraph_memory_*` only | note-taking / knowledge-base workflows |
| `security` | pro security tools only (empty on community) | pro security audits |

### VS Code settings

```jsonc
{
  "codegraph.indexOnStartup": true,
  "codegraph.indexPaths": ["/path/to/project-a", "/path/to/project-b"],
  "codegraph.excludePatterns": ["**/cmake-build-debug/**", "**/generated/**"],
  "codegraph.embeddingModel": "bge-small",
  "codegraph.maxFileSizeKB": 1024,
  "codegraph.debug": false
}
```

Full-body embeddings are enabled by default. Function body text is captured at parse time with zero I/O overhead.

Built-in exclusions (always skipped) cover ~47 directories across three categories:

- **Build / cache**: `node_modules`, `target`, `dist`, `build`, `out`, `.git`, `__pycache__`, `vendor`, `.venv`, `venv`, `.tox`, `.pytest_cache`, `.mypy_cache`, `.ruff_cache`, `.next`, `.nuxt`, `.svelte-kit`, `.parcel-cache`, `.npm`, `.yarn`, `.pnpm-store`, `.cache`, `.cargo`, `.bundle`, `.gradle`, `DerivedData`, `Pods`, `xcuserdata`, `cmake-build-*`
- **IDE / IaC state**: `.idea`, `.vscode-test`, `.fleet`, `.terraform`, `.terragrunt-cache`, `.serverless`
- **Sensitive credential dirs**: `.aws`, `.ssh`, `.gnupg`, `.kube`, `.docker`

Plus glob patterns for binary archives, native libraries, OS metadata, and **secret file extensions** (`*.pem`, `*.key`, `*.p12`, `*.pfx`, `*.crt`, `*.gpg`, `*.kdbx`, SSH key conventions like `id_rsa`, etc.) — defense in depth against accidentally embedding credentials.

---

## Tools (41 community + 27 pro, 17 security)

### Code Analysis (11)

| Tool | What it does |
|------|-------------|
| `get_ai_context` | **Primary context tool.** Intent-aware (explain/modify/debug/test) with token budgeting. Returns source, related symbols, imports, siblings, debug hints. |
| `get_edit_context` | Everything needed before editing: source + callers + tests + memories + git history |
| `get_curated_context` | Cross-codebase context for a natural language query ("how does auth work?") |
| `analyze_impact` | Blast radius prediction — what breaks if you modify, delete, or rename |
| `analyze_complexity` | Cyclomatic complexity with breakdown (branches, loops, nesting, exceptions, early returns) |
| `find_circular_deps` | Detect circular import/dependency chains across files |
| `find_hot_paths` | Most-called functions ranked by transitive caller count |
| `find_dead_imports` | Find unused imports — modules imported but never referenced |
| `get_module_summary` | High-level summary of a directory: file count, functions, language breakdown, top complex functions |
| `search_by_pattern` | Regex search across function bodies, signatures, names, and docstrings |
| `search_by_error` | Find functions that throw, catch, or handle specific error types |

### Code Navigation (13)

| Tool | What it does |
|------|-------------|
| `symbol_search` | Find symbols by name or natural language (hybrid BM25 + semantic search) |
| `get_callers` / `get_callees` | Who calls this? What does it call? (with transitive depth) |
| `get_detailed_symbol` | Full symbol info: source, callers, callees, complexity |
| `get_symbol_info` | Quick metadata: signature, visibility, kind |
| `get_dependency_graph` | File/module import relationships with depth control |
| `get_call_graph` | Function call chains (callers and callees) |
| `find_by_imports` | Find files importing a module |
| `find_by_signature` | Search by param count, return type, modifiers |
| `find_entry_points` | Main functions, HTTP handlers, CLI commands, event handlers |
| `find_implementors` | Find all functions registered as ops struct callbacks |
| `find_related_tests` | Tests that exercise a given function |
| `traverse_graph` | Custom graph traversal with edge/node type filters |

### Indexing (3)

| Tool | What it does |
|------|-------------|
| `reindex_workspace` | Full or incremental workspace reindex |
| `index_files` | Add/update specific files without full reindex |
| `index_directory` | Add directory to graph alongside existing data |

### Memory (7)

Persistent AI context across sessions — debugging insights, architectural decisions, known issues.

| Tool | What it does |
|------|-------------|
| `memory_store` / `memory_get` / `memory_search` | Store, retrieve, search memories (BM25 + semantic) |
| `memory_context` | Get memories relevant to a file/function |
| `memory_list` / `memory_invalidate` / `memory_stats` | Browse, retire, monitor |

### Documentation (7)

Persistent project documentation — index design docs, search them semantically, verify code matches the design, generate architecture docs from the code graph.

| Tool | What it does |
|------|-------------|
| `index_markdown` | Index a local `.md` file (ARCHITECTURE.md, API_DESIGN.md, etc.) into the persistent docs store. Heading-tree chunking with leaf-node embeddings. |
| `search_docs` | Semantic search over indexed docs — returns matching sections with heading-path breadcrumbs |
| `list_doc_sources` | List all indexed source files |
| `remove_doc_source` | Remove all indexed chunks from a source file |
| `verify_design` | Cross-reference doc claims vs code graph. `direction=forward` (doc→code), `reverse` (code→doc), or `both` |
| `design_gaps` | Find identifiers described in docs that don't exist in code yet — build TODO lists from specs |
| `generate_architecture_doc` | Auto-generate a structured ARCHITECTURE.md from the live code graph (modules, hot paths, complexity, circular deps) |

All tool names are prefixed with `codegraph_` (e.g. `codegraph_get_ai_context`). Tools that target a specific symbol accept `uri` + `line` or `nodeId` from `symbol_search` results.

---

### Usage examples

**Index a design doc and search it:**
```
codegraph_index_markdown(path: "/projects/myapp/docs/ARCHITECTURE.md")
codegraph_search_docs(query: "how does the auth module handle JWT refresh?")
```

**Check if the code matches the design:**
```
codegraph_verify_design(source: "/projects/myapp/docs/ARCHITECTURE.md", direction: "forward")
// → "132/132 identifiers verified, 0 gaps"
```

**Find what's described in docs but not yet implemented:**
```
codegraph_design_gaps(source: "/projects/myapp/docs/API_DESIGN.md")
// → "4 of 12 identifiers not found in code: PaymentService, RefundHandler, ..."
```

**Generate architecture docs from the code graph:**
```
codegraph_generate_architecture_doc(scope: "src/", topN: 5)
// → Markdown with modules, complexity hotspots, hot paths, circular deps
```

**Save a debugging insight for future sessions:**
```
codegraph_memory_store(kind: "debug_context", title: "Nginx body size limit",
  content: "The /upload endpoint fails on payloads > 1MB...",
  problem: "API returns 500 on large uploads",
  solution: "Increase nginx client_max_body_size to 10M",
  agentSource: "claude")
```

**Get AI context with automatic design doc augmentation:**
```
codegraph_get_ai_context(uri: "file:///projects/myapp/src/auth.rs", line: 42, intent: "modify")
// → Code context + design_context section from indexed docs mentioning "auth"
```

**Narrow the tool surface for chatty sessions:**
```bash
codegraph-server --mcp --profile=core  # Only 8 tools: search + symbol info + AI context
```

---

### CodeGraph Pro

Additional tools available in [CodeGraph Pro](https://codegraph.astudioplus.com/pro):

| Tool | What it does |
|------|-------------|
| `scan_security` | Security vulnerability scan: 40+ dangerous function patterns, source-to-sink taint tracing, auth coverage for HTTP endpoints (7 languages/frameworks), architectural layer violations, weak crypto, hardcoded secrets |
| `analyze_coupling` | Module coupling metrics and instability scores |
| `find_unused_code` | Dead code detection with confidence scoring |
| `find_duplicates` | Detect duplicate/near-duplicate functions |
| `find_similar` / `cluster_symbols` / `compare_symbols` | Embedding-based code similarity |
| `cross_project_search` | Search across all indexed projects |
| `mine_git_history` / `mine_git_history_for_file` / `search_git_history` | Git history mining and semantic search |
| `security_control_flow` | Map every execution path through a function — "can this return without hitting the auth check?" |
| `security_trace_data_flow` | Follow a variable from birth to death — "does user input reach this SQL query?" |
| `security_generate_sbom` | CycloneDX SBOM from 8 lockfile formats |
| `security_audit_deps` | OSV vulnerability check on dependencies |
| `security_check_unchecked_returns` / `_resource_leaks` / `_misconfig` / `_input_validation` / `_error_exposure` | 5 heuristic analyzers covering ~80% of CWE Top 25 |
| `security_scan_iac` | Docker / Kubernetes / Terraform misconfiguration scan |
| `security_check_licenses` | Lockfile license policy enforcement (copyleft detection) |
| `security_check_secrets_entropy` | Shannon-entropy hardcoded-secret detection |
| `security_detect_injection` | Focused SQL/XSS/cmd/path/deser/template injection detection (20 patterns) |
| `security_check_search_path` | Untrusted search-path / DLL-hijacking detection (CWE-426/CWE-427) |
| `security_check_crypto` | Cryptographic misuse: weak ciphers/hashes/PRNG/keys, static IVs, timing-leak comparisons (CWE-208/326-330/338/916, 35 patterns) |
| `security_export_sarif` | Aggregate findings as SARIF 2.1.0 (GitHub Code Scanning, GitLab SAST) |

**Cross-cutting features (all `security_check_*` tools):**
- `include_tests` / `treat_as_production` — first-class skip for tests/samples/vendored
- `check_compile_gates` — C/C++ findings inside `#ifdef X` are marked DEFENSIVE_GATED_OFF when X isn't defined by CMake/Cargo/Makefile
- 25-marker suppression honoring (`# nosec`, `// NOLINT`, `// codeql[ignore]`, `# rubocop:disable`, etc.) at line and function level
- Telemetry blocks per scan: `path_filter` (examined/matched/skipped) + `compile_gate` (gated_off count)

---

## Languages

38 languages parsed via tree-sitter — functions, classes, imports, call graph, complexity metrics, dependency graphs, symbol search, and impact analysis:

| Category | Languages |
|---|---|
| **Systems** | C, C++, Rust, Zig, Objective-C |
| **JVM** | Java, Kotlin, Scala, Groovy, Clojure |
| **Web/Scripting** | TypeScript/JS, Python, Ruby, PHP, Perl, Lua, Elixir, Elm |
| **Web/Style** | CSS |
| **Mobile** | Swift, Dart |
| **Functional** | Haskell, OCaml, Julia, Erlang, Elm, Clojure |
| **Enterprise** | C#, COBOL, Fortran, Go |
| **Blockchain** | Solidity |
| **Shell/Config** | Bash, HCL/Terraform, TOML, YAML |
| **Hardware** | Verilog/SystemVerilog, Tcl |
| **Data Science** | R, Julia |

HTTP handler detection: Python (FastAPI/Flask/Django), TypeScript (NestJS), Java (Spring/JAX-RS), Go (stdlib/Gin/Echo/Fiber), C# (ASP.NET), Ruby (Rails), PHP (Laravel/Symfony).

---

## Architecture

```
MCP Client (Claude, Cursor, ...)        VS Code Extension
        |                                       |
    MCP (stdio)                            LSP Protocol
        |                                       |
        └───────────┐               ┌───────────┘
                    ▼               ▼
            ┌─────────────────────────────┐
            │       codegraph-server      │
            ├─────────────────────────────┤
            │  38 tree-sitter parsers     │
            │  Semantic graph engine      │
            │  AI query engine (BM25)     │
            │  Memory layer (RocksDB)     │
            │  Docs store (RocksDB+HNSW)  │
            │  Full-body embeddings (BGE) │
            │  HNSW vector index          │
            └─────────────────────────────┘
```

A single Rust binary serves both MCP and LSP protocols.

- **Indexing**: ~60 files/sec. Incremental re-indexing on file changes via FNV-1a content hashing.
- **Persistence**: Graph and embeddings persist to `~/.codegraph/graph.db` (RocksDB). Instant startup on restart — no re-parsing, no re-embedding.
- **Queries**: Sub-100ms. Cross-file import and call resolution at index time.
- **Embeddings**: Full-body (function bodies captured at parse time, zero disk I/O). Vectors stored in RocksDB alongside the graph. Auto-downloads model on first run.

---

## Building from Source

```bash
git clone https://github.com/codegraph-ai/codegraph
cd codegraph
cargo build --release -p codegraph-server    # Rust server
cd vscode && npm install && npm run esbuild  # VS Code extension
npx @vscode/vsce package                     # VSIX
```

Requires Rust stable, Node.js 18+, VS Code 1.90+.

---

## Support the project

CodeGraph is free, open-source, and maintained by a solo developer.
If it saves you time, consider [sponsoring on GitHub](https://github.com/sponsors/anvanster) — it helps keep the project alive and growing.

---

## License

Apache-2.0
