# CodeGraph MCP Server

Cross-language code intelligence for AI agents — 28 tools, 17 languages, persistent memory.

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

### Cursor / Other MCP clients

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

### Optional: automatic context injection in Claude Code

Install a PreToolUse hook that nudges Claude to fetch graph context (`get_edit_context`, `analyze_impact`) before editing source files in indexed workspaces. Skips non-source files and ad-hoc edits silently. Never blocks tool calls.

```bash
npx codegraph-mcp-install-hooks         # interactive prompt
npx codegraph-mcp-install-hooks --dry   # preview the change
npx codegraph-mcp-install-hooks --force # skip prompt
npx codegraph-mcp-install-hooks --uninstall
```

The installer modifies `~/.claude/settings.json` idempotently. Re-running it after a package upgrade refreshes the hook path. Inspired by [GitNexus's](https://github.com/abhigyanpatwari/GitNexus) precomputed-context model — addresses the "agents ship blind edits" failure mode by surfacing call-graph awareness automatically instead of relying on the agent to remember to ask.

## Tools (28)

**Analysis**: `get_ai_context`, `get_edit_context`, `get_curated_context`, `analyze_impact`, `analyze_complexity`

**Navigation**: `symbol_search`, `get_callers`, `get_callees`, `get_detailed_symbol`, `get_symbol_info`, `get_dependency_graph`, `get_call_graph`, `find_by_imports`, `find_by_signature`, `find_entry_points`, `find_implementors`, `find_related_tests`, `traverse_graph`

**Indexing**: `reindex_workspace`, `index_files`, `index_directory`

**Memory**: `memory_store`, `memory_get`, `memory_search`, `memory_context`, `memory_list`, `memory_stats`, `memory_invalidate`

## Languages

TypeScript/JS, Python, Rust, Go, C, C++, Java, Kotlin, C#, PHP, Ruby, Swift, Tcl, Verilog, COBOL, Fortran

## License

Apache-2.0 — [GitHub](https://github.com/codegraph-ai/CodeGraph)
