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

    async fn handle_index_directory(&self, params: Value) -> Result<Value> {
        let paths: Vec<String> = params
            .get("paths")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        if paths.is_empty() {
            return Err(Error::invalid_params("Missing or empty 'paths' array"));
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
