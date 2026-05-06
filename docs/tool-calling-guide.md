# CodeGraph Tool Calling Guide

Reference for calling all 66 CodeGraph MCP tools (34 community + 32 pro, 22 security). Each tool is prefixed with `codegraph_` (e.g., `codegraph_symbol_search`).

> **Pro tool extras (apply to every `codegraph_security_*` tool):**
> All security detectors accept three cross-cutting parameters and emit shared
> telemetry blocks. Detailed in [Cross-cutting Parameters](#cross-cutting-parameters)
> and [Response Telemetry](#response-telemetry) below.

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

Response:
```json
{
  "query_time_ms": 9,
  "total_matches": 12,
  "results": [
    {
      "match_reason": "SymbolName",
      "score": 0.96,
      "node_id": 1549,
      "symbol": {
        "name": "parse_file",
        "kind": "Function",
        "is_public": true,
        "location": {
          "file": "/path/to/parser_impl.rs",
          "line": 83,
          "end_line": 107
        }
      }
    }
  ]
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

Response:
```json
{
  "symbol_name": "run",
  "callers": [
    {
      "node_id": 1685,
      "depth": 1,
      "symbol": {
        "name": "main",
        "kind": "Function",
        "signature": "async fn main() {",
        "location": { "file": "/path/to/main.rs", "line": 60, "end_line": 130 }
      }
    }
  ]
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

## Analysis (7 tools)

### analyze_complexity

Cyclomatic complexity metrics. Omit `line` to analyze all functions in a file.

```json
{
  "uri": "file:///path/to/server.rs",
  "threshold": 10
}
```

Response:
```json
{
  "summary": {
    "total_functions": 32,
    "average_complexity": 10.4,
    "max_complexity": 157,
    "above_threshold": 6,
    "overall_grade": "B"
  },
  "functions": [
    {
      "name": "execute_tool",
      "complexity": 157,
      "grade": "F",
      "line_start": 1084,
      "line_end": 2693,
      "details": {
        "complexity_branches": 152,
        "complexity_loops": 0,
        "complexity_nesting": 7,
        "complexity_early_returns": 42,
        "lines_of_code": 1610
      }
    }
  ],
  "recommendations": [
    "Consider refactoring 'execute_tool' (complexity: 157, grade: F)"
  ]
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

### find_circular_deps

Detect circular import/dependency chains across the codebase.

```json
{
  "max_cycle_length": 10,
  "compact": false
}
```

No required parameters. Returns all cycles found. Use `compact: true` for just the count.

Response:
```json
{
  "has_circular_deps": true,
  "total_cycles": 2,
  "cycles": [
    { "files": ["auth.rs", "session.rs", "auth.rs"], "length": 2 },
    { "files": ["a.py", "b.py", "c.py", "a.py"], "length": 3 }
  ]
}
```

### find_hot_paths

Find the most-called functions, ranked by transitive caller count.

```json
{
  "limit": 20
}
```

Score = direct callers + 0.5 * depth-2 callers + 0.25 * depth-3 callers.

Response:
```json
{
  "total_analyzed": 500,
  "functions": [
    {
      "name": "log",
      "path": "/path/to/logger.rs",
      "line_start": 15,
      "direct_callers": 47,
      "transitive_callers": 230,
      "score": 162.5,
      "signature": "pub fn log(level: Level, msg: &str)"
    }
  ]
}
```

### find_dead_imports

Find imports that are never referenced by any function in the importing file.

```json
{
  "uri": "file:///path/to/file.rs",
  "limit": 100
}
```

Omit `uri` to scan all files. Returns `dead_imports` (in-graph but unused) and `unresolved_imports` (external/not indexed).

Response:
```json
{
  "total_imports": 45,
  "dead_count": 3,
  "dead_imports": [
    { "file": "/path/to/handler.py", "imported_module": "os.path", "line": 4 }
  ],
  "unresolved_imports": [
    { "file": "/path/to/app.py", "imported_module": "third_party_lib" }
  ]
}
```

### get_module_summary

High-level summary of a directory: file count, function count, language breakdown, top complex functions.

```json
{
  "path": "/absolute/path/to/module",
  "top_n": 5
}
```

Response:
```json
{
  "directory": "/path/to/src/auth",
  "files": 12,
  "total_functions": 84,
  "total_classes": 6,
  "total_imports": 31,
  "total_lines": 2400,
  "languages": [
    { "language": "rust", "files": 10, "functions": 78 },
    { "language": "toml", "files": 2, "functions": 6 }
  ],
  "top_complex_functions": [
    { "name": "validate_token", "path": "/path/to/jwt.rs", "complexity": 22, "line_start": 45 }
  ],
  "external_deps": ["jsonwebtoken", "chrono", "serde"]
}
```

### search_by_pattern

Regex search across function bodies, signatures, names, and docstrings.

```json
{
  "pattern": "unwrap\\(\\)",
  "scope": "function_body",
  "node_type": "function",
  "limit": 50
}
```

`scope`: `function_body`, `signature`, `name`, `docstring`, `any` (default)
`node_type`: `function`, `class`, `any` (default)

Response:
```json
{
  "pattern": "unwrap\\(\\)",
  "scope": "function_body",
  "total_matches": 23,
  "matches": [
    {
      "name": "parse_config",
      "kind": "Function",
      "path": "/path/to/config.rs",
      "line_start": 45,
      "line_end": 72,
      "matched_in": "body",
      "matched_text": "let value = map.get(key).unwrap();",
      "signature": "fn parse_config(path: &Path) -> Config"
    }
  ]
}
```

### search_by_error

Find functions that throw, catch, or handle specific error types.

```json
{
  "error_type": "IoError",
  "mode": "throws",
  "limit": 50
}
```

`mode`: `throws` (raise/throw/Err/panic), `catches` (catch/except/?), `any` (default)
Omit `error_type` to find all error-handling functions.

Response:
```json
{
  "mode": "throws",
  "error_type_filter": "IoError",
  "total_matches": 5,
  "functions": [
    {
      "name": "read_config",
      "path": "/path/to/config.rs",
      "line_start": 30,
      "signature": "fn read_config(path: &Path) -> Result<Config, IoError>",
      "error_patterns": ["Result<", "Err(", "?"],
      "error_role": "both"
    }
  ]
}
```

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

### Workspace exclusion: `.codegraphignore` + default skip list

`reindex_workspace` and `index_directory` honor a per-folder `.codegraphignore` file (gitignore-like syntax — one pattern per line, `#` comments, blank lines ignored; no `!` negation in v1) plus a built-in skip list for binary archives, compiled artifacts, OS metadata, and bulky non-source:

- Archives: `**/*.tar.gz`, `**/*.tar.bz2`, `**/*.tar.xz`, `**/*.tgz`, `**/*.tbz2`, `**/*.zip`, `**/*.7z`, `**/*.rar`, `**/*.deb`, `**/*.rpm`, `**/*.pkg`, `**/*.dmg`, `**/*.iso`, `**/*.img`
- Binaries: `**/*.exe`, `**/*.dll`, `**/*.so`, `**/*.dylib`, `**/*.bin`, `**/*.o`, `**/*.a`, `**/*.lib`, `**/*.obj`, `**/*.pdb`, `**/*.pyc`, `**/*.class`, `**/*.jar`
- Disk images: `**/*.qcow2`, `**/*.vmdk`, `**/*.vdi`, `**/*.vhd`
- Non-source media: `**/*.pdf`, `**/*.docx`, `**/*.png`, `**/*.mp4`, etc.
- OS metadata: `**/.DS_Store`, `**/Thumbs.db`
- Misc: `**/*.sqlite`, `**/*.db`, `**/*.lock`

Prevents fastembed/ONNX runaway on workspaces containing proof bundles, cloned upstream targets, prebuilt binaries, or triage doc folders. Per-folder so each workspace can have its own rules.

Example `.codegraphignore` for a bounty workspace:

```
# Cloned target trees and proof bundles
proof-*-*/
*-MSRC-*.tar.gz
*-PSIRT-*.zip

# Cache + scratch
.poc.sh
*.poc.cpp

# Internal triage doc directories not worth indexing
triage/
```

### Embedding model selection

The MCP server accepts `--embedding-model <name>` at startup or `embeddingModel` in the LSP init options:

| Name | Dimensions | Context | Notes |
|---|---|---|---|
| `bge-small` *(default)* | 384 | 512 | Fast, English-only, 33M params |
| `jina-code-v2` | 768 | 8K | Code-aware, 6× slower than BGE |
| `granite-97m` *(or `granite`)* | 384 | 32K | IBM ModernBERT multilingual, 7.7× slower than BGE on Rust corpora; 200+ languages, +0.04 mean similarity vs BGE on related-code pairs in evals |

`granite-97m` is the long-context option for workspaces with multilingual code, generated stubs with large match arms, or functions exceeding ~2k chars (where BGE silently truncates). Storage-compatible with BGE (same 384d) but the vectors are not interchangeable; clear `~/.codegraph/graph.db/` and re-embed when switching.

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

## Pro Tools (35 tools)

### Analysis & Similarity (10)

### security_scan

Security vulnerability scan: dangerous functions, taint tracing, auth coverage, secrets.

```json
{
  "scope": "src/api",
  "severity": "high",
  "category": "injection"
}
```

`category`: `injection`, `xss`, `overflow`, `crypto`, `secrets`, `unsafe`. Returns findings ranked by severity with file/line and remediation.

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

### get_control_flow

Map every execution path through a function. Answers "can this return without hitting the auth check?" or "how many paths lead to the database call?"

```json
{
  "uri": "file:///path/to/file.rs",
  "line": 42,
  "format": "summary"
}
```

`format`: `graph` (full CFG — blocks + edges for AI agent traversal), `summary` (human-readable overview)

Returns: basic blocks (Entry, Conditional, LoopHeader, TryBlock, Return, Exit), edges (true/false/exception/break/continue), cyclomatic complexity. Language-aware for Rust, Python, TS/JS, Go.

Summary response:
```json
{
  "function_name": "run",
  "complexity": 6,
  "has_early_returns": true,
  "has_exceptions": false,
  "summary": "Function: run\nCyclomatic complexity: 6\nBasic blocks: 14\nEdges: 13\nHas early returns\nConditional branches: 3\nLoops: 1"
}
```

Graph response (truncated):
```json
{
  "function_name": "add_files_to_index",
  "complexity": 6,
  "entry_block": 0,
  "exit_blocks": [13],
  "has_early_returns": false,
  "blocks": [
    { "id": 0, "block_type": "Entry", "label": "entry",
      "statements": ["let mut indexed = 0;", "let mut failed = 0;"] },
    { "id": 1, "block_type": "LoopHeader", "label": "loop_header_L7" },
    { "id": 3, "block_type": "Conditional", "label": "if_L8" },
    { "id": 4, "block_type": "Normal", "label": "if_true_L8",
      "statements": ["tracing::warn!(\"File not found\");", "failed += 1;"] }
  ],
  "edges": [
    { "from": 0, "to": 1, "edge_type": "Unconditional", "label": "enter_loop" },
    { "from": 3, "to": 4, "edge_type": "ConditionalTrue", "label": "true" },
    { "from": 4, "to": 1, "edge_type": "Continue", "label": "continue" }
  ]
}
```

### trace_data_flow

Follow a variable from birth to death. Answers "does user input reach this SQL query?" or "is this API key passed to a logging function?"

```json
{
  "uri": "file:///path/to/file.rs",
  "line": 42,
  "variable": "response"
}
```

Returns: definitions, usages (function args, return values, field access), flow edges, taint sources (request, stdin, env, argv), and whether the variable reaches return values or external calls.

Response:
```json
{
  "function_name": "run",
  "variable": "response",
  "definitions": [
    { "line": 11, "point_type": "Definition",
      "statement": "let response = self.handle_request(request).await;" },
    { "line": 25, "point_type": "Definition",
      "statement": "let response = JsonRpcResponse::error(" }
  ],
  "usages": [
    { "line": 13, "point_type": "FunctionArg",
      "statement": "transport.write_response(&response).await?;" }
  ],
  "flows": [
    { "from_line": 11, "to_line": 13, "flow_type": "parameter" }
  ],
  "tainted_by": ["request"],
  "reaches_external_call": true,
  "reaches_return": false
}
```

### Security — Tier 1 (4)

### security_generate_sbom

Generate a Software Bill of Materials from lockfiles. Supports 8 formats: Cargo.lock, package-lock.json, yarn.lock, requirements.txt, Pipfile.lock, go.sum, Gemfile.lock, pom.xml/packages.lock.json/.csproj.

```json
{
  "format": "cyclonedx"
}
```

`format`: `cyclonedx` (full SBOM JSON), `summary` (counts per package manager).

### security_audit_deps

Check dependencies against the OSV vulnerability database. Returns vulnerable packages, severity, fixed version, and CVE IDs.

```json
{
  "severity": "high",
  "ecosystem": "npm"
}
```

`ecosystem`: `npm`, `pypi`, `crates.io`, `go`, `maven`, `nuget`, `rubygems` (omit for all).

### security_control_flow

Map every execution path through a function — same schema as `get_control_flow` above (renamed under the security namespace).

```json
{
  "uri": "file:///path/to/file.rs",
  "line": 42,
  "format": "summary"
}
```

### security_trace_data_flow

Follow a variable from birth to death — same schema as `trace_data_flow` above.

```json
{
  "uri": "file:///path/to/handler.py",
  "line": 18,
  "variable": "user_input"
}
```

### Security — Heuristic Analyzers (5)

Five category-level CWE detectors that examine function bodies for anti-patterns. Each takes the same parameters: `scope` (path substring filter), `severity`, `limit`.

### security_check_unchecked_returns

Find function calls whose return values are silently ignored — may miss errors or security-relevant status. Languages: Rust, Go, Python, JS/TS. CWE-252, CWE-391, CWE-754.

```json
{
  "scope": "src/api",
  "severity": "medium",
  "limit": 50
}
```

### security_check_resource_leaks

Find unclosed files/connections/sockets — `open()` without `close()`, `acquire()` without `release()`. CWE-401, CWE-772.

```json
{ "scope": "src/io" }
```

### security_check_misconfig

Find security misconfigurations across multiple categories. **CWE-16** (configuration), **CWE-295** (TLS verification), **CWE-614** (insecure cookies), **CWE-1004** (cookies missing HttpOnly).

Detects:
- **Debug mode in production:** `debug = true`, `DEBUG = True`, `"debug": true` in JSON/YAML
- **Permissive CORS:** `Access-Control-Allow-Origin: *`, `AllowAllOrigins`, `cors(allow_all`, `origin: '*'`
- **Cookie security:** `secure: false`, `httpOnly: false`, `Secure: false`, `HttpOnly: false`
- **TLS-verification disabled (CWE-295)** — 20 sinks across languages:
  - **libcurl:** `CURLOPT_SSL_VERIFYPEER, 0`, `CURLOPT_SSL_VERIFYHOST, 0` (with/without `L` suffix)
  - **OpenSSL:** `SSL_VERIFY_NONE`
  - **mbedTLS:** `MBEDTLS_SSL_VERIFY_NONE`
  - **Go:** `InsecureSkipVerify: true` / `InsecureSkipVerify:true`
  - **Python:** `verify=False`, `verify: false`, `ssl._create_unverified_context`, `CERT_NONE`
  - **Node.js:** `rejectUnauthorized: false`
  - **Java:** `NoopHostnameVerifier`, `ALLOW_ALL_HOSTNAME_VERIFIER`, `AllowAllHostnameVerifier`
  - **.NET:** `ServerCertificateValidationCallback`, `ServerCertificateCustomValidationCallback`, always-true lambda `(sender, cert, chain, errors) => true`
  - **Rust:** `danger_accept_invalid_certs(true)`, `danger_accept_invalid_hostnames(true)`
- **HTTP URLs in config-style code** (not localhost)

```json
{
  "scope": "config",
  "severity": "high",
  "include_tests": false
}
```

**Response example** (with TLS-verify finding):
```json
{
  "findings": [
    {
      "function_name": "fetch_unverified_curl",
      "dangerous_call": "CURLOPT_SSL_VERIFYPEER, 0L",
      "file": "src/network.c",
      "line": 42,
      "category": "misconfig",
      "severity": "critical",
      "description": "libcurl peer certificate verification disabled — accepts any cert",
      "cwe": "CWE-295",
      "owasp": "A05:2021",
      "remediation": "Always verify SSL certificates in production — use proper CA bundles"
    }
  ],
  "total": 1,
  "actionable": 1,
  "analyzer": "misconfig",
  "cwe": ["CWE-16", "CWE-295", "CWE-614", "CWE-1004"],
  "path_filter": {"findings_examined": 1, "findings_kept": 1, "skipped": {...}},
  "compile_gate": {"checked": true, "gated_off": 0, "build_defines_count": 12}
}
```

### security_check_input_validation

Find function parameters used in dangerous operations (array index, SQL, file path) without prior validation. CWE-20, CWE-129.

```json
{ "scope": "src/handlers" }
```

### security_check_error_exposure

Find error messages that leak stack traces, file paths, or internal state to users. CWE-209, CWE-497.

```json
{ "scope": "src/api" }
```

### Security — Tier 2 (4)

### security_scan_iac

Scan infrastructure-as-code files for misconfigurations: Docker (USER root, :latest tag, exposed ports, secrets in ARG/ENV), Kubernetes (privileged, hostNetwork, runAsUser:0, missing limits), Terraform (0.0.0.0/0 cidr, public RDS, missing encryption, force_destroy).

```json
{
  "scope": "infra/",
  "platform": "docker",
  "severity": "high"
}
```

`platform`: `docker`, `kubernetes`, `terraform`, `all`.

### security_check_licenses

Parse lockfile licenses and flag policy violations. Detects copyleft (GPL), unknown licenses, license-incompatible mixes.

```json
{
  "policy": "permissive_only"
}
```

`policy`: `permissive_only` (flag everything except MIT/BSD/Apache), `allow_weak_copyleft` (permit LGPL/MPL/EPL), `flag_unknown_only`.

### security_check_secrets_entropy

Find hardcoded secrets via Shannon entropy analysis — catches API keys, tokens, credentials that don't match known regex patterns. Reports redacted values.

```json
{
  "scope": "src",
  "min_entropy": 4.5,
  "severity": "medium"
}
```

`min_entropy`: bits/char. Default 4.0; raise to 4.5 for fewer false positives.

### security_detect_injection

Focused injection vulnerability detection: SQL, XSS, command injection, path traversal, deserialization, template injection. Examines body for string concatenation in dangerous contexts (unlike `security_scan` which matches function names).

```json
{
  "scope": "src/api",
  "injection_type": "sql",
  "severity": "high"
}
```

`injection_type`: `sql`, `xss`, `command`, `path_traversal`, `deserialization`, `template` (omit for all).

### Security — Tier 3 / Bounty (11)

### security_check_search_path

Detect untrusted search-path / library-loading vulnerabilities (CWE-426, CWE-427): `dlopen` with relative path, `LoadLibrary`/`LoadLibraryEx` without full path, `execvp`/`execlp` (PATH search), `System.loadLibrary`, `ctypes.CDLL` with relative path, `Process.Start` with bare command name. Filters `dlopen(NULL, ...)` self-lookup and absolute paths as safe.

```json
{
  "scope": "linux-sgx/sdk",
  "severity": "medium",
  "include_tests": false,
  "check_compile_gates": true
}
```

Severity rules:
- String literal with relative path → `high` (attacker can plant file)
- Variable argument → `medium` (needs taint review)
- Absolute path or NULL → skipped (not a finding)

Findings inside `#ifdef X` where X is never defined by the project's build system are marked `status: "DEFENSIVE_GATED_OFF: X"` and excluded from the `actionable` count.

### security_check_crypto

Detect cryptographic misuse — the bug class that historically pays Tier 1/2 bounties on attestation, HSM, key-management, and SDK targets. **113 patterns across 12 CWEs, 8 languages** (C/C++/Python/Java/Go/Rust/Ruby/PHP/Swift/JS/TS). Validated end-to-end on 5 confidential-computing targets (OE, CCF, Azure CVM SKR, AWS Nitro SDK-C, ROCm/clr) — ~100% precision after FP filtering.

**Categories:**
- **CWE-327 broken cipher:** AES-ECB, DES/3DES, RC4, Blowfish (Sweet32), CAST5/IDEA/SEED — 6 languages incl. Ruby `OpenSSL::Cipher.new("DES...")`, Swift `CCAlgorithmDES/3DES/RC4`, PHP `mcrypt_*`/`openssl_encrypt("aes-*-ecb")`
- **CWE-328 weak hash:** MD5, SHA-1, MD2, MD4 — **context-aware severity** via `classify_hash_context()`: `verify_*`/`sign_*`/`hash_password`/`hmac` + body with password/signature/auth_tag → high; `cache_key`/`etag`/`dedup` or bcrypt/argon2/scrypt/pbkdf2 present → low
- **CWE-326 weak key size:** RSA <2048, DSA <2048, weak ECC curves (secp192r1/secp192k1/secp224r1, brainpoolP192), weak TLS (SSLv3/TLSv1.0/TLSv1.1 across OpenSSL/Python/Java/Go)
- **CWE-916 weak KDF:** PBKDF2 iteration count <600k, bcrypt cost <12 (5 forms)
- **CWE-330/338 weak PRNG:** `srand(time(`, `RAND_pseudo_bytes`, `Math.random()`, `java.util.Random`
- **CWE-329 static IV/nonce**
- **CWE-347 JWT auth bypass:** 12 literal patterns (PyJWT alg=none, jsonwebtoken verify=false, JJWT, jwt-go) + HS256-with-public-key algorithm-confusion detection via `check_jwt_algorithm_confusion()`
- **CWE-798 hardcoded keys:** 6 PEM-header patterns + hex/base64 key-literal recognition on `key`/`aes_key`/`iv`/`nonce`/`secret`/`salt`/`encryption_key`/`signing_key`/`api_key` variables at standard key sizes (16/24/32 bytes)
- **CWE-310 AES-CBC without MAC:** per-function padding-oracle check (Vaudenay/POODLE class) — flags when CBC is used without hmac/HMAC_Init/Mac.getInstance present in the same body
- **CWE-780 RSA PKCS#1 v1.5 (Bleichenbacher):** 5 patterns across OpenSSL/PyCryptodome/Java
- **CWE-1239 truncated MAC compare:** `memcmp`/`strncmp`/`bcmp` with literal length <16 on MAC-named variables
- **CWE-208 timing leak:** `memcmp`/`strcmp`/`strncmp`/`bcmp` + `==`/`===` + method `.compare()`/`.equals()` on secret-typed identifiers (`token`, `password`, `hmac`, `_mac`, `signature`, `auth_tag`, `digest`, `checksum`, `_key`, `apikey`, `session_id`, `csrf`). **Suppressed entirely** when body uses any of 18 constant-time primitives: `hmac.compare_digest`, `subtle.ConstantTimeCompare`, `CRYPTO_memcmp`, `MessageDigest.isEqual`, `constant_time_eq`, `timingSafeEqual`, `Curl_timestrcmp`, `mbedtls_ct_memcmp`, `sodium_memcmp`, `sodium_compare`, `NSS_SECItemCompare`, `gnutls_memcmp`, `br_ssl_engine_memcmp`, `wolfssl_memcmp`, `IsEqual_TimingAttackResistant`, `CryptographicOperations.FixedTimeEquals` — teams using these primitives route secret compares through them, so the flagged memcmp is reliably on non-secret fields.

**False-positive filters (validated on real bounty targets, including curl full-sweep 2026-04-23):**
- **Public vs secret** — drops `memcmp` findings when operands reference `public_key`/`pub_key`/`pubkey`/`rsa_pub`/`verification_key`/`cert_pem`/`cert_chain`/`certificate`/`fingerprint`/`serial_number` etc.
- **Content-addressable IDs** — drops findings on `object_id`/`commit_hash`/`content_id`/`etag`/`merkle`/`tree_hash`/`ipfs_cid`/`image_digest`/`manifest_digest`/`layer_digest`/`oci_digest`/`store_path`/`nix_hash`/`rekor_uuid` — public-by-construction
- **Cleanup functions** — skips `*_destroy`/`*_free`/`*_cleanup`/`*_release`/`*_dispose`/`*_reset`/`*_finalize` (sentinel comparisons, not secrets)
- **Magic-byte format detection** — drops when operand is a ≤8-byte string literal (ELF/PNG/PK/MZ/OggS) or a named magic constant (`ELFMAG`/`PNG_MAGIC`/`kOffloadBundleUncompressedMagicStr`, plus auth-protocol signatures `NTLMSSP_SIGNATURE`/`SMB_SIGNATURE`/`type2_marker`/`request_marker`/`_sentinel`/`_header_tag`). Applied to both CWE-208 (ct_compare) and CWE-1239 (truncated_mac).
- **STL iterator boundaries** — drops `it == container.end()` / `.begin()` / `.cend()` patterns
- **Function-signature meta** — distinguishes `kernel.signature().version()` (function meta) from crypto signatures
- **Non-secret-compare functions** — skips public-key verifiers, validity sentinels (`*_is_valid`/`is_zero`/`sanity_check`), format detectors (`Is*Elf`/`isHsaCo`/`check_*_magic`), keygen (`*_generate_key_*`/`derive_key`/`keypair`/`*_from_private`), and content-addressable lookups (`lookup_*`/`find_by_hash`/`dedup_*`/`cache_lookup_*`/`compare_oid`) — gated on func name NOT containing `verify_`/`sign_`/`hmac_`/`auth_`/`crypto_`/`password`/`token`
- **Comparison-region localization** — needles only fire when they appear inside the actual comparison region (bounded by `;`/`{}`/`()`/`,`/`?`/`&&`/`||`), not anywhere on the line; inline `//` and `/*` comments stripped before matching
- **Protocol-mandated primitives** — weak-primitive findings in files matching a protocol-implementation pattern get severity downgraded to `low` and `status: "PROTOCOL_MANDATED: <protocol>"`. 6 mappings: NTLM/`ntlm` (DES/MD4/RC4), HTTP Digest/`digest` (MD5), Kerberos 5/`krb`|`kerberos` (DES/3DES), SASL/`sasl`|`vauth` (DES/MD4/RC4/MD5). Rationale: these primitives are RFC-required; removing them breaks interop rather than fixing a bug.

```json
{
  "scope": "src/crypto",
  "severity": "high",
  "include_tests": false
}
```

**Response example:**
```json
{
  "findings": [
    {
      "function_name": "hash_password_md5",
      "dangerous_call": "hashlib.md5(",
      "file": "/src/auth/passwords.py",
      "line": 29,
      "category": "weak_hash",
      "severity": "medium",
      "description": "MD5 hash — broken; flagged as medium since MD5 is also used for non-security purposes...",
      "language": "python",
      "cwe": "CWE-328",
      "owasp": "A02:2021",
      "remediation": "If used for passwords: use bcrypt/scrypt/argon2/PBKDF2. If for security: use SHA-256+"
    },
    {
      "function_name": "verify_token_unsafe",
      "dangerous_call": "memcmp/strcmp on secret",
      "file": "/src/auth/verify.c",
      "line": 77,
      "category": "timing_leak",
      "severity": "high",
      "description": "non-constant-time comparison of secret-typed data — comparison time may leak information",
      "cwe": "CWE-208",
      "remediation": "Use constant-time comparison: CRYPTO_memcmp (OpenSSL), subtle.ConstantTimeCompare (Go), hmac.compare_digest (Python)..."
    }
  ],
  "total": 2,
  "actionable": 2,
  "analyzer": "crypto_misuse",
  "cwe": ["CWE-208", "CWE-326", "CWE-327", "CWE-328", "CWE-329", "CWE-330", "CWE-338", "CWE-916"],
  "path_filter": {"findings_examined": 2, "findings_kept": 2, "skipped": {...}},
  "compile_gate": {"checked": true, "gated_off": 0, "build_defines_count": 0}
}
```

Each finding's `line` field points at the actual matching body line (not the function header) — jump straight to the sink. `status: "DEFENSIVE_GATED_OFF: X"` marks findings behind an `#ifdef X` that the build system never defines.

### security_check_integer_overflow

Detect integer-overflow patterns leading to buffer bugs (**CWE-190 → CWE-120**). Targets parsers (media codecs, model loaders, image/archive/video parsers), kernel drivers, and any C/C++ code that reads length fields from untrusted input. Integer-overflow bugs in parsers are one of the highest-paying bounty classes because they typically yield remote code execution. **C/C++ only** for v1.

**Two patterns:**
1. **Allocation multiply overflow** — `malloc(n*size)` / `kmalloc(a*b)` / `realloc(p, a*b)` / `new T[a*b]` where the product can overflow `size_t`, producing a small allocation followed by a buffer overflow on write. Covers 19 allocator APIs: `malloc`, `realloc`, `kmalloc`, `kzalloc`, `kvmalloc`, `kvzalloc`, `krealloc`, `vmalloc`, `vzalloc`, `g_malloc`/`g_malloc0`/`g_try_malloc`/`g_try_malloc0`, `aligned_alloc`, `HeapAlloc`, `LocalAlloc`, `GlobalAlloc`, `VirtualAlloc`, `mmap`, plus C++ `new T[expr]`.
2. **Length-copy arithmetic** — `memcpy`/`memmove`/`memset`/`strncpy`/`strncat`/`bcopy`/`wmemcpy`/`wmemmove`/`wmemset`/`memcpy_s`/`memmove_s` with an arithmetic length expression (`len + offset`, `count * size`, `1 << n`) that can wrap and write past the destination.

**FP guards:**
- `calloc(n, size)` is deliberately **not** flagged — it performs the overflow check itself and is the recommended remediation.
- Identifier-boundary match so `xmalloc`/`my_malloc`/custom wrappers don't match the bare `malloc` pattern.
- C type-word filter (`void *p`, `char *buf`, `const *ptr`, `uint32_t *out`, etc.) avoids treating pointer declarations as multiplication.
- `p->field` member access not flagged as arithmetic (skips `->`, `++`, `--`).
- Struct-member access (`p->size`) and constant-size (`sizeof(T)`) lengths kept as safe.
- **Compile-time-constant arithmetic** (added 2026-04-23 from curl triage) — suppressed when every operand is one of: `sizeof(...)`, integer/char literals, `strlen("literal")`, or SCREAMING_CASE identifiers (enum/define convention). `memset(dst, 0, STRING_LAST * sizeof(char *))` and `memset(&msg, 0, sizeof(msg) - sizeof(msg.bytes))` no longer flag — the product is known at compile time and cannot wrap at runtime.
- **Bounds-check predecessor detection** (added 2026-04-23 from curl triage) — for a flagged `memcpy`/`memset`/`memmove`, scan 50 lines back within the same function body for a gate `if (len_ident >= dst_size) return|goto|abort;` that covers the length-expression identifiers. Suppresses when found. **Strict:** only `>=` / `>` accepted (rejects `<=` / `<` because `memcpy(dst, src, len + 1)` under a `<= dst_size` gate allows `len == dst_size` which is a 1-byte overflow). Gate body must exit the function (`return`/`goto`/`abort`/`exit`/`BUG`/`panic!`/`break`/`continue`).

```json
{
  "scope": "src/parser",
  "severity": "medium",
  "include_tests": false
}
```

**Response example:**
```json
{
  "findings": [
    {
      "function_name": "parse_header",
      "dangerous_call": "malloc with multiplication",
      "file": "/src/parser/image.c",
      "line": 142,
      "category": "integer_overflow",
      "severity": "medium",
      "description": "malloc() size argument uses multiplication — if the operands are attacker-controlled or unbounded, the product can overflow size_t, producing a small allocation followed by a buffer overflow on write (CWE-190 → CWE-120).",
      "language": "c",
      "cwe": "CWE-190",
      "owasp": "A04:2021",
      "remediation": "Use calloc(n, size) — it returns NULL on overflow. Alternatively, check `n > SIZE_MAX / size` before multiplying, or use compiler built-ins like __builtin_mul_overflow."
    },
    {
      "function_name": "copy_chunk",
      "dangerous_call": "memcpy with arithmetic length",
      "file": "/src/parser/image.c",
      "line": 218,
      "category": "integer_overflow",
      "severity": "high",
      "description": "memcpy() length argument is a computed expression — if operands can overflow or are attacker-controlled without bounds checks, the byte count wraps and the destination buffer is written beyond its bounds (CWE-190 → CWE-120).",
      "cwe": "CWE-190",
      "remediation": "Validate length ≤ destination buffer size BEFORE the copy. For length-prefixed formats, check `len + offset <= buf_size && len + offset >= len` (detects wrap). Prefer memcpy_s (C11 annex K) where available."
    }
  ],
  "total": 2,
  "actionable": 2,
  "analyzer": "integer_overflow",
  "cwe": ["CWE-190", "CWE-120", "CWE-680"],
  "path_filter": {"findings_examined": 1, "findings_kept": 1, "skipped": {...}},
  "compile_gate": {"checked": true, "gated_off": 0, "build_defines_count": 0}
}
```

### security_check_null_deref

Detect NULL-pointer dereferences (**CWE-476**). For each allocation site, scans the next 25 lines of the same function for either a NULL check or a dereference. If a deref appears first, flag it. Crashes are often a DoS primitive; under certain memory mappings an unchecked NULL deref becomes a write-to-NULL primitive exploitable for privilege escalation. **C/C++ only** for v1.

**Allocators tracked (56 total):**
- **C standard:** `malloc`, `calloc`, `realloc`, `aligned_alloc`, `valloc`, `reallocarray`, `strdup`, `strndup`
- **Linux kernel:** `kmalloc`, `kzalloc`, `kcalloc`, `kmalloc_array`, `kvmalloc`, `kvzalloc`, `krealloc`, `vmalloc`, `vzalloc`, `alloc_skb`, `alloc_pages`, `__get_free_pages`
- **GLib:** `g_malloc`, `g_malloc0`, `g_try_malloc`, `g_try_malloc0`, `g_strdup`, `g_strndup`, `g_new`, `g_new0`, `g_try_new`, `g_try_new0`
- **I/O / environment:** `fopen`, `freopen`, `popen`, `tmpfile`, `getenv`, `secure_getenv`
- **Windows heap:** `HeapAlloc`, `LocalAlloc`, `GlobalAlloc`, `VirtualAlloc`, `CoTaskMemAlloc`
- **Project-specific wrappers** (added 2026-04-23): `curlx_malloc`, `curlx_calloc`, `curlx_realloc`, `curlx_strdup`, `curlx_strndup`, `Curl_saferealloc` (curl); `apr_palloc`, `apr_pcalloc` (Apache Portable Runtime); `talloc`, `talloc_zero`, `talloc_array` (Samba); `gnutls_malloc`, `gnutls_calloc`, `gnutls_strdup` (GnuTLS); `xstrdup`; `OPENSSL_malloc`, `OPENSSL_zalloc`, `OPENSSL_strdup`, `CRYPTO_malloc`, `CRYPTO_zalloc` (OpenSSL); `palloc`, `palloc0`, `repalloc`, `pstrdup` (PostgreSQL)

`operator new` is excluded by design — it throws on failure; only `new (std::nothrow)` returns NULL (caught separately if needed).

**NULL-check forms accepted (stops the scan):**
- `if (!p)`, `if (p)`, `if (p == NULL)`, `if (NULL == p)`, `if (p != NULL)`, `if (p == 0)`, `if (p == nullptr)`
- `BUG_ON(!p)`, `WARN_ON(!p)` (kernel idioms), `assert(p)` (userspace)

**Deref forms that trigger the finding:**
- `p->field` — arrow member access
- `*p` — star deref (not `&p`, `**p`, or `*p2`)
- `p[i]` — array index
- `(*p)` — parenthesized deref

**FP guards:**
- Identifier-boundary match so `xmalloc`/`my_malloc`/custom non-null wrappers don't match `malloc`.
- C-keyword filter on LHS (won't extract `void`/`int`/`const`/etc. as variable names).
- Member/array LHS (`obj->ptr = malloc(...)`, `arr[i] = malloc(...)`) rejected — can't reliably track.
- Reassignment tracking: if the variable is reassigned before a deref, the window resets.

```json
{
  "scope": "src/driver",
  "severity": "medium",
  "include_tests": false
}
```

**Response example:**
```json
{
  "findings": [
    {
      "function_name": "handle_packet",
      "dangerous_call": "kmalloc without NULL check",
      "file": "/drivers/net/foo.c",
      "line": 42,
      "category": "null_deref",
      "severity": "high",
      "description": "Pointer `skb` is returned by kmalloc() (which may return NULL on failure) and is dereferenced before any NULL check — crash at minimum, often a DoS primitive and sometimes an exploitable write-to-NULL primitive under specific memory mappings (CWE-476).",
      "language": "c",
      "cwe": "CWE-476",
      "owasp": "A04:2021",
      "remediation": "Check for NULL before dereferencing: `if (!skb) return -ENOMEM;` (or appropriate error code). For kernel code, consider `unlikely(!skb)` for the hot path. Propagate allocation failures up to the caller rather than continuing with a NULL pointer."
    }
  ],
  "total": 1,
  "actionable": 1,
  "analyzer": "null_deref",
  "cwe": ["CWE-476"],
  "path_filter": {"findings_examined": 1, "findings_kept": 1, "skipped": {...}},
  "compile_gate": {"checked": true, "gated_off": 0, "build_defines_count": 0}
}
```

### security_check_ssrf

Detect Server-Side Request Forgery (**CWE-918**). For each HTTP request handler (or helper function called from one), checks whether the body extracts a URL from request input AND performs an outbound HTTP/network fetch WITHOUT an SSRF safeguard. Canonical targets: Grafana CVE-2020-13379 (avatar handler), CVE-2022-31107 (OAuth implicit grant), CVE-2024-1442 (multi-tenant DataSource URL).

**Source patterns (4 tiers, in confidence order):**
1. **Explicit request-extraction** — `c.Query(...)`, `req.body.url`, `params[:url]`, `@RequestParam`, `request.args.get(...)` etc. across Go (gin/chi/echo/fiber/Grafana), Python (Flask/FastAPI/Django/aiohttp), JS/TS (Express/Fastify/NestJS/Koa), Java (Spring/JAX-RS), Ruby (Rails), .NET (ASP.NET).
2. **Receiver-style URL field access** — `ds.URL`, `proxy.ds.URL`, `route.URL`, `dsInfo.URL`, `webhook.Url`, `notifier.URL`. Catches the CVE-2024-1442 lane where DataSource URLs are stored as struct fields.
3. **URL-like parameter on function signature** + a caller within MAX_DEPTH=6 has an explicit source — promotes the heuristic to high confidence (cross-function dataflow).
4. **URL-like parameter only** (no explicit-source caller) — medium confidence.

**Sink patterns:** 50+ across Go (`http.Get/Post/NewRequest`, `net.Dial*`, `httputil.NewSingleHostReverseProxy`), Python (`requests.get`, `urllib*.urlopen`, `httpx.*`, `aiohttp.*`), JS/TS (`fetch`, `axios.*`, `http.request/get`, `got`), Java (`URL.openConnection/openStream`, `HttpClient.send`, `OkHttpClient.newCall`, `RestTemplate.getForObject`), C/C++ (libcurl `CURLOPT_URL`), Rust (`reqwest::*`, `hyper::Client`, `ureq::*`, language-gated bare `.get(`/`.post(`), .NET (`WebRequest.Create`, `HttpClient.GetAsync`).

**Safeguard patterns** (presence in body suppresses the finding) — 30+ across `ssrfprotect.*`, `safeurl.*`, `NewSafeHTTPTransport`, `IsPrivateIP`, `IsLoopback`, host allowlist forms, Grafana's `datasourceProxyTransport`, Facebook's `safeurl`, Wikimedia's `ssrfprotect`.

**Round-6 refinements:**
- **Trust-boundary tiering** — every finding is classified `server-admin` (single-tenant trust, demoted to LOW), `org-admin` (multi-tenant CVE lane, retained), `authenticated`, or `untrusted`. Field name + struct name + path heuristic. ServerAdmin: `authParams.Url`, `Plugin.*Url*`, `cfg.*Url*`, `setting.*`, `/pluginproxy/`. OrgAdmin: `DataSource.URL`, `dsInfo.*`, `webhook.*`, `notifier.*`, `/tsdb/`, `/ngalert/`, `/datasourceproxy/`.
- **Admin-gating awareness** — caller-chain BFS for `c.IsGrafanaAdmin`/`c.HasRole`/`ReqGrafanaAdmin`/`ReqOrgAdmin`/`@Secured("ADMIN")`/`@PreAuthorize`/`requireAdmin`/`[Authorize(Roles="Admin")]`. When found, severity downgrades by one level.
- **Upstream input-validation tracing** — caller-chain BFS for regex (`MatchString`/`re.fullmatch`/`Pattern.matches`/`RegExp.test`), allowlist (`slices.Contains`/`whitelist.contains`), or prefix bound (`strings.HasPrefix`) followed by abort (`return`/`panic`/`raise`/`throw`/`JsonApiErr`/`abort`). When the input is constrained upstream, the finding is suppressed entirely. Grafana's avatar gravatar-hash regex is the canonical case.
- **Cross-project BFS scoping** (#M3 #26) — caller walks scoped to project root (`go.mod`/`Cargo.toml`/`package.json`/`.git` etc.) so multi-target sweeps don't produce phantom edges across separately-indexed repos.

```json
{
  "scope": "pkg/api",
  "severity": "medium",
  "include_tests": false
}
```

### security_check_idor

Detect Insecure Direct Object Reference / missing-authorization-correlation (**CWE-639/284**). For each HTTP handler with an object-lookup but no body-local authz call, flag as candidate IDOR. Canonical targets: Grafana CVE-2022-21713 (dashboard IDOR), CVE-2023-4822 (org isolation bypass), Mattermost historical CVEs.

**Lookup-by-ID patterns:** GORM/xorm/ent (`.First`, `.Take`, `db.Find`), named helpers (`*GetByID`, `*FindByID`, `*LoadByID`, `*LookupByID`), Django/SQLAlchemy (`.objects.get`, `.query.get`, `.filter_by`), Rails AR (`.find(params[:id])`), JPA/Hibernate (`repository.findById`, `entityManager.find`), raw SQL (`SELECT ... WHERE id = ?`).

**Authz-evaluator patterns:** 50+ across Grafana (`ac.Evaluate`, `accesscontrol.EvalPermission`, `authorizeInOrg`, middleware role checks), Kubernetes (`authorizer.Authorize`, `SubjectAccessReview`), Django (`@permission_required`, `user.has_perm`), Rails Pundit/CanCan (`authorize @resource`, `policy(...).show?`), Spring Security (`@PreAuthorize`, `@Secured`), NestJS (`@UseGuards`), generic `*Authorize*`/`*CheckAccess*`/`*HasPermission*`/`*Permit*` named calls.

**Round-6 refinements:**
- **Route-level authz middleware recognition** — scans `**/api/**`, `*routes*.go`, `*router*.go` files (read directly from disk to bypass body_prefix truncation) for `routing.Wrap(handler)` and `<router>.<Verb>("/path", ..., handler)` calls. When the registration body also contains an authz pattern (`authorizeInOrg`, `@PreAuthorize`, `requireAuth`, etc.), the wrapped handler name is added to the gated set and IDOR findings on it are suppressed. Catches the dominant FP class on Grafana and most Go web frameworks.
- **Session-derived ID exclusion** — when both URL `:id` and self-reference (`c.SignedInUser.UserID`, `request.user.id`, etc.) are present in the body, traces the variable used as the lookup-line ID back to its assignment site. If the assignment source is a self-reference expression, suppresses (the URL `:id` is for an unrelated entity; the actual lookup is on the caller's own data).
- **Public-token URL-param suppression** — invite codes / password-reset tokens / email-verification codes are public-by-design (`:code`/`:token`/`:invite`/`:reset_token`); the token IS the authorization. Not flagged.

```json
{
  "scope": "pkg/api",
  "severity": "medium",
  "include_tests": false
}
```

### security_check_fail_open_verify

Detect fail-open verification (**CWE-755 → CWE-347 / CWE-295**). Catches Go code where a verify call returns error, the caller enters the `if err != nil` branch, logs a warning, and continues past the check treating unverified input as verified. Canonical target: helm CVE-2026-35205 (`pkg/downloader/chart_downloader.go DownloadTo`).

**Two detection passes:**

1. **Verify-named-call shape** — `if err := Verify*(...); err != nil { warn(); /* no return */ }`. Verify keywords: `verify`, `validate`, `authenticate`, `checksig`, `checksum`, `authorize`, `checkauth`, `digest`, `hmaccheck`. Branch-exit detection: any of `return err`, `return nil, err`, `return nil, fmt.Errorf(...)`, `panic`, `os.Exit`, `log.Fatal*`. Logging without a return is fail-open.

2. **Verify-flag-conditional shape** (round-6 #25) — any `if err != nil` branch that contains an inner conditional gated by a `verify`/`validate`/`strict`/`required`/`mandatory`/`enforce` flag (`if c.Verify == VerifyAlways`, `if opts.Strict`), where the strict path returns an error AND the default path warns and returns success-shaped (`return ..., nil`). The combined pattern is the signature of CVE-2026-35205-class bugs.

**FP guards:**
- **Truncation-fallback** — when `body_prefix` is at the 1024-char cap, the full function body is read from disk via brace-matching from `line_start`. The CVE-2026-35205 site lives ~80 lines into a >100-line function; the truncated prefix doesn't contain the fail-open shape.
- **Aggregator-pattern recognition** (round-6 #24) — `errs = append(errs, err); ...; return errors.Join(errs...)` and analogues (`multierr.Combine`, `.ErrorOrNil()`, `AggregateError(errors)`, Python `ExceptionGroup`, JS `AggregateError`) — errors ARE propagated, just deferred to function exit. Suppressed.

```json
{
  "scope": "pkg/downloader",
  "severity": "medium"
}
```

Go only for v1; Python/Java/Rust variants to follow.

### security_check_fd_path_asymmetry

Detect functions that use path-resolving filesystem ops (`os.Remove`, `os.Symlink`, `os.MkdirAll`, `os.OpenFile`, etc.) on a security-boundary path string (`rootfs`, `containerRoot`, `chroot`, etc.) when the same file ALSO opens an fd-handle (`os.OpenFile(p, O_DIRECTORY|O_PATH|O_NOFOLLOW)`) for the same root, OR uses fd-based ops elsewhere (`Openat`, `Symlinkat`, `pathrs.*`). The asymmetry signals an incomplete migration to fd-anchored fs ops — the file is mid-migration but a helper got missed (**CWE-367**, TOCTOU).

Bug class behind **runc CVE-2025-31133** (maskedPaths /dev/null source-swap), **CVE-2025-52565** (/dev/console bind-mount source-swap), **CVE-2025-52881** (procfs write redirect). All three were fixed by migrating to fd-based + safe-procfs API.

```json
{
  "scope": "/path/to/runc",
  "severity": "medium"
}
```

**Identifier sub-classification (v1.1):**
- **HIGH-prior:** `Rootfs`, `RootFS`, `containerRoot`, `chroot`, `attestation_root` — capitalized + container-specific.
- **MEDIUM-prior:** `rootfs` (lowercase), `RootDir`, `rootDir` — ambiguous shapes.
- **LOW-prior:** `stateDir`, `cacheDir`, `dataDir`, `workDir`, `imagesDir`, `confDir` — operator-controlled paths.

**Severity matrix:**

| Identifier prior | fd-holder/fd-ops in file | Severity |
|---|---|---|
| HIGH | yes | HIGH (mid-migration TP) |
| HIGH | no | MEDIUM (unmigrated container code) |
| MEDIUM | yes | HIGH (mid-migration on ambiguous identifier) |
| MEDIUM | no | suppressed (operator-controlled false positive class) |
| LOW | yes | MEDIUM (path-only helper alongside fd ops; might be a real miss) |
| LOW | no | suppressed (operator state dir, no migration signal) |

**Validated:** runc rootfs_linux.go (4 HIGH TPs including bonus `mountToRootfs:628 os.Lstat(dest)` that manual triage missed); containerd `cleanupWorkDirs` and incus-migrate `transferRootfs` correctly suppressed under the LOW/MEDIUM-prior + no-fd-signal rule.

v0: Go only. Cross-file analysis deferred to v1.

### security_check_path_join_absolute_rhs

Detect Rust `<base>.join(<rhs>)` calls where `<rhs>` is attacker-controlled (tar/zip entry path, or derived from one) AND no absolute-path guard precedes AND a write-class filesystem sink (`fs::hard_link`, `fs::symlink`, `fs::write`, `xattr::set`, `nix::sys::stat::mknod`) consumes the result (**CWE-22**).

Detector key insight: Rust `Path::join` silently returns rhs verbatim when rhs is absolute — `dest.join("/etc")` yields `/etc`. String-concat path-traversal detectors miss this method-call shape entirely.

Bug class behind **GHSA-84rc-2q4r-45pc** (image-rs `try_hardlink_fallback`, CVE-class fixed) and the image-rs `convert_whiteout` opaque-dir whiteout escape (sibling found by bounty session 2026-05-05).

```json
{
  "scope": "/path/to/image-rs",
  "severity": "low"
}
```

**Severity bands:**
- **HIGH:** write-class sink, no upstream guard, no downstream check.
- **MEDIUM:** write-class sink + downstream `canonicalize() / fs::canonicalize(...)` + `starts_with(<base>...)` check.
- **LOW (audit):** read-only sink (`fs::read`, `fs::metadata`, `Path::exists`).

**Suppressed:**
- rhs is a string literal (no taint).
- Upstream guard: `<rhs>.is_absolute()`, `<rhs>.is_relative()`, `<rhs>.strip_prefix("/")`, `Component::RootDir` filter — bound to the specific variable being guarded.
- `Iterator::join` (rhs is a separator string like `","`).
- Commented-out joins.

**Taint sources recognized:** `tar::Entry::path()`, `tokio_tar::Entry`, `astral_tokio_tar::Entry`, `zip::name`, `header().path_bytes()`, `file.path()`, `entry.link_name()`, plus archive-shaped param names (`entry`, `entry_path`, `path`, `linkname`, `header`, `name`, `tar_path`, `archive_path`, `zip_path`).

Multi-line `let X = expr;` assignments are followed via the full expression (paren/brace-balanced) — required to catch the canonical `try_hardlink_fallback` shape where the taint-providing `.path()` call lives on a continuation line.

v0: Rust only. Python `os.path.join` has the same quirk and is queued for v1.

### security_check_rest_handler_missing_auth

Detect REST handler functions that lack authorization checks when sibling handlers in the same file or directory have them. Asymmetry signals copy-paste oversight, mid-migration miss, or undocumented intentional exemption (**CWE-862**).

Bug class behind iotedge mgmt API `restart_or_start_or_stop.rs:77 fn post` (only mutating mgmt endpoint missing `auth_agent`), seven GET endpoints across `module/`, `identity/`, `system_info/`, and the workload-API `module/list.rs:43 fn get` (leaks env vars + registry credentials across module isolation boundary).

```json
{
  "scope": "/path/to/edgelet-http-mgmt",
  "severity": "medium"
}
```

**Detection:**
- Handlers identified as `async fn (get|post|put|delete|head|patch)(...)` definitions (covers `impl Route for ...` trait impls and axum/hyper-direct shapes).
- Auth-call detection: 17 patterns (`auth_agent`, `auth_caller`, `require_auth`, `check_auth`, `verify_token`, `current_user.is_authenticated`, axum extractors `AuthenticatedUser`/`RequireAuth`/`AuthBearer`, etc.).
- Sibling comparison scope: same directory.
- Test-module truncation: analysis stops at `#[cfg(test)] mod tests { ... }` boundary so nested test-helper fns aren't mis-classified.

**Severity bands:**
- **HIGH:** state-changing method (POST/PUT/DELETE/PATCH) without auth, ≥1 sibling has auth — copy-paste signal.
- **MEDIUM:** GET/HEAD without auth, ≥1 mutating sibling has auth — sensitive read leak.
- **LOW (audit):** solo handler with no sibling for comparison.

**Suppressed:**
- `// PUBLIC` / `// UNAUTH` / `// intentionally unauthenticated` / `#[unauth]` doc markers (5-line lookback above the handler).
- Health-check paths: `/health`, `/livez`, `/readyz`, `/metrics`, `/version`, `/openapi`, `/docs/`, `/swagger`, `health_check`, `readiness`, `liveness`.
- Static-literal-only handler bodies (no `self.runtime` / `.lock` / `.query` / `.await` AND ≤200 chars).
- Auth via signature extractor (`AuthenticatedUser` etc. as a fn-signature param type).
- Symmetric-absence (all siblings also lack auth = consistent design choice).

**Validated:** edgelet-http-mgmt — 5 of 5 production TPs caught (1 HIGH POST + 4 MEDIUM GETs), 0 FPs. The 4 nested-test FPs from `#[tokio::test] async fn auth() { async fn delete(...) }` shape correctly suppressed.

v0: Rust only. Multi-language (Go chi/gin/echo, Python FastAPI/Flask) queued for v1.

### security_export_sarif

Aggregate findings from all 16 security detector classes into a single SARIF 2.1.0 document. Output is uploadable to GitHub Code Scanning, GitLab SAST, Azure DevOps. Each finding maps to a SARIF rule keyed by CWE.

```json
{
  "scope": "src",
  "severity": "high",
  "detectors": ["scan", "injection", "search_path"]
}
```

`detectors`: array of detector names. Omit or pass `[]` for all. Names: `scan`, `injection`, `search_path`, `iac`, `secrets_entropy`, `unchecked_returns`, `resource_leaks`, `misconfig`, `input_validation`, `error_exposure`, `crypto`, `integer_overflow`, `null_deref`, `ssrf`, `idor`, `fail_open_verify`, `fd_path_asymmetry`, `path_join_absolute_rhs`, `rest_handler_missing_auth`.

Severity mapping: `critical`/`high` → SARIF `error`, `medium` → `warning`, `low` → `note`.

Returns SARIF 2.1.0 JSON with `$schema`, `version`, and `runs[]` containing `tool.driver.rules` (one per CWE) and `results[]` with stable fingerprints for diff-aware tooling.

Pipe to GitHub Code Scanning:
```bash
codegraph_security_export_sarif scope=intel/linux-sgx severity=high \
  | gh code-scanning upload-sarif --ref refs/heads/main
```

---

## Cross-cutting Parameters

Every `codegraph_security_*` tool accepts these three parameters in addition to its tool-specific args. They are optional and have sensible defaults.

### `include_tests` (boolean, default `false`)

When `false` (default), files matching test/sample/vendored patterns are skipped entirely. Categories:
- **test:** `tests/`, `/test/`, `_test.`, `.spec.`, `__tests__`, `_test.go`/`.rs`/`.py`, `.test.ts`/`.js`
- **sample:** `samples/`, `examples/`, `demo/`, `fixtures/`, `tutorial/`
- **vendored:** `third_party/`, `vendor/`, `external/`, `gperftools/`, `libunwind/`, `boost/`, `node_modules/`, plus filename prefixes (`bootstrap.*`, `jquery.*`, `lodash.*`, `react.*`, etc.) and `.min.js`/`.min.css`
- **build_or_docs:** `docs/`, `build/`, `dist/`, `target/`, `out/`

Pass `include_tests=true` to scan everything including test fixtures and vendored libraries.

### `treat_as_production` (string array, default `[]`)

Override the auto-skip for specific paths. Substring match against the file path. Useful when a vendor ships a sample app as the canonical product (e.g., `cvm-securekey-release-app`).

```json
{
  "scope": "vendor/secure-app",
  "treat_as_production": ["secure-app", "production-template"]
}
```

### `check_compile_gates` (boolean, default `true`)

When `true` and the workspace contains build-system files (CMakeLists.txt, Cargo.toml, Makefile), C/C++ findings are cross-referenced against `#ifdef X` gates. If `X` is not defined by any build file, the finding is annotated with `status: "DEFENSIVE_GATED_OFF: X"` and excluded from the `actionable` count (but still present in `findings[]`).

Set to `false` to skip the cross-reference (faster scan, no telemetry).

---

## Response Telemetry

Every security detector returns three telemetry blocks alongside the findings.

### `actionable` (integer)

Count of findings WITHOUT a `status` (i.e., not gated off). This is the number that warrants triage. Always ≤ `total`.

### `path_filter` (object)

```json
{
  "findings_examined": 18,
  "findings_kept": 14,
  "skipped": {
    "test": 2,
    "sample": 0,
    "vendored": 2,
    "build_or_docs": 0
  }
}
```

- `findings_examined` — total findings considered before path filtering
- `findings_kept` — findings retained after path filtering (= `examined - sum(skipped)`)
- `skipped` — breakdown by category

Surfaces "silent omissions" so you can verify the scan actually examined what you expected.

### `compile_gate` (object, C/C++ workspaces only)

```json
{
  "checked": true,
  "gated_off": 1,
  "build_defines_count": 12
}
```

- `checked` — whether the gating cross-reference ran
- `gated_off` — number of findings annotated `DEFENSIVE_GATED_OFF`
- `build_defines_count` — how many distinct macros were discovered in the workspace's build files

Lets you trust the cross-reference: if `build_defines_count` is 0, the workspace has no build files (or none were parsed), so gating won't catch anything. Investigate before relying on the result.

### `status` field on individual findings

Findings include an optional `status` field:

| Value | Meaning |
|---|---|
| (omitted) | Active finding — needs triage |
| `"DEFENSIVE_GATED_OFF: USE_NEW_API"` | Inside `#ifdef USE_NEW_API` which is never defined in the build system |

The status is set by the compile_gate post-processor. Findings with status are kept in the result (so you can audit them) but don't count toward `actionable`.

### `taint_reachability` (per-finding)

Bounty round-5 #M3 — every security finding is annotated with whether the function is reachable from a request handler entry point.

```json
{
  "taint_reachability": {
    "reachable_from_request": true,
    "source": "/repo/pkg/api/handler.go:42 (gin handler)"
  }
}
```

Tri-state semantics:
- `Some(true)` — call-graph BFS reached an entry point within MAX_DEPTH=6 hops. `source` field shows the framework-detected handler.
- `Some(false)` — fully exhausted; either no callers (orphan / startup code / test-only helper) or all transitive callers are non-handlers.
- `None` (`null`) — unknown. BFS hit MAX_DEPTH without exhausting (chain longer than 6 hops, or passes through dynamic/interface dispatch / indexer miss / unresolved-import edge).

Entry-point classifier covers Go (gin/chi/echo/fiber/iris/net-http/Grafana ReqContext), Python (Flask/FastAPI/Django REST/aiohttp decorators), Java (Spring `@RequestMapping`/`@GetMapping` etc., JAX-RS `@Path`), Ruby (Rails routes), NestJS (TypeScript decorators), plus Go `main` / `cobra.Command`.

**Project-scoped BFS** (round-6 #26) — call-graph walks are bounded by project root (located via marker files: `go.mod`, `Cargo.toml`, `package.json`, `pyproject.toml`, `pom.xml`, `build.gradle`, `composer.json`, `Gemfile`, `.git`). When multiple repos are indexed in the same graph, an entry point in project B is rejected as a phantom edge for a finding in project A. Walk continues past the rejected node — its callers may still be in the right project.

### `reachability` (scan-level aggregate)

```json
{
  "reachability": {
    "entry_points": 928,
    "reachable_from_request": 10,
    "unknown": 2,
    "unreachable_from_request": 19
  }
}
```

Calibration data: lets a triager track precision over time. If `entry_points` is 0, the indexer didn't pick up any framework signature — the reachability signal is unreliable and should be ignored.

### Trust-tier classification (SSRF only)

`security_check_ssrf` adds a `Trust tier:` clause to every finding's description, classifying the source as one of:

- `server-admin (single-tenant trust — exploit requires server-admin role)` — severity demoted to LOW. Examples: plugin metadata `JWTTokenAuth.Url`, `cfg.GravatarURL`, `setting.*` fields.
- `org-admin (multi-tenant CVE lane — DataSource/webhook/integration URL configured per-org)` — KEPT at full severity. Historical Grafana SSRF lane.
- `authenticated (any logged-in user)` — kept at default severity.
- `untrusted (anonymous request input)` — highest urgency.

Triagers should rank `org-admin` and `untrusted` highest; `server-admin` is rarely a real exploit primitive in single-tenant deployments.

### Cross-function source promotion (SSRF / IDOR / fail-open)

Helper functions that take a URL/ID parameter from a caller inherit the caller's source confidence. Example SSRF description:

```
Function `sendReqNoTimeout` ... AND performs an outbound HTTP/network call ...
Cross-function source: caller `GetPluginArchive` extracts user input via `dlOpts.URL` URL field.
```

The walk is depth-bounded (MAX_DEPTH=6), cycle-safe, project-scoped, and excludes test-context callers (so tests building mock URLs don't masquerade as the source).

---

## Suppression Comment Honoring

Five detectors (`unchecked_returns`, `misconfig`, `search_path`, `injection_detection`, `entropy_secrets`) honor 25 inline suppression markers used by industry linters. When a suppression marker is present:
- **Inline suppression** (same line): the line is skipped during scanning
- **Body-level suppression** (any line in the function): the entire function is treated as security-reviewed and skipped

**Recognized markers:**
- bandit (Python): `# nosec`, `# nosec B608`
- flake8: `# noqa`, `# noqa: E501`
- pylint: `# pylint: disable`
- mypy: `# type: ignore`
- C/C++/Go/Java/Rust: `// nolint`, `// NOLINT`, `/* nolint */`
- CodeQL: `// codeql[ignore]`, `# codeql[ignore]`
- Semgrep: `// nosemgrep`, `# nosemgrep`
- Coverity: `// coverity[...]`
- RuboCop: `# rubocop:disable`
- ESLint: `// eslint-disable`, `/* eslint-disable`
- Generic: `// SAFETY:`, `// SAFE:`, `# pragma: allowlist`

Suppression is detected even when the marker is on a different line than the dangerous pattern (common SQL pattern: `# nosec` on the query string declaration, `cursor.execute(query)` later in the function).

---

## Bounty Triage Workflow

Recommended sequence for security audits, especially against vendor open-source repos (Intel, AMD, NVIDIA, etc.):

### 1. Index the production code (skip tests/samples)

```bash
codegraph_index_directory path=/tmp/intel-target/src
codegraph_index_directory path=/tmp/intel-target/lib
# Skip /tmp/intel-target/{tests,samples,external} — those are filtered later
```

### 2. Run the bounty-relevant detectors with default filters

```bash
# Search-path / DLL-hijack class (CWE-426/427)
codegraph_security_check_search_path scope=/tmp/intel-target severity=medium

# Cryptographic misuse (CWE-208/310/326-330/338/347/780/798/916/1239)
codegraph_security_check_crypto scope=/tmp/intel-target severity=medium

# Injection class (CWE-22/78/79/89/502/1336)
codegraph_security_detect_injection scope=/tmp/intel-target severity=high

# TLS-verify class (CWE-295) via misconfig
codegraph_security_check_misconfig scope=/tmp/intel-target severity=high

# Integer overflow in parsers/codecs (CWE-190 → CWE-120)
codegraph_security_check_integer_overflow scope=/tmp/intel-target severity=medium

# NULL-pointer deref in C/C++ (CWE-476)
codegraph_security_check_null_deref scope=/tmp/intel-target severity=medium

# SSRF — multi-tenant SaaS DataSource/webhook URL → outbound HTTP (CWE-918)
codegraph_security_check_ssrf scope=/tmp/intel-target severity=medium

# IDOR — handler with object-lookup but no authz check (CWE-639/284)
codegraph_security_check_idor scope=/tmp/intel-target severity=medium

# Fail-open verification — error suppressed in error branch (CWE-755/347/295)
codegraph_security_check_fail_open_verify scope=/tmp/intel-target severity=medium

# Memory safety (CWE-120/134/787) via security_scan
codegraph_security_scan scope=/tmp/intel-target severity=critical category=overflow

# Aggregate everything as SARIF for triage tooling
codegraph_security_export_sarif scope=/tmp/intel-target severity=high
```

### 3. Triage by reading the response telemetry

For each scan, check three numbers:

| Field | What it tells you |
|---|---|
| `actionable` | How many findings actually need human review |
| `path_filter.skipped.vendored` | How many findings were dropped as upstream code (good — those route to upstream, not the vendor) |
| `compile_gate.gated_off` | How many findings sat behind `#ifdef X` where X was never defined (good — those are dead code) |
| `compile_gate.build_defines_count` | Sanity check: if 0, gating didn't find any CMake/Cargo/Makefile to parse — investigate |

### 4. Verify each `actionable` finding

- **search_path:** trace the path arg back to a literal. If it's a relative `.so`/`.dll`, confirm `RPATH`/`RUNPATH`/`SetDefaultDllDirectories` mitigation. Run `readelf -d` on the shipped binary.
- **crypto:** the detector's context-aware severity already classifies `verify_*`/`sign_*`/`hmac_*`/`hash_password` functions as high and `cache_key`/`etag`/`dedup` as low. Confirm the classification matches the actual use site. FP filters already drop public-key compares, cleanup functions, and content-addressable ID compares (git OIDs, OCI digests, etags).
- **integer_overflow:** trace size/length operands back to their source. If the operand is bounded (small constant, validated against a maximum), mark as FP. High-severity cases: length derived from an attacker-supplied header field without a prior `if (len > MAX)` check.
- **null_deref:** verify the allocation is actually reachable and not guarded by an earlier check (e.g., a custom panic-on-OOM wrapper). The detector's 25-line window can miss checks further down; confirm the pattern by reading the function body. Kernel `kmalloc` + dereference on the next line is almost always a real bug — ship as high.
- **ssrf:** check the trust-tier classification in the description. `server-admin`-tier findings are demoted automatically. `org-admin` is the multi-tenant CVE lane — those are real-shape against multi-org self-hosted Grafana / GitLab / etc. Verify the URL flows from the configured field into the fetch sink without an `IsPrivateIP`/`safeurl` check; trace the per-request taint to confirm an org-admin-only attacker can hit internal endpoints (169.254.169.254, 10.0.0.0/8, etc.).
- **idor:** the detector already suppresses route-level authz middleware (`routing.Wrap` + `authorizeInOrg`/`@PreAuthorize`/etc.) and session-derived ID lookups (`c.SignedInUser.UserID`). Remaining findings need cross-check: trace the lookup variable back to its source — if it's a URL `:id` parameter and the handler doesn't appear in the project's route registration with an authz wrap, that's a real candidate. Confirm by issuing a request as a low-privilege user against another tenant's resource.
- **fail_open_verify:** `verify-flag-conditional` findings are typically real — check the strict branch returns the error AND the default branch only logs/warns then returns nil. Aggregator patterns (`return errors.Join(errs...)`) are auto-suppressed.
- **injection:** trace the dangerous string back to its source. If it's a constant or sanitized variable, mark as FP.
- **TLS-verify:** confirm the disabling code is reachable in production builds (not just behind a `--debug` flag).

### 5. Generate the triage report

Use the [`/bounty` skill](#) to scaffold a triage doc. The skill workflow:
1. Reads the relevant `codegraph_security_*` results
2. Cross-references against the vendor's bounty program scope
3. Generates a triage doc at `~/projects/bounty/<vendor>/YYYY-MM-DD-<repo>-<vector>.md`
4. Generates a PoC harness at `<same>.poc.sh` (you verify in a lab VM)

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

## Supported Languages (38)

Bash, C, C++, C#, Clojure, COBOL, CSS, Dart, Dockerfile, Elixir, Elm, Erlang, Fortran, Go, Groovy, Haskell, HCL/Terraform, Java, Julia, Kotlin, Lua, Objective-C, OCaml, Perl, PHP, Python, R, Ruby, Rust, Scala, Solidity, Swift, Tcl, TOML, TypeScript/JS, Verilog/SystemVerilog, YAML, Zig
