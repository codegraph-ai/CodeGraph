// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Navigation-related helper functions.

use crate::backend::CodeGraphBackend;
use crate::domain::node_props;
use codegraph::NodeId;
use serde::{Deserialize, Serialize};
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{Range, Url};

/// Request to get a node's location by ID.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetNodeLocationParams {
    pub node_id: String,
}

/// Response with node location.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeLocationResponse {
    pub uri: String,
    pub range: Range,
}

impl CodeGraphBackend {
    /// Get the location of a node by its ID.
    pub async fn handle_get_node_location(
        &self,
        params: GetNodeLocationParams,
    ) -> Result<Option<NodeLocationResponse>> {
        let node_id: NodeId = params
            .node_id
            .parse()
            .map_err(|_| tower_lsp::jsonrpc::Error::invalid_params("Invalid node ID"))?;

        let graph = self.graph.read().await;

        let node = match graph.get_node(node_id) {
            Ok(n) => n,
            Err(_) => return Ok(None),
        };

        let path = match node.properties.get_string("path") {
            Some(p) => p,
            None => return Ok(None),
        };

        let start_line: u32 = node_props::line_start(node).saturating_sub(1);

        let start_col: u32 = node_props::col_start_from_props(&node.properties);

        let end_line: u32 = node_props::line_end(node).saturating_sub(1);

        let end_col: u32 = node_props::col_end_from_props(&node.properties);

        let uri = Url::from_file_path(path)
            .map_err(|_| tower_lsp::jsonrpc::Error::invalid_params("Invalid path"))?;

        Ok(Some(NodeLocationResponse {
            uri: uri.to_string(),
            range: Range {
                start: tower_lsp::lsp_types::Position {
                    line: start_line,
                    character: start_col,
                },
                end: tower_lsp::lsp_types::Position {
                    line: end_line,
                    character: end_col,
                },
            },
        }))
    }
}

/// Request for workspace symbols.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSymbolsParams {
    pub query: Option<String>,
}

/// Symbol information for tree view.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SymbolInfo {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub language: String,
    pub uri: String,
    pub range: Range,
    pub children: Option<Vec<SymbolInfo>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSymbolsResponse {
    pub symbols: Vec<SymbolInfo>,
}

/// Request for per-document CodeLens / hover stats.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentCodeLensParams {
    pub uri: String,
}

/// Graph-derived stats for one function/method, shown inline as a CodeLens and
/// on hover. Counts only; the editor formats them.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeLensSymbol {
    pub name: String,
    /// 0-based start line (LSP convention), so the client anchors without math.
    pub line: u32,
    pub caller_count: u32,
    pub test_count: u32,
    pub complexity: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentCodeLensResponse {
    pub symbols: Vec<CodeLensSymbol>,
}

impl CodeGraphBackend {
    /// Get workspace symbols, optionally filtered by query.
    pub async fn handle_get_workspace_symbols(
        &self,
        params: WorkspaceSymbolsParams,
    ) -> Result<WorkspaceSymbolsResponse> {
        let graph = self.graph.read().await;

        let node_ids = if let Some(query) = &params.query {
            if query.is_empty() {
                // Return top-level symbols (modules, files)
                self.symbol_index.get_by_type("Module")
            } else {
                self.symbol_index.search_by_name(query)
            }
        } else {
            // Return all symbols (limited)
            let mut all = Vec::new();
            all.extend(self.symbol_index.get_by_type("Function"));
            all.extend(self.symbol_index.get_by_type("Class"));
            all.extend(self.symbol_index.get_by_type("Module"));
            all.truncate(100); // Limit results
            all
        };

        let mut symbols = Vec::new();

        for node_id in node_ids {
            if let Ok(node) = graph.get_node(node_id) {
                let name = node_props::name(node).to_string();
                let kind = format!("{:?}", node.node_type);
                let language = {
                    let l = node_props::language(node);
                    if l.is_empty() {
                        "unknown".to_string()
                    } else {
                        l.to_string()
                    }
                };
                let path = node_props::path(node).to_string();

                let start_line: u32 = node_props::line_start(node).saturating_sub(1);

                let start_col: u32 = node_props::col_start_from_props(&node.properties);

                let end_line: u32 = node_props::line_end(node).saturating_sub(1);

                let end_col: u32 = node_props::col_end_from_props(&node.properties);

                let uri = if !path.is_empty() {
                    Url::from_file_path(&path)
                        .map(|u| u.to_string())
                        .unwrap_or(path.clone())
                } else {
                    String::new()
                };

                symbols.push(SymbolInfo {
                    id: node_id.to_string(),
                    name,
                    kind,
                    language,
                    uri,
                    range: Range {
                        start: tower_lsp::lsp_types::Position {
                            line: start_line,
                            character: start_col,
                        },
                        end: tower_lsp::lsp_types::Position {
                            line: end_line,
                            character: end_col,
                        },
                    },
                    children: None,
                });
            }
        }

        Ok(WorkspaceSymbolsResponse { symbols })
    }

    /// Compute per-function CodeLens stats for a single document in one pass:
    /// caller count, test count, and cyclomatic complexity for every function
    /// or method symbol in the file. Batched so the editor issues one request
    /// per document rather than N per-symbol calls. Test functions are skipped
    /// (a CodeLens on a test is noise), and incoming callers are split into
    /// test vs non-test using the same rule as PR review.
    pub async fn handle_get_document_code_lens(
        &self,
        params: DocumentCodeLensParams,
    ) -> Result<DocumentCodeLensResponse> {
        let path = Url::parse(&params.uri)
            .ok()
            .and_then(|u| u.to_file_path().ok())
            .ok_or_else(|| tower_lsp::jsonrpc::Error::invalid_params("Invalid uri"))?;

        let graph = self.graph.read().await;
        let node_ids = self.symbol_index.get_file_symbols(&path);

        let mut symbols = Vec::new();
        for node_id in node_ids {
            let Ok(node) = graph.get_node(node_id) else {
                continue;
            };
            if node.node_type != codegraph::NodeType::Function || node_props::is_test(node) {
                continue;
            }

            let mut caller_count = 0u32;
            let mut test_count = 0u32;
            if let Ok(neighbors) = graph.get_neighbors(node_id, codegraph::Direction::Incoming) {
                for caller_id in neighbors {
                    // Only genuine call edges count - a raw incoming-neighbor
                    // scan also returns the containing file/class `Contains`
                    // edge, which would inflate every function by one. Mirror
                    // the canonical `helpers::get_callers` Calls-edge filter.
                    let calls = graph
                        .get_edges_between(caller_id, node_id)
                        .ok()
                        .into_iter()
                        .flatten()
                        .filter_map(|eid| graph.get_edge(eid).ok())
                        .any(|edge| edge.edge_type == codegraph::EdgeType::Calls);
                    if !calls {
                        continue;
                    }
                    if let Ok(caller) = graph.get_node(caller_id) {
                        if node_props::is_test_like(caller) {
                            test_count += 1;
                        } else {
                            caller_count += 1;
                        }
                    }
                }
            }

            symbols.push(CodeLensSymbol {
                name: node_props::name(node).to_string(),
                line: node_props::line_start(node).saturating_sub(1),
                caller_count,
                test_count,
                complexity: node.properties.get_int("complexity").unwrap_or(0).max(0) as u32,
            });
        }

        Ok(DocumentCodeLensResponse { symbols })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_query::QueryEngine;
    use crate::backend::CodeGraphBackend;
    use codegraph::{CodeGraph, NodeType, PropertyMap, PropertyValue};
    use std::sync::Arc;
    use tokio::sync::RwLock;

    /// Helper to add a node to the symbol index
    fn add_node_to_index(
        backend: &CodeGraphBackend,
        path: &std::path::Path,
        node_id: NodeId,
        name: &str,
        node_type: &str,
        start_line: u32,
        end_line: u32,
    ) {
        backend.symbol_index.add_node_for_test(
            path.to_path_buf(),
            node_id,
            name,
            node_type,
            start_line,
            end_line,
        );
    }

    /// Helper to create a test backend with nodes
    async fn create_backend_with_nodes() -> (CodeGraphBackend, NodeId, NodeId) {
        let graph = Arc::new(RwLock::new(
            CodeGraph::in_memory().expect("Failed to create graph"),
        ));

        let (func_id, class_id) = {
            let mut g = graph.write().await;

            // Create a function node
            let mut props1 = PropertyMap::new();
            props1.insert(
                "name".to_string(),
                PropertyValue::String("test_function".to_string()),
            );
            props1.insert(
                "path".to_string(),
                PropertyValue::String("/test/file.rs".to_string()),
            );
            props1.insert("start_line".to_string(), PropertyValue::Int(10));
            props1.insert("end_line".to_string(), PropertyValue::Int(20));
            props1.insert("start_col".to_string(), PropertyValue::Int(0));
            props1.insert("end_col".to_string(), PropertyValue::Int(50));
            props1.insert(
                "language".to_string(),
                PropertyValue::String("rust".to_string()),
            );
            let func_id = g.add_node(NodeType::Function, props1).unwrap();

            // Create a class node
            let mut props2 = PropertyMap::new();
            props2.insert(
                "name".to_string(),
                PropertyValue::String("TestClass".to_string()),
            );
            props2.insert(
                "path".to_string(),
                PropertyValue::String("/test/file.rs".to_string()),
            );
            props2.insert("start_line".to_string(), PropertyValue::Int(30));
            props2.insert("end_line".to_string(), PropertyValue::Int(50));
            props2.insert("start_col".to_string(), PropertyValue::Int(0));
            props2.insert("end_col".to_string(), PropertyValue::Int(100));
            props2.insert(
                "language".to_string(),
                PropertyValue::String("rust".to_string()),
            );
            let class_id = g.add_node(NodeType::Class, props2).unwrap();

            (func_id, class_id)
        };

        let query_engine = Arc::new(QueryEngine::new(Arc::clone(&graph)));
        let backend = CodeGraphBackend::new_for_test(graph, query_engine);

        // Add nodes to symbol index
        let path = std::path::Path::new("/test/file.rs");
        add_node_to_index(&backend, path, func_id, "test_function", "Function", 10, 20);
        add_node_to_index(&backend, path, class_id, "TestClass", "Class", 30, 50);

        (backend, func_id, class_id)
    }

    #[tokio::test]
    async fn test_handle_get_node_location_valid() {
        let (backend, func_id, _) = create_backend_with_nodes().await;

        let params = GetNodeLocationParams {
            node_id: func_id.to_string(),
        };

        let result = backend.handle_get_node_location(params).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        assert!(response.is_some());

        let location = response.unwrap();
        assert!(location.uri.contains("file.rs"));
        // start_line 10 -> 0-indexed = 9
        assert_eq!(location.range.start.line, 9);
        // end_line 20 -> 0-indexed = 19
        assert_eq!(location.range.end.line, 19);
    }

    #[tokio::test]
    async fn test_handle_get_node_location_invalid_id() {
        let (backend, _, _) = create_backend_with_nodes().await;

        let params = GetNodeLocationParams {
            node_id: "not_a_number".to_string(),
        };

        let result = backend.handle_get_node_location(params).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_handle_get_node_location_nonexistent_node() {
        let (backend, _, _) = create_backend_with_nodes().await;

        let params = GetNodeLocationParams {
            node_id: "99999".to_string(),
        };

        let result = backend.handle_get_node_location(params).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_handle_get_node_location_no_path() {
        let graph = Arc::new(RwLock::new(
            CodeGraph::in_memory().expect("Failed to create graph"),
        ));

        let node_id = {
            let mut g = graph.write().await;
            let mut props = PropertyMap::new();
            props.insert(
                "name".to_string(),
                PropertyValue::String("orphan_node".to_string()),
            );
            // No path property
            g.add_node(NodeType::Function, props).unwrap()
        };

        let query_engine = Arc::new(QueryEngine::new(Arc::clone(&graph)));
        let backend = CodeGraphBackend::new_for_test(graph, query_engine);

        let params = GetNodeLocationParams {
            node_id: node_id.to_string(),
        };

        let result = backend.handle_get_node_location(params).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_handle_get_workspace_symbols_all() {
        let (backend, _, _) = create_backend_with_nodes().await;

        let params = WorkspaceSymbolsParams { query: None };

        let result = backend.handle_get_workspace_symbols(params).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        assert!(!response.symbols.is_empty());
    }

    #[tokio::test]
    async fn test_handle_get_workspace_symbols_with_query() {
        let (backend, _, _) = create_backend_with_nodes().await;

        let params = WorkspaceSymbolsParams {
            query: Some("test_function".to_string()),
        };

        let result = backend.handle_get_workspace_symbols(params).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        assert!(!response.symbols.is_empty());
        assert!(response.symbols.iter().any(|s| s.name == "test_function"));
    }

    #[tokio::test]
    async fn test_handle_get_workspace_symbols_empty_query() {
        let (backend, _, _) = create_backend_with_nodes().await;

        // Add a module node for empty query test
        let mod_id = {
            let mut g = backend.graph.write().await;
            let mut props = PropertyMap::new();
            props.insert(
                "name".to_string(),
                PropertyValue::String("test_module".to_string()),
            );
            props.insert(
                "path".to_string(),
                PropertyValue::String("/test/mod.rs".to_string()),
            );
            props.insert(
                "language".to_string(),
                PropertyValue::String("rust".to_string()),
            );
            g.add_node(NodeType::Module, props).unwrap()
        };

        let path = std::path::Path::new("/test/mod.rs");
        add_node_to_index(&backend, path, mod_id, "test_module", "Module", 1, 100);

        let params = WorkspaceSymbolsParams {
            query: Some("".to_string()),
        };

        let result = backend.handle_get_workspace_symbols(params).await;
        assert!(result.is_ok());

        // Empty query returns top-level symbols (modules)
        let response = result.unwrap();
        // Should return module symbols when query is empty
        assert!(response.symbols.iter().any(|s| s.kind == "Module"));
    }

    #[tokio::test]
    async fn test_handle_get_workspace_symbols_no_match() {
        let (backend, _, _) = create_backend_with_nodes().await;

        let params = WorkspaceSymbolsParams {
            query: Some("nonexistent_symbol_xyz".to_string()),
        };

        let result = backend.handle_get_workspace_symbols(params).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        assert!(response.symbols.is_empty());
    }

    #[tokio::test]
    async fn test_symbol_info_structure() {
        let (backend, func_id, _) = create_backend_with_nodes().await;

        let params = WorkspaceSymbolsParams {
            query: Some("test_function".to_string()),
        };

        let result = backend.handle_get_workspace_symbols(params).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        let symbol = response.symbols.iter().find(|s| s.name == "test_function");
        assert!(symbol.is_some());

        let symbol = symbol.unwrap();
        assert_eq!(symbol.id, func_id.to_string());
        assert_eq!(symbol.name, "test_function");
        assert_eq!(symbol.kind, "Function");
        assert_eq!(symbol.language, "rust");
        assert!(!symbol.uri.is_empty());
    }

    #[tokio::test]
    async fn test_get_document_code_lens_counts_callers_tests_complexity() {
        use codegraph::EdgeType;

        let graph = Arc::new(RwLock::new(
            CodeGraph::in_memory().expect("Failed to create graph"),
        ));

        let target_path = "/test/lens.rs";
        let (target_id, _prod_caller, _test_caller, _skipped_test) = {
            let mut g = graph.write().await;

            let mk = |g: &mut CodeGraph, name: &str, path: &str, line: i64, is_test: bool| {
                let mut p = PropertyMap::new();
                p.insert("name".to_string(), PropertyValue::String(name.to_string()));
                p.insert("path".to_string(), PropertyValue::String(path.to_string()));
                p.insert("start_line".to_string(), PropertyValue::Int(line));
                p.insert("end_line".to_string(), PropertyValue::Int(line + 5));
                p.insert("complexity".to_string(), PropertyValue::Int(7));
                p.insert("is_test".to_string(), PropertyValue::Bool(is_test));
                g.add_node(NodeType::Function, p).unwrap()
            };

            // Symbol under inspection, plus a test function in the same file
            // (must be skipped in the output).
            let target = mk(&mut g, "do_work", target_path, 5, false);
            let skipped_test = mk(&mut g, "test_does_work", target_path, 40, true);
            // A production caller and a test caller, both in other files.
            let prod_caller = mk(&mut g, "run", "/test/main.rs", 3, false);
            let test_caller = mk(&mut g, "test_do_work", "/test/lens_test.rs", 3, true);

            g.add_edge(prod_caller, target, EdgeType::Calls, PropertyMap::new())
                .unwrap();
            g.add_edge(test_caller, target, EdgeType::Calls, PropertyMap::new())
                .unwrap();

            // The containing file's `Contains` edge is an incoming neighbor but
            // must NOT be counted as a caller (regression guard).
            let mut file_props = PropertyMap::new();
            file_props.insert(
                "name".to_string(),
                PropertyValue::String("lens.rs".to_string()),
            );
            file_props.insert(
                "path".to_string(),
                PropertyValue::String(target_path.to_string()),
            );
            let file_id = g.add_node(NodeType::CodeFile, file_props).unwrap();
            g.add_edge(file_id, target, EdgeType::Contains, PropertyMap::new())
                .unwrap();

            (target, prod_caller, test_caller, skipped_test)
        };

        let query_engine = Arc::new(QueryEngine::new(Arc::clone(&graph)));
        let backend = CodeGraphBackend::new_for_test(graph, query_engine);
        let path = std::path::Path::new(target_path);
        add_node_to_index(&backend, path, target_id, "do_work", "Function", 5, 10);
        // The skipped in-file test must be indexed too, to prove it's filtered.
        add_node_to_index(&backend, path, _skipped_test, "test_does_work", "Function", 40, 45);

        let uri = Url::from_file_path(target_path).unwrap().to_string();
        let response = backend
            .handle_get_document_code_lens(DocumentCodeLensParams { uri })
            .await
            .unwrap();

        // Only the non-test function is reported.
        assert_eq!(response.symbols.len(), 1);
        let s = &response.symbols[0];
        assert_eq!(s.name, "do_work");
        assert_eq!(s.line, 4); // 1-based 5 -> 0-based 4
        assert_eq!(s.caller_count, 1); // run, not the test caller
        assert_eq!(s.test_count, 1); // test_do_work
        assert_eq!(s.complexity, 7);
    }

    #[tokio::test]
    async fn test_get_document_code_lens_invalid_uri_errors() {
        let (backend, _, _) = create_backend_with_nodes().await;
        let result = backend
            .handle_get_document_code_lens(DocumentCodeLensParams {
                uri: "not a uri".to_string(),
            })
            .await;
        assert!(result.is_err());
    }
}
