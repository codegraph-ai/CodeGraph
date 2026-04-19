# CodeGraph Tool Calling Guide

Reference for calling all 61 CodeGraph MCP tools (34 community + 27 pro, 17 security). Each tool is prefixed with `codegraph_` (e.g., `codegraph_symbol_search`).

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

## Pro Tools (26 tools)

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
  "path_filter": {"files_examined": 1, "files_matched": 1, "skipped": {...}},
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

### Security — Tier 3 / Bounty (3)

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

Detect cryptographic misuse — the bug class that historically pays Tier 1/2 bounties on attestation, HSM, key-management, and SDK targets. **35 patterns across 8 CWEs.**

**Categories:**
- **CWE-327 broken cipher:** AES-ECB (`EVP_aes_*_ecb`, `AES/ECB/`, `MODE_ECB`, `Aes128Ecb`, `Aes256Ecb`), DES/3DES (`DES_set_key`, `EVP_des_ecb/cbc/ede3`, `DESede`, `Cipher.getInstance("DES`)
- **CWE-328 weak hash:** MD5 (`hashlib.md5`, `MD5.new`, `MessageDigest.getInstance("MD5")`, `EVP_md5`), SHA-1 (`hashlib.sha1`, `MessageDigest.getInstance("SHA-1")`, `EVP_sha1`)
- **CWE-326 weak key size:** RSA <2048 (`RSA.generate(1024`, `RSA_generate_key(512`, `.initialize(1024)`)
- **CWE-916 weak KDF:** PBKDF2 (verify iteration count ≥600k for SHA-256), PBKDF2-HMAC-SHA1
- **CWE-330/338 weak PRNG for crypto:** `srand(time(`, `RAND_pseudo_bytes`, `Math.random()`, `java.util.Random`
- **CWE-329 static IV:** `iv = b'\x00' * 16`, `iv = bytes(16)`, `[0u8; 16]`
- **CWE-208 timing leak:** `memcmp`/`strcmp`/`strncmp` AND `==`/`===` on secret-typed identifiers (token, password, hmac, signature, digest, _key, csrf, session_id). Filters out `== 0`/`null`/`true`/`false` to avoid control-flow noise.

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
  "path_filter": {"files_examined": 2, "files_matched": 2, "skipped": {...}},
  "compile_gate": {"checked": true, "gated_off": 0, "build_defines_count": 0}
}
```

### security_export_sarif

Aggregate findings from all 10 security detectors into a single SARIF 2.1.0 document. Output is uploadable to GitHub Code Scanning, GitLab SAST, Azure DevOps. Each finding maps to a SARIF rule keyed by CWE.

```json
{
  "scope": "src",
  "severity": "high",
  "detectors": ["scan", "injection", "search_path"]
}
```

`detectors`: array of detector names. Omit or pass `[]` for all. Names: `scan`, `injection`, `search_path`, `iac`, `secrets_entropy`, `unchecked_returns`, `resource_leaks`, `misconfig`, `input_validation`, `error_exposure`.

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
  "files_examined": 18,
  "files_matched": 14,
  "skipped": {
    "test": 2,
    "sample": 0,
    "vendored": 2,
    "build_or_docs": 0
  }
}
```

- `files_examined` — total findings considered before path filtering
- `files_matched` — findings kept after path filtering (= `examined - sum(skipped)`)
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

# Cryptographic misuse (CWE-208/326-330/338/916)
codegraph_security_check_crypto scope=/tmp/intel-target severity=medium

# Injection class (CWE-22/78/79/89/502/1336)
codegraph_security_detect_injection scope=/tmp/intel-target severity=high

# TLS-verify class (CWE-295) via misconfig
codegraph_security_check_misconfig scope=/tmp/intel-target severity=high

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
- **crypto:** check whether the algorithm is used for security purposes (passwords, signatures, HMAC) vs benign (cache keys). MD5/SHA1 for cache = OK; for passwords = real bug.
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

## Supported Languages (31)

Bash, C, C++, C#, COBOL, Dart, Elixir, Fortran, Go, Groovy, Haskell, HCL/Terraform, Java, Julia, Kotlin, Lua, OCaml, Perl, PHP, Python, R, Ruby, Rust, Scala, Swift, Tcl, TOML, TypeScript/JS, Verilog/SystemVerilog, YAML, Zig
