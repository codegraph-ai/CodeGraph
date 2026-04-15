# CodeGraph

**Cross-language code intelligence for AI agents and developers.**

[![License](https://img.shields.io/badge/License-Apache%202.0-green.svg)](LICENSE)

CodeGraph builds a semantic graph of your codebase — functions, classes, imports, call chains — and exposes it through **39 MCP tools**, a **VS Code extension**, and a **persistent memory layer**. Parses **31 languages** via tree-sitter. AI agents get structured code understanding instead of grepping through files.

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

---

## Configuration

### MCP Server flags

| Flag | Default | Description |
|------|---------|-------------|
| `--workspace <path>` | current dir | Directories to index (repeatable for multi-project) |
| `--exclude <dir>` | — | Directories to skip (repeatable) |
| `--embedding-model <model>` | `bge-small` | `bge-small` (384d, fast) or `jina-code-v2` (768d, 6x slower) |
| `--full-body-embedding` | `true` | Embed full function body (~50 lines) for better semantic search and duplicate detection |
| `--max-files <n>` | 5000 | Maximum files to index |

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

Built-in exclusions (always skipped): `node_modules`, `target`, `dist`, `build`, `out`, `.git`, `__pycache__`, `vendor`, `DerivedData`, `tmp`, `coverage`, `logs`.

---

## Tools (28)

### Code Analysis (5)

| Tool | What it does |
|------|-------------|
| `get_ai_context` | **Primary context tool.** Intent-aware (explain/modify/debug/test) with token budgeting. Returns source, related symbols, imports, siblings, debug hints. |
| `get_edit_context` | Everything needed before editing: source + callers + tests + memories + git history |
| `get_curated_context` | Cross-codebase context for a natural language query ("how does auth work?") |
| `analyze_impact` | Blast radius prediction — what breaks if you modify, delete, or rename |
| `analyze_complexity` | Cyclomatic complexity with breakdown (branches, loops, nesting, exceptions, early returns) |

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

All tool names are prefixed with `codegraph_` (e.g. `codegraph_get_ai_context`). Tools that target a specific symbol accept `uri` + `line` or `nodeId` from `symbol_search` results.

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

---

## Languages

31 languages parsed via tree-sitter — functions, classes, imports, call graph, complexity metrics, dependency graphs, symbol search, and impact analysis:

| Category | Languages |
|---|---|
| **Systems** | C, C++, Rust, Zig |
| **JVM** | Java, Kotlin, Scala, Groovy |
| **Web/Scripting** | TypeScript/JS, Python, Ruby, PHP, Perl, Lua, Elixir |
| **Mobile** | Swift, Dart |
| **Functional** | Haskell, OCaml, Julia |
| **Enterprise** | C#, COBOL, Fortran, Go |
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
            │  31 tree-sitter parsers     │
            │  Semantic graph engine      │
            │  AI query engine (BM25)     │
            │  Memory layer (RocksDB)     │
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

## License

Apache-2.0
