// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Custom LSP requests for CodeGraph-specific features.
//!
//! Tower-LSP handles custom requests through the request method on LanguageServer trait.

use crate::backend::CodeGraphBackend;
use crate::handlers::*;
use crate::watcher::GraphUpdater;
use serde_json::Value;
use tower_lsp::jsonrpc::{Error, Result};

/// Custom request handler dispatcher
impl CodeGraphBackend {
    pub async fn handle_custom_request(&self, method: &str, params: Value) -> Result<Value> {
        match method {
            "codegraph/getDependencyGraph" => {
                let params: DependencyGraphParams = serde_json::from_value(params)
                    .map_err(|e| Error::invalid_params(format!("Invalid params: {e}")))?;
                let response = self.handle_get_dependency_graph(params).await?;
                serde_json::to_value(response).map_err(|_| Error::internal_error())
            }

            "codegraph/getCallGraph" => {
                let params: CallGraphParams = serde_json::from_value(params)
                    .map_err(|e| Error::invalid_params(format!("Invalid params: {e}")))?;
                let response = self.handle_get_call_graph(params).await?;
                serde_json::to_value(response).map_err(|_| Error::internal_error())
            }

            "codegraph/analyzeImpact" => {
                let params: ImpactAnalysisParams = serde_json::from_value(params)
                    .map_err(|e| Error::invalid_params(format!("Invalid params: {e}")))?;
                let response = self.handle_analyze_impact(params).await?;
                serde_json::to_value(response).map_err(|_| Error::internal_error())
            }

            "codegraph/getParserMetrics" => {
                let params: ParserMetricsParams = serde_json::from_value(params)
                    .map_err(|e| Error::invalid_params(format!("Invalid params: {e}")))?;
                let response = self.handle_get_parser_metrics(params).await?;
                serde_json::to_value(response).map_err(|_| Error::internal_error())
            }

            "codegraph/reindexWorkspace" => {
                let total_indexed = self.handle_reindex_workspace().await?;
                serde_json::to_value(serde_json::json!({
                    "status": "success",
                    "message": format!("Workspace reindexed: {total_indexed} files"),
                    "files_indexed": total_indexed
                }))
                .map_err(|_| Error::internal_error())
            }

            "codegraph/getAIContext" => {
                let params: AIContextParams = serde_json::from_value(params)
                    .map_err(|e| Error::invalid_params(format!("Invalid params: {e}")))?;
                let response = self.handle_get_ai_context(params).await?;
                serde_json::to_value(response).map_err(|_| Error::internal_error())
            }

            "codegraph/findRelatedTests" => {
                let params: RelatedTestsParams = serde_json::from_value(params)
                    .map_err(|e| Error::invalid_params(format!("Invalid params: {e}")))?;
                let response = self.handle_find_related_tests(params).await?;
                serde_json::to_value(response).map_err(|_| Error::internal_error())
            }

            "codegraph/getNodeLocation" => {
                let params: GetNodeLocationParams = serde_json::from_value(params)
                    .map_err(|e| Error::invalid_params(format!("Invalid params: {e}")))?;
                let response = self.handle_get_node_location(params).await?;
                serde_json::to_value(response).map_err(|_| Error::internal_error())
            }

            "codegraph/getWorkspaceSymbols" => {
                let params: WorkspaceSymbolsParams = serde_json::from_value(params)
                    .map_err(|e| Error::invalid_params(format!("Invalid params: {e}")))?;
                let response = self.handle_get_workspace_symbols(params).await?;
                serde_json::to_value(response).map_err(|_| Error::internal_error())
            }

            "codegraph/analyzeComplexity" => {
                let params: ComplexityParams = serde_json::from_value(params)
                    .map_err(|e| Error::invalid_params(format!("Invalid params: {e}")))?;
                let response = self.handle_analyze_complexity(params).await?;
                serde_json::to_value(response).map_err(|_| Error::internal_error())
            }

            // AI Agent Query Primitives
            "codegraph/symbolSearch" => {
                let params: SymbolSearchParams = serde_json::from_value(params)
                    .map_err(|e| Error::invalid_params(format!("Invalid params: {e}")))?;
                let response = self.handle_symbol_search(params).await?;
                serde_json::to_value(response).map_err(|_| Error::internal_error())
            }

            "codegraph/findByImports" => {
                let params: FindByImportsParams = serde_json::from_value(params)
                    .map_err(|e| Error::invalid_params(format!("Invalid params: {e}")))?;
                let response = self.handle_find_by_imports(params).await?;
                serde_json::to_value(response).map_err(|_| Error::internal_error())
            }

            "codegraph/findEntryPoints" => {
                let params: FindEntryPointsParams = serde_json::from_value(params)
                    .map_err(|e| Error::invalid_params(format!("Invalid params: {e}")))?;
                let response = self.handle_find_entry_points(params).await?;
                serde_json::to_value(response).map_err(|_| Error::internal_error())
            }

            "codegraph/traverseGraph" => {
                let params: TraverseGraphParams = serde_json::from_value(params)
                    .map_err(|e| Error::invalid_params(format!("Invalid params: {e}")))?;
                let response = self.handle_traverse_graph(params).await?;
                serde_json::to_value(response).map_err(|_| Error::internal_error())
            }

            "codegraph/getCallers" => {
                let params: GetCallersParams = serde_json::from_value(params)
                    .map_err(|e| Error::invalid_params(format!("Invalid params: {e}")))?;
                let response = self.handle_get_callers(params).await?;
                serde_json::to_value(response).map_err(|_| Error::internal_error())
            }

            "codegraph/getCallees" => {
                let params: GetCallersParams = serde_json::from_value(params)
                    .map_err(|e| Error::invalid_params(format!("Invalid params: {e}")))?;
                let response = self.handle_get_callees(params).await?;
                serde_json::to_value(response).map_err(|_| Error::internal_error())
            }

            "codegraph/getDetailedSymbolInfo" => {
                let params: GetDetailedInfoParams = serde_json::from_value(params)
                    .map_err(|e| Error::invalid_params(format!("Invalid params: {e}")))?;
                let response = self.handle_get_detailed_symbol_info(params).await?;
                serde_json::to_value(response).map_err(|_| Error::internal_error())
            }

            "codegraph/findBySignature" => {
                let params: FindBySignatureParams = serde_json::from_value(params)
                    .map_err(|e| Error::invalid_params(format!("Invalid params: {e}")))?;
                let response = self.handle_find_by_signature(params).await?;
                serde_json::to_value(response).map_err(|_| Error::internal_error())
            }

            "codegraph/indexFiles" => self.handle_index_files(params).await,

            "codegraph/indexDirectory" => self.handle_index_directory(params).await,

            "codegraph/updateConfiguration" => self.handle_update_configuration(params).await,

            _ => Err(Error::method_not_found()),
        }
    }

    /// Handle reindex workspace request
    async fn handle_reindex_workspace(&self) -> Result<usize> {
        tracing::info!("Reindexing workspace");

        // Clear current graph and indexes
        {
            let mut graph = self.graph.write().await;
            *graph = codegraph::CodeGraph::in_memory().expect("Failed to create in-memory graph");
        }
        self.symbol_index.clear();
        self.file_cache.clear();

        self.client
            .log_message(
                tower_lsp::lsp_types::MessageType::INFO,
                "Clearing indexes...",
            )
            .await;

        // Re-index all workspace folders
        let folders = self.workspace_folders.read().await.clone();
        let mut total_indexed = 0;

        for folder in &folders {
            let count = self.index_directory(folder).await;
            total_indexed += count;
            self.client
                .log_message(
                    tower_lsp::lsp_types::MessageType::INFO,
                    format!("Reindexed {} files from {}", count, folder.display()),
                )
                .await;
        }

        // Resolve cross-file imports after all files are indexed
        {
            let mut graph = self.graph.write().await;
            GraphUpdater::resolve_cross_file_imports(&mut graph);
        }

        // Rebuild symbol index from graph (it was cleared at the start of reindex)
        {
            let graph = self.graph.read().await;
            self.symbol_index.rebuild_from_graph(&graph);
            tracing::info!(
                "Rebuilt symbol index: {} files",
                self.symbol_index.file_count()
            );
        }

        // Rebuild AI query engine indexes
        self.query_engine.build_indexes().await;

        self.client
            .log_message(
                tower_lsp::lsp_types::MessageType::INFO,
                format!("Workspace reindexed: {total_indexed} files"),
            )
            .await;

        Ok(total_indexed)
    }

    async fn handle_index_files(&self, params: Value) -> Result<Value> {
        // Accept both "files" (VS Code LM tools) and "paths" (MCP convention)
        let files: Vec<String> = params
            .get("files")
            .or_else(|| params.get("paths"))
            .and_then(|v| serde_json::from_value::<Vec<String>>(v.clone()).ok())
            .unwrap_or_default();

        if files.is_empty() {
            return Err(Error::invalid_params(
                "files parameter is required (array of file paths)",
            ));
        }

        tracing::info!("Indexing {} files", files.len());
        let mut indexed = 0usize;
        let mut failed = 0usize;

        for path_str in &files {
            let path = std::path::PathBuf::from(path_str);
            if !path.exists() {
                tracing::warn!("Skipping non-existent file: {}", path_str);
                failed += 1;
                continue;
            }
            // Remove old nodes before re-parsing
            {
                let mut graph = self.graph.write().await;
                if let Ok(old_nodes) = graph
                    .query()
                    .property("path", path.to_string_lossy().as_ref())
                    .execute()
                {
                    for old_id in old_nodes {
                        let _ = graph.delete_node(old_id);
                    }
                }
            }
            match self.indexer.index_file(&self.graph, &path).await {
                Ok(_) => indexed += 1,
                Err(e) => {
                    tracing::warn!("Failed to index {:?}: {}", path, e);
                    failed += 1;
                }
            }
        }

        // Resolve cross-file imports
        {
            let mut graph = self.graph.write().await;
            GraphUpdater::resolve_cross_file_imports(&mut graph);
        }
        self.query_engine.build_indexes().await;

        Ok(serde_json::json!({
            "status": "success",
            "files_indexed": indexed,
            "files_failed": failed,
            "message": format!("Indexed {} files ({} failed)", indexed, failed)
        }))
    }

    async fn handle_index_directory(&self, params: Value) -> Result<Value> {
        // Accept "path" (singular string) or "paths" (array)
        let paths: Vec<String> = if let Some(path) = params.get("path").and_then(|v| v.as_str()) {
            vec![path.to_string()]
        } else {
            params
                .get("paths")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default()
        };

        if paths.is_empty() {
            return Err(Error::invalid_params(
                "path parameter is required (string or array of paths)",
            ));
        }

        tracing::info!("Indexing directories: {:?}", paths);
        let mut total_indexed = 0;

        for path_str in &paths {
            let path = std::path::PathBuf::from(path_str);
            if !path.is_dir() {
                tracing::warn!("Skipping non-directory path: {}", path_str);
                continue;
            }
            let count = self.index_directory(&path).await;
            total_indexed += count;
            self.client
                .log_message(
                    tower_lsp::lsp_types::MessageType::INFO,
                    format!("Indexed {} files from {}", count, path.display()),
                )
                .await;
        }

        // Resolve cross-file imports after all files are indexed
        {
            let mut graph = self.graph.write().await;
            GraphUpdater::resolve_cross_file_imports(&mut graph);
        }

        // Rebuild AI query engine indexes
        self.query_engine.build_indexes().await;

        // Start or extend file watcher for the newly indexed directories
        let indexed_paths: Vec<std::path::PathBuf> =
            paths.iter().map(std::path::PathBuf::from).collect();
        self.watch_directories(&indexed_paths).await;

        self.client
            .log_message(
                tower_lsp::lsp_types::MessageType::INFO,
                format!(
                    "Index complete: {total_indexed} files from {} directories (watching for changes)",
                    paths.len()
                ),
            )
            .await;

        Ok(serde_json::json!({ "indexed": total_indexed }))
    }

    async fn handle_update_configuration(&self, params: Value) -> Result<Value> {
        use crate::backend::CodeGraphConfig;

        let new_config: CodeGraphConfig = serde_json::from_value(params)
            .map_err(|e| Error::invalid_params(format!("Invalid configuration: {e}")))?;

        tracing::info!("Updating configuration: {:?}", new_config);

        {
            let mut config = self.config.write().await;
            *config = new_config;
        }

        self.client
            .log_message(
                tower_lsp::lsp_types::MessageType::INFO,
                "Configuration updated".to_string(),
            )
            .await;

        Ok(Value::Null)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_query::QueryEngine;
    use codegraph::CodeGraph;
    use serde_json::json;
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use tower_lsp::jsonrpc::ErrorCode;

    /// Build a backend over an empty in-memory graph.
    fn test_backend() -> CodeGraphBackend {
        let graph = Arc::new(RwLock::new(
            CodeGraph::in_memory().expect("Failed to create graph"),
        ));
        let query_engine = Arc::new(QueryEngine::new(Arc::clone(&graph)));
        CodeGraphBackend::new_for_test(graph, query_engine)
    }

    #[tokio::test]
    async fn unknown_method_returns_method_not_found() {
        let backend = test_backend();
        let err = backend
            .handle_custom_request("codegraph/doesNotExist", Value::Null)
            .await
            .expect_err("unknown method must error");
        assert_eq!(err.code, ErrorCode::MethodNotFound);
    }

    #[tokio::test]
    async fn typed_handler_rejects_malformed_params() {
        let backend = test_backend();
        // A bare number cannot deserialize into the DependencyGraphParams struct.
        let err = backend
            .handle_custom_request("codegraph/getDependencyGraph", json!(42))
            .await
            .expect_err("malformed params must error");
        assert_eq!(err.code, ErrorCode::InvalidParams);
        assert!(err.message.contains("Invalid params"));
    }

    #[tokio::test]
    async fn index_files_requires_non_empty_list() {
        let backend = test_backend();
        let err = backend
            .handle_custom_request("codegraph/indexFiles", json!({}))
            .await
            .expect_err("missing files must error");
        assert_eq!(err.code, ErrorCode::InvalidParams);
        assert!(err.message.contains("files parameter is required"));
    }

    #[tokio::test]
    async fn index_files_counts_missing_paths_as_failed() {
        let backend = test_backend();
        let result = backend
            .handle_custom_request(
                "codegraph/indexFiles",
                json!({ "paths": ["/definitely/not/a/real/file.rs"] }),
            )
            .await
            .expect("dispatch should succeed");
        assert_eq!(result["files_indexed"], json!(0));
        assert_eq!(result["files_failed"], json!(1));
        assert_eq!(result["status"], json!("success"));
    }

    #[tokio::test]
    async fn index_files_indexes_a_real_source_file() {
        let backend = test_backend();
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("sample.rs");
        std::fs::write(&file, "fn hello() -> i32 { 1 }\n").expect("write file");

        let result = backend
            .handle_custom_request(
                "codegraph/indexFiles",
                json!({ "files": [file.to_string_lossy()] }),
            )
            .await
            .expect("dispatch should succeed");
        assert_eq!(result["files_indexed"], json!(1));
        assert_eq!(result["files_failed"], json!(0));

        // The parsed function should now be present in the graph.
        let count = {
            let graph = backend.graph.read().await;
            graph.node_count()
        };
        assert!(count > 0, "graph should contain the indexed function");
    }

    #[tokio::test]
    async fn index_directory_requires_a_path() {
        let backend = test_backend();
        let err = backend
            .handle_custom_request("codegraph/indexDirectory", json!({}))
            .await
            .expect_err("missing path must error");
        assert_eq!(err.code, ErrorCode::InvalidParams);
        assert!(err.message.contains("path parameter is required"));
    }

    #[tokio::test]
    async fn update_configuration_accepts_values_and_returns_null() {
        let backend = test_backend();
        let result = backend
            .handle_custom_request(
                "codegraph/updateConfiguration",
                json!({ "maxFileSizeKb": 512, "embedOnOpen": false }),
            )
            .await
            .expect("valid config should apply");
        assert_eq!(result, Value::Null);

        let config = backend.config.read().await;
        assert_eq!(config.max_file_size_kb, 512);
        assert!(!config.embed_on_open);
    }

    #[tokio::test]
    async fn update_configuration_rejects_wrong_types() {
        let backend = test_backend();
        let err = backend
            .handle_custom_request(
                "codegraph/updateConfiguration",
                json!({ "maxFileSizeKb": "not-a-number" }),
            )
            .await
            .expect_err("bad config must error");
        assert_eq!(err.code, ErrorCode::InvalidParams);
        assert!(err.message.contains("Invalid configuration"));
    }

    #[tokio::test]
    async fn reindex_workspace_with_no_folders_indexes_zero() {
        let backend = test_backend();
        let result = backend
            .handle_custom_request("codegraph/reindexWorkspace", Value::Null)
            .await
            .expect("reindex should succeed on an empty workspace");
        assert_eq!(result["files_indexed"], json!(0));
        assert_eq!(result["status"], json!("success"));
    }

    #[tokio::test]
    async fn index_directory_skips_non_directory_path() {
        let backend = test_backend();
        // `path` points at a regular file, not a directory: the per-path loop
        // hits the `!path.is_dir()` skip branch, so nothing is indexed.
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("not-a-dir.rs");
        std::fs::write(&file, "fn f() {}\n").expect("write file");

        let result = backend
            .handle_custom_request(
                "codegraph/indexDirectory",
                json!({ "path": file.to_string_lossy() }),
            )
            .await
            .expect("dispatch should succeed even when the path is skipped");
        // Note the distinct response shape: index_directory returns `indexed`,
        // not the `files_indexed`/`status` shape used by index_files/reindex.
        assert_eq!(result, json!({ "indexed": 0 }));
    }

    #[tokio::test]
    async fn index_directory_indexes_a_real_directory() {
        let backend = test_backend();
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("sample.rs"), "fn hello() -> i32 { 1 }\n")
            .expect("write file");

        let result = backend
            .handle_custom_request(
                "codegraph/indexDirectory",
                json!({ "path": dir.path().to_string_lossy() }),
            )
            .await
            .expect("dispatch should succeed");
        assert_eq!(result["indexed"], json!(1));

        // The parsed function should now be present in the graph.
        let count = {
            let graph = backend.graph.read().await;
            graph.node_count()
        };
        assert!(count > 0, "graph should contain the indexed function");
    }

    #[tokio::test]
    async fn index_directory_accepts_paths_array_alias() {
        let backend = test_backend();
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("sample.rs"), "fn world() {}\n").expect("write file");

        // The `paths` array alias should be accepted just like the singular `path`.
        let result = backend
            .handle_custom_request(
                "codegraph/indexDirectory",
                json!({ "paths": [dir.path().to_string_lossy()] }),
            )
            .await
            .expect("dispatch should succeed via the paths alias");
        assert_eq!(result["indexed"], json!(1));
    }

    // ---- Success-path dispatch arms ----
    //
    // The typed-handler arms (getDependencyGraph, symbolSearch, findByImports,
    // findEntryPoints, getWorkspaceSymbols) were only exercised on their error
    // path (`typed_handler_rejects_malformed_params` feeds a bad payload to
    // getDependencyGraph). The success half of each arm — deserialize params,
    // invoke the handler, and re-serialize the response with `serde_to_value`
    // — stayed unexercised through `handle_custom_request`. These pin the
    // happy path against an empty in-memory graph, where each handler returns
    // an empty-but-Ok response so the full arm (including the final
    // to_value) runs.

    #[tokio::test]
    async fn dependency_graph_dispatch_returns_empty_on_empty_graph() {
        let backend = test_backend();
        let result = backend
            .handle_custom_request(
                "codegraph/getDependencyGraph",
                json!({ "uri": "file:///tmp/nonexistent.rs" }),
            )
            .await
            .expect("valid params should dispatch and serialize a response");
        assert_eq!(result["nodes"], json!([]));
        assert_eq!(result["edges"], json!([]));
    }

    #[tokio::test]
    async fn symbol_search_dispatch_returns_no_matches_on_empty_graph() {
        let backend = test_backend();
        let result = backend
            .handle_custom_request("codegraph/symbolSearch", json!({ "query": "anything" }))
            .await
            .expect("valid params should dispatch and serialize a response");
        assert_eq!(result["results"], json!([]));
        assert_eq!(result["totalMatches"], json!(0));
    }

    #[tokio::test]
    async fn find_by_imports_dispatch_returns_no_results_on_empty_graph() {
        let backend = test_backend();
        // An empty `libraries` list skips the per-library search loop entirely,
        // still driving the arm's deserialize + handle + to_value path.
        let result = backend
            .handle_custom_request("codegraph/findByImports", json!({ "libraries": [] }))
            .await
            .expect("valid params should dispatch and serialize a response");
        assert_eq!(result["results"], json!([]));
    }

    #[tokio::test]
    async fn find_entry_points_dispatch_returns_none_on_empty_graph() {
        let backend = test_backend();
        // All fields are optional, so an empty object deserializes cleanly.
        let result = backend
            .handle_custom_request("codegraph/findEntryPoints", json!({}))
            .await
            .expect("valid params should dispatch and serialize a response");
        assert_eq!(result["entryPoints"], json!([]));
        assert_eq!(result["totalFound"], json!(0));
    }

    #[tokio::test]
    async fn workspace_symbols_dispatch_returns_empty_on_empty_graph() {
        let backend = test_backend();
        // An empty query asks for top-level Module symbols; the empty symbol
        // index yields none, exercising the arm's success path end to end.
        let result = backend
            .handle_custom_request("codegraph/getWorkspaceSymbols", json!({ "query": "" }))
            .await
            .expect("valid params should dispatch and serialize a response");
        assert_eq!(result["symbols"], json!([]));
    }
}
