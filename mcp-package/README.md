# CodeGraph MCP Server

Cross-language code intelligence for AI agents — 42 tools, 38 languages, persistent memory, documentation intelligence, one-call PR review.

## Install

```bash
npm install -g @astudioplus/codegraph-mcp
```

## Usage

### Claude Code

Add to `~/.claude.json`:

```json
{
  "mcpServers": {
    "codegraph": {
      "command": "codegraph-mcp",
      "args": []
    }
  }
}
```

### Cursor / Windsurf / Cline / Other MCP clients

Same config — the `codegraph-mcp` command starts the server in MCP (stdio) mode.

### Options

Pass flags after `--`:

```json
{
  "mcpServers": {
    "codegraph": {
      "command": "codegraph-mcp",
      "args": ["--workspace", "/path/to/project", "--exclude", "vendor"]
    }
  }
}
```

| Flag | Default | Description |
|------|---------|-------------|
| `--workspace <path>` | current dir | Directories to index (repeatable) |
| `--exclude <dir>` | — | Directories to skip (repeatable) |
| `--embedding-model <model>` | `bge-small` | `bge-small`, `jina-code-v2`, or `granite-97m` (32K context, multilingual) |
| `--max-files <n>` | 5000 | Maximum files to index |
| `--profile <name>` | `all` | Scope tool surface: `core` (8), `graph` (16), `memory` (14), `security` (pro), `all` (42) |
| `--graph-only` | off | Skip embeddings — graph + structural tools only. No ONNX model load, 10-50× faster indexing. For CI / one-shot graph queries. |
| `--run-tool <name>` | — | One-shot: index, run a single tool, print result, exit. No MCP handshake. Pair with `--tool-args '<json>'`. |

### Agent rules (recommended)

Pre-configured rule files that teach your AI agent to use CodeGraph tools before falling back to grep / multi-file reads:

→ **[codegraph-rules-for-agents](https://github.com/codegraph-ai/codegraph-rules-for-agents)**

### GitHub Action — automatic PR review

Get a code-graph analysis comment on every PR — blast radius, test gaps,
stale docs, suggested reviewers. Runs **graph-only** (no embeddings, no
API keys, just `GITHUB_TOKEN`). The core invocation:

```bash
codegraph-server --graph-only \
  --run-tool codegraph_pr_context \
  --tool-args '{"baseBranch":"main","format":"markdown"}'
```

This prints a ready-to-post markdown PR comment. A copy-paste workflow
template lives at [`.github/workflows/codegraph-pr.yml`](https://github.com/codegraph-ai/CodeGraph/blob/main/.github/workflows/codegraph-pr.yml) in the main repo.

### Optional: automatic context injection in Claude Code

Install a PreToolUse hook that nudges Claude to fetch graph context (`get_edit_context`, `analyze_impact`) before editing source files in indexed workspaces. Skips non-source files and ad-hoc edits silently. Never blocks tool calls.

```bash
npx codegraph-mcp-install-hooks         # interactive prompt
npx codegraph-mcp-install-hooks --dry   # preview the change
npx codegraph-mcp-install-hooks --force # skip prompt
npx codegraph-mcp-install-hooks --uninstall
```

## Tools (42)

**Analysis** (11): `get_ai_context`, `get_edit_context`, `get_curated_context`, `analyze_impact`, `analyze_complexity`, `find_circular_deps`, `find_hot_paths`, `find_dead_imports`, `get_module_summary`, `search_by_pattern`, `search_by_error`

**PR review** (1): `pr_context` — one-call PR analysis: blast radius, test gaps, stale docs, commit hint, suggested reviewers. Supports `format:"markdown"` for ready-to-post CI comments.

**Navigation** (13): `symbol_search`, `get_callers`, `get_callees`, `get_detailed_symbol`, `get_symbol_info`, `get_dependency_graph`, `get_call_graph`, `find_by_imports`, `find_by_signature`, `find_entry_points`, `find_implementors`, `find_related_tests`, `traverse_graph`

**Memory** (7): `memory_store`, `memory_get`, `memory_search`, `memory_context`, `memory_list`, `memory_stats`, `memory_invalidate`

**Documentation** (7): `index_markdown`, `search_docs`, `list_doc_sources`, `remove_doc_source`, `verify_design`, `design_gaps`, `generate_architecture_doc`

**Indexing** (3): `reindex_workspace`, `index_files`, `index_directory`

All tool names are prefixed with `codegraph_` (e.g. `codegraph_symbol_search`).

## Languages (38)

**Systems**: C, C++, Rust, Zig, Objective-C  
**JVM**: Java, Kotlin, Scala, Groovy, Clojure  
**Web/Scripting**: TypeScript/JS, Python, Ruby, PHP, Perl, Lua, Elixir, Elm  
**Web/Style**: CSS  
**Mobile**: Swift, Dart  
**Functional**: Haskell, OCaml, Julia, Erlang  
**Enterprise**: C#, COBOL, Fortran, Go  
**Blockchain**: Solidity  
**Shell/Config**: Bash, HCL/Terraform, TOML, YAML, Dockerfile  
**Hardware**: Verilog/SystemVerilog, Tcl  
**Data Science**: R

## Telemetry

Anonymous usage telemetry helps improve CodeGraph. Events tracked: tool invocations (name + duration), startup, errors. No file paths, code content, or PII is ever sent.

Opt out: set `CODEGRAPH_TELEMETRY=off` in your environment.

## License

Apache-2.0 — [GitHub](https://github.com/codegraph-ai/CodeGraph)
