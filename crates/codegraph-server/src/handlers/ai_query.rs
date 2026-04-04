// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AI Agent Query Handlers
//!
//! This module implements the LSP handlers for AI agent query primitives.
//! These are exposed via workspace/executeCommand and provide fast,
//! composable query primitives for AI agents to explore codebases.

use crate::ai_query::{
    EntryType, ImportMatchMode, ImportSearchOptions, SearchOptions, SignaturePattern, SymbolType,
    TraversalDirection, TraversalFilter,
};
use crate::backend::CodeGraphBackend;
use codegraph::NodeId;
use serde::{Deserialize, Serialize};
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::Url;

// ==========================================
// Symbol Search Request
// ==========================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SymbolSearchParams {
    /// Search query keywords
    pub query: String,
    /// Search scope: "workspace", "module", or "file"
    #[serde(default)]
    pub scope: Option<String>,
    /// Filter by symbol types: "function", "class", "variable", "module", "interface", "type"
    #[serde(default)]
    pub symbol_types: Option<Vec<String>>,
    /// Maximum number of results
    #[serde(default)]
    pub limit: Option<usize>,
    /// Include private symbols (default: false)
    #[serde(default)]
    pub include_private: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SymbolSearchResult {
    pub results: Vec<SymbolMatchResponse>,
    pub total_matches: usize,
    pub query_time_ms: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SymbolMatchResponse {
    pub node_id: String,
    pub symbol: SymbolInfoResponse,
    pub score: f32,
    pub match_reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SymbolInfoResponse {
    pub name: String,
    pub kind: String,
    pub location: SymbolLocationResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docstring: Option<String>,
    pub is_public: bool,
    pub visibility: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SymbolLocationResponse {
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

// ==========================================
// Find By Imports Request
// ==========================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FindByImportsParams {
    /// Libraries/modules to search for
    #[serde(default)]
    pub libraries: Vec<String>,
    /// Single module name for MCP compatibility
    #[serde(default)]
    pub module_name: Option<String>,
    /// Match mode: "exact", "prefix", "contains", or "fuzzy"
    #[serde(default)]
    pub match_mode: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FindByImportsResponse {
    pub results: Vec<SymbolMatchResponse>,
    pub query_time_ms: u64,
}

// ==========================================
// Find Entry Points Request
// ==========================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FindEntryPointsParams {
    /// Entry type: "http_handler", "cli_command", "public_api", "event_handler", "test_entry", "main", "all"
    #[serde(default)]
    pub entry_type: Option<String>,
    /// Filter by framework (e.g., "express", "fastapi", "actix")
    #[serde(default)]
    pub framework: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FindEntryPointsResponse {
    pub entry_points: Vec<EntryPointResponse>,
    pub total_found: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryPointResponse {
    pub node_id: String,
    pub entry_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub symbol: SymbolInfoResponse,
}

// ==========================================
// Traverse Graph Request
// ==========================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraverseGraphParams {
    /// Starting point: either node_id or uri+line
    #[serde(default)]
    pub start_node_id: Option<String>,
    #[serde(default)]
    pub uri: Option<String>,
    #[serde(default)]
    pub line: Option<u32>,
    /// Traversal direction: "outgoing", "incoming", or "both"
    #[serde(default)]
    pub direction: Option<String>,
    /// Maximum traversal depth
    #[serde(default)]
    pub depth: Option<u32>,
    /// Filter by symbol types
    #[serde(default)]
    pub filter_symbol_types: Option<Vec<String>>,
    /// Maximum nodes to return
    #[serde(default)]
    pub max_nodes: Option<usize>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraverseGraphResponse {
    pub nodes: Vec<TraversalNodeResponse>,
    pub query_time_ms: u64,
    /// Whether a fallback to nearest symbol was used
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_fallback: Option<bool>,
    /// Message explaining fallback behavior if used
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_message: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraversalNodeResponse {
    pub node_id: String,
    pub depth: u32,
    pub path: Vec<String>,
    pub edge_type: String,
    pub symbol: SymbolInfoResponse,
}

// ==========================================
// Get Callers/Callees Request
// ==========================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetCallersParams {
    /// Node ID or uri+line
    #[serde(default)]
    pub node_id: Option<String>,
    #[serde(default)]
    pub uri: Option<String>,
    #[serde(default)]
    pub line: Option<u32>,
    /// Depth of caller chain (default: 1)
    #[serde(default)]
    pub depth: Option<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetCallersResponse {
    pub callers: Vec<CallInfoResponse>,
    pub query_time_ms: u64,
    /// Whether a fallback to nearest symbol was used
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_fallback: Option<bool>,
    /// Message explaining fallback behavior if used
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_message: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallInfoResponse {
    pub node_id: String,
    pub symbol: SymbolInfoResponse,
    pub call_site: SymbolLocationResponse,
    pub depth: u32,
}

/// Get Callees parameters (same structure as GetCallersParams)
pub type GetCalleesParams = GetCallersParams;

/// Get Detailed Symbol parameters with source code options
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetDetailedSymbolParams {
    #[serde(default)]
    pub node_id: Option<String>,
    #[serde(default)]
    pub uri: Option<String>,
    #[serde(default)]
    pub line: Option<u32>,
    #[serde(default)]
    pub include_source: Option<bool>,
    #[serde(default)]
    pub include_callers: Option<bool>,
    #[serde(default)]
    pub include_callees: Option<bool>,
}

// ==========================================
// Get Detailed Symbol Info Request
// ==========================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetDetailedInfoParams {
    #[serde(default)]
    pub node_id: Option<String>,
    #[serde(default)]
    pub uri: Option<String>,
    #[serde(default)]
    pub line: Option<u32>,
    #[serde(default)]
    pub include_callers: Option<bool>,
    #[serde(default)]
    pub include_callees: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetailedSymbolResponse {
    pub symbol: SymbolInfoResponse,
    pub callers: Vec<CallInfoResponse>,
    pub callees: Vec<CallInfoResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub complexity: Option<u32>,
    pub lines_of_code: usize,
    pub is_public: bool,
    pub is_deprecated: bool,
    pub reference_count: usize,
    /// Whether a fallback to nearest symbol was used
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_fallback: Option<bool>,
    /// Message explaining fallback behavior if used
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_message: Option<String>,
}

// ==========================================
// Get Detailed Symbol Response (with source code)
// ==========================================

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetDetailedSymbolResponse {
    pub symbol: SymbolInfoResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    pub callers: Vec<CallInfoResponse>,
    pub callees: Vec<CallInfoResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub complexity: Option<u32>,
    pub lines_of_code: usize,
    pub is_public: bool,
    pub is_deprecated: bool,
    pub reference_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_fallback: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_message: Option<String>,
}

// ==========================================
// Find By Signature Request
// ==========================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FindBySignatureParams {
    /// Regex pattern for function name
    #[serde(default)]
    pub name_pattern: Option<String>,
    /// Expected return type
    #[serde(default)]
    pub return_type: Option<String>,
    /// Parameter count range: { min, max }
    #[serde(default)]
    pub param_count: Option<ParamCountRange>,
    /// Required modifiers: ["async", "public", "static", "const"]
    #[serde(default)]
    pub modifiers: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParamCountRange {
    pub min: usize,
    pub max: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FindBySignatureResponse {
    pub results: Vec<SymbolMatchResponse>,
    pub query_time_ms: u64,
}

// ==========================================
// Handler Implementations
// ==========================================

impl CodeGraphBackend {
    /// Handle symbol search request
    pub async fn handle_symbol_search(
        &self,
        params: SymbolSearchParams,
    ) -> Result<SymbolSearchResult> {
        // Build search options
        let mut options = SearchOptions::new();

        if let Some(limit) = params.limit {
            options = options.with_limit(limit);
        }

        if params.include_private.unwrap_or(false) {
            options = options.include_private();
        }

        if let Some(types) = params.symbol_types {
            let symbol_types: Vec<SymbolType> = types
                .iter()
                .filter_map(|t| match t.as_str() {
                    "function" => Some(SymbolType::Function),
                    "class" => Some(SymbolType::Class),
                    "variable" => Some(SymbolType::Variable),
                    "module" => Some(SymbolType::Module),
                    "interface" => Some(SymbolType::Interface),
                    "type" => Some(SymbolType::Type),
                    _ => None,
                })
                .collect();
            options = options.with_symbol_types(symbol_types);
        }

        let result = self
            .query_engine
            .symbol_search(&params.query, &options)
            .await;

        let results = result
            .results
            .into_iter()
            .map(|m| SymbolMatchResponse {
                node_id: m.node_id.to_string(),
                symbol: symbol_info_to_response(&m.symbol),
                score: m.score,
                match_reason: m.match_reason,
            })
            .collect();

        Ok(SymbolSearchResult {
            results,
            total_matches: result.total_matches,
            query_time_ms: result.query_time_ms,
        })
    }

    /// Handle find by imports request
    pub async fn handle_find_by_imports(
        &self,
        params: FindByImportsParams,
    ) -> Result<FindByImportsResponse> {
        let start = std::time::Instant::now();

        let match_mode = match params.match_mode.as_deref() {
            Some("exact") => ImportMatchMode::Exact,
            Some("prefix") => ImportMatchMode::Prefix,
            _ => ImportMatchMode::Fuzzy, // default to fuzzy (substring) matching
        };

        let options = ImportSearchOptions::new().with_match_mode(match_mode);

        // Search for each library
        let mut all_results = Vec::new();
        for library in &params.libraries {
            let results = self.query_engine.find_by_imports(library, &options).await;
            all_results.extend(results);
        }

        let results = all_results
            .into_iter()
            .map(|m| SymbolMatchResponse {
                node_id: m.node_id.to_string(),
                symbol: symbol_info_to_response(&m.symbol),
                score: m.score,
                match_reason: m.match_reason,
            })
            .collect();

        Ok(FindByImportsResponse {
            results,
            query_time_ms: start.elapsed().as_millis() as u64,
        })
    }

    /// Handle find entry points request
    pub async fn handle_find_entry_points(
        &self,
        params: FindEntryPointsParams,
    ) -> Result<FindEntryPointsResponse> {
        let entry_types: Vec<EntryType> = if let Some(et) = params.entry_type {
            match et.as_str() {
                "http_handler" => vec![EntryType::HttpHandler],
                "cli_command" => vec![EntryType::CliCommand],
                "public_api" => vec![EntryType::PublicApi],
                "event_handler" => vec![EntryType::EventHandler],
                "test_entry" => vec![EntryType::TestEntry],
                "main" => vec![EntryType::Main],
                _ => vec![],
            }
        } else {
            vec![]
        };

        let results = self.query_engine.find_entry_points(&entry_types).await;
        let total_found = results.len();

        let entry_points = results
            .into_iter()
            .map(|ep| EntryPointResponse {
                node_id: ep.node_id.to_string(),
                entry_type: format!("{:?}", ep.entry_type).to_lowercase(),
                route: ep.route,
                method: ep.method,
                description: ep.description,
                symbol: symbol_info_to_response(&ep.symbol),
            })
            .collect();

        Ok(FindEntryPointsResponse {
            entry_points,
            total_found,
        })
    }

    /// Handle traverse graph request
    pub async fn handle_traverse_graph(
        &self,
        params: TraverseGraphParams,
    ) -> Result<TraverseGraphResponse> {
        let start = std::time::Instant::now();

        // Resolve start node with fallback support
        let (start_node, used_fallback, fallback_message) = self
            .resolve_node_id_with_fallback(&params.start_node_id, &params.uri, &params.line)
            .await?;

        let direction = match params.direction.as_deref() {
            Some("incoming") => TraversalDirection::Incoming,
            Some("both") => TraversalDirection::Both,
            _ => TraversalDirection::Outgoing,
        };

        let depth = params.depth.unwrap_or(3);

        let mut filter = TraversalFilter::new();
        if let Some(max) = params.max_nodes {
            filter = filter.with_max_nodes(max);
        }
        if let Some(types) = params.filter_symbol_types {
            let symbol_types: Vec<SymbolType> = types
                .iter()
                .filter_map(|t| match t.as_str() {
                    "function" => Some(SymbolType::Function),
                    "class" => Some(SymbolType::Class),
                    "variable" => Some(SymbolType::Variable),
                    "module" => Some(SymbolType::Module),
                    "interface" => Some(SymbolType::Interface),
                    "type" => Some(SymbolType::Type),
                    _ => None,
                })
                .collect();
            filter = filter.with_symbol_types(symbol_types);
        }

        let results = self
            .query_engine
            .traverse_graph(start_node, direction, depth, &filter)
            .await;

        let nodes = results
            .into_iter()
            .map(|n| TraversalNodeResponse {
                node_id: n.node_id.to_string(),
                depth: n.depth,
                path: n.path.iter().map(|id| id.to_string()).collect(),
                edge_type: n.edge_type,
                symbol: symbol_info_to_response(&n.symbol),
            })
            .collect();

        Ok(TraverseGraphResponse {
            nodes,
            query_time_ms: start.elapsed().as_millis() as u64,
            used_fallback: if used_fallback { Some(true) } else { None },
            fallback_message,
        })
    }

    /// Handle get callers request
    pub async fn handle_get_callers(&self, params: GetCallersParams) -> Result<GetCallersResponse> {
        let start = std::time::Instant::now();

        let (node_id, used_fallback, _) = self
            .resolve_node_id_with_fallback(&params.node_id, &params.uri, &params.line)
            .await?;
        let depth = params.depth.unwrap_or(1);

        let domain_result = crate::domain::callers::get_callers(
            &self.graph,
            &self.query_engine,
            node_id,
            depth,
            used_fallback,
            params.line,
        )
        .await;

        Ok(GetCallersResponse {
            callers: domain_result
                .callers
                .into_iter()
                .map(call_info_to_response)
                .collect(),
            query_time_ms: start.elapsed().as_millis() as u64,
            used_fallback: domain_result.used_fallback,
            fallback_message: domain_result.fallback_message,
        })
    }

    /// Handle get callees request
    pub async fn handle_get_callees(&self, params: GetCallersParams) -> Result<GetCallersResponse> {
        let start = std::time::Instant::now();

        let (node_id, used_fallback, _) = self
            .resolve_node_id_with_fallback(&params.node_id, &params.uri, &params.line)
            .await?;
        let depth = params.depth.unwrap_or(1);

        let domain_result = crate::domain::callers::get_callees(
            &self.graph,
            &self.query_engine,
            node_id,
            depth,
            used_fallback,
            params.line,
        )
        .await;

        Ok(GetCallersResponse {
            callers: domain_result
                .callees
                .into_iter()
                .map(call_info_to_response)
                .collect(),
            query_time_ms: start.elapsed().as_millis() as u64,
            used_fallback: domain_result.used_fallback,
            fallback_message: domain_result.fallback_message,
        })
    }

    /// Handle get detailed info request
    pub async fn handle_get_detailed_symbol_info(
        &self,
        params: GetDetailedInfoParams,
    ) -> Result<DetailedSymbolResponse> {
        let (node_id, used_fallback, _) = self
            .resolve_node_id_with_fallback(&params.node_id, &params.uri, &params.line)
            .await?;

        let include_callers = params.include_callers.unwrap_or(true);
        let include_callees = params.include_callees.unwrap_or(true);

        let domain_result = crate::domain::symbol_info::get_symbol_info(
            &self.graph,
            &self.query_engine,
            node_id,
            include_callers || include_callees,
            used_fallback,
            params.line,
        )
        .await
        .ok_or_else(|| tower_lsp::jsonrpc::Error::invalid_params("Symbol not found"))?;

        let callers = if include_callers {
            domain_result
                .callers
                .unwrap_or_default()
                .into_iter()
                .map(call_info_to_response)
                .collect()
        } else {
            Vec::new()
        };

        let callees = if include_callees {
            domain_result
                .callees
                .unwrap_or_default()
                .into_iter()
                .map(call_info_to_response)
                .collect()
        } else {
            Vec::new()
        };

        Ok(DetailedSymbolResponse {
            symbol: symbol_info_to_response(&domain_result.symbol),
            callers,
            callees,
            complexity: domain_result.complexity,
            lines_of_code: domain_result.lines_of_code,
            is_public: domain_result.is_public,
            is_deprecated: domain_result.is_deprecated,
            reference_count: domain_result.reference_count,
            used_fallback: domain_result.used_fallback,
            fallback_message: domain_result.fallback_message,
        })
    }

    /// Handle get detailed symbol request (with optional source code)
    pub async fn handle_get_detailed_symbol(
        &self,
        params: GetDetailedSymbolParams,
    ) -> Result<GetDetailedSymbolResponse> {
        let (node_id, used_fallback, _) = self
            .resolve_node_id_with_fallback(&params.node_id, &params.uri, &params.line)
            .await?;

        let include_source = params.include_source.unwrap_or(false);
        let include_callers = params.include_callers.unwrap_or(true);
        let include_callees = params.include_callees.unwrap_or(true);

        let domain_result = crate::domain::symbol_info::get_detailed_symbol(
            &self.graph,
            &self.query_engine,
            node_id,
            include_source,
            include_callers,
            include_callees,
            used_fallback,
            params.line,
        )
        .await;

        let info = domain_result
            .symbol
            .ok_or_else(|| tower_lsp::jsonrpc::Error::invalid_params("Symbol not found"))?;

        Ok(GetDetailedSymbolResponse {
            symbol: symbol_info_to_response(&info.symbol),
            source: domain_result.source,
            callers: domain_result
                .callers
                .unwrap_or_default()
                .into_iter()
                .map(call_info_to_response)
                .collect(),
            callees: domain_result
                .callees
                .unwrap_or_default()
                .into_iter()
                .map(call_info_to_response)
                .collect(),
            complexity: info.complexity,
            lines_of_code: info.lines_of_code,
            is_public: info.is_public,
            is_deprecated: info.is_deprecated,
            reference_count: info.reference_count,
            used_fallback: domain_result.used_fallback,
            fallback_message: domain_result.fallback_message,
        })
    }

    /// Handle find by signature request
    pub async fn handle_find_by_signature(
        &self,
        params: FindBySignatureParams,
    ) -> Result<FindBySignatureResponse> {
        let start = std::time::Instant::now();

        // Build signature pattern from params
        let pattern = SignaturePattern {
            name_pattern: params.name_pattern,
            return_type: params.return_type,
            param_count: params.param_count.map(|r| (r.min, r.max)),
            modifiers: params.modifiers.unwrap_or_default(),
        };

        let results = self.query_engine.find_by_signature(&pattern, None).await;

        let response_results = results
            .into_iter()
            .map(|m| SymbolMatchResponse {
                node_id: m.node_id.to_string(),
                symbol: symbol_info_to_response(&m.symbol),
                score: m.score,
                match_reason: m.match_reason,
            })
            .collect();

        Ok(FindBySignatureResponse {
            results: response_results,
            query_time_ms: start.elapsed().as_millis() as u64,
        })
    }

    /// Helper to resolve a node ID from either direct ID or uri+line
    /// Returns (NodeId, used_fallback, fallback_message)
    async fn resolve_node_id_with_fallback(
        &self,
        node_id: &Option<String>,
        uri: &Option<String>,
        line: &Option<u32>,
    ) -> Result<(NodeId, bool, Option<String>)> {
        if let Some(id_str) = node_id {
            let node_id = id_str
                .parse::<NodeId>()
                .map_err(|_| tower_lsp::jsonrpc::Error::invalid_params("Invalid node ID"))?;
            return Ok((node_id, false, None));
        }

        let uri_str = uri.as_ref().ok_or_else(|| {
            tower_lsp::jsonrpc::Error::invalid_params("Must provide node_id or uri+line")
        })?;

        let url = Url::parse(uri_str)
            .map_err(|_| tower_lsp::jsonrpc::Error::invalid_params("Invalid URI"))?;

        let path = url
            .to_file_path()
            .map_err(|_| tower_lsp::jsonrpc::Error::invalid_params("Invalid file path"))?;

        let line_num = line.unwrap_or(1);
        let position = tower_lsp::lsp_types::Position {
            line: line_num.saturating_sub(1), // LSP is 0-indexed
            character: 0,
        };

        let graph = self.graph.read().await;

        // Use find_nearest_node which falls back to nearest symbol if exact position not found
        let (node_id, used_fallback) = self
            .find_nearest_node(&graph, &path, position)?
            .ok_or_else(|| {
                tower_lsp::jsonrpc::Error::invalid_params(
                    "No symbols found in file. Try indexing the workspace first.",
                )
            })?;

        let fallback_message = if used_fallback {
            // Get the symbol name to include in the message
            let name = if let Ok(node) = graph.get_node(node_id) {
                node.properties
                    .get_string("name")
                    .unwrap_or("unknown")
                    .to_string()
            } else {
                "unknown".to_string()
            };
            Some(format!(
                "No symbol at line {line_num}. Using nearest symbol '{name}' instead."
            ))
        } else {
            None
        };

        Ok((node_id, used_fallback, fallback_message))
    }
}

/// Helper to convert internal CallInfo to response format
fn call_info_to_response(c: crate::ai_query::CallInfo) -> CallInfoResponse {
    CallInfoResponse {
        node_id: c.node_id.to_string(),
        symbol: symbol_info_to_response(&c.symbol),
        call_site: SymbolLocationResponse {
            file: c.call_site.file,
            line: c.call_site.line,
            column: c.call_site.column,
            end_line: c.call_site.end_line,
            end_column: c.call_site.end_column,
        },
        depth: c.depth,
    }
}

/// Helper to convert internal SymbolInfo to response format
fn symbol_info_to_response(info: &crate::ai_query::SymbolInfo) -> SymbolInfoResponse {
    SymbolInfoResponse {
        name: info.name.clone(),
        kind: info.kind.clone(),
        location: SymbolLocationResponse {
            file: info.location.file.clone(),
            line: info.location.line,
            column: info.location.column,
            end_line: info.location.end_line,
            end_column: info.location.end_column,
        },
        signature: info.signature.clone(),
        docstring: info.docstring.clone(),
        is_public: info.is_public,
        visibility: info.visibility.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_query::QueryEngine;
    use codegraph::{CodeGraph, EdgeType, NodeType, PropertyMap, PropertyValue};
    use std::sync::Arc;
    use tokio::sync::RwLock;

    /// Create a test backend with an in-memory graph
    async fn create_test_backend() -> (CodeGraphBackend, Arc<RwLock<CodeGraph>>) {
        let graph = Arc::new(RwLock::new(
            CodeGraph::in_memory().expect("Failed to create in-memory graph"),
        ));
        let query_engine = Arc::new(QueryEngine::new(Arc::clone(&graph)));

        let backend = CodeGraphBackend::new_for_test(Arc::clone(&graph), query_engine);
        (backend, graph)
    }

    // ==========================================
    // Symbol Search Handler Tests
    // ==========================================

    #[tokio::test]
    async fn test_handle_symbol_search_basic() {
        let (backend, graph) = create_test_backend().await;

        // Add test data
        {
            let mut g = graph.write().await;
            let mut props = PropertyMap::new();
            props.insert(
                "name".to_string(),
                PropertyValue::String("findUser".to_string()),
            );
            props.insert(
                "path".to_string(),
                PropertyValue::String("/src/users.rs".to_string()),
            );
            props.insert("line_start".to_string(), PropertyValue::Int(10));
            props.insert("line_end".to_string(), PropertyValue::Int(20));
            g.add_node(NodeType::Function, props)
                .expect("Failed to add node");
        }

        backend.query_engine.build_indexes().await;

        let params = SymbolSearchParams {
            query: "find".to_string(),
            scope: None,
            symbol_types: None,
            limit: None,
            include_private: None,
        };

        let result = backend.handle_symbol_search(params).await.unwrap();

        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].symbol.name, "findUser");
    }

    #[tokio::test]
    async fn test_handle_symbol_search_with_type_filter() {
        let (backend, graph) = create_test_backend().await;

        // Add function and class
        {
            let mut g = graph.write().await;

            let mut func_props = PropertyMap::new();
            func_props.insert(
                "name".to_string(),
                PropertyValue::String("processData".to_string()),
            );
            func_props.insert(
                "path".to_string(),
                PropertyValue::String("/src/lib.rs".to_string()),
            );
            func_props.insert("line_start".to_string(), PropertyValue::Int(1));
            g.add_node(NodeType::Function, func_props)
                .expect("Failed to add function");

            let mut class_props = PropertyMap::new();
            class_props.insert(
                "name".to_string(),
                PropertyValue::String("DataProcessor".to_string()),
            );
            class_props.insert(
                "path".to_string(),
                PropertyValue::String("/src/lib.rs".to_string()),
            );
            class_props.insert("line_start".to_string(), PropertyValue::Int(20));
            g.add_node(NodeType::Class, class_props)
                .expect("Failed to add class");
        }

        backend.query_engine.build_indexes().await;

        // Search only for classes
        let params = SymbolSearchParams {
            query: "data".to_string(),
            scope: None,
            symbol_types: Some(vec!["class".to_string()]),
            limit: None,
            include_private: None,
        };

        let result = backend.handle_symbol_search(params).await.unwrap();

        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].symbol.kind, "Class");
    }

    #[tokio::test]
    async fn test_handle_symbol_search_with_limit() {
        let (backend, graph) = create_test_backend().await;

        // Add multiple functions
        {
            let mut g = graph.write().await;
            for i in 0..10 {
                let mut props = PropertyMap::new();
                props.insert(
                    "name".to_string(),
                    PropertyValue::String(format!("processItem{i}")),
                );
                props.insert(
                    "path".to_string(),
                    PropertyValue::String("/src/lib.rs".to_string()),
                );
                props.insert("line_start".to_string(), PropertyValue::Int(i * 10));
                g.add_node(NodeType::Function, props)
                    .expect("Failed to add node");
            }
        }

        backend.query_engine.build_indexes().await;

        let params = SymbolSearchParams {
            query: "process".to_string(),
            scope: None,
            symbol_types: None,
            limit: Some(3),
            include_private: None,
        };

        let result = backend.handle_symbol_search(params).await.unwrap();

        assert_eq!(result.results.len(), 3);
    }

    // ==========================================
    // Find By Imports Handler Tests
    // ==========================================

    #[tokio::test]
    async fn test_handle_find_by_imports() {
        let (backend, graph) = create_test_backend().await;

        // Add nodes with import relationship
        {
            let mut g = graph.write().await;

            // Source file
            let mut src_props = PropertyMap::new();
            src_props.insert(
                "name".to_string(),
                PropertyValue::String("app.rs".to_string()),
            );
            src_props.insert(
                "path".to_string(),
                PropertyValue::String("/src/app.rs".to_string()),
            );
            src_props.insert("line_start".to_string(), PropertyValue::Int(1));
            let src_id = g
                .add_node(NodeType::Module, src_props)
                .expect("Failed to add source");

            // Imported module
            let mut lib_props = PropertyMap::new();
            lib_props.insert(
                "name".to_string(),
                PropertyValue::String("serde".to_string()),
            );
            lib_props.insert(
                "path".to_string(),
                PropertyValue::String("serde".to_string()),
            );
            lib_props.insert("line_start".to_string(), PropertyValue::Int(1));
            let lib_id = g
                .add_node(NodeType::Module, lib_props)
                .expect("Failed to add lib");

            // Create import edge
            g.add_edge(src_id, lib_id, EdgeType::Imports, PropertyMap::new())
                .expect("Failed to add edge");
        }

        backend.query_engine.build_indexes().await;

        let params = FindByImportsParams {
            libraries: vec!["serde".to_string()],
            module_name: None,
            match_mode: Some("exact".to_string()),
        };

        let result = backend.handle_find_by_imports(params).await.unwrap();

        assert_eq!(result.results.len(), 1);
    }

    #[tokio::test]
    async fn test_handle_find_by_imports_fuzzy() {
        let (backend, graph) = create_test_backend().await;

        {
            let mut g = graph.write().await;

            let mut src_props = PropertyMap::new();
            src_props.insert(
                "name".to_string(),
                PropertyValue::String("main.rs".to_string()),
            );
            src_props.insert(
                "path".to_string(),
                PropertyValue::String("/src/main.rs".to_string()),
            );
            src_props.insert("line_start".to_string(), PropertyValue::Int(1));
            let src_id = g
                .add_node(NodeType::Module, src_props)
                .expect("Failed to add source");

            let mut lib_props = PropertyMap::new();
            lib_props.insert(
                "name".to_string(),
                PropertyValue::String("tokio_runtime".to_string()),
            );
            lib_props.insert(
                "path".to_string(),
                PropertyValue::String("tokio_runtime".to_string()),
            );
            lib_props.insert("line_start".to_string(), PropertyValue::Int(1));
            let lib_id = g
                .add_node(NodeType::Module, lib_props)
                .expect("Failed to add lib");

            g.add_edge(src_id, lib_id, EdgeType::Imports, PropertyMap::new())
                .expect("Failed to add edge");
        }

        backend.query_engine.build_indexes().await;

        let params = FindByImportsParams {
            libraries: vec!["tokio".to_string()],
            module_name: None,
            match_mode: Some("fuzzy".to_string()),
        };

        let result = backend.handle_find_by_imports(params).await.unwrap();

        assert_eq!(result.results.len(), 1);
    }

    // ==========================================
    // Find Entry Points Handler Tests
    // ==========================================

    #[tokio::test]
    async fn test_handle_find_entry_points_main() {
        let (backend, graph) = create_test_backend().await;

        {
            let mut g = graph.write().await;
            let mut props = PropertyMap::new();
            props.insert(
                "name".to_string(),
                PropertyValue::String("main".to_string()),
            );
            props.insert(
                "path".to_string(),
                PropertyValue::String("/src/main.rs".to_string()),
            );
            props.insert("line_start".to_string(), PropertyValue::Int(1));
            g.add_node(NodeType::Function, props)
                .expect("Failed to add main");
        }

        backend.query_engine.build_indexes().await;

        let params = FindEntryPointsParams {
            entry_type: Some("main".to_string()),
            framework: None,
        };

        let result = backend.handle_find_entry_points(params).await.unwrap();

        assert_eq!(result.total_found, 1);
        assert_eq!(result.entry_points[0].symbol.name, "main");
        assert_eq!(result.entry_points[0].entry_type, "main");
    }

    #[tokio::test]
    async fn test_handle_find_entry_points_http_handler() {
        let (backend, graph) = create_test_backend().await;

        {
            let mut g = graph.write().await;
            let mut props = PropertyMap::new();
            props.insert(
                "name".to_string(),
                PropertyValue::String("get_users".to_string()),
            );
            props.insert(
                "path".to_string(),
                PropertyValue::String("/src/api.rs".to_string()),
            );
            props.insert("line_start".to_string(), PropertyValue::Int(1));
            props.insert(
                "route".to_string(),
                PropertyValue::String("/api/users".to_string()),
            );
            props.insert(
                "http_method".to_string(),
                PropertyValue::String("GET".to_string()),
            );
            g.add_node(NodeType::Function, props)
                .expect("Failed to add handler");
        }

        backend.query_engine.build_indexes().await;

        let params = FindEntryPointsParams {
            entry_type: Some("http_handler".to_string()),
            framework: None,
        };

        let result = backend.handle_find_entry_points(params).await.unwrap();

        assert_eq!(result.total_found, 1);
        assert_eq!(result.entry_points[0].route, Some("/api/users".to_string()));
        assert_eq!(result.entry_points[0].method, Some("GET".to_string()));
    }

    #[tokio::test]
    async fn test_handle_find_entry_points_all() {
        let (backend, graph) = create_test_backend().await;

        {
            let mut g = graph.write().await;

            // Main
            let mut main_props = PropertyMap::new();
            main_props.insert(
                "name".to_string(),
                PropertyValue::String("main".to_string()),
            );
            main_props.insert(
                "path".to_string(),
                PropertyValue::String("/src/main.rs".to_string()),
            );
            main_props.insert("line_start".to_string(), PropertyValue::Int(1));
            g.add_node(NodeType::Function, main_props)
                .expect("Failed to add main");

            // Test
            let mut test_props = PropertyMap::new();
            test_props.insert(
                "name".to_string(),
                PropertyValue::String("test_users".to_string()),
            );
            test_props.insert(
                "path".to_string(),
                PropertyValue::String("/src/tests.rs".to_string()),
            );
            test_props.insert("line_start".to_string(), PropertyValue::Int(10));
            g.add_node(NodeType::Function, test_props)
                .expect("Failed to add test");
        }

        backend.query_engine.build_indexes().await;

        // Empty entry_type means find all
        let params = FindEntryPointsParams {
            entry_type: None,
            framework: None,
        };

        let result = backend.handle_find_entry_points(params).await.unwrap();

        assert!(result.total_found >= 2);
    }

    // ==========================================
    // Find By Signature Handler Tests
    // ==========================================

    #[tokio::test]
    async fn test_handle_find_by_signature_name_pattern() {
        let (backend, graph) = create_test_backend().await;

        {
            let mut g = graph.write().await;

            for name in ["getUserById", "getOrderById", "createUser"] {
                let mut props = PropertyMap::new();
                props.insert("name".to_string(), PropertyValue::String(name.to_string()));
                props.insert(
                    "path".to_string(),
                    PropertyValue::String("/src/api.rs".to_string()),
                );
                props.insert("line_start".to_string(), PropertyValue::Int(1));
                g.add_node(NodeType::Function, props)
                    .expect("Failed to add node");
            }
        }

        backend.query_engine.build_indexes().await;

        let params = FindBySignatureParams {
            name_pattern: Some("get.*ById".to_string()),
            return_type: None,
            param_count: None,
            modifiers: None,
        };

        let result = backend.handle_find_by_signature(params).await.unwrap();

        assert_eq!(result.results.len(), 2);
        let names: Vec<&str> = result
            .results
            .iter()
            .map(|r| r.symbol.name.as_str())
            .collect();
        assert!(names.contains(&"getUserById"));
        assert!(names.contains(&"getOrderById"));
    }

    #[tokio::test]
    async fn test_handle_find_by_signature_with_modifiers() {
        let (backend, graph) = create_test_backend().await;

        {
            let mut g = graph.write().await;

            // Async public function
            let mut async_props = PropertyMap::new();
            async_props.insert(
                "name".to_string(),
                PropertyValue::String("fetchData".to_string()),
            );
            async_props.insert(
                "path".to_string(),
                PropertyValue::String("/src/lib.rs".to_string()),
            );
            async_props.insert("line_start".to_string(), PropertyValue::Int(1));
            async_props.insert("is_async".to_string(), PropertyValue::Bool(true));
            async_props.insert("is_public".to_string(), PropertyValue::Bool(true));
            g.add_node(NodeType::Function, async_props)
                .expect("Failed to add async fn");

            // Sync function
            let mut sync_props = PropertyMap::new();
            sync_props.insert(
                "name".to_string(),
                PropertyValue::String("processData".to_string()),
            );
            sync_props.insert(
                "path".to_string(),
                PropertyValue::String("/src/lib.rs".to_string()),
            );
            sync_props.insert("line_start".to_string(), PropertyValue::Int(10));
            sync_props.insert("is_async".to_string(), PropertyValue::Bool(false));
            g.add_node(NodeType::Function, sync_props)
                .expect("Failed to add sync fn");
        }

        backend.query_engine.build_indexes().await;

        let params = FindBySignatureParams {
            name_pattern: None,
            return_type: None,
            param_count: None,
            modifiers: Some(vec!["async".to_string()]),
        };

        let result = backend.handle_find_by_signature(params).await.unwrap();

        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].symbol.name, "fetchData");
    }

    #[tokio::test]
    async fn test_handle_find_by_signature_param_count() {
        let (backend, graph) = create_test_backend().await;

        {
            let mut g = graph.write().await;

            for (name, count) in [("noArgs", 0), ("oneArg", 1), ("twoArgs", 2)] {
                let mut props = PropertyMap::new();
                props.insert("name".to_string(), PropertyValue::String(name.to_string()));
                props.insert(
                    "path".to_string(),
                    PropertyValue::String("/src/lib.rs".to_string()),
                );
                props.insert("line_start".to_string(), PropertyValue::Int(1));
                props.insert("param_count".to_string(), PropertyValue::Int(count));
                g.add_node(NodeType::Function, props)
                    .expect("Failed to add node");
            }
        }

        backend.query_engine.build_indexes().await;

        let params = FindBySignatureParams {
            name_pattern: None,
            return_type: None,
            param_count: Some(ParamCountRange { min: 1, max: 2 }),
            modifiers: None,
        };

        let result = backend.handle_find_by_signature(params).await.unwrap();

        assert_eq!(result.results.len(), 2);
        let names: Vec<&str> = result
            .results
            .iter()
            .map(|r| r.symbol.name.as_str())
            .collect();
        assert!(names.contains(&"oneArg"));
        assert!(names.contains(&"twoArgs"));
    }

    // ==========================================
    // Get Callers Handler Tests
    // ==========================================

    #[tokio::test]
    async fn test_handle_get_callers() {
        let (backend, graph) = create_test_backend().await;

        let target_id;
        {
            let mut g = graph.write().await;

            // Caller function
            let mut caller_props = PropertyMap::new();
            caller_props.insert(
                "name".to_string(),
                PropertyValue::String("caller".to_string()),
            );
            caller_props.insert(
                "path".to_string(),
                PropertyValue::String("/src/lib.rs".to_string()),
            );
            caller_props.insert("line_start".to_string(), PropertyValue::Int(1));
            let caller_id = g
                .add_node(NodeType::Function, caller_props)
                .expect("Failed to add caller");

            // Target function
            let mut target_props = PropertyMap::new();
            target_props.insert(
                "name".to_string(),
                PropertyValue::String("target".to_string()),
            );
            target_props.insert(
                "path".to_string(),
                PropertyValue::String("/src/lib.rs".to_string()),
            );
            target_props.insert("line_start".to_string(), PropertyValue::Int(10));
            target_id = g
                .add_node(NodeType::Function, target_props)
                .expect("Failed to add target");

            // Create call edge
            g.add_edge(caller_id, target_id, EdgeType::Calls, PropertyMap::new())
                .expect("Failed to add call edge");
        }

        backend.query_engine.build_indexes().await;

        let params = GetCallersParams {
            node_id: Some(target_id.to_string()),
            uri: None,
            line: None,
            depth: Some(1),
        };

        let result = backend.handle_get_callers(params).await.unwrap();

        assert_eq!(result.callers.len(), 1);
        assert_eq!(result.callers[0].symbol.name, "caller");
    }

    // ==========================================
    // Get Callees Handler Tests
    // ==========================================

    #[tokio::test]
    async fn test_handle_get_callees() {
        let (backend, graph) = create_test_backend().await;

        let caller_id;
        {
            let mut g = graph.write().await;

            // Caller function
            let mut caller_props = PropertyMap::new();
            caller_props.insert(
                "name".to_string(),
                PropertyValue::String("caller".to_string()),
            );
            caller_props.insert(
                "path".to_string(),
                PropertyValue::String("/src/lib.rs".to_string()),
            );
            caller_props.insert("line_start".to_string(), PropertyValue::Int(1));
            caller_id = g
                .add_node(NodeType::Function, caller_props)
                .expect("Failed to add caller");

            // First callee
            let mut callee1_props = PropertyMap::new();
            callee1_props.insert(
                "name".to_string(),
                PropertyValue::String("callee1".to_string()),
            );
            callee1_props.insert(
                "path".to_string(),
                PropertyValue::String("/src/lib.rs".to_string()),
            );
            callee1_props.insert("line_start".to_string(), PropertyValue::Int(10));
            let callee1_id = g
                .add_node(NodeType::Function, callee1_props)
                .expect("Failed to add callee1");

            // Second callee
            let mut callee2_props = PropertyMap::new();
            callee2_props.insert(
                "name".to_string(),
                PropertyValue::String("callee2".to_string()),
            );
            callee2_props.insert(
                "path".to_string(),
                PropertyValue::String("/src/lib.rs".to_string()),
            );
            callee2_props.insert("line_start".to_string(), PropertyValue::Int(20));
            let callee2_id = g
                .add_node(NodeType::Function, callee2_props)
                .expect("Failed to add callee2");

            // Create call edges
            g.add_edge(caller_id, callee1_id, EdgeType::Calls, PropertyMap::new())
                .expect("Failed to add edge 1");
            g.add_edge(caller_id, callee2_id, EdgeType::Calls, PropertyMap::new())
                .expect("Failed to add edge 2");
        }

        backend.query_engine.build_indexes().await;

        let params = GetCallersParams {
            node_id: Some(caller_id.to_string()),
            uri: None,
            line: None,
            depth: Some(1),
        };

        let result = backend.handle_get_callees(params).await.unwrap();

        assert_eq!(result.callers.len(), 2);
        let names: Vec<&str> = result
            .callers
            .iter()
            .map(|c| c.symbol.name.as_str())
            .collect();
        assert!(names.contains(&"callee1"));
        assert!(names.contains(&"callee2"));
    }

    // ==========================================
    // Traverse Graph Handler Tests
    // ==========================================

    #[tokio::test]
    async fn test_handle_traverse_graph() {
        let (backend, graph) = create_test_backend().await;

        let start_id;
        {
            let mut g = graph.write().await;

            // Start node
            let mut start_props = PropertyMap::new();
            start_props.insert(
                "name".to_string(),
                PropertyValue::String("start".to_string()),
            );
            start_props.insert(
                "path".to_string(),
                PropertyValue::String("/src/lib.rs".to_string()),
            );
            start_props.insert("line_start".to_string(), PropertyValue::Int(1));
            start_id = g
                .add_node(NodeType::Function, start_props)
                .expect("Failed to add start");

            // Middle node
            let mut middle_props = PropertyMap::new();
            middle_props.insert(
                "name".to_string(),
                PropertyValue::String("middle".to_string()),
            );
            middle_props.insert(
                "path".to_string(),
                PropertyValue::String("/src/lib.rs".to_string()),
            );
            middle_props.insert("line_start".to_string(), PropertyValue::Int(10));
            let middle_id = g
                .add_node(NodeType::Function, middle_props)
                .expect("Failed to add middle");

            // End node
            let mut end_props = PropertyMap::new();
            end_props.insert("name".to_string(), PropertyValue::String("end".to_string()));
            end_props.insert(
                "path".to_string(),
                PropertyValue::String("/src/lib.rs".to_string()),
            );
            end_props.insert("line_start".to_string(), PropertyValue::Int(20));
            let end_id = g
                .add_node(NodeType::Function, end_props)
                .expect("Failed to add end");

            // Create call chain
            g.add_edge(start_id, middle_id, EdgeType::Calls, PropertyMap::new())
                .expect("Failed to add edge 1");
            g.add_edge(middle_id, end_id, EdgeType::Calls, PropertyMap::new())
                .expect("Failed to add edge 2");
        }

        backend.query_engine.build_indexes().await;

        let params = TraverseGraphParams {
            start_node_id: Some(start_id.to_string()),
            uri: None,
            line: None,
            direction: Some("outgoing".to_string()),
            depth: Some(2),
            filter_symbol_types: None,
            max_nodes: None,
        };

        let result = backend.handle_traverse_graph(params).await.unwrap();

        assert_eq!(result.nodes.len(), 2);
        let names: Vec<&str> = result
            .nodes
            .iter()
            .map(|n| n.symbol.name.as_str())
            .collect();
        assert!(names.contains(&"middle"));
        assert!(names.contains(&"end"));
    }

    #[tokio::test]
    async fn test_handle_traverse_graph_with_type_filter() {
        let (backend, graph) = create_test_backend().await;

        let start_id;
        {
            let mut g = graph.write().await;

            // Start function
            let mut start_props = PropertyMap::new();
            start_props.insert(
                "name".to_string(),
                PropertyValue::String("startFn".to_string()),
            );
            start_props.insert(
                "path".to_string(),
                PropertyValue::String("/src/lib.rs".to_string()),
            );
            start_props.insert("line_start".to_string(), PropertyValue::Int(1));
            start_id = g
                .add_node(NodeType::Function, start_props)
                .expect("Failed to add start");

            // Connected function
            let mut fn_props = PropertyMap::new();
            fn_props.insert(
                "name".to_string(),
                PropertyValue::String("helperFn".to_string()),
            );
            fn_props.insert(
                "path".to_string(),
                PropertyValue::String("/src/lib.rs".to_string()),
            );
            fn_props.insert("line_start".to_string(), PropertyValue::Int(10));
            let fn_id = g
                .add_node(NodeType::Function, fn_props)
                .expect("Failed to add function");

            // Connected class
            let mut class_props = PropertyMap::new();
            class_props.insert(
                "name".to_string(),
                PropertyValue::String("MyClass".to_string()),
            );
            class_props.insert(
                "path".to_string(),
                PropertyValue::String("/src/lib.rs".to_string()),
            );
            class_props.insert("line_start".to_string(), PropertyValue::Int(20));
            let class_id = g
                .add_node(NodeType::Class, class_props)
                .expect("Failed to add class");

            // Create edges
            g.add_edge(start_id, fn_id, EdgeType::Calls, PropertyMap::new())
                .expect("Failed to add edge 1");
            g.add_edge(start_id, class_id, EdgeType::References, PropertyMap::new())
                .expect("Failed to add edge 2");
        }

        backend.query_engine.build_indexes().await;

        let params = TraverseGraphParams {
            start_node_id: Some(start_id.to_string()),
            uri: None,
            line: None,
            direction: Some("outgoing".to_string()),
            depth: Some(1),
            filter_symbol_types: Some(vec!["function".to_string()]),
            max_nodes: None,
        };

        let result = backend.handle_traverse_graph(params).await.unwrap();

        assert_eq!(result.nodes.len(), 1);
        assert_eq!(result.nodes[0].symbol.name, "helperFn");
        assert_eq!(result.nodes[0].symbol.kind, "Function");
    }

    // ==========================================
    // Get Detailed Symbol Info Handler Tests
    // ==========================================

    #[tokio::test]
    async fn test_handle_get_detailed_symbol_info() {
        let (backend, graph) = create_test_backend().await;

        let node_id;
        {
            let mut g = graph.write().await;
            let mut props = PropertyMap::new();
            props.insert(
                "name".to_string(),
                PropertyValue::String("myFunction".to_string()),
            );
            props.insert(
                "path".to_string(),
                PropertyValue::String("/src/lib.rs".to_string()),
            );
            props.insert("line_start".to_string(), PropertyValue::Int(10));
            props.insert("line_end".to_string(), PropertyValue::Int(25));
            props.insert("is_public".to_string(), PropertyValue::Bool(true));
            props.insert(
                "doc".to_string(),
                PropertyValue::String("My documentation".to_string()),
            );
            props.insert("complexity".to_string(), PropertyValue::Int(5));
            node_id = g
                .add_node(NodeType::Function, props)
                .expect("Failed to add node");
        }

        backend.query_engine.build_indexes().await;

        let params = GetDetailedInfoParams {
            node_id: Some(node_id.to_string()),
            uri: None,
            line: None,
            include_callers: Some(true),
            include_callees: Some(true),
        };

        let result = backend
            .handle_get_detailed_symbol_info(params)
            .await
            .unwrap();

        assert_eq!(result.symbol.name, "myFunction");
        assert!(result.is_public);
        assert_eq!(result.lines_of_code, 16); // 25 - 10 + 1
        assert_eq!(result.complexity, Some(5));
    }

    #[tokio::test]
    async fn test_handle_get_detailed_symbol_info_with_callers() {
        let (backend, graph) = create_test_backend().await;

        let target_id;
        {
            let mut g = graph.write().await;

            // Caller
            let mut caller_props = PropertyMap::new();
            caller_props.insert(
                "name".to_string(),
                PropertyValue::String("caller".to_string()),
            );
            caller_props.insert(
                "path".to_string(),
                PropertyValue::String("/src/lib.rs".to_string()),
            );
            caller_props.insert("line_start".to_string(), PropertyValue::Int(1));
            let caller_id = g
                .add_node(NodeType::Function, caller_props)
                .expect("Failed to add caller");

            // Target
            let mut target_props = PropertyMap::new();
            target_props.insert(
                "name".to_string(),
                PropertyValue::String("target".to_string()),
            );
            target_props.insert(
                "path".to_string(),
                PropertyValue::String("/src/lib.rs".to_string()),
            );
            target_props.insert("line_start".to_string(), PropertyValue::Int(10));
            target_id = g
                .add_node(NodeType::Function, target_props)
                .expect("Failed to add target");

            // Callee
            let mut callee_props = PropertyMap::new();
            callee_props.insert(
                "name".to_string(),
                PropertyValue::String("callee".to_string()),
            );
            callee_props.insert(
                "path".to_string(),
                PropertyValue::String("/src/lib.rs".to_string()),
            );
            callee_props.insert("line_start".to_string(), PropertyValue::Int(20));
            let callee_id = g
                .add_node(NodeType::Function, callee_props)
                .expect("Failed to add callee");

            // Create edges
            g.add_edge(caller_id, target_id, EdgeType::Calls, PropertyMap::new())
                .expect("Failed to add edge 1");
            g.add_edge(target_id, callee_id, EdgeType::Calls, PropertyMap::new())
                .expect("Failed to add edge 2");
        }

        backend.query_engine.build_indexes().await;

        let params = GetDetailedInfoParams {
            node_id: Some(target_id.to_string()),
            uri: None,
            line: None,
            include_callers: Some(true),
            include_callees: Some(true),
        };

        let result = backend
            .handle_get_detailed_symbol_info(params)
            .await
            .unwrap();

        assert_eq!(result.symbol.name, "target");
        assert_eq!(result.callers.len(), 1);
        assert_eq!(result.callers[0].symbol.name, "caller");
        assert_eq!(result.callees.len(), 1);
        assert_eq!(result.callees[0].symbol.name, "callee");
    }

    // ==========================================
    // Error Handling Tests
    // ==========================================

    #[tokio::test]
    async fn test_resolve_node_id_invalid() {
        let (backend, _) = create_test_backend().await;

        let params = GetCallersParams {
            node_id: Some("invalid_id".to_string()),
            uri: None,
            line: None,
            depth: None,
        };

        let result = backend.handle_get_callers(params).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_resolve_node_id_missing_params() {
        let (backend, _) = create_test_backend().await;

        let params = GetCallersParams {
            node_id: None,
            uri: None,
            line: None,
            depth: None,
        };

        let result = backend.handle_get_callers(params).await;

        assert!(result.is_err());
    }
}
