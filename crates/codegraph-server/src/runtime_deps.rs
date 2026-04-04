// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Runtime dependency detection for HTTP routes and client calls.
//!
//! Post-processes the code graph after indexing to:
//! 1. Detect HTTP route handlers from function decorators/attributes
//! 2. Detect HTTP client calls from function call patterns
//! 3. Match client calls to route handlers via `RuntimeCalls` edges
//!
//! ## Supported Frameworks
//!
//! **Route handlers (decorator-based):**
//! - Python Flask/FastAPI: `@app.route("/path")`, `@app.get("/path")`, `@router.post("/path")`
//! - Python Django REST: `@api_view(["GET"])` (with urlpatterns)
//! - NestJS: `@Get("/path")`, `@Post("/path")`, `@Controller("/api")`
//! - Spring Boot: `@GetMapping("/path")`, `@RequestMapping("/path")`
//!
//! **HTTP client calls (name-based):**
//! - JS/TS: `fetch()`, `axios.get()`, `http.get()`
//! - Python: `requests.get()`, `httpx.get()`, `aiohttp`
//! - Go: `http.Get()`, `http.NewRequest()`
//! - Rust: `reqwest::get()`, `Client::get()`

use codegraph::{CodeGraph, EdgeType, NodeId, NodeType, PropertyMap};
use std::collections::HashMap;

/// HTTP methods recognized by route detection.
#[allow(dead_code)] // Used as reference; matching done inline
const HTTP_METHODS: &[&str] = &["get", "post", "put", "patch", "delete", "head", "options"];

/// Decorator patterns that indicate a route handler.
/// Format: (decorator_prefix, extracts_method_from_name)
const ROUTE_DECORATOR_PATTERNS: &[(&str, bool)] = &[
    // Python Flask/FastAPI — method is in the decorator name
    ("app.route", false), // @app.route("/path", methods=["GET"])
    ("app.get", true),    // @app.get("/path")
    ("app.post", true),
    ("app.put", true),
    ("app.patch", true),
    ("app.delete", true),
    ("router.get", true), // FastAPI APIRouter
    ("router.post", true),
    ("router.put", true),
    ("router.patch", true),
    ("router.delete", true),
    // Spring Boot — must be before NestJS short names to avoid partial matches
    ("GetMapping", true),
    ("PostMapping", true),
    ("PutMapping", true),
    ("PatchMapping", true),
    ("DeleteMapping", true),
    ("RequestMapping", false),
    // NestJS — short names last
    ("Get", true),
    ("Post", true),
    ("Put", true),
    ("Patch", true),
    ("Delete", true),
];

/// Function names that indicate HTTP client calls.
const HTTP_CLIENT_FUNCTIONS: &[&str] = &[
    // JavaScript/TypeScript
    "fetch",
    // Python requests
    "requests.get",
    "requests.post",
    "requests.put",
    "requests.patch",
    "requests.delete",
    // Python httpx
    "httpx.get",
    "httpx.post",
    "httpx.put",
    "httpx.patch",
    "httpx.delete",
    // Go
    "http.Get",
    "http.Post",
    "http.NewRequest",
    // Rust
    "reqwest::get",
    // Axios
    "axios.get",
    "axios.post",
    "axios.put",
    "axios.patch",
    "axios.delete",
    // Angular HttpClient
    "http.get",
    "http.post",
    "http.put",
    "http.patch",
    "http.delete",
];

/// Scan all Function nodes for route handler decorators and set
/// `route` and `http_method` properties where detected.
///
/// Returns the number of routes detected.
pub fn detect_route_handlers(graph: &mut CodeGraph) -> usize {
    let mut updates: Vec<(NodeId, String, String)> = Vec::new();

    for (node_id, node) in graph.iter_nodes() {
        if node.node_type != NodeType::Function {
            continue;
        }

        // Check attributes property for route decorators
        let attrs = match node.properties.get_string_list_compat("attributes") {
            Some(a) if !a.is_empty() => a,
            _ => continue,
        };

        for attr in &attrs {
            if let Some((route, method)) = parse_route_decorator(attr) {
                updates.push((node_id, route, method));
                break; // One route per function
            }
        }
    }

    let count = updates.len();

    for (node_id, route, method) in updates {
        let mut props = PropertyMap::new();
        props.insert("route", route.as_str());
        props.insert("http_method", method.as_str());
        let _ = graph.update_node_properties(node_id, props);
    }

    if count > 0 {
        tracing::info!("Detected {} HTTP route handlers", count);
    }

    count
}

/// Parse a single decorator string for route information.
///
/// Returns `Some((route, method))` if the decorator matches a known pattern.
///
/// Examples:
/// - `@app.get("/users")` → `("/users", "GET")`
/// - `@router.post("/items/{id}")` → `("/items/{id}", "POST")`
/// - `@Get("/users")` → `("/users", "GET")`
/// - `@GetMapping("/users")` → `("/users", "GET")`
/// - `@app.route("/path")` → `("/path", "ANY")`
fn parse_route_decorator(decorator: &str) -> Option<(String, String)> {
    // Strip leading @ if present
    let dec = decorator.trim_start_matches('@');

    for &(pattern, method_in_name) in ROUTE_DECORATOR_PATTERNS {
        // Check if decorator starts with this pattern (case-sensitive)
        if !dec.starts_with(pattern) {
            continue;
        }

        // Extract the route path from arguments: pattern("/path") or pattern("/path", ...)
        let after_pattern = &dec[pattern.len()..];

        let route = extract_first_string_arg(after_pattern).unwrap_or_default();

        let method = if method_in_name {
            // Extract HTTP method from the pattern name
            let method_name = pattern.rsplit('.').next().unwrap_or(pattern).to_lowercase();

            // Handle Spring's *Mapping pattern
            let method_name = method_name.trim_end_matches("mapping");

            match method_name {
                "get" => "GET",
                "post" => "POST",
                "put" => "PUT",
                "patch" => "PATCH",
                "delete" => "DELETE",
                "head" => "HEAD",
                "options" => "OPTIONS",
                "request" => "ANY",
                _ => "ANY",
            }
        } else {
            "ANY"
        };

        return Some((route, method.to_string()));
    }

    None
}

/// Extract the first string argument from a decorator's argument list.
///
/// Input: `("/users/{id}", response_model=User)` → `"/users/{id}"`
/// Input: `("/users")` → `"/users"`
/// Input: `(value="/users")` → `"/users"` (Spring-style)
fn extract_first_string_arg(args: &str) -> Option<String> {
    let args = args.trim();
    if !args.starts_with('(') {
        return None;
    }

    let inner = &args[1..]; // skip opening paren

    // Find the first quoted string (single or double quotes)
    for quote in ['"', '\''] {
        if let Some(start) = inner.find(quote) {
            let after_quote = &inner[start + 1..];
            if let Some(end) = after_quote.find(quote) {
                return Some(after_quote[..end].to_string());
            }
        }
    }

    None
}

/// Detect functions that make HTTP client calls based on their callee names.
///
/// Marks functions with `http_client_call: true` and `http_client_method: GET/POST/...`
/// properties when they call known HTTP client functions.
///
/// Returns the number of HTTP client callers detected.
pub fn detect_http_client_calls(graph: &mut CodeGraph) -> usize {
    let client_fn_names: Vec<&str> = HTTP_CLIENT_FUNCTIONS.to_vec();

    // Build a map of callee targets from Calls edges: source_id → Vec<callee_name>
    let mut call_targets: HashMap<NodeId, Vec<String>> = HashMap::new();
    for (_edge_id, edge) in graph.iter_edges() {
        if edge.edge_type != EdgeType::Calls {
            continue;
        }
        if let Ok(target) = graph.get_node(edge.target_id) {
            if let Some(name) = target.properties.get_string("name") {
                call_targets
                    .entry(edge.source_id)
                    .or_default()
                    .push(name.to_string());
            }
        }
    }

    // Find all Functions that call known HTTP client functions
    let mut callers: Vec<(NodeId, String)> = Vec::new();

    for (node_id, node) in graph.iter_nodes() {
        if node.node_type != NodeType::Function {
            continue;
        }

        // Check unresolved_calls property for HTTP client function names
        let unresolved = node
            .properties
            .get_string_list_compat("unresolved_calls")
            .unwrap_or_default();

        for callee in &unresolved {
            for &client_fn in &client_fn_names {
                if callee == client_fn || callee.ends_with(client_fn) {
                    callers.push((node_id, extract_http_method_from_name(callee)));
                    break;
                }
            }
        }

        // Check resolved Calls edges
        if let Some(targets) = call_targets.get(&node_id) {
            for target_name in targets {
                for &client_fn in &client_fn_names {
                    if target_name == client_fn || target_name.ends_with(client_fn) {
                        callers.push((node_id, extract_http_method_from_name(target_name)));
                        break;
                    }
                }
            }
        }
    }

    let count = callers.len();

    for (node_id, method) in callers {
        let mut props = PropertyMap::new();
        props.insert("http_client_call", "true");
        props.insert("http_client_method", method.as_str());
        let _ = graph.update_node_properties(node_id, props);
    }

    if count > 0 {
        tracing::info!("Detected {} HTTP client callers", count);
    }

    count
}

/// Extract the HTTP method from a client function name.
///
/// `requests.get` → `"GET"`, `fetch` → `"ANY"`, `http.NewRequest` → `"ANY"`
fn extract_http_method_from_name(name: &str) -> String {
    let last_part = name.rsplit('.').next().unwrap_or(name);
    let last_part = last_part.rsplit("::").next().unwrap_or(last_part);

    match last_part.to_lowercase().as_str() {
        "get" => "GET".to_string(),
        "post" => "POST".to_string(),
        "put" => "PUT".to_string(),
        "patch" => "PATCH".to_string(),
        "delete" => "DELETE".to_string(),
        _ => "ANY".to_string(),
    }
}

/// Match HTTP client calls to route handlers and create `RuntimeCalls` edges.
///
/// For now, this creates edges based on route pattern matching within the same
/// project. Cross-project matching (T1-4) will scan other namespaces in the
/// shared database.
///
/// Returns the number of RuntimeCalls edges created.
pub fn create_runtime_call_edges(graph: &mut CodeGraph) -> usize {
    // Collect all route handlers: route pattern → node_id
    let mut route_handlers: HashMap<String, Vec<NodeId>> = HashMap::new();

    for (node_id, node) in graph.iter_nodes() {
        if let Some(route) = node.properties.get_string("route") {
            let route = route.to_string();
            route_handlers.entry(route).or_default().push(node_id);
        }
    }

    if route_handlers.is_empty() {
        return 0;
    }

    // Collect all HTTP client callers
    let mut client_callers: Vec<NodeId> = Vec::new();
    for (node_id, node) in graph.iter_nodes() {
        if node.properties.get_string("http_client_call").is_some() {
            client_callers.push(node_id);
        }
    }

    if route_handlers.is_empty() || client_callers.is_empty() {
        return 0;
    }

    // Extract URL paths from client callers by reading their source code
    // and scanning for string literals that look like URL paths
    let mut edges_to_add: Vec<(NodeId, NodeId, PropertyMap)> = Vec::new();

    for &caller_id in &client_callers {
        let source = crate::domain::source_code::get_symbol_source(graph, caller_id);
        if let Some(source) = source {
            let urls = extract_url_paths_from_source(&source);
            for url in urls {
                // Normalize: strip query params, trailing slash
                let normalized = normalize_route(&url);
                // Try exact match first, then pattern match
                if let Some(handlers) = route_handlers.get(&normalized) {
                    for &handler_id in handlers {
                        let props = PropertyMap::new()
                            .with("matched_route", normalized.as_str())
                            .with("match_type", "exact");
                        edges_to_add.push((caller_id, handler_id, props));
                    }
                } else {
                    // Try pattern matching (e.g. "/users/123" matches "/users/{id}")
                    for (pattern, handler_ids) in &route_handlers {
                        if route_pattern_matches(pattern, &normalized) {
                            for &handler_id in handler_ids {
                                let props = PropertyMap::new()
                                    .with("matched_route", pattern.as_str())
                                    .with("match_type", "pattern");
                                edges_to_add.push((caller_id, handler_id, props));
                            }
                        }
                    }
                }
            }
        }
    }

    let count = edges_to_add.len();
    for (from, to, props) in edges_to_add {
        let _ = graph.add_edge(from, to, EdgeType::RuntimeCalls, props);
    }

    if count > 0 {
        tracing::info!(
            "Created {} RuntimeCalls edges ({} routes, {} callers)",
            count,
            route_handlers.len(),
            client_callers.len()
        );
    }

    count
}

/// Extract URL path strings from function source code.
///
/// Scans for string literals that look like URL paths (start with "/").
/// Handles: `fetch("/api/users")`, `requests.get("http://host/api/users")`,
/// `axios.post('/items')`, template literals with paths.
fn extract_url_paths_from_source(source: &str) -> Vec<String> {
    let mut paths = Vec::new();

    // Match string literals that contain URL paths
    // Pattern: quoted string starting with "/" or containing "://"
    for cap in regex::Regex::new(r#"["'`](/[a-zA-Z0-9_/{}\-\.]+)"#)
        .unwrap()
        .captures_iter(source)
    {
        if let Some(path) = cap.get(1) {
            paths.push(path.as_str().to_string());
        }
    }

    // Also extract paths from full URLs: "http://host:port/path"
    // Use a non-greedy match for the host part so we capture the path
    for cap in regex::Regex::new(r#"https?://[^/\s"'`]+(/[a-zA-Z0-9_/{}\-\.]+)"#)
        .unwrap()
        .captures_iter(source)
    {
        if let Some(path) = cap.get(1) {
            let p = path.as_str().to_string();
            if !paths.contains(&p) {
                paths.push(p);
            }
        }
    }

    paths
}

/// Normalize a route path for matching.
///
/// Strips trailing slash, query params, fragments.
fn normalize_route(route: &str) -> String {
    let route = route.split('?').next().unwrap_or(route);
    let route = route.split('#').next().unwrap_or(route);
    let route = route.trim_end_matches('/');
    if route.is_empty() {
        "/".to_string()
    } else {
        route.to_string()
    }
}

/// Check if a route pattern matches a concrete URL path.
///
/// Handles path parameters: "/users/{id}" matches "/users/123"
fn route_pattern_matches(pattern: &str, path: &str) -> bool {
    let pattern_parts: Vec<&str> = pattern.split('/').collect();
    let path_parts: Vec<&str> = path.split('/').collect();

    if pattern_parts.len() != path_parts.len() {
        return false;
    }

    pattern_parts
        .iter()
        .zip(path_parts.iter())
        .all(|(pat, actual)| {
            pat == actual
                || pat.starts_with('{') && pat.ends_with('}') // {id}, {user_id}, etc.
                || pat.starts_with(':') // :id (Express-style)
                || pat.starts_with('<') && pat.ends_with('>') // <id> (Flask-style)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_route_decorator_flask_get() {
        let result = parse_route_decorator("@app.get(\"/users\")");
        assert_eq!(result, Some(("/users".to_string(), "GET".to_string())));
    }

    #[test]
    fn test_parse_route_decorator_flask_post() {
        let result = parse_route_decorator("@app.post(\"/items\")");
        assert_eq!(result, Some(("/items".to_string(), "POST".to_string())));
    }

    #[test]
    fn test_parse_route_decorator_fastapi_router() {
        let result = parse_route_decorator("@router.get(\"/users/{user_id}\")");
        assert_eq!(
            result,
            Some(("/users/{user_id}".to_string(), "GET".to_string()))
        );
    }

    #[test]
    fn test_parse_route_decorator_flask_route() {
        let result = parse_route_decorator("@app.route(\"/hello\")");
        assert_eq!(result, Some(("/hello".to_string(), "ANY".to_string())));
    }

    #[test]
    fn test_parse_route_decorator_nestjs() {
        let result = parse_route_decorator("@Get(\"/users\")");
        assert_eq!(result, Some(("/users".to_string(), "GET".to_string())));
    }

    #[test]
    fn test_parse_route_decorator_spring() {
        let result = parse_route_decorator("@GetMapping(\"/api/users\")");
        assert_eq!(result, Some(("/api/users".to_string(), "GET".to_string())));
    }

    #[test]
    fn test_parse_route_decorator_spring_request_mapping() {
        let result = parse_route_decorator("@RequestMapping(\"/api\")");
        assert_eq!(result, Some(("/api".to_string(), "ANY".to_string())));
    }

    #[test]
    fn test_parse_route_decorator_single_quotes() {
        let result = parse_route_decorator("@app.get('/users')");
        assert_eq!(result, Some(("/users".to_string(), "GET".to_string())));
    }

    #[test]
    fn test_parse_route_decorator_with_extra_args() {
        let result = parse_route_decorator("@app.get(\"/users\", response_model=List[User])");
        assert_eq!(result, Some(("/users".to_string(), "GET".to_string())));
    }

    #[test]
    fn test_parse_route_decorator_no_match() {
        assert_eq!(parse_route_decorator("@property"), None);
        assert_eq!(parse_route_decorator("@staticmethod"), None);
        assert_eq!(parse_route_decorator("@pytest.mark.skip"), None);
    }

    #[test]
    fn test_extract_first_string_arg() {
        assert_eq!(
            extract_first_string_arg("(\"/users\")"),
            Some("/users".to_string())
        );
        assert_eq!(
            extract_first_string_arg("('/items/{id}', methods=['GET'])"),
            Some("/items/{id}".to_string())
        );
        assert_eq!(
            extract_first_string_arg("(value=\"/api\")"),
            Some("/api".to_string())
        );
        assert_eq!(extract_first_string_arg(""), None);
        assert_eq!(extract_first_string_arg("()"), None);
    }

    #[test]
    fn test_extract_http_method_from_name() {
        assert_eq!(extract_http_method_from_name("requests.get"), "GET");
        assert_eq!(extract_http_method_from_name("axios.post"), "POST");
        assert_eq!(extract_http_method_from_name("http.put"), "PUT");
        assert_eq!(extract_http_method_from_name("fetch"), "ANY");
        assert_eq!(extract_http_method_from_name("http.NewRequest"), "ANY");
    }

    #[test]
    fn test_detect_route_handlers_sets_properties() {
        let mut graph = CodeGraph::in_memory().unwrap();

        // Add a function with route decorator
        let props = PropertyMap::new()
            .with("name", "get_users")
            .with("attributes", "@app.get(\"/api/users\")");
        let node_id = graph.add_node(NodeType::Function, props).unwrap();

        let count = detect_route_handlers(&mut graph);
        assert_eq!(count, 1);

        let node = graph.get_node(node_id).unwrap();
        assert_eq!(node.properties.get_string("route"), Some("/api/users"));
        assert_eq!(node.properties.get_string("http_method"), Some("GET"));
    }

    #[test]
    fn test_detect_route_handlers_ignores_non_route() {
        let mut graph = CodeGraph::in_memory().unwrap();

        let props = PropertyMap::new()
            .with("name", "helper")
            .with("attributes", "@staticmethod");
        graph.add_node(NodeType::Function, props).unwrap();

        let count = detect_route_handlers(&mut graph);
        assert_eq!(count, 0);
    }

    #[test]
    fn test_detect_route_handlers_multiple_decorators() {
        let mut graph = CodeGraph::in_memory().unwrap();

        // Comma-separated decorators (as stored in graph properties)
        let props = PropertyMap::new()
            .with("name", "create_item")
            .with("attributes", "@requires_auth,@router.post(\"/items\")");
        let node_id = graph.add_node(NodeType::Function, props).unwrap();

        let count = detect_route_handlers(&mut graph);
        assert_eq!(count, 1);

        let node = graph.get_node(node_id).unwrap();
        assert_eq!(node.properties.get_string("route"), Some("/items"));
        assert_eq!(node.properties.get_string("http_method"), Some("POST"));
    }
}

#[test]
fn test_extract_url_paths_from_source() {
    let source = r#"
        async function getUsers() {
            const resp = await fetch("/api/users");
            const data = await axios.get("http://localhost:3000/api/items");
        }
        "#;
    let paths = extract_url_paths_from_source(source);
    assert!(paths.contains(&"/api/users".to_string()));
    assert!(paths.contains(&"/api/items".to_string()));
}

#[test]
fn test_normalize_route() {
    assert_eq!(normalize_route("/users/"), "/users");
    assert_eq!(normalize_route("/users?page=1"), "/users");
    assert_eq!(normalize_route("/"), "/");
}

#[test]
fn test_route_pattern_matches() {
    assert!(route_pattern_matches("/users/{id}", "/users/123"));
    assert!(route_pattern_matches("/users/:id", "/users/456"));
    assert!(route_pattern_matches("/api/items", "/api/items"));
    assert!(!route_pattern_matches("/users/{id}", "/posts/123"));
    assert!(!route_pattern_matches("/users/{id}/posts", "/users/123"));
}
