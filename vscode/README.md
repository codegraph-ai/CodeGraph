# CodeGraph

**Cross-language code intelligence for AI agents and developers.**

[![License](https://img.shields.io/badge/License-Apache%202.0-green.svg)](LICENSE)

CodeGraph builds a semantic graph of your codebase — functions, classes, imports, call chains — and exposes it through **42 MCP tools**, a **VS Code extension**, and a **persistent memory layer**. Parses **38 languages** via tree-sitter. AI agents get structured code understanding instead of grepping through files.

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

Install from the marketplace, or sideload the VSIX:

```bash
code --install-extension codegraph-0.20.1.vsix
```

One VSIX serves every platform.
The analysis engine is not bundled: on first activation the extension offers to download the engine built for your platform, verifies it against the published checksum, and installs it into `~/.codegraph/bin`.
The download is offered rather than performed automatically, because it is a native binary that runs with your permissions - decline it and run **CodeGraph: Download Analysis Engine** from the command palette whenever you are ready.

Once an engine is present, the extension starts it automatically and registers all tools as Language Model Tools for Copilot.
CodeGraph's Symbols and Memories views live in the CodeGraph activity-bar container, and inline CodeLens above each function reports callers, related tests and complexity (`codegraph.codeLens.enabled` / `codegraph.hover.enabled` turn those off).

---

## Configuration

### MCP Server flags

| Flag | Default | Description |
|------|---------|-------------|
| `--workspace <path>` | current dir | Directories to index (repeatable for multi-project) |
| `--exclude <dir>` | — | Directories to skip (repeatable) |
| `--embedding-model <model>` | `bge-small` | `bge-small` (384d, fast), `jina-code-v2` (768d, 6x slower), or `static` (model2vec, 256d — ~100× faster indexing, no ONNX, ~90% of BGE quality in hybrid search; needs a local model directory, see `codegraph.staticModelPath` below) |
| `--full-body-embedding` | `true` | Embed full function body (~50 lines) for better semantic search and duplicate detection |
| `--max-files <n>` | 5000 | Maximum files to index |

### VS Code settings

```jsonc
{
  "codegraph.indexOnStartup": true,
  "codegraph.indexPaths": ["/path/to/project-a", "/path/to/project-b"],
  "codegraph.excludePatterns": ["**/cmake-build-debug/**", "**/generated/**"],
  "codegraph.embeddingModel": "bge-small",        // or "static" for ~100× faster indexing
  "codegraph.staticModelPath": "",                // only to override the default model2vec model dir
  "codegraph.maxFileSizeKB": 1024,
  "codegraph.codeLens.enabled": true,             // caller / test / complexity counts above functions
  "codegraph.hover.enabled": true,                // the same stats on hover
  "codegraph.debug": false
}
```

The static (model2vec) model is not bundled with the extension.
The engine looks for it in `~/.codegraph/static_models/jina-code-static-256`, which is where the `@astudioplus/codegraph-mcp` npm install puts it, so a model in that location needs no setting at all.
Set `codegraph.staticModelPath` only to point at a model somewhere else - for instance one you distilled yourself with [`scripts/distill_static_model.py`](https://github.com/codegraph-ai/codegraph/blob/main/scripts/distill_static_model.py).

Full-body embeddings are enabled by default. Function body text is captured at parse time with zero I/O overhead.

Built-in exclusions (always skipped): `node_modules`, `target`, `dist`, `build`, `out`, `.git`, `__pycache__`, `vendor`, `DerivedData`, `tmp`, `coverage`, `logs`.

---

## Tools

**42 community tools**, plus 27 more (17 of them security analyzers) in [CodeGraph Pro](https://codegraph.astudioplus.com/pro).
All names are prefixed with `codegraph_` (e.g. `codegraph_get_ai_context`); tools that target a symbol accept `uri` + `line`, or a `nodeId` from `symbol_search` results.

| Category | Count | What's in it |
|---|---|---|
| Code analysis | 11 | AI/edit/curated context, impact, complexity, circular deps, hot paths, module summary |
| Search | 8 | Symbol search (BM25 + semantic), by imports/signature/pattern/error, entry points, traversal |
| Navigation | 3 | Callers, callees, detailed symbol |
| Memory | 7 | Store, get, search, context, list, invalidate, stats |
| Documentation | 7 | Markdown indexing, doc search + sources, design verification, architecture docs |
| Indexing | 3 | Reindex workspace, index files, index directory |
| PR analysis | 1 | One-call review context for a change |
| Dead imports / ops structs | 2 | Unused imports, ops-struct callback implementors |

→ **[Full tool reference](https://github.com/codegraph-ai/codegraph#tools)** — every tool with its description, and the pro/security surface.

---

## Languages

**38 languages** parsed via tree-sitter — functions, classes, imports, call graph, complexity metrics, dependency graphs, symbol search, and impact analysis.
Systems (C, C++, Rust, Zig, Objective-C), JVM (Java, Kotlin, Scala, Groovy, Clojure), web/scripting (TypeScript/JS, Python, Ruby, PHP, Perl, Lua, Elixir, Elm, CSS), mobile (Swift, Dart), functional (Haskell, OCaml, Julia, Erlang), enterprise (C#, COBOL, Fortran, Go), Solidity, shell/config (Bash, Dockerfile, HCL/Terraform, TOML, YAML), hardware (Verilog/SystemVerilog, Tcl) and R.

HTTP handler detection: Python (FastAPI/Flask/Django), TypeScript (NestJS), Java (Spring/JAX-RS), Go (stdlib/Gin/Echo/Fiber), C# (ASP.NET), Ruby (Rails), PHP (Laravel/Symfony).

→ **[Full language table](https://github.com/codegraph-ai/codegraph#languages)**, including which languages need the `extra-languages` build.

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
