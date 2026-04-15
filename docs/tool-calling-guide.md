# CodeGraph Tool Calling Guide

Reference for calling all 39 CodeGraph MCP tools. Each tool is prefixed with `codegraph_` (e.g., `codegraph_symbol_search`).

## Identifying Symbols

Most tools need to identify a specific symbol. Two methods:

### Method 1: uri + line

Use `file://` prefix. Lines are **0-indexed** (line 1 in editor = line 0 in tool).

```json
{
  "uri": "file:///Users/you/project/src/main.rs",
  "line": 42
}
```

### Method 2: nodeId

Use the `node_id` from `symbol_search` results. Pass as **string**.

```json
{
  "nodeId": "1549"
}
```

---

## Search & Discovery (6 tools)

### symbol_search

Find symbols by name or natural language. **Start here** when you don't know where code is.

```json
{
  "query": "parse_file",
  "symbolType": "function",
  "limit": 10,
  "compact": true
}
```

`symbolType`: `function`, `class`, `method`, `variable`, `interface`, `type`, `module`, `any`

### find_entry_points

Discover application entry points: main functions, HTTP handlers, CLI commands, event handlers.

```json
{
  "entryType": "http_handler",
  "compact": true,
  "limit": 20
}
```

`entryType`: `main`, `http_handler`, `cli_command`, `event_handler`, `test`, `public`, `all`

### find_by_imports

Find all symbols that import a specific module.

```json
{
  "module": "tree_sitter"
}
```

### find_by_signature

Search functions by parameter count, return type, or modifiers.

```json
{
  "paramCount": 2,
  "returnType": "Result"
}
```

### find_implementors

Find functions registered as ops struct callback implementations (C codebases).

```json
{
  "structType": "net_device_ops",
  "fieldName": "ndo_open"
}
```

Omit both to list all ops struct registrations.

### find_related_tests

Find tests that exercise a given function.

```json
{
  "uri": "file:///path/to/src/parser.rs",
  "line": 50
}
```

---

## Symbol Details (7 tools)

### get_symbol_info

Quick metadata: signature, visibility, kind. Lightweight.

```json
{
  "uri": "file:///path/to/file.rs",
  "line": 83
}
```

### get_detailed_symbol

Full symbol info: source code, callers, callees, complexity. Heavier but complete.

```json
{
  "nodeId": "1549",
  "includeSource": true,
  "includeCallers": true,
  "includeCallees": true
}
```

### get_callers

Who calls this function? Use `depth > 1` for transitive callers.

```json
{
  "uri": "file:///path/to/file.rs",
  "line": 83,
  "depth": 2
}
```

### get_callees

What does this function call?

```json
{
  "nodeId": "1549",
  "depth": 1
}
```

### get_call_graph

Full call chain (callers + callees) with depth control.

```json
{
  "uri": "file:///path/to/file.rs",
  "line": 83,
  "depth": 3
}
```

### get_dependency_graph

File/module import relationships.

```json
{
  "uri": "file:///path/to/file.rs",
  "direction": "both",
  "depth": 2
}
```

`direction`: `incoming`, `outgoing`, `both`

### traverse_graph

Custom graph traversal with edge/node type filters.

```json
{
  "nodeId": "1549",
  "edgeTypes": ["Calls", "Contains"],
  "direction": "outgoing",
  "depth": 2
}
```

---

## AI Context (3 tools)

### get_ai_context

Primary context tool. Intent-aware with token budgeting.

```json
{
  "uri": "file:///path/to/file.rs",
  "line": 83,
  "intent": "modify"
}
```

`intent`: `explain`, `modify`, `debug`, `test`

### get_edit_context

Everything needed before editing: source + callers + tests + memories + git history.

```json
{
  "uri": "file:///path/to/file.rs",
  "line": 83
}
```

### get_curated_context

Cross-codebase context for a natural language query.

```json
{
  "query": "how does authentication work"
}
```

---

## Analysis (3 tools)

### analyze_complexity

Cyclomatic complexity metrics. Omit `line` to analyze all functions in a file.

```json
{
  "uri": "file:///path/to/server.rs",
  "threshold": 10
}
```

Single function:

```json
{
  "uri": "file:///path/to/server.rs",
  "line": 83
}
```

### analyze_impact

Blast radius prediction. What breaks if you modify, delete, or rename?

```json
{
  "uri": "file:///path/to/file.rs",
  "line": 83,
  "changeType": "delete"
}
```

`changeType`: `modify`, `delete`, `rename`

### analyze_coupling

Module coupling metrics and instability scores. (Pro)

```json
{
  "uri": "file:///path/to/module.rs"
}
```

---

## Indexing (3 tools)

### reindex_workspace

Full or incremental workspace reindex. Use `force: true` after parser upgrades.

```json
{
  "force": true
}
```

### index_files

Add/update specific files. Paths are **absolute, without `file://` prefix**.

```json
{
  "paths": [
    "/absolute/path/to/file.rs",
    "/absolute/path/to/other.rs"
  ]
}
```

### index_directory

Add an entire directory alongside existing data.

```json
{
  "path": "/absolute/path/to/new/module",
  "embed": true
}
```

---

## Memory (7 tools)

Persistent AI context across sessions.

### memory_store

```json
{
  "title": "Auth bug root cause",
  "content": "The JWT validation skips expiry check when...",
  "kind": "project_context",
  "tags": ["auth", "bug"]
}
```

### memory_get

```json
{
  "id": "543d582c-126b-4b4d-b659-e0ab2d052350"
}
```

### memory_search

```json
{
  "query": "authentication issues"
}
```

### memory_context

Get memories relevant to a specific file.

```json
{
  "uri": "file:///path/to/auth.rs"
}
```

### memory_list

No parameters required.

```json
{}
```

### memory_invalidate

```json
{
  "id": "543d582c-126b-4b4d-b659-e0ab2d052350",
  "reason": "Fixed in v2"
}
```

### memory_stats

No parameters required.

```json
{}
```

---

## Pro Tools (10 tools)

### scan_security

Security vulnerability scan: dangerous functions, taint tracing, auth coverage, secrets.

```json
{
  "uri": "file:///path/to/handler.rs"
}
```

### find_unused_code

Dead code detection with confidence scoring.

```json
{
  "uri": "file:///path/to/module.rs"
}
```

### find_duplicates

Detect duplicate/near-duplicate functions via embeddings.

```json
{
  "threshold": 0.85
}
```

### find_similar

Find functions semantically similar to a given one.

```json
{
  "nodeId": "1549",
  "limit": 10
}
```

### cluster_symbols

Group similar symbols by embedding distance.

```json
{
  "symbolType": "function",
  "minClusterSize": 3
}
```

### compare_symbols

Side-by-side comparison of two symbols.

```json
{
  "nodeIdA": "1549",
  "nodeIdB": "1550"
}
```

### cross_project_search

Search across all indexed projects.

```json
{
  "query": "parse_file"
}
```

### mine_git_history

Semantic search over commit history.

```json
{
  "query": "refactored auth"
}
```

### mine_git_history_for_file

Git history for a specific file.

```json
{
  "uri": "file:///path/to/file.rs"
}
```

### search_git_history

Search git log with natural language.

```json
{
  "query": "fix authentication bug"
}
```

---

## Common Gotchas

| Issue | Fix |
|---|---|
| "Could not find symbol" | Use `file://` prefix: `file:///path/to/file.rs` not `/path/to/file.rs` |
| Wrong function found | Lines are 0-indexed. Line 1 in editor = `"line": 0` |
| "paths parameter required" | `index_files` takes absolute paths **without** `file://` prefix |
| nodeId type error | Pass as string: `"1549"` not `1549` |
| Empty results after restart | Run `reindex_workspace` or wait for auto-indexing |
| "Invalid URI" | Missing `file://` prefix on URI parameters |

## Supported Languages (31)

Bash, C, C++, C#, COBOL, Dart, Elixir, Fortran, Go, Groovy, Haskell, HCL/Terraform, Java, Julia, Kotlin, Lua, OCaml, Perl, PHP, Python, R, Ruby, Rust, Scala, Swift, Tcl, TOML, TypeScript/JS, Verilog/SystemVerilog, YAML, Zig
