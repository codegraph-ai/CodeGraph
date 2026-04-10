// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! LSP Backend Implementation
//!
//! This module implements the Language Server Protocol for CodeGraph.

use crate::ai_query::QueryEngine;
use crate::branch_watcher::BranchWatcher;
use crate::cache::QueryCache;
use crate::domain::node_props;
use crate::error::{LspError, LspResult};
use crate::index::SymbolIndex;
use crate::memory::MemoryManager;
use crate::parser_registry::ParserRegistry;
use crate::watcher::{FileWatcher, GraphUpdater};
use codegraph::{CodeGraph, Direction, EdgeType, NodeId, NodeType};
use codegraph_parser_api::FileInfo;
use dashmap::DashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

/// Configuration for indexing behavior, populated from VS Code settings.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeGraphConfig {
    #[serde(default)]
    pub index_on_startup: bool,
    #[serde(default)]
    pub exclude_patterns: Vec<String>,
    #[serde(default)]
    pub index_paths: Vec<String>,
    #[serde(default = "default_max_file_size_kb")]
    pub max_file_size_kb: u64,
}

fn default_max_file_size_kb() -> u64 {
    1024
}

impl Default for CodeGraphConfig {
    fn default() -> Self {
        Self {
            index_on_startup: false,
            exclude_patterns: Vec::new(),
            index_paths: Vec::new(),
            max_file_size_kb: 1024,
        }
    }
}

/// CodeGraph Language Server backend.
pub struct CodeGraphBackend {
    /// LSP client for sending notifications.
    pub client: Client,

    /// The code graph database.
    pub graph: Arc<RwLock<CodeGraph>>,

    /// Parser registry for all supported languages.
    pub parsers: Arc<ParserRegistry>,

    /// File cache: URI -> FileInfo.
    pub file_cache: Arc<DashMap<Url, FileInfo>>,

    /// Query cache for performance.
    pub query_cache: Arc<QueryCache>,

    /// Symbol index for fast lookups.
    pub symbol_index: Arc<SymbolIndex>,

    /// AI Agent Query Engine for fast code exploration.
    pub query_engine: Arc<QueryEngine>,

    /// Memory manager for persistent AI context.
    pub memory_manager: Arc<MemoryManager>,

    /// Workspace folders
    pub workspace_folders: Arc<RwLock<Vec<std::path::PathBuf>>>,

    /// File system watcher for incremental updates.
    file_watcher: Arc<Mutex<Option<FileWatcher>>>,

    /// Branch watcher for detecting git branch switches.
    branch_watcher: Arc<Mutex<Option<BranchWatcher>>>,

    /// Indexing configuration from VS Code settings.
    pub config: Arc<RwLock<CodeGraphConfig>>,

    /// Extension point for pro LSP commands (community uses NoopProCommandProvider).
    pub pro_commands: Arc<dyn crate::lsp_pro_hooks::ProCommandProvider>,

    /// Command namespace prefix for LSP commands (e.g., "codegraph" or "stellarion").
    command_prefix: String,
}

impl CodeGraphBackend {
    /// Create a new CodeGraph backend.
    pub fn new(client: Client) -> Self {
        let graph = Arc::new(RwLock::new(
            CodeGraph::in_memory().expect("Failed to create in-memory graph"),
        ));

        Self {
            client,
            query_engine: Arc::new(QueryEngine::new(Arc::clone(&graph))),
            graph,
            parsers: Arc::new(ParserRegistry::new()),
            file_cache: Arc::new(DashMap::new()),
            query_cache: Arc::new(QueryCache::new(1000)),
            symbol_index: Arc::new(SymbolIndex::new()),
            memory_manager: Arc::new(MemoryManager::new(None)),
            workspace_folders: Arc::new(RwLock::new(Vec::new())),
            file_watcher: Arc::new(Mutex::new(None)),
            branch_watcher: Arc::new(Mutex::new(None)),
            config: Arc::new(RwLock::new(CodeGraphConfig::default())),
            pro_commands: Arc::new(crate::lsp_pro_hooks::NoopProCommandProvider),
            command_prefix: "codegraph".to_string(),
        }
    }

    /// Create a backend with a custom pro command provider for LSP.
    /// The command prefix is taken from the provider (e.g., "stellarion").
    pub fn with_pro_commands(
        client: Client,
        pro_commands: Arc<dyn crate::lsp_pro_hooks::ProCommandProvider>,
    ) -> Self {
        let prefix = pro_commands.command_prefix().to_string();
        let mut backend = Self::new(client);
        backend.pro_commands = pro_commands;
        backend.command_prefix = prefix;
        backend
    }

    /// Create a backend for testing with a pre-configured graph and query engine.
    /// This allows tests to inject their own graph state without needing a real LSP client.
    #[cfg(test)]
    pub fn new_for_test(graph: Arc<RwLock<CodeGraph>>, query_engine: Arc<QueryEngine>) -> Self {
        use tower_lsp::LspService;

        // Create a dummy client for testing
        let (service, _socket) = LspService::new(Self::new);
        let dummy_client = service.inner().client.clone();

        Self {
            client: dummy_client,
            query_engine,
            graph,
            parsers: Arc::new(ParserRegistry::new()),
            file_cache: Arc::new(DashMap::new()),
            query_cache: Arc::new(QueryCache::new(1000)),
            symbol_index: Arc::new(SymbolIndex::new()),
            memory_manager: Arc::new(MemoryManager::new(None)),
            workspace_folders: Arc::new(RwLock::new(Vec::new())),
            file_watcher: Arc::new(Mutex::new(None)),
            branch_watcher: Arc::new(Mutex::new(None)),
            config: Arc::new(RwLock::new(CodeGraphConfig::default())),
            pro_commands: Arc::new(crate::lsp_pro_hooks::NoopProCommandProvider),
            command_prefix: "codegraph".to_string(),
        }
    }

    /// Start the file watcher for the given workspace folders.
    pub async fn start_file_watcher(&self, folders: &[PathBuf]) {
        // Create the file watcher
        match FileWatcher::new(
            Arc::clone(&self.graph),
            Arc::clone(&self.parsers),
            self.client.clone(),
            Arc::clone(&self.memory_manager),
        ) {
            Ok(mut watcher) => {
                // Start watching each folder
                for folder in folders {
                    if let Err(e) = watcher.watch(folder) {
                        self.client
                            .log_message(
                                MessageType::WARNING,
                                format!("Failed to watch {}: {}", folder.display(), e),
                            )
                            .await;
                    } else {
                        self.client
                            .log_message(
                                MessageType::INFO,
                                format!("Watching folder: {}", folder.display()),
                            )
                            .await;
                    }
                }

                // Store the watcher
                *self.file_watcher.lock().await = Some(watcher);
            }
            Err(e) => {
                self.client
                    .log_message(
                        MessageType::ERROR,
                        format!("Failed to create file watcher: {e}"),
                    )
                    .await;
            }
        }
    }

    /// Add directories to the file watcher, creating one if it doesn't exist.
    ///
    /// Called after on-demand indexing so file changes trigger incremental updates.
    pub async fn watch_directories(&self, paths: &[PathBuf]) {
        let mut guard = self.file_watcher.lock().await;
        if let Some(ref mut watcher) = *guard {
            for path in paths {
                if let Err(e) = watcher.watch(path) {
                    tracing::warn!("Failed to watch {}: {}", path.display(), e);
                } else {
                    tracing::info!("Now watching: {}", path.display());
                }
            }
        } else {
            // No watcher yet — create one
            drop(guard);
            self.start_file_watcher(paths).await;
        }
    }

    /// Start the branch watcher for git-aware re-indexing on branch switches.
    pub async fn start_branch_watcher(&self, workspace_root: &Path) {
        match BranchWatcher::new(
            Arc::clone(&self.graph),
            Arc::clone(&self.parsers),
            Arc::clone(&self.symbol_index),
            Arc::clone(&self.query_engine),
            Arc::clone(&self.query_cache),
            self.client.clone(),
            Arc::clone(&self.memory_manager),
            workspace_root.to_path_buf(),
        ) {
            Ok(watcher) => {
                *self.branch_watcher.lock().await = Some(watcher);
                self.client
                    .log_message(MessageType::INFO, "Branch watcher started")
                    .await;
            }
            Err(e) => {
                self.client
                    .log_message(
                        MessageType::WARNING,
                        format!("Failed to start branch watcher: {e}"),
                    )
                    .await;
            }
        }
    }

    /// Remove all nodes associated with a file from the graph.
    ///
    /// Also auto-invalidates any memories linked to the removed nodes.
    async fn remove_file_from_graph(&self, path: &std::path::Path) {
        let path_str = path.to_string_lossy().to_string();
        let node_id_strings: Vec<String>;

        // Scope the graph lock to avoid holding it across await
        {
            let mut graph = self.graph.write().await;

            // Query for all nodes with this file path using the query builder
            if let Ok(nodes) = graph.query().property("path", path_str.clone()).execute() {
                // Collect node IDs as strings for memory invalidation
                node_id_strings = nodes.iter().map(|n| n.to_string()).collect();

                // Delete the nodes from the graph
                for node_id in nodes {
                    let _ = graph.delete_node(node_id);
                }
            } else {
                node_id_strings = Vec::new();
            }
        }

        // Auto-invalidate memories linked to these nodes (after releasing graph lock)
        if !node_id_strings.is_empty() {
            let reason = format!("Code changed: {}", path_str);
            if let Err(e) = self
                .memory_manager
                .invalidate_for_code_nodes(&node_id_strings, &reason)
                .await
            {
                tracing::warn!("Failed to invalidate memories for {}: {}", path_str, e);
            }
        }

        // Invalidate caches
        self.query_cache.invalidate_file(&path.to_path_buf());
        self.symbol_index.remove_file(path);
    }

    /// Maximum recursion depth for directory traversal. Prevents runaway
    /// indexing into deeply nested result/log directory trees.
    const MAX_INDEX_DEPTH: u32 = 20;

    /// Maximum number of files to index per workspace. Acts as a safety valve
    /// so the server cannot OOM on huge directory trees.
    const MAX_INDEXED_FILES: usize = 5_000;

    /// Directories that should be skipped during indexing.
    const SKIP_DIRECTORIES: &'static [&'static str] = &[
        "node_modules",
        "target",
        "dist",
        "build",
        "out",
        ".git",
        "__pycache__",
        "vendor",
        "DerivedData",
        "tmp",
        "coverage",
        "htmlcov",
        "results",
        "logs",
    ];

    /// Build a `GlobSet` from user-configured exclude patterns.
    /// Uses the `globset` crate which properly supports `**` (globstar).
    /// Logs warnings for any patterns that fail to compile.
    fn build_exclude_set(patterns: &[String]) -> globset::GlobSet {
        let mut builder = globset::GlobSetBuilder::new();
        for pattern in patterns {
            match globset::Glob::new(pattern) {
                Ok(g) => {
                    builder.add(g);
                }
                Err(e) => {
                    tracing::warn!("Invalid exclude pattern '{}': {}", pattern, e);
                }
            }
        }
        builder.build().unwrap_or_else(|e| {
            tracing::warn!("Failed to build exclude GlobSet: {}", e);
            globset::GlobSet::empty()
        })
    }

    /// Index all supported files in a directory
    pub fn index_directory<'a>(
        &'a self,
        dir: &'a std::path::Path,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = usize> + Send + 'a>> {
        let counter = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        self.index_directory_inner(dir, 0, counter)
    }

    /// Inner recursive implementation with depth tracking and a shared file counter.
    fn index_directory_inner<'a>(
        &'a self,
        dir: &'a std::path::Path,
        depth: u32,
        total_files: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = usize> + Send + 'a>> {
        Box::pin(async move {
            use std::fs;
            use std::sync::atomic::Ordering;

            if depth > Self::MAX_INDEX_DEPTH {
                tracing::warn!(
                    "Skipping {:?}: exceeded max indexing depth of {}",
                    dir,
                    Self::MAX_INDEX_DEPTH
                );
                return 0;
            }

            if total_files.load(Ordering::Relaxed) >= Self::MAX_INDEXED_FILES {
                return 0;
            }

            // Read config for exclude patterns and max file size
            let config = self.config.read().await.clone();
            let max_file_size = config.max_file_size_kb * 1024;
            let exclude_set = Self::build_exclude_set(&config.exclude_patterns);

            let mut indexed_count = 0;
            let supported_extensions = self.parsers.supported_extensions();

            tracing::info!("Indexing directory: {:?}", dir);

            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.flatten() {
                    if total_files.load(Ordering::Relaxed) >= Self::MAX_INDEXED_FILES {
                        tracing::warn!(
                            "Reached max indexed file limit of {}; stopping",
                            Self::MAX_INDEXED_FILES
                        );
                        break;
                    }

                    let path = entry.path();

                    // Skip hidden files and directories
                    if let Some(name) = path.file_name() {
                        if name.to_string_lossy().starts_with('.') {
                            continue;
                        }
                    }

                    if path.is_dir() {
                        let dir_name = path.file_name().unwrap().to_string_lossy();

                        // Skip hardcoded default directories
                        if Self::SKIP_DIRECTORIES
                            .iter()
                            .any(|&s| s == dir_name.as_ref())
                        {
                            continue;
                        }

                        // Skip directories matching user-configured exclude globs
                        let path_str = path.to_string_lossy();
                        if exclude_set.is_match(path_str.as_ref())
                            || exclude_set.is_match(dir_name.as_ref())
                        {
                            tracing::info!("Skipping {:?}: matched exclude pattern", path);
                            continue;
                        }

                        // Recursively index subdirectories
                        indexed_count += self
                            .index_directory_inner(&path, depth + 1, total_files.clone())
                            .await;
                    } else if path.is_file() {
                        // Skip files matching exclude globs
                        let path_str = path.to_string_lossy();
                        if exclude_set.is_match(path_str.as_ref()) {
                            continue;
                        }

                        // Skip files that exceed the configurable size limit
                        if let Ok(metadata) = fs::metadata(&path) {
                            if metadata.len() > max_file_size {
                                tracing::info!(
                                    "Skipping {:?}: file size {} exceeds limit of {}",
                                    path,
                                    metadata.len(),
                                    max_file_size
                                );
                                continue;
                            }
                        }

                        // Check if file has supported extension
                        if let Some(ext) = path.extension() {
                            let ext_str = ext.to_string_lossy();
                            if supported_extensions
                                .iter()
                                .any(|&e| e.trim_start_matches('.') == ext_str)
                            {
                                // Parse the file using parse_file (which updates metrics)
                                if let Some(parser) = self.parsers.parser_for_path(&path) {
                                    let mut graph = self.graph.write().await;

                                    // Remove old nodes for this file to prevent duplicates on re-index
                                    let path_str = path.to_string_lossy().to_string();
                                    if let Ok(old_nodes) =
                                        graph.query().property("path", path_str).execute()
                                    {
                                        for old_id in old_nodes {
                                            let _ = graph.delete_node(old_id);
                                        }
                                    }

                                    match parser.parse_file(&path, &mut graph) {
                                        Ok(file_info) => {
                                            self.symbol_index.add_file(
                                                path.clone(),
                                                &file_info,
                                                &graph,
                                            );
                                            self.file_cache.insert(
                                                Url::from_file_path(&path).unwrap(),
                                                file_info,
                                            );
                                            indexed_count += 1;
                                            total_files.fetch_add(1, Ordering::Relaxed);
                                        }
                                        Err(e) => {
                                            tracing::warn!("Failed to parse {:?}: {}", path, e);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            indexed_count
        })
    }

    /// Find node at the given position.
    /// Position line is 0-indexed (LSP style), but we convert to 1-indexed for graph.
    pub fn find_node_at_position(
        &self,
        graph: &CodeGraph,
        path: &std::path::Path,
        position: Position,
    ) -> LspResult<Option<NodeId>> {
        // LSP positions are 0-indexed, our index stores 1-indexed
        let line = (position.line + 1) as i64;
        let col = position.character as i64;

        tracing::info!(
            "find_node_at_position: path={:?}, LSP position={}:{}, converted={}:{}",
            path,
            position.line,
            position.character,
            line,
            col
        );

        // Try the symbol index first (faster)
        if let Some(node_id) = self
            .symbol_index
            .find_at_position(path, line as u32, col as u32)
        {
            tracing::info!("Found node in symbol index: {:?}", node_id);
            return Ok(Some(node_id));
        }

        tracing::info!("Not found in symbol index, trying graph query");

        // Fallback: Get all nodes from the symbol index for this file and check positions
        // The graph stores line_start/line_end (not start_line/end_line)
        let file_symbols = self.symbol_index.get_file_symbols(path);

        tracing::info!(
            "Symbol index returned {} symbols for {:?}",
            file_symbols.len(),
            path
        );

        for node_id in file_symbols {
            if let Ok(node) = graph.get_node(node_id) {
                // Parsers use line_start/line_end (not start_line/end_line)
                let start_line = node_props::line_start(node) as i64;
                let end_line = node_props::line_end(node) as i64;
                // Column info not available from most parsers, default to full line
                let start_col = node.properties.get_int("col_start").unwrap_or(0);
                let end_col = node.properties.get_int("col_end").unwrap_or(i64::MAX);

                tracing::info!(
                    "Checking node {:?} '{}' at {}:{} to {}:{}",
                    node_id,
                    node_props::name(node),
                    start_line,
                    start_col,
                    end_line,
                    end_col
                );

                if line >= start_line && line <= end_line {
                    if line == start_line && col < start_col {
                        continue;
                    }
                    if line == end_line && col > end_col {
                        continue;
                    }
                    tracing::info!("Found matching node: {:?}", node_id);
                    return Ok(Some(node_id));
                }
            }
        }

        tracing::warn!("No node found at position {}:{} in {:?}", line, col, path);
        Ok(None)
    }

    /// Find the nearest symbol to a position, or the first symbol in the file.
    /// This is used as a fallback when no symbol is found at the exact cursor position.
    /// Returns (node_id, was_fallback) where was_fallback indicates if this was not an exact match.
    pub fn find_nearest_node(
        &self,
        graph: &CodeGraph,
        path: &std::path::Path,
        position: Position,
    ) -> LspResult<Option<(NodeId, bool)>> {
        // First try exact position match
        if let Some(node_id) = self.find_node_at_position(graph, path, position)? {
            return Ok(Some((node_id, false)));
        }

        // LSP positions are 0-indexed, our index stores 1-indexed
        let target_line = (position.line + 1) as i64;

        // Get all symbols in the file
        let file_symbols = self.symbol_index.get_file_symbols(path);

        if file_symbols.is_empty() {
            tracing::info!("No symbols in file {:?}", path);
            return Ok(None);
        }

        // Find the nearest symbol by line distance
        let mut best_match: Option<(NodeId, i64)> = None;

        for node_id in file_symbols {
            if let Ok(node) = graph.get_node(node_id) {
                let start_line = node_props::line_start(node) as i64;
                let end_line = node_props::line_end(node) as i64;

                // Calculate distance - prefer symbols that start after cursor (looking ahead)
                // or symbols that contain the cursor line
                let distance = if target_line >= start_line && target_line <= end_line {
                    // Cursor is within this symbol's range
                    0
                } else if start_line > target_line {
                    // Symbol starts after cursor - prefer these (looking forward)
                    start_line - target_line
                } else {
                    // Symbol ends before cursor - less preferred
                    (target_line - end_line) + 1000 // Add penalty for looking backward
                };

                if best_match.is_none() || distance < best_match.unwrap().1 {
                    best_match = Some((node_id, distance));
                }
            }
        }

        if let Some((node_id, _)) = best_match {
            if let Ok(node) = graph.get_node(node_id) {
                let name = node_props::name(node);
                tracing::info!(
                    "Fallback: found nearest symbol '{}' for position {}:{} in {:?}",
                    name,
                    target_line,
                    position.character,
                    path
                );
            }
            return Ok(Some((node_id, true)));
        }

        Ok(None)
    }

    /// Find all edges connected to a node.
    pub fn get_connected_edges(
        &self,
        graph: &CodeGraph,
        node_id: NodeId,
        direction: Direction,
    ) -> Vec<(NodeId, NodeId, EdgeType)> {
        let mut edges = Vec::new();

        // Get neighbors in the specified direction
        let neighbors = match graph.get_neighbors(node_id, direction) {
            Ok(n) => n,
            Err(_) => return edges,
        };

        for neighbor_id in neighbors {
            // Get edges between this node and the neighbor
            let (source, target) = match direction {
                Direction::Outgoing => (node_id, neighbor_id),
                Direction::Incoming => (neighbor_id, node_id),
                Direction::Both => {
                    // Try both directions
                    if let Ok(edge_ids) = graph.get_edges_between(node_id, neighbor_id) {
                        for edge_id in edge_ids {
                            if let Ok(edge) = graph.get_edge(edge_id) {
                                edges.push((edge.source_id, edge.target_id, edge.edge_type));
                            }
                        }
                    }
                    if let Ok(edge_ids) = graph.get_edges_between(neighbor_id, node_id) {
                        for edge_id in edge_ids {
                            if let Ok(edge) = graph.get_edge(edge_id) {
                                edges.push((edge.source_id, edge.target_id, edge.edge_type));
                            }
                        }
                    }
                    continue;
                }
            };

            if let Ok(edge_ids) = graph.get_edges_between(source, target) {
                for edge_id in edge_ids {
                    if let Ok(edge) = graph.get_edge(edge_id) {
                        edges.push((edge.source_id, edge.target_id, edge.edge_type));
                    }
                }
            }
        }

        edges
    }

    /// Find the definition node for a reference.
    fn find_definition_for_reference(
        &self,
        graph: &CodeGraph,
        ref_node_id: NodeId,
    ) -> LspResult<Option<NodeId>> {
        let edges = self.get_connected_edges(graph, ref_node_id, Direction::Outgoing);

        for (_, target, edge_type) in edges {
            match edge_type {
                EdgeType::Calls | EdgeType::References | EdgeType::Imports => {
                    return Ok(Some(target));
                }
                _ => continue,
            }
        }

        Ok(None)
    }

    /// Convert a node to an LSP Location.
    pub fn node_to_location(&self, graph: &CodeGraph, node_id: NodeId) -> LspResult<Location> {
        let node = graph
            .get_node(node_id)
            .map_err(|e| LspError::Graph(e.to_string()))?;

        // Try to get path from node properties first, fallback to symbol index
        let path_str = match node.properties.get_string("path") {
            Some(p) => p.to_string(),
            None => {
                // Fallback: look up file path from symbol index
                match self.symbol_index.find_file_for_node(node_id) {
                    Some(path_buf) => path_buf.to_string_lossy().to_string(),
                    None => {
                        let node_name = node_props::name(node);
                        let node_type = format!("{}", node.node_type);
                        tracing::warn!(
                            "Cannot determine file path for node {}: {} '{}' (not in symbol index)",
                            node_id,
                            node_type,
                            node_name
                        );
                        return Err(LspError::NodeNotFound(format!(
                            "Cannot determine file path for {node_type} '{node_name}'"
                        )));
                    }
                }
            }
        };

        // Support both property name conventions (line_start or start_line)
        let start_line = node_props::line_start(node);
        let start_col = node
            .properties
            .get_int("col_start")
            .or_else(|| node.properties.get_int("start_col"))
            .unwrap_or(0) as u32;
        let end_line = {
            let e = node_props::line_end(node);
            if e == 0 {
                start_line
            } else {
                e
            }
        };
        let end_col = node
            .properties
            .get_int("col_end")
            .or_else(|| node.properties.get_int("end_col"))
            .unwrap_or(0) as u32;

        // Convert to 0-indexed
        let start_line = start_line.saturating_sub(1);
        let end_line = end_line.saturating_sub(1);

        Ok(Location {
            uri: Url::from_file_path(&path_str)
                .map_err(|_| LspError::InvalidUri(path_str.clone()))?,
            range: Range {
                start: Position {
                    line: start_line,
                    character: start_col,
                },
                end: Position {
                    line: end_line,
                    character: end_col,
                },
            },
        })
    }

    /// Get the source code for a node.
    pub async fn get_node_source_code(&self, node_id: NodeId) -> LspResult<Option<String>> {
        let graph = self.graph.read().await;

        // Primary: use domain source code reader (handles inline source + path from node properties)
        if let Some(source) = crate::domain::source_code::get_symbol_source(&graph, node_id) {
            return Ok(Some(source));
        }

        // Fallback: try SymbolIndex for path resolution when node has no path property
        let node = graph
            .get_node(node_id)
            .map_err(|e| LspError::Graph(e.to_string()))?;
        if node.properties.get_string("path").is_none() {
            if let Some(file_path) = self.symbol_index.find_file_for_node(node_id) {
                let start = crate::domain::node_props::line_start_opt(node).unwrap_or(1) as usize;
                let end =
                    crate::domain::node_props::line_end_opt(node).unwrap_or(start as u32) as usize;
                if let Ok(content) = tokio::fs::read_to_string(&file_path).await {
                    let lines: Vec<&str> = content.lines().collect();
                    if start > 0 && end <= lines.len() {
                        return Ok(Some(lines[start - 1..end].join("\n")));
                    }
                }
            }
        }

        Ok(None)
    }

    /// Helper to get a string property from a node
    #[allow(dead_code)]
    fn get_node_string_property(
        &self,
        graph: &CodeGraph,
        node_id: NodeId,
        key: &str,
    ) -> Option<String> {
        graph
            .get_node(node_id)
            .ok()?
            .properties
            .get_string(key)
            .map(|s| s.to_string())
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for CodeGraphBackend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        tracing::info!("Initializing CodeGraph LSP server");

        // Extract extension path and config from initialization options
        let init_opts = params.initialization_options;

        let extension_path = init_opts.as_ref().and_then(|opts| {
            opts.get("extensionPath")
                .and_then(|v| v.as_str())
                .map(std::path::PathBuf::from)
        });

        // Parse indexing configuration
        if let Some(ref opts) = init_opts {
            let config = CodeGraphConfig {
                index_on_startup: opts
                    .get("indexOnStartup")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                exclude_patterns: opts
                    .get("excludePatterns")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default(),
                index_paths: opts
                    .get("indexPaths")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default(),
                max_file_size_kb: opts
                    .get("maxFileSizeKB")
                    .and_then(|v| v.as_u64())
                    .unwrap_or_else(default_max_file_size_kb),
            };
            tracing::info!("CodeGraph config: index_on_startup={}, exclude_patterns={:?}, index_paths={:?}, max_file_size_kb={}",
                config.index_on_startup, config.exclude_patterns, config.index_paths, config.max_file_size_kb);
            *self.config.write().await = config;
        }

        if let Some(path) = extension_path {
            tracing::info!(
                "[LSP::initialize] Extension path received: {}",
                path.display()
            );
            // Update memory manager with extension path by replacing it
            // Read embedding model from init options
            let raw_model = init_opts
                .as_ref()
                .and_then(|opts| opts.get("embeddingModel"));
            tracing::info!(
                "[LSP::initialize] embeddingModel from init options: {:?}",
                raw_model
            );

            let embedding_model = raw_model
                .and_then(|v| v.as_str())
                .and_then(|s| {
                    tracing::info!("[LSP::initialize] Parsing embedding model string: {:?}", s);
                    match s {
                        "bge-small" => Some(codegraph_memory::CodeGraphEmbeddingModel::BgeSmall),
                        "jina-code-v2" => {
                            Some(codegraph_memory::CodeGraphEmbeddingModel::JinaCodeV2)
                        }
                        _ => {
                            tracing::warn!(
                                "[LSP::initialize] Unknown embedding model: {:?}, using default",
                                s
                            );
                            None
                        }
                    }
                })
                .unwrap_or_default();

            tracing::info!(
                "[LSP::initialize] Selected embedding model: {}",
                embedding_model.display_name()
            );

            // Safety: We're replacing the Arc contents during initialization before any use
            let new_manager = Arc::new(MemoryManager::with_model(
                Some(path.clone()),
                embedding_model,
            ));
            let self_mut = self as *const Self as *mut Self;
            unsafe {
                (*self_mut).memory_manager = new_manager;
            }
            tracing::info!("[LSP::initialize] MemoryManager updated with extension path and model");

            // Read full-body embedding setting
            let full_body = init_opts
                .as_ref()
                .and_then(|opts| opts.get("fullBodyEmbedding"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            self.query_engine.set_full_body_embedding(full_body);
            tracing::info!("[LSP::initialize] Full-body embedding: {}", full_body);
        } else {
            tracing::error!(
                "[LSP::initialize] CRITICAL: No extension path provided in initialization options!"
            );
            tracing::warn!("[LSP::initialize] No extension path provided — fastembed will auto-download model to ~/.codegraph/fastembed_cache/");
        }

        // Store workspace folders
        if let Some(folders) = params.workspace_folders {
            let mut workspace_folders = self.workspace_folders.write().await;
            for folder in folders {
                if let Ok(path) = folder.uri.to_file_path() {
                    tracing::info!("Workspace folder: {}", path.display());
                    workspace_folders.push(path);
                }
            }
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::FULL),
                        save: Some(TextDocumentSyncSaveOptions::SaveOptions(SaveOptions {
                            include_text: Some(true),
                        })),
                        ..Default::default()
                    },
                )),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                call_hierarchy_provider: Some(CallHierarchyServerCapability::Simple(true)),
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: {
                        let p = &self.command_prefix;
                        let base_commands = vec![
                            format!("{p}.getDependencyGraph"),
                            format!("{p}.getCallGraph"),
                            format!("{p}.analyzeImpact"),
                            format!("{p}.getParserMetrics"),
                            format!("{p}.reindexWorkspace"),
                            format!("{p}.getAIContext"),
                            format!("{p}.getEditContext"),
                            format!("{p}.getCuratedContext"),
                            format!("{p}.findRelatedTests"),
                            format!("{p}.getNodeLocation"),
                            format!("{p}.getWorkspaceSymbols"),
                            format!("{p}.analyzeComplexity"),
                            format!("{p}.symbolSearch"),
                            format!("{p}.findByImports"),
                            format!("{p}.findEntryPoints"),
                            format!("{p}.traverseGraph"),
                            format!("{p}.getCallers"),
                            format!("{p}.getCallees"),
                            format!("{p}.getDetailedSymbolInfo"),
                            format!("{p}.findBySignature"),
                            format!("{p}.memoryStore"),
                            format!("{p}.memorySearch"),
                            format!("{p}.memoryGet"),
                            format!("{p}.memoryInvalidate"),
                            format!("{p}.memoryList"),
                            format!("{p}.memoryUpdate"),
                            format!("{p}.memoryContext"),
                            format!("{p}.memoryStats"),
                            format!("{p}.mineGitHistory"),
                            format!("{p}.mineGitHistoryForFile"),
                            format!("{p}.searchGitHistory"),
                            format!("{p}.indexDirectory"),
                            format!("{p}.updateConfiguration"),
                        ];
                        // Note: pro commands are NOT advertised here to avoid
                        // vscode-languageclient auto-registering them as VS Code commands
                        // (conflicts with toolManager registrations). Pro commands work
                        // via the execute_command fallthrough to ProCommandProvider.
                        base_commands
                    },
                    work_done_progress_options: WorkDoneProgressOptions::default(),
                }),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "CodeGraph LSP server initialized")
            .await;

        // Index workspace folders only if configured to do so
        let folders = self.workspace_folders.read().await.clone();
        let config = self.config.read().await.clone();
        let mut total_indexed = 0;

        if config.index_on_startup {
            // Determine which paths to index
            let paths_to_index: Vec<std::path::PathBuf> = if config.index_paths.is_empty() {
                folders.clone()
            } else {
                config
                    .index_paths
                    .iter()
                    .map(std::path::PathBuf::from)
                    .collect()
            };

            for folder in &paths_to_index {
                let count = self.index_directory(folder).await;
                total_indexed += count;
                self.client
                    .log_message(
                        MessageType::INFO,
                        format!("Indexed {} files from {}", count, folder.display()),
                    )
                    .await;
            }

            self.client
                .log_message(
                    MessageType::INFO,
                    format!("Total files indexed: {total_indexed}"),
                )
                .await;

            // Resolve cross-file imports after all files are indexed
            {
                let mut graph = self.graph.write().await;
                GraphUpdater::resolve_cross_file_imports(&mut graph);
            }
            self.client
                .log_message(MessageType::INFO, "Cross-file imports resolved")
                .await;

            // Build AI query engine indexes (includes embedding all symbols — can take 30-120s)
            self.client
                .log_message(
                    MessageType::INFO,
                    format!("Building semantic search index ({total_indexed} files)... This may take a moment."),
                )
                .await;
            self.query_engine.build_indexes().await;
            self.client
                .log_message(MessageType::INFO, "Semantic search index ready")
                .await;
        } else {
            self.client
                .log_message(MessageType::INFO, "Skipping auto-index (indexOnStartup=false). Use 'Index Directory' command to index specific paths.")
                .await;
        }

        // Initialize memory store for persistent AI context
        if let Some(first_folder) = folders.first() {
            tracing::info!(
                "Starting memory store initialization for workspace: {}",
                first_folder.display()
            );
            self.client
                .log_message(
                    MessageType::INFO,
                    format!(
                        "[DEBUG] Initializing memory store for: {}",
                        first_folder.display()
                    ),
                )
                .await;

            match self.memory_manager.initialize(first_folder).await {
                Ok(_) => {
                    tracing::info!("Memory store initialization succeeded");
                    self.client
                        .log_message(MessageType::INFO, "✓ Memory store initialized successfully")
                        .await;

                    // Share vector engine with query engine for semantic symbol search
                    if let Some(engine) = self.memory_manager.get_vector_engine().await {
                        self.query_engine.set_vector_engine(engine).await;
                        self.query_engine.build_symbol_vectors().await;
                        self.client
                            .log_message(MessageType::INFO, "✓ Semantic symbol search initialized")
                            .await;
                    }
                }
                Err(e) => {
                    tracing::error!("Memory store initialization failed: {:?}", e);
                    self.client
                        .log_message(
                            MessageType::ERROR,
                            format!("✗ Failed to initialize memory store: {}. Memory features will be disabled.", e),
                        )
                        .await;
                }
            }
        } else {
            tracing::warn!("No workspace folders available for memory initialization");
            self.client
                .log_message(
                    MessageType::WARNING,
                    "[DEBUG] No workspace folders found - memory store not initialized",
                )
                .await;
        }

        // Start file watcher for incremental updates (only if we indexed something)
        if config.index_on_startup && !folders.is_empty() {
            let watch_paths = if config.index_paths.is_empty() {
                folders.clone()
            } else {
                config
                    .index_paths
                    .iter()
                    .map(std::path::PathBuf::from)
                    .collect()
            };
            self.start_file_watcher(&watch_paths).await;
        }

        // Start branch watcher for git-aware re-indexing on branch switches
        if let Some(first_folder) = folders.first() {
            self.start_branch_watcher(first_folder).await;
        }
    }

    async fn shutdown(&self) -> Result<()> {
        tracing::info!("Shutting down CodeGraph LSP server");
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let text = params.text_document.text;

        tracing::info!("did_open called for: {}", uri);

        let path = match uri.to_file_path() {
            Ok(p) => p,
            Err(_) => {
                tracing::warn!("Invalid URI: {}", uri);
                return;
            }
        };

        // Respect indexOnStartup=false: only index on did_open if the file
        // was previously indexed (exists in symbol_index), or if indexOnStartup is true.
        // We check symbol_index instead of file_cache because did_close removes from
        // file_cache, causing VS Code's close/reopen cycles to lose track of files.
        let config = self.config.read().await.clone();
        let previously_indexed = !self.symbol_index.get_file_symbols(&path).is_empty();
        if !config.index_on_startup && !previously_indexed {
            tracing::info!(
                "Skipping did_open index for {:?}: indexOnStartup=false and file not yet indexed",
                path
            );
            return;
        }

        // Skip files matching exclude patterns
        let exclude_set = Self::build_exclude_set(&config.exclude_patterns);
        let path_str = path.to_string_lossy();
        if exclude_set.is_match(path_str.as_ref()) {
            tracing::info!("Skipping did_open for {:?}: matched exclude pattern", path);
            return;
        }

        if let Some(parser) = self.parsers.parser_for_path(&path) {
            tracing::info!("Parser found for: {:?}", path);

            // Remove old entries first to prevent duplicate nodes with stale IDs
            self.remove_file_from_graph(&path).await;

            let mut graph = self.graph.write().await;

            match parser.parse_source(&text, &path, &mut graph) {
                Ok(file_info) => {
                    tracing::info!("Parse succeeded for: {:?}", path);

                    // Resolve cross-file imports after parsing
                    GraphUpdater::resolve_cross_file_imports(&mut graph);

                    // Update symbol index
                    self.symbol_index.add_file(path.clone(), &file_info, &graph);

                    // Update file cache
                    self.file_cache.insert(uri.clone(), file_info);

                    self.client
                        .log_message(MessageType::INFO, format!("Indexed: {uri}"))
                        .await;
                }
                Err(e) => {
                    tracing::error!("Parse failed for {:?}: {}", path, e);
                    self.client
                        .log_message(MessageType::ERROR, format!("Parse error in {uri}: {e}"))
                        .await;
                }
            }

            // Drop graph write lock before rebuilding query indexes
            drop(graph);

            // Rebuild AI query engine indexes so callee/caller indexes reflect new node IDs
            self.query_engine.build_indexes().await;
        } else {
            tracing::warn!("No parser found for: {:?}", path);
        }
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;

        let path = match uri.to_file_path() {
            Ok(p) => p,
            Err(_) => return,
        };

        // Get the full text (assuming full sync mode)
        if let Some(change) = params.content_changes.into_iter().next() {
            if let Some(parser) = self.parsers.parser_for_path(&path) {
                // Remove old entries
                self.remove_file_from_graph(&path).await;

                // Re-parse with new content
                {
                    let mut graph = self.graph.write().await;
                    if let Ok(file_info) = parser.parse_source(&change.text, &path, &mut graph) {
                        // Resolve cross-file imports after parsing
                        GraphUpdater::resolve_cross_file_imports(&mut graph);

                        self.symbol_index.add_file(path.clone(), &file_info, &graph);
                        self.file_cache.insert(uri, file_info);
                    }
                }

                // Rebuild AI query engine indexes so callee/caller indexes reflect new node IDs
                self.query_engine.build_indexes().await;
            }
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = params.text_document.uri;
        tracing::info!("did_save called for: {}", uri);

        let path = match uri.to_file_path() {
            Ok(p) => p,
            Err(_) => return,
        };

        if let Some(parser) = self.parsers.parser_for_path(&path) {
            if let Some(text) = params.text {
                tracing::info!("did_save has text, re-parsing + re-embedding: {}", uri);
                self.remove_file_from_graph(&path).await;

                {
                    let mut graph = self.graph.write().await;
                    if let Ok(file_info) = parser.parse_source(&text, &path, &mut graph) {
                        // Resolve cross-file imports after parsing
                        GraphUpdater::resolve_cross_file_imports(&mut graph);

                        self.symbol_index.add_file(path.clone(), &file_info, &graph);
                        self.file_cache.insert(uri, file_info);
                    }
                }

                // Rebuild AI query engine indexes
                self.query_engine.build_indexes().await;

                // Incrementally re-embed only this file's symbols (not the whole codebase)
                let path_str = path.to_string_lossy().to_string();
                self.query_engine.update_file_vectors(&path_str).await;
            }
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        // Keep in graph for cross-file references, but remove from file cache
        self.file_cache.remove(&params.text_document.uri);
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let path = uri
            .to_file_path()
            .map_err(|_| tower_lsp::jsonrpc::Error::invalid_params("Invalid URI"))?;

        let graph = self.graph.read().await;

        // Find node at the given position
        let node_id = match self.find_node_at_position(&graph, &path, position)? {
            Some(id) => id,
            None => return Ok(None),
        };

        // Check if this is a reference - find its definition
        if let Some(def_node_id) = self.find_definition_for_reference(&graph, node_id)? {
            tracing::info!(
                "goto_definition: found reference node {} -> definition node {}",
                node_id,
                def_node_id
            );
            match self.node_to_location(&graph, def_node_id) {
                Ok(location) => return Ok(Some(GotoDefinitionResponse::Scalar(location))),
                Err(e) => {
                    // Log the error but try to return the source location as fallback
                    tracing::warn!(
                        "Failed to get definition location: {}, trying source node",
                        e
                    );
                    // Fall through to try source node
                }
            }
        }

        // Already at definition (or fallback if definition lookup failed)
        let location = self.node_to_location(&graph, node_id)?;
        Ok(Some(GotoDefinitionResponse::Scalar(location)))
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = &params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let include_declaration = params.context.include_declaration;

        let path = uri
            .to_file_path()
            .map_err(|_| tower_lsp::jsonrpc::Error::invalid_params("Invalid URI"))?;

        let graph = self.graph.read().await;

        // Find node at position
        let node_id = match self.find_node_at_position(&graph, &path, position)? {
            Some(id) => id,
            None => return Ok(None),
        };

        // Find the definition
        let def_node_id = self
            .find_definition_for_reference(&graph, node_id)?
            .unwrap_or(node_id);

        let mut locations = Vec::new();

        // Include declaration if requested
        if include_declaration {
            if let Ok(loc) = self.node_to_location(&graph, def_node_id) {
                locations.push(loc);
            }
        }

        // Find all incoming edges (references to this definition)
        let edges = self.get_connected_edges(&graph, def_node_id, Direction::Incoming);

        for (source, _, _) in edges {
            if let Ok(loc) = self.node_to_location(&graph, source) {
                locations.push(loc);
            }
        }

        if locations.is_empty() {
            Ok(None)
        } else {
            Ok(Some(locations))
        }
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let path = uri
            .to_file_path()
            .map_err(|_| tower_lsp::jsonrpc::Error::invalid_params("Invalid URI"))?;

        let graph = self.graph.read().await;

        let node_id = match self.find_node_at_position(&graph, &path, position)? {
            Some(id) => id,
            None => return Ok(None),
        };

        let node = graph
            .get_node(node_id)
            .map_err(|_| tower_lsp::jsonrpc::Error::internal_error())?;

        // Build hover content
        let name = node_props::name(node).to_string();
        let kind = format!("{}", node.node_type);
        let signature = node
            .properties
            .get_string("signature")
            .unwrap_or("")
            .to_string();
        let doc = node.properties.get_string("doc").map(|s| s.to_string());
        let def_path = node_props::path(node).to_string();

        // Count references
        let ref_count = self
            .get_connected_edges(&graph, node_id, Direction::Incoming)
            .len();

        let mut content = format!("**{kind}** `{name}`");

        if !signature.is_empty() {
            content.push_str(&format!("\n\n```\n{signature}\n```"));
        }

        if let Some(doc) = doc {
            content.push_str(&format!("\n\n{doc}"));
        }

        content.push_str(&format!(
            "\n\n---\n\n**Defined in:** {def_path}\n**References:** {ref_count}"
        ));

        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: content,
            }),
            range: None,
        }))
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = &params.text_document.uri;

        let path = uri
            .to_file_path()
            .map_err(|_| tower_lsp::jsonrpc::Error::invalid_params("Invalid URI"))?;

        let graph = self.graph.read().await;

        // Get all symbols in this file
        let node_ids = self.symbol_index.get_file_symbols(&path);

        let mut symbols = Vec::new();

        for node_id in node_ids {
            if let Ok(node) = graph.get_node(node_id) {
                let name = node_props::name(node).to_string();
                let kind = match node.node_type {
                    NodeType::Function => SymbolKind::FUNCTION,
                    NodeType::Class => SymbolKind::CLASS,
                    NodeType::Interface => SymbolKind::INTERFACE,
                    NodeType::Module => SymbolKind::MODULE,
                    NodeType::Variable => SymbolKind::VARIABLE,
                    NodeType::Type => SymbolKind::TYPE_PARAMETER,
                    NodeType::CodeFile => SymbolKind::FILE,
                    _ => SymbolKind::VARIABLE,
                };

                if let Ok(location) = self.node_to_location(&graph, node_id) {
                    #[allow(deprecated)]
                    symbols.push(SymbolInformation {
                        name,
                        kind,
                        tags: None,
                        deprecated: None,
                        location,
                        container_name: None,
                    });
                }
            }
        }

        if symbols.is_empty() {
            Ok(None)
        } else {
            Ok(Some(DocumentSymbolResponse::Flat(symbols)))
        }
    }

    async fn prepare_call_hierarchy(
        &self,
        params: CallHierarchyPrepareParams,
    ) -> Result<Option<Vec<CallHierarchyItem>>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let path = uri
            .to_file_path()
            .map_err(|_| tower_lsp::jsonrpc::Error::invalid_params("Invalid URI"))?;

        let graph = self.graph.read().await;

        let node_id = match self.find_node_at_position(&graph, &path, position)? {
            Some(id) => id,
            None => return Ok(None),
        };

        let node = graph
            .get_node(node_id)
            .map_err(|_| tower_lsp::jsonrpc::Error::internal_error())?;

        // Only functions can have call hierarchies
        if node.node_type != NodeType::Function {
            return Ok(None);
        }

        let name = node_props::name(node).to_string();
        let location = self.node_to_location(&graph, node_id)?;

        Ok(Some(vec![CallHierarchyItem {
            name,
            kind: SymbolKind::FUNCTION,
            tags: None,
            detail: node
                .properties
                .get_string("signature")
                .map(|s| s.to_string()),
            uri: location.uri,
            range: location.range,
            selection_range: location.range,
            data: Some(serde_json::json!({ "nodeId": node_id.to_string() })),
        }]))
    }

    async fn incoming_calls(
        &self,
        params: CallHierarchyIncomingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyIncomingCall>>> {
        let node_id = self.extract_node_id_from_item(&params.item)?;

        let graph = self.graph.read().await;

        let edges = self.get_connected_edges(&graph, node_id, Direction::Incoming);

        let mut calls = Vec::new();

        for (source, _, edge_type) in edges {
            if edge_type == EdgeType::Calls {
                if let Ok(node) = graph.get_node(source) {
                    let name = node_props::name(node).to_string();

                    if let Ok(location) = self.node_to_location(&graph, source) {
                        calls.push(CallHierarchyIncomingCall {
                            from: CallHierarchyItem {
                                name,
                                kind: SymbolKind::FUNCTION,
                                tags: None,
                                detail: node
                                    .properties
                                    .get_string("signature")
                                    .map(|s| s.to_string()),
                                uri: location.uri.clone(),
                                range: location.range,
                                selection_range: location.range,
                                data: Some(serde_json::json!({ "nodeId": source.to_string() })),
                            },
                            from_ranges: vec![location.range],
                        });
                    }
                }
            }
        }

        if calls.is_empty() {
            Ok(None)
        } else {
            Ok(Some(calls))
        }
    }

    async fn outgoing_calls(
        &self,
        params: CallHierarchyOutgoingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyOutgoingCall>>> {
        let node_id = self.extract_node_id_from_item(&params.item)?;

        let graph = self.graph.read().await;

        let edges = self.get_connected_edges(&graph, node_id, Direction::Outgoing);

        let mut calls = Vec::new();

        for (_, target, edge_type) in edges {
            if edge_type == EdgeType::Calls {
                if let Ok(node) = graph.get_node(target) {
                    let name = node_props::name(node).to_string();

                    if let Ok(location) = self.node_to_location(&graph, target) {
                        calls.push(CallHierarchyOutgoingCall {
                            to: CallHierarchyItem {
                                name,
                                kind: SymbolKind::FUNCTION,
                                tags: None,
                                detail: node
                                    .properties
                                    .get_string("signature")
                                    .map(|s| s.to_string()),
                                uri: location.uri.clone(),
                                range: location.range,
                                selection_range: location.range,
                                data: Some(serde_json::json!({ "nodeId": target.to_string() })),
                            },
                            from_ranges: vec![location.range],
                        });
                    }
                }
            }
        }

        if calls.is_empty() {
            Ok(None)
        } else {
            Ok(Some(calls))
        }
    }

    async fn execute_command(
        &self,
        params: ExecuteCommandParams,
    ) -> Result<Option<serde_json::Value>> {
        tracing::info!("Executing command: {}", params.command);

        // Remap incoming command prefix to "codegraph." for internal dispatch.
        // e.g., "stellarion.getDependencyGraph" → "codegraph.getDependencyGraph"
        let canonical_command = if self.command_prefix != "codegraph" {
            params.command.replacen(&self.command_prefix, "codegraph", 1)
        } else {
            params.command.clone()
        };

        match canonical_command.as_str() {
            "codegraph.getDependencyGraph" => {
                let args = params.arguments.first().ok_or_else(|| {
                    tower_lsp::jsonrpc::Error::invalid_params("Missing arguments")
                })?;
                let params: crate::handlers::DependencyGraphParams =
                    serde_json::from_value(args.clone()).map_err(|e| {
                        tower_lsp::jsonrpc::Error::invalid_params(format!("Invalid params: {e}"))
                    })?;
                let response = self.handle_get_dependency_graph(params).await?;
                Ok(Some(serde_json::to_value(response).unwrap()))
            }

            "codegraph.getCallGraph" => {
                let args = params.arguments.first().ok_or_else(|| {
                    tower_lsp::jsonrpc::Error::invalid_params("Missing arguments")
                })?;
                let params: crate::handlers::CallGraphParams = serde_json::from_value(args.clone())
                    .map_err(|e| {
                        tower_lsp::jsonrpc::Error::invalid_params(format!("Invalid params: {e}"))
                    })?;
                let response = self.handle_get_call_graph(params).await?;
                Ok(Some(serde_json::to_value(response).unwrap()))
            }

            "codegraph.analyzeImpact" => {
                let args = params.arguments.first().ok_or_else(|| {
                    tower_lsp::jsonrpc::Error::invalid_params("Missing arguments")
                })?;
                let params: crate::handlers::ImpactAnalysisParams =
                    serde_json::from_value(args.clone()).map_err(|e| {
                        tower_lsp::jsonrpc::Error::invalid_params(format!("Invalid params: {e}"))
                    })?;
                let response = self.handle_analyze_impact(params).await?;
                Ok(Some(serde_json::to_value(response).unwrap()))
            }

            "codegraph.getParserMetrics" => {
                let response = self
                    .handle_get_parser_metrics(crate::handlers::ParserMetricsParams {
                        language: None,
                    })
                    .await?;
                Ok(Some(serde_json::to_value(response).unwrap()))
            }

            "codegraph.reindexWorkspace" => {
                // Clear graph and caches
                {
                    let mut graph = self.graph.write().await;
                    *graph = CodeGraph::in_memory().expect("Failed to create graph");
                }
                self.symbol_index.clear();
                self.file_cache.clear();
                self.query_cache.invalidate_all();

                self.client
                    .log_message(MessageType::INFO, "Reindexing workspace...")
                    .await;

                // Use indexPaths if configured, otherwise fall back to workspace folders
                let config = self.config.read().await.clone();
                let paths_to_index: Vec<std::path::PathBuf> = if config.index_paths.is_empty() {
                    self.workspace_folders.read().await.clone()
                } else {
                    config
                        .index_paths
                        .iter()
                        .map(std::path::PathBuf::from)
                        .collect()
                };
                let mut total_indexed = 0;

                for folder in paths_to_index {
                    tracing::info!("Indexing folder: {:?}", folder);
                    let count = self.index_directory(&folder).await;
                    total_indexed += count;
                }

                // Resolve cross-file imports and calls before building indexes
                {
                    let mut graph = self.graph.write().await;
                    GraphUpdater::resolve_cross_file_imports(&mut graph);
                }

                // Rebuild AI query engine indexes
                self.query_engine.build_indexes().await;
                self.query_engine.build_symbol_vectors().await;

                self.client
                    .log_message(
                        MessageType::INFO,
                        format!("Workspace reindexed: {total_indexed} files"),
                    )
                    .await;

                Ok(Some(serde_json::json!({
                    "status": "success",
                    "message": format!("Workspace reindexed: {total_indexed} files"),
                    "files_indexed": total_indexed
                })))
            }

            "codegraph.getAIContext" => {
                let args = params.arguments.first().ok_or_else(|| {
                    tower_lsp::jsonrpc::Error::invalid_params("Missing arguments")
                })?;
                let params: crate::handlers::AIContextParams = serde_json::from_value(args.clone())
                    .map_err(|e| {
                        tower_lsp::jsonrpc::Error::invalid_params(format!("Invalid params: {e}"))
                    })?;
                let response = self.handle_get_ai_context(params).await?;
                Ok(Some(serde_json::to_value(response).unwrap()))
            }

            "codegraph.getEditContext" => {
                let args = params.arguments.first().ok_or_else(|| {
                    tower_lsp::jsonrpc::Error::invalid_params("Missing arguments")
                })?;
                let edit_params: serde_json::Value = serde_json::from_value(args.clone())
                    .map_err(|e| tower_lsp::jsonrpc::Error::invalid_params(e.to_string()))?;
                let response = self.handle_get_edit_context(edit_params).await?;
                Ok(Some(response))
            }

            "codegraph.getCuratedContext" => {
                let args = params.arguments.first().ok_or_else(|| {
                    tower_lsp::jsonrpc::Error::invalid_params("Missing arguments")
                })?;
                let curated_params: serde_json::Value = serde_json::from_value(args.clone())
                    .map_err(|e| tower_lsp::jsonrpc::Error::invalid_params(e.to_string()))?;
                let response = self.handle_get_curated_context(curated_params).await?;
                Ok(Some(response))
            }

            "codegraph.findRelatedTests" => {
                let args = params.arguments.first().ok_or_else(|| {
                    tower_lsp::jsonrpc::Error::invalid_params("Missing arguments")
                })?;
                let params: crate::handlers::RelatedTestsParams =
                    serde_json::from_value(args.clone()).map_err(|e| {
                        tower_lsp::jsonrpc::Error::invalid_params(format!("Invalid params: {e}"))
                    })?;
                let response = self.handle_find_related_tests(params).await?;
                Ok(Some(serde_json::to_value(response).unwrap()))
            }

            "codegraph.getNodeLocation" => {
                let args = params.arguments.first().ok_or_else(|| {
                    tower_lsp::jsonrpc::Error::invalid_params("Missing arguments")
                })?;
                let params: crate::handlers::GetNodeLocationParams =
                    serde_json::from_value(args.clone()).map_err(|e| {
                        tower_lsp::jsonrpc::Error::invalid_params(format!("Invalid params: {e}"))
                    })?;
                let response = self.handle_get_node_location(params).await?;
                Ok(Some(serde_json::to_value(response).unwrap()))
            }

            "codegraph.getWorkspaceSymbols" => {
                let args = params.arguments.first().ok_or_else(|| {
                    tower_lsp::jsonrpc::Error::invalid_params("Missing arguments")
                })?;
                let params: crate::handlers::WorkspaceSymbolsParams =
                    serde_json::from_value(args.clone()).map_err(|e| {
                        tower_lsp::jsonrpc::Error::invalid_params(format!("Invalid params: {e}"))
                    })?;
                let response = self.handle_get_workspace_symbols(params).await?;
                Ok(Some(serde_json::to_value(response).unwrap()))
            }

            "codegraph.analyzeComplexity" => {
                let args = params.arguments.first().ok_or_else(|| {
                    tower_lsp::jsonrpc::Error::invalid_params("Missing arguments")
                })?;
                let params: crate::handlers::ComplexityParams =
                    serde_json::from_value(args.clone()).map_err(|e| {
                        tower_lsp::jsonrpc::Error::invalid_params(format!("Invalid params: {e}"))
                    })?;
                let response = self.handle_analyze_complexity(params).await?;
                Ok(Some(serde_json::to_value(response).unwrap()))
            }

            // AI Agent Query Primitives
            "codegraph.symbolSearch" => {
                let args = params.arguments.first().ok_or_else(|| {
                    tower_lsp::jsonrpc::Error::invalid_params("Missing arguments")
                })?;
                let params: crate::handlers::SymbolSearchParams =
                    serde_json::from_value(args.clone()).map_err(|e| {
                        tower_lsp::jsonrpc::Error::invalid_params(format!("Invalid params: {e}"))
                    })?;
                let response = self.handle_symbol_search(params).await?;
                Ok(Some(serde_json::to_value(response).unwrap()))
            }

            "codegraph.findByImports" => {
                let args = params.arguments.first().ok_or_else(|| {
                    tower_lsp::jsonrpc::Error::invalid_params("Missing arguments")
                })?;
                let params: crate::handlers::FindByImportsParams =
                    serde_json::from_value(args.clone()).map_err(|e| {
                        tower_lsp::jsonrpc::Error::invalid_params(format!("Invalid params: {e}"))
                    })?;
                let response = self.handle_find_by_imports(params).await?;
                Ok(Some(serde_json::to_value(response).unwrap()))
            }

            "codegraph.findEntryPoints" => {
                let args = params.arguments.first().ok_or_else(|| {
                    tower_lsp::jsonrpc::Error::invalid_params("Missing arguments")
                })?;
                let params: crate::handlers::FindEntryPointsParams =
                    serde_json::from_value(args.clone()).map_err(|e| {
                        tower_lsp::jsonrpc::Error::invalid_params(format!("Invalid params: {e}"))
                    })?;
                let response = self.handle_find_entry_points(params).await?;
                Ok(Some(serde_json::to_value(response).unwrap()))
            }

            "codegraph.traverseGraph" => {
                let args = params.arguments.first().ok_or_else(|| {
                    tower_lsp::jsonrpc::Error::invalid_params("Missing arguments")
                })?;
                let params: crate::handlers::TraverseGraphParams =
                    serde_json::from_value(args.clone()).map_err(|e| {
                        tower_lsp::jsonrpc::Error::invalid_params(format!("Invalid params: {e}"))
                    })?;
                let response = self.handle_traverse_graph(params).await?;
                Ok(Some(serde_json::to_value(response).unwrap()))
            }

            "codegraph.getCallers" => {
                let args = params.arguments.first().ok_or_else(|| {
                    tower_lsp::jsonrpc::Error::invalid_params("Missing arguments")
                })?;
                let params: crate::handlers::GetCallersParams =
                    serde_json::from_value(args.clone()).map_err(|e| {
                        tower_lsp::jsonrpc::Error::invalid_params(format!("Invalid params: {e}"))
                    })?;
                let response = self.handle_get_callers(params).await?;
                Ok(Some(serde_json::to_value(response).unwrap()))
            }

            "codegraph.getCallees" => {
                let args = params.arguments.first().ok_or_else(|| {
                    tower_lsp::jsonrpc::Error::invalid_params("Missing arguments")
                })?;
                let params: crate::handlers::GetCallersParams =
                    serde_json::from_value(args.clone()).map_err(|e| {
                        tower_lsp::jsonrpc::Error::invalid_params(format!("Invalid params: {e}"))
                    })?;
                let response = self.handle_get_callees(params).await?;
                Ok(Some(serde_json::to_value(response).unwrap()))
            }

            "codegraph.getDetailedSymbolInfo" => {
                let args = params.arguments.first().ok_or_else(|| {
                    tower_lsp::jsonrpc::Error::invalid_params("Missing arguments")
                })?;
                let params: crate::handlers::GetDetailedInfoParams =
                    serde_json::from_value(args.clone()).map_err(|e| {
                        tower_lsp::jsonrpc::Error::invalid_params(format!("Invalid params: {e}"))
                    })?;
                let response = self.handle_get_detailed_symbol_info(params).await?;
                Ok(Some(serde_json::to_value(response).unwrap()))
            }

            "codegraph.findBySignature" => {
                let args = params.arguments.first().ok_or_else(|| {
                    tower_lsp::jsonrpc::Error::invalid_params("Missing arguments")
                })?;
                let params: crate::handlers::FindBySignatureParams =
                    serde_json::from_value(args.clone()).map_err(|e| {
                        tower_lsp::jsonrpc::Error::invalid_params(format!("Invalid params: {e}"))
                    })?;
                let response = self.handle_find_by_signature(params).await?;
                Ok(Some(serde_json::to_value(response).unwrap()))
            }

            // Memory Layer Commands
            "codegraph.memoryStore" => {
                let args = params.arguments.first().ok_or_else(|| {
                    tower_lsp::jsonrpc::Error::invalid_params("Missing arguments")
                })?;
                let params: crate::handlers::MemoryStoreParams =
                    serde_json::from_value(args.clone()).map_err(|e| {
                        tower_lsp::jsonrpc::Error::invalid_params(format!("Invalid params: {e}"))
                    })?;
                let response = self.handle_memory_store(params).await?;
                Ok(Some(serde_json::to_value(response).unwrap()))
            }

            "codegraph.memorySearch" => {
                let args = params.arguments.first().ok_or_else(|| {
                    tower_lsp::jsonrpc::Error::invalid_params("Missing arguments")
                })?;
                let params: crate::handlers::MemorySearchParams =
                    serde_json::from_value(args.clone()).map_err(|e| {
                        tower_lsp::jsonrpc::Error::invalid_params(format!("Invalid params: {e}"))
                    })?;
                let response = self.handle_memory_search(params).await?;
                Ok(Some(serde_json::to_value(response).unwrap()))
            }

            "codegraph.memoryGet" => {
                let args = params.arguments.first().ok_or_else(|| {
                    tower_lsp::jsonrpc::Error::invalid_params("Missing arguments")
                })?;
                let params: crate::handlers::MemoryGetParams = serde_json::from_value(args.clone())
                    .map_err(|e| {
                        tower_lsp::jsonrpc::Error::invalid_params(format!("Invalid params: {e}"))
                    })?;
                let response = self.handle_memory_get(params).await?;
                Ok(Some(serde_json::to_value(response).unwrap()))
            }

            "codegraph.memoryInvalidate" => {
                let args = params.arguments.first().ok_or_else(|| {
                    tower_lsp::jsonrpc::Error::invalid_params("Missing arguments")
                })?;
                let params: crate::handlers::MemoryInvalidateParams =
                    serde_json::from_value(args.clone()).map_err(|e| {
                        tower_lsp::jsonrpc::Error::invalid_params(format!("Invalid params: {e}"))
                    })?;
                let response = self.handle_memory_invalidate(params).await?;
                Ok(Some(serde_json::to_value(response).unwrap()))
            }

            "codegraph.memoryList" => {
                let args = params
                    .arguments
                    .first()
                    .cloned()
                    .unwrap_or(serde_json::json!({}));
                let params: crate::handlers::MemoryListParams = serde_json::from_value(args)
                    .map_err(|e| {
                        tower_lsp::jsonrpc::Error::invalid_params(format!("Invalid params: {e}"))
                    })?;
                let response = self.handle_memory_list(params).await?;
                Ok(Some(serde_json::to_value(response).unwrap()))
            }

            "codegraph.memoryUpdate" => {
                let args = params.arguments.first().ok_or_else(|| {
                    tower_lsp::jsonrpc::Error::invalid_params("Missing arguments")
                })?;
                let params: crate::handlers::MemoryUpdateParams =
                    serde_json::from_value(args.clone()).map_err(|e| {
                        tower_lsp::jsonrpc::Error::invalid_params(format!("Invalid params: {e}"))
                    })?;
                let response = self.handle_memory_update(params).await?;
                Ok(Some(serde_json::to_value(response).unwrap()))
            }

            "codegraph.memoryContext" => {
                let args = params.arguments.first().ok_or_else(|| {
                    tower_lsp::jsonrpc::Error::invalid_params("Missing arguments")
                })?;
                let params: crate::handlers::MemoryContextParams =
                    serde_json::from_value(args.clone()).map_err(|e| {
                        tower_lsp::jsonrpc::Error::invalid_params(format!("Invalid params: {e}"))
                    })?;
                let response = self.handle_memory_context(params).await?;
                Ok(Some(serde_json::to_value(response).unwrap()))
            }

            "codegraph.memoryStats" => {
                let response = self.handle_memory_stats().await?;
                Ok(Some(response))
            }

            // Git mining commands
            "codegraph.mineGitHistory" => {
                let args = params.arguments.first().ok_or_else(|| {
                    tower_lsp::jsonrpc::Error::invalid_params("Missing arguments")
                })?;
                let params: serde_json::Value = serde_json::from_value(args.clone())
                    .map_err(|e| tower_lsp::jsonrpc::Error::invalid_params(e.to_string()))?;
                let response = self.handle_mine_git_history(params).await?;
                Ok(Some(response))
            }

            "codegraph.mineGitHistoryForFile" => {
                let args = params.arguments.first().ok_or_else(|| {
                    tower_lsp::jsonrpc::Error::invalid_params("Missing arguments")
                })?;
                let file_params: serde_json::Value = serde_json::from_value(args.clone())
                    .map_err(|e| tower_lsp::jsonrpc::Error::invalid_params(e.to_string()))?;
                let response = self.handle_mine_git_history_for_file(file_params).await?;
                Ok(Some(response))
            }

            "codegraph.searchGitHistory" => {
                let args = params.arguments.first().ok_or_else(|| {
                    tower_lsp::jsonrpc::Error::invalid_params("Missing arguments")
                })?;
                let search_params: serde_json::Value = serde_json::from_value(args.clone())
                    .map_err(|e| tower_lsp::jsonrpc::Error::invalid_params(e.to_string()))?;
                let response = self.handle_search_git_history(search_params).await?;
                Ok(Some(response))
            }

            "codegraph.updateConfiguration" => {
                let args = params.arguments.first().ok_or_else(|| {
                    tower_lsp::jsonrpc::Error::invalid_params("Missing arguments")
                })?;
                let new_config: CodeGraphConfig =
                    serde_json::from_value(args.clone()).map_err(|e| {
                        tower_lsp::jsonrpc::Error::invalid_params(format!("Invalid config: {e}"))
                    })?;
                tracing::info!("Configuration updated: {:?}", new_config);
                *self.config.write().await = new_config;
                self.client
                    .log_message(MessageType::INFO, "Configuration updated")
                    .await;
                Ok(Some(serde_json::json!({"status": "ok"})))
            }

            other => {
                // Fall through to pro command provider
                let args = params
                    .arguments
                    .first()
                    .cloned()
                    .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
                let ctx = crate::lsp_pro_hooks::ProCommandContext {
                    graph: Arc::clone(&self.graph),
                    query_engine: Arc::clone(&self.query_engine),
                    memory_manager: Arc::clone(&self.memory_manager),
                    workspace_folders: self.workspace_folders.read().await.clone(),
                };
                if let Some(future) = self.pro_commands.handle_command(other, args, ctx) {
                    match future.await {
                        Ok(result) => Ok(result),
                        Err(e) => Err(tower_lsp::jsonrpc::Error::invalid_params(e)),
                    }
                } else {
                    Err(tower_lsp::jsonrpc::Error::method_not_found())
                }
            }
        }
    }
}

impl CodeGraphBackend {
    /// Extract node ID from CallHierarchyItem data.
    fn extract_node_id_from_item(&self, item: &CallHierarchyItem) -> Result<NodeId> {
        let data = item
            .data
            .as_ref()
            .ok_or_else(|| tower_lsp::jsonrpc::Error::invalid_params("Missing data"))?;

        let node_id_str = data
            .get("nodeId")
            .and_then(|v| v.as_str())
            .ok_or_else(|| tower_lsp::jsonrpc::Error::invalid_params("Missing nodeId"))?;

        node_id_str
            .parse::<NodeId>()
            .map_err(|_| tower_lsp::jsonrpc::Error::invalid_params("Invalid nodeId"))
    }
}

// ==========================================
// Memory Layer Handlers
// ==========================================

impl CodeGraphBackend {
    /// Store a new memory in the memory layer.
    pub async fn handle_memory_store(
        &self,
        params: crate::handlers::MemoryStoreParams,
    ) -> Result<crate::handlers::MemoryStoreResponse> {
        use crate::memory::{LinkedNodeType, MemoryNode};

        // Parse the memory kind using the builder pattern for convenience
        let mut builder = MemoryNode::builder();

        builder = match params.kind.as_str() {
            "debug_context" => {
                let problem = params
                    .kind_data
                    .get("problem")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let solution = params
                    .kind_data
                    .get("solution")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                builder.debug_context(problem, solution)
            }
            "architectural_decision" => {
                let decision = params
                    .kind_data
                    .get("decision")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let rationale = params
                    .kind_data
                    .get("rationale")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                builder.architectural_decision(decision, rationale)
            }
            "known_issue" => {
                let description = params
                    .kind_data
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let severity = match params
                    .kind_data
                    .get("severity")
                    .and_then(|v| v.as_str())
                    .unwrap_or("medium")
                {
                    "critical" => crate::memory::IssueSeverity::Critical,
                    "high" => crate::memory::IssueSeverity::High,
                    "medium" => crate::memory::IssueSeverity::Medium,
                    "low" => crate::memory::IssueSeverity::Low,
                    _ => crate::memory::IssueSeverity::Medium,
                };
                builder.known_issue(description, severity)
            }
            "convention" => {
                let name = params
                    .kind_data
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let description = params
                    .kind_data
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                builder.convention(name, description)
            }
            "project_context" => {
                let topic = params
                    .kind_data
                    .get("topic")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let description = params
                    .kind_data
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                builder.project_context(topic, description)
            }
            _ => {
                return Err(tower_lsp::jsonrpc::Error::invalid_params(format!(
                    "Unknown memory kind: {}",
                    params.kind
                )))
            }
        };

        // Set title and content
        builder = builder.title(&params.title).content(&params.content);

        // Add tags
        for tag in params.tags {
            builder = builder.tag(&tag);
        }

        // Add code links
        for link in params.code_links {
            let node_type = match link.node_type.as_str() {
                "function" => LinkedNodeType::Function,
                "class" => LinkedNodeType::Class,
                "module" => LinkedNodeType::Module,
                "file" => LinkedNodeType::File,
                "variable" => LinkedNodeType::Variable,
                "import" => LinkedNodeType::Import,
                "interface" => LinkedNodeType::Interface,
                "trait" => LinkedNodeType::Trait,
                _ => LinkedNodeType::Function, // Default fallback
            };
            builder = builder.link_to_code(link.node_id, node_type);
        }

        // Set confidence if provided
        if let Some(conf) = params.confidence {
            builder = builder.confidence(conf);
        }

        let memory = builder.build().map_err(|e| {
            tower_lsp::jsonrpc::Error::invalid_params(format!("Failed to build memory: {e}"))
        })?;

        // Store the memory
        let id = self
            .memory_manager
            .put(memory)
            .await
            .map_err(|_| tower_lsp::jsonrpc::Error::internal_error())?;

        Ok(crate::handlers::MemoryStoreResponse { id, success: true })
    }

    /// Search memories using hybrid search.
    pub async fn handle_memory_search(
        &self,
        params: crate::handlers::MemorySearchParams,
    ) -> Result<crate::handlers::MemorySearchResponse> {
        use crate::memory::{MemoryKindFilter, SearchConfig};

        // Build search config
        let mut config = SearchConfig {
            limit: params.limit,
            current_only: params.current_only,
            ..Default::default()
        };

        // Set tag filter (tags is Vec, not Option)
        if !params.tags.is_empty() {
            config.tags = params.tags;
        }

        // Set kind filter (kinds is Vec, not Option)
        if !params.kinds.is_empty() {
            config.kinds = params
                .kinds
                .iter()
                .filter_map(|k| match k.as_str() {
                    "debug_context" => Some(MemoryKindFilter::DebugContext),
                    "architectural_decision" => Some(MemoryKindFilter::ArchitecturalDecision),
                    "known_issue" => Some(MemoryKindFilter::KnownIssue),
                    "convention" => Some(MemoryKindFilter::Convention),
                    "project_context" => Some(MemoryKindFilter::ProjectContext),
                    _ => None,
                })
                .collect();
        }

        // Perform search
        let results = self
            .memory_manager
            .search(&params.query, &config, &params.code_context)
            .await
            .map_err(|_| tower_lsp::jsonrpc::Error::internal_error())?;

        let total = results.len();
        let search_results: Vec<crate::handlers::MemorySearchResult> = results
            .into_iter()
            .map(|r| {
                let kind_str = match &r.memory.kind {
                    crate::memory::MemoryKind::DebugContext { .. } => "debug_context",
                    crate::memory::MemoryKind::ArchitecturalDecision { .. } => {
                        "architectural_decision"
                    }
                    crate::memory::MemoryKind::KnownIssue { .. } => "known_issue",
                    crate::memory::MemoryKind::Convention { .. } => "convention",
                    crate::memory::MemoryKind::ProjectContext { .. } => "project_context",
                };

                crate::handlers::MemorySearchResult {
                    id: r.memory.id.to_string(),
                    kind: kind_str.to_string(),
                    title: r.memory.title.clone(),
                    content: r.memory.content.clone(),
                    tags: r.memory.tags.clone(),
                    score: r.score,
                    is_current: r.memory.is_current(),
                }
            })
            .collect();

        Ok(crate::handlers::MemorySearchResponse {
            results: search_results,
            total,
        })
    }

    /// Get a memory by ID.
    pub async fn handle_memory_get(
        &self,
        params: crate::handlers::MemoryGetParams,
    ) -> Result<Option<crate::handlers::MemoryGetResponse>> {
        let memory = self
            .memory_manager
            .get(&params.id)
            .await
            .map_err(|_| tower_lsp::jsonrpc::Error::internal_error())?;

        let response = memory.map(|m| {
            let kind_json = match &m.kind {
                crate::memory::MemoryKind::DebugContext {
                    problem_description,
                    root_cause,
                    solution,
                    symptoms,
                    related_errors,
                } => {
                    serde_json::json!({
                        "type": "debug_context",
                        "problem_description": problem_description,
                        "root_cause": root_cause,
                        "solution": solution,
                        "symptoms": symptoms,
                        "related_errors": related_errors
                    })
                }
                crate::memory::MemoryKind::ArchitecturalDecision {
                    decision,
                    rationale,
                    alternatives_considered,
                    stakeholders,
                } => {
                    serde_json::json!({
                        "type": "architectural_decision",
                        "decision": decision,
                        "rationale": rationale,
                        "alternatives_considered": alternatives_considered,
                        "stakeholders": stakeholders
                    })
                }
                crate::memory::MemoryKind::KnownIssue {
                    description,
                    severity,
                    workaround,
                    tracking_id,
                } => {
                    serde_json::json!({
                        "type": "known_issue",
                        "description": description,
                        "severity": format!("{:?}", severity).to_lowercase(),
                        "workaround": workaround,
                        "tracking_id": tracking_id
                    })
                }
                crate::memory::MemoryKind::Convention {
                    name,
                    description,
                    pattern,
                    anti_pattern,
                } => {
                    serde_json::json!({
                        "type": "convention",
                        "name": name,
                        "description": description,
                        "pattern": pattern,
                        "anti_pattern": anti_pattern
                    })
                }
                crate::memory::MemoryKind::ProjectContext {
                    topic,
                    description,
                    tags,
                } => {
                    serde_json::json!({
                        "type": "project_context",
                        "topic": topic,
                        "description": description,
                        "tags": tags
                    })
                }
            };

            let code_links: Vec<crate::handlers::CodeLinkResponse> = m
                .code_links
                .iter()
                .map(|link| crate::handlers::CodeLinkResponse {
                    node_id: link.node_id.clone(),
                    node_type: format!("{:?}", link.node_type).to_lowercase(),
                })
                .collect();

            crate::handlers::MemoryGetResponse {
                id: m.id.to_string(),
                kind: kind_json,
                title: m.title.clone(),
                content: m.content.clone(),
                tags: m.tags.clone(),
                code_links,
                confidence: m.confidence,
                is_current: m.is_current(),
                created_at: m.temporal.created_at.to_rfc3339(),
                valid_from: m.temporal.valid_at.to_rfc3339().into(),
            }
        });

        Ok(response)
    }

    /// Invalidate a memory by ID.
    pub async fn handle_memory_invalidate(
        &self,
        params: crate::handlers::MemoryInvalidateParams,
    ) -> Result<crate::handlers::MemoryInvalidateResponse> {
        self.memory_manager
            .invalidate(&params.id, "Invalidated via LSP command")
            .await
            .map_err(|_| tower_lsp::jsonrpc::Error::internal_error())?;

        Ok(crate::handlers::MemoryInvalidateResponse { success: true })
    }

    /// List memories with optional filters.
    pub async fn handle_memory_list(
        &self,
        params: crate::handlers::MemoryListParams,
    ) -> Result<crate::handlers::MemoryListResponse> {
        // Get all current memories
        let all_memories = self
            .memory_manager
            .get_all_current()
            .await
            .map_err(|_| tower_lsp::jsonrpc::Error::internal_error())?;

        // Apply filters
        let filtered: Vec<_> = all_memories
            .into_iter()
            .filter(|m| {
                // Filter by current_only
                if params.current_only && !m.is_current() {
                    return false;
                }

                // Filter by kinds
                if !params.kinds.is_empty() {
                    let kind_str = match &m.kind {
                        crate::memory::MemoryKind::DebugContext { .. } => "debug_context",
                        crate::memory::MemoryKind::ArchitecturalDecision { .. } => {
                            "architectural_decision"
                        }
                        crate::memory::MemoryKind::KnownIssue { .. } => "known_issue",
                        crate::memory::MemoryKind::Convention { .. } => "convention",
                        crate::memory::MemoryKind::ProjectContext { .. } => "project_context",
                    };
                    if !params.kinds.contains(&kind_str.to_string()) {
                        return false;
                    }
                }

                // Filter by tags
                if !params.tags.is_empty() && !params.tags.iter().any(|t| m.tags.contains(t)) {
                    return false;
                }

                true
            })
            .collect();

        let total = filtered.len();
        let has_more = params.offset + params.limit < total;

        // Apply pagination
        let paginated: Vec<crate::handlers::MemorySearchResult> = filtered
            .into_iter()
            .skip(params.offset)
            .take(params.limit)
            .map(|m| {
                let kind_str = match &m.kind {
                    crate::memory::MemoryKind::DebugContext { .. } => "debug_context",
                    crate::memory::MemoryKind::ArchitecturalDecision { .. } => {
                        "architectural_decision"
                    }
                    crate::memory::MemoryKind::KnownIssue { .. } => "known_issue",
                    crate::memory::MemoryKind::Convention { .. } => "convention",
                    crate::memory::MemoryKind::ProjectContext { .. } => "project_context",
                };

                crate::handlers::MemorySearchResult {
                    id: m.id.to_string(),
                    kind: kind_str.to_string(),
                    title: m.title.clone(),
                    content: m.content.clone(),
                    tags: m.tags.clone(),
                    score: m.confidence,
                    is_current: m.is_current(),
                }
            })
            .collect();

        Ok(crate::handlers::MemoryListResponse {
            memories: paginated,
            total,
            has_more,
        })
    }

    /// Update an existing memory.
    pub async fn handle_memory_update(
        &self,
        params: crate::handlers::MemoryUpdateParams,
    ) -> Result<crate::handlers::MemoryUpdateResponse> {
        use crate::memory::{CodeLink, LinkedNodeType};

        // Get existing memory
        let existing = self
            .memory_manager
            .get(&params.id)
            .await
            .map_err(|_| tower_lsp::jsonrpc::Error::internal_error())?;

        let Some(mut memory) = existing else {
            return Ok(crate::handlers::MemoryUpdateResponse {
                success: false,
                memory: None,
            });
        };

        // Apply updates
        if let Some(title) = params.title {
            memory.title = title;
        }
        if let Some(content) = params.content {
            memory.content = content;
        }
        if let Some(tags) = params.tags {
            memory.tags = tags;
        }
        if let Some(confidence) = params.confidence {
            memory.confidence = confidence;
        }

        // Handle code link additions
        for link_param in params.add_code_links {
            let node_type = match link_param.node_type.as_str() {
                "function" => LinkedNodeType::Function,
                "class" => LinkedNodeType::Class,
                "module" => LinkedNodeType::Module,
                "file" => LinkedNodeType::File,
                "variable" => LinkedNodeType::Variable,
                "import" => LinkedNodeType::Import,
                "interface" => LinkedNodeType::Interface,
                "trait" => LinkedNodeType::Trait,
                _ => LinkedNodeType::Function, // Default fallback
            };
            memory
                .code_links
                .push(CodeLink::new(link_param.node_id, node_type));
        }

        // Handle code link removals
        for node_id in params.remove_code_links {
            memory.code_links.retain(|link| link.node_id != node_id);
        }

        // Clear embeddings so they get regenerated
        memory.embedding = None;

        // Store updated memory
        let id = self
            .memory_manager
            .put(memory)
            .await
            .map_err(|_| tower_lsp::jsonrpc::Error::internal_error())?;

        // Get the updated memory for response
        let updated = self
            .memory_manager
            .get(&id)
            .await
            .map_err(|_| tower_lsp::jsonrpc::Error::internal_error())?;

        let response_memory = updated.map(|m| {
            let kind_json = match &m.kind {
                crate::memory::MemoryKind::DebugContext {
                    problem_description,
                    root_cause,
                    solution,
                    symptoms,
                    related_errors,
                } => {
                    serde_json::json!({
                        "type": "debug_context",
                        "problem_description": problem_description,
                        "root_cause": root_cause,
                        "solution": solution,
                        "symptoms": symptoms,
                        "related_errors": related_errors
                    })
                }
                crate::memory::MemoryKind::ArchitecturalDecision {
                    decision,
                    rationale,
                    alternatives_considered,
                    stakeholders,
                } => {
                    serde_json::json!({
                        "type": "architectural_decision",
                        "decision": decision,
                        "rationale": rationale,
                        "alternatives_considered": alternatives_considered,
                        "stakeholders": stakeholders
                    })
                }
                crate::memory::MemoryKind::KnownIssue {
                    description,
                    severity,
                    workaround,
                    tracking_id,
                } => {
                    serde_json::json!({
                        "type": "known_issue",
                        "description": description,
                        "severity": format!("{:?}", severity).to_lowercase(),
                        "workaround": workaround,
                        "tracking_id": tracking_id
                    })
                }
                crate::memory::MemoryKind::Convention {
                    name,
                    description,
                    pattern,
                    anti_pattern,
                } => {
                    serde_json::json!({
                        "type": "convention",
                        "name": name,
                        "description": description,
                        "pattern": pattern,
                        "anti_pattern": anti_pattern
                    })
                }
                crate::memory::MemoryKind::ProjectContext {
                    topic,
                    description,
                    tags,
                } => {
                    serde_json::json!({
                        "type": "project_context",
                        "topic": topic,
                        "description": description,
                        "tags": tags
                    })
                }
            };

            let code_links: Vec<crate::handlers::CodeLinkResponse> = m
                .code_links
                .iter()
                .map(|link| crate::handlers::CodeLinkResponse {
                    node_id: link.node_id.clone(),
                    node_type: format!("{:?}", link.node_type).to_lowercase(),
                })
                .collect();

            crate::handlers::MemoryGetResponse {
                id: m.id.to_string(),
                kind: kind_json,
                title: m.title.clone(),
                content: m.content.clone(),
                tags: m.tags.clone(),
                code_links,
                confidence: m.confidence,
                is_current: m.is_current(),
                created_at: m.temporal.created_at.to_rfc3339(),
                valid_from: m.temporal.valid_at.to_rfc3339().into(),
            }
        });

        Ok(crate::handlers::MemoryUpdateResponse {
            success: true,
            memory: response_memory,
        })
    }

    /// Get memories relevant to a code context.
    pub async fn handle_memory_context(
        &self,
        params: crate::handlers::MemoryContextParams,
    ) -> Result<crate::handlers::MemoryContextResponse> {
        use crate::memory::SearchConfig;

        // Parse URI and find code context
        let uri = Url::parse(&params.uri)
            .map_err(|_| tower_lsp::jsonrpc::Error::invalid_params("Invalid URI"))?;

        let path = uri
            .to_file_path()
            .map_err(|_| tower_lsp::jsonrpc::Error::invalid_params("Invalid file path"))?;

        // Build code context from the graph
        let mut code_context = Vec::new();
        let graph = self.graph.read().await;

        // Get file node
        let path_str = path.to_string_lossy().to_string();
        if let Ok(file_nodes) = graph.query().property("path", path_str).execute() {
            for node_id in file_nodes {
                code_context.push(node_id.to_string());
            }
        }

        // If position provided, find node at position
        if let Some(pos) = params.position {
            let position = Position {
                line: pos.line,
                character: pos.character,
            };
            if let Ok(Some(node_id)) = self.find_node_at_position(&graph, &path, position) {
                code_context.push(node_id.to_string());
            }
        }

        drop(graph);

        // Search with code context for graph proximity scoring
        let mut config = SearchConfig {
            limit: params.limit,
            current_only: true,
            ..Default::default()
        };

        // Set kind filter if provided (kinds is Vec, not Option)
        if !params.kinds.is_empty() {
            config.kinds = params
                .kinds
                .iter()
                .filter_map(|k| match k.as_str() {
                    "debug_context" => Some(crate::memory::MemoryKindFilter::DebugContext),
                    "architectural_decision" => {
                        Some(crate::memory::MemoryKindFilter::ArchitecturalDecision)
                    }
                    "known_issue" => Some(crate::memory::MemoryKindFilter::KnownIssue),
                    "convention" => Some(crate::memory::MemoryKindFilter::Convention),
                    "project_context" => Some(crate::memory::MemoryKindFilter::ProjectContext),
                    _ => None,
                })
                .collect();
        }

        // Use file name as query for semantic relevance
        let query = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let results = self
            .memory_manager
            .search(&query, &config, &code_context)
            .await
            .map_err(|_| tower_lsp::jsonrpc::Error::internal_error())?;

        let memories: Vec<crate::handlers::ContextMemory> = results
            .into_iter()
            .map(|r| {
                let kind_str = match &r.memory.kind {
                    crate::memory::MemoryKind::DebugContext { .. } => "debug_context",
                    crate::memory::MemoryKind::ArchitecturalDecision { .. } => {
                        "architectural_decision"
                    }
                    crate::memory::MemoryKind::KnownIssue { .. } => "known_issue",
                    crate::memory::MemoryKind::Convention { .. } => "convention",
                    crate::memory::MemoryKind::ProjectContext { .. } => "project_context",
                };

                let reason = r
                    .match_reasons
                    .first()
                    .map(|mr| format!("{:?}", mr))
                    .unwrap_or_else(|| "Related to code context".to_string());

                crate::handlers::ContextMemory {
                    id: r.memory.id.to_string(),
                    kind: kind_str.to_string(),
                    title: r.memory.title.clone(),
                    content: r.memory.content.clone(),
                    tags: r.memory.tags.clone(),
                    relevance_score: r.score,
                    relevance_reason: reason,
                }
            })
            .collect();

        Ok(crate::handlers::MemoryContextResponse { memories })
    }

    /// Get memory store statistics.
    pub async fn handle_memory_stats(&self) -> Result<serde_json::Value> {
        let stats = self
            .memory_manager
            .stats()
            .await
            .map_err(|_| tower_lsp::jsonrpc::Error::internal_error())?;

        Ok(stats)
    }

    /// Mine git history and create memories from relevant commits.
    pub async fn handle_mine_git_history(
        &self,
        params: serde_json::Value,
    ) -> Result<serde_json::Value> {
        use crate::git_mining::{GitMiner, MiningConfig};

        // Get workspace folder
        let workspace_folders = self.workspace_folders.read().await;
        let workspace_path = workspace_folders
            .first()
            .ok_or_else(tower_lsp::jsonrpc::Error::invalid_request)?;

        // Parse configuration from params
        let max_commits = params
            .get("maxCommits")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(500);

        let min_confidence = params
            .get("minConfidence")
            .and_then(|v| v.as_f64())
            .map(|v| v as f32)
            .unwrap_or(0.7);

        let mine_bug_fixes = params
            .get("mineBugFixes")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let mine_arch_decisions = params
            .get("mineArchDecisions")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let mine_breaking_changes = params
            .get("mineBreakingChanges")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let mine_reverts = params
            .get("mineReverts")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let mine_features = params
            .get("mineFeatures")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let mine_deprecations = params
            .get("mineDeprecations")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let include_hotspots = params
            .get("includeHotspots")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let include_coupling = params
            .get("includeCoupling")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let config = MiningConfig {
            max_commits,
            min_confidence,
            mine_bug_fixes,
            mine_arch_decisions,
            mine_breaking_changes,
            mine_reverts,
            mine_features,
            mine_deprecations,
        };

        // Create miner and run
        let miner = GitMiner::new(workspace_path)
            .map_err(|_| tower_lsp::jsonrpc::Error::invalid_request())?;

        let mut result = miner
            .mine_repository(&self.memory_manager, &self.graph, &config)
            .await
            .map_err(|e| {
                tracing::error!("Git mining failed: {}", e);
                tower_lsp::jsonrpc::Error::internal_error()
            })?;

        // Detect hotspots if requested
        let mut hotspots_created = 0;
        if include_hotspots {
            match miner.detect_hotspots(10).await {
                Ok(hotspots) => {
                    for hotspot in hotspots.iter().take(20) {
                        // Create memory for each hotspot
                        let memory = codegraph_memory::MemoryNode::builder()
                            .project_context(
                                format!("High-activity file: {}", hotspot.file_path),
                                format!(
                                    "Modified {} times across {} commits. This file shows high churn, \
                                     indicating active development or potential complexity.",
                                    hotspot.change_count, hotspot.unique_commits
                                ),
                            )
                            .title(format!("Hotspot: {}", hotspot.file_path))
                            .content(format!(
                                "**Change Count:** {}\n**Unique Commits:** {}\n**Recent Changes:**\n{}",
                                hotspot.change_count,
                                hotspot.unique_commits,
                                hotspot.recent_changes.join("\n- ")
                            ))
                            .tag("hotspot")
                            .tag("git-mined")
                            .confidence(0.7)
                            .build()
                            .ok();

                        if let Some(m) = memory {
                            if let Ok(id) = self.memory_manager.put(m).await {
                                result.memory_ids.push(id);
                                hotspots_created += 1;
                            }
                        }
                    }
                }
                Err(e) => {
                    result
                        .warnings
                        .push(format!("Failed to detect hotspots: {}", e));
                }
            }
        }

        // Detect coupling if requested
        let mut couplings_created = 0;
        if include_coupling {
            match miner.detect_coupling(0.7).await {
                Ok(couplings) => {
                    for coupling in couplings.iter().take(15) {
                        // Create convention memory for strong couplings
                        let memory = codegraph_memory::MemoryNode::builder()
                            .convention(
                                format!("Co-change: {} ↔ {}", coupling.file_a, coupling.file_b),
                                format!(
                                    "These files change together {:.0}% of the time ({} of {} changes). \
                                     When modifying one, consider checking the other.",
                                    coupling.coupling_strength * 100.0,
                                    coupling.co_change_count,
                                    coupling.total_changes
                                ),
                            )
                            .title(format!("Coupling: {} ↔ {}", 
                                coupling.file_a.split('/').next_back().unwrap_or(&coupling.file_a),
                                coupling.file_b.split('/').next_back().unwrap_or(&coupling.file_b)))
                            .content(format!(
                                "**File A:** {}\n**File B:** {}\n**Coupling Strength:** {:.1}%\n\
                                 **Co-changes:** {} out of {} total changes",
                                coupling.file_a,
                                coupling.file_b,
                                coupling.coupling_strength * 100.0,
                                coupling.co_change_count,
                                coupling.total_changes
                            ))
                            .tag("coupling")
                            .tag("git-mined")
                            .confidence(coupling.coupling_strength)
                            .build()
                            .ok();

                        if let Some(m) = memory {
                            if let Ok(id) = self.memory_manager.put(m).await {
                                result.memory_ids.push(id);
                                couplings_created += 1;
                            }
                        }
                    }
                }
                Err(e) => {
                    result
                        .warnings
                        .push(format!("Failed to detect coupling: {}", e));
                }
            }
        }

        Ok(serde_json::json!({
            "commitsProcessed": result.commits_processed,
            "memoriesCreated": result.memories_created + hotspots_created + couplings_created,
            "commitsSkipped": result.commits_skipped,
            "memoryIds": result.memory_ids,
            "warnings": result.warnings,
            "hotspotsDetected": hotspots_created,
            "couplingsDetected": couplings_created,
        }))
    }

    /// Mine git history for a specific file.
    pub async fn handle_mine_git_history_for_file(
        &self,
        params: serde_json::Value,
    ) -> Result<serde_json::Value> {
        use crate::git_mining::{GitMiner, MiningConfig};

        // Get file path from params
        let uri_str = params
            .get("uri")
            .and_then(|v| v.as_str())
            .ok_or_else(|| tower_lsp::jsonrpc::Error::invalid_params("Missing uri parameter"))?;

        let file_path = Url::parse(uri_str)
            .ok()
            .and_then(|url| url.to_file_path().ok())
            .ok_or_else(|| tower_lsp::jsonrpc::Error::invalid_params("Invalid file URI"))?;

        // Get workspace folder
        let workspace_folders = self.workspace_folders.read().await;
        let workspace_path = workspace_folders
            .first()
            .ok_or_else(tower_lsp::jsonrpc::Error::invalid_request)?;

        // Parse configuration from params
        let max_commits = params
            .get("maxCommits")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(100);

        let config = MiningConfig {
            max_commits,
            min_confidence: 0.7,
            ..Default::default()
        };

        // Create miner and run for specific file
        let miner = GitMiner::new(workspace_path)
            .map_err(|_| tower_lsp::jsonrpc::Error::invalid_request())?;

        let result = miner
            .mine_file(&file_path, &self.memory_manager, &self.graph, &config)
            .await
            .map_err(|e| {
                tracing::error!("Git mining for file failed: {}", e);
                tower_lsp::jsonrpc::Error::internal_error()
            })?;

        Ok(serde_json::json!({
            "file": file_path.to_string_lossy(),
            "commitsProcessed": result.commits_processed,
            "memoriesCreated": result.memories_created,
            "commitsSkipped": result.commits_skipped,
            "memoryIds": result.memory_ids,
            "warnings": result.warnings
        }))
    }

    /// Read source code for a graph node by extracting its file path and line range.
    async fn get_source_for_node(&self, node_id: NodeId) -> Option<String> {
        let graph = self.graph.read().await;
        crate::domain::source_code::get_symbol_source(&graph, node_id)
    }

    /// Handle get_edit_context LSP request by forwarding to MCP-style composition.
    ///
    /// Params: `{ uri: string, line: number, maxTokens?: number }`
    pub async fn handle_get_edit_context(
        &self,
        params: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let uri = params
            .get("uri")
            .and_then(|v| v.as_str())
            .ok_or_else(|| tower_lsp::jsonrpc::Error::invalid_params("Missing 'uri' parameter"))?;
        let line = params
            .get("line")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
            .ok_or_else(|| tower_lsp::jsonrpc::Error::invalid_params("Missing 'line' parameter"))?;
        let max_tokens = params
            .get("maxTokens")
            .or_else(|| params.get("max_tokens"))
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(8000);

        let start_time = std::time::Instant::now();

        // Resolve target symbol
        let url = tower_lsp::lsp_types::Url::parse(uri)
            .map_err(|_| tower_lsp::jsonrpc::Error::invalid_params("Invalid URI"))?;
        let file_path = url
            .to_file_path()
            .map_err(|_| tower_lsp::jsonrpc::Error::invalid_params("Invalid file path"))?;
        let path_str = file_path.to_string_lossy().to_string();

        // Find symbol at line
        let (target, name, node_type, language, line_start, line_end) = {
            let graph = self.graph.read().await;

            // Use domain node resolution for exact match + nearest
            let found = crate::domain::node_resolution::find_nearest_node(&graph, &path_str, line)
                .map(|(id, _)| id);

            let target = found.ok_or_else(|| {
                tower_lsp::jsonrpc::Error::invalid_params("No symbols found at this location")
            })?;

            let node = graph
                .get_node(target)
                .map_err(|_| tower_lsp::jsonrpc::Error::internal_error())?;
            let name = node_props::name(node).to_string();
            let node_type = format!("{:?}", node.node_type).to_lowercase();
            let language = node
                .properties
                .get_string("language")
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    std::path::Path::new(&path_str)
                        .extension()
                        .and_then(|e| e.to_str())
                        .map(|e| e.to_string())
                        .unwrap_or_else(|| "unknown".to_string())
                });
            let line_start = node_props::line_start(node) as i64;
            let line_end = {
                let e = node_props::line_end(node) as i64;
                if e == 0 {
                    line_start
                } else {
                    e
                }
            };
            (target, name, node_type, language, line_start, line_end)
        };

        // Section 1: Symbol source code
        let source_code = self.get_source_for_node(target).await.unwrap_or_default();
        let source_tokens = source_code.len() / 4;
        let mut budget_remaining = max_tokens.saturating_sub(source_tokens);

        let symbol = serde_json::json!({
            "name": name,
            "type": node_type,
            "code": source_code,
            "language": language,
            "location": {
                "uri": uri,
                "range": {
                    "start": { "line": line_start, "character": 0 },
                    "end": { "line": line_end, "character": 0 },
                }
            }
        });

        // Section 2: Callers
        let caller_budget = max_tokens / 4;
        let callers = self.query_engine.get_callers(target, 1).await;
        let mut caller_tokens_used = 0usize;
        let mut callers_json = Vec::new();
        for caller in callers.iter().take(10) {
            if caller_tokens_used >= caller_budget {
                break;
            }
            let code = self.get_source_for_node(caller.node_id).await;
            let code_tokens = code.as_ref().map(|c| c.len() / 4).unwrap_or(0);
            caller_tokens_used += code_tokens;
            callers_json.push(serde_json::json!({
                "name": caller.symbol.name,
                "code": code,
                "file": caller.symbol.location.file,
                "line": caller.symbol.location.line,
            }));
        }
        budget_remaining = budget_remaining.saturating_sub(caller_tokens_used);

        // Section 3: Related tests (compact — names only)
        let mut tests_json = Vec::new();
        {
            let entry_types = vec![crate::ai_query::EntryType::TestEntry];
            let tests = self.query_engine.find_entry_points(&entry_types).await;
            let mut seen = std::collections::HashSet::new();
            seen.insert(target);
            for test in tests.iter().take(20) {
                if tests_json.len() >= 5 {
                    break;
                }
                let callees = self.query_engine.get_callees(test.node_id, 3).await;
                if callees.iter().any(|c| c.node_id == target) && seen.insert(test.node_id) {
                    tests_json.push(serde_json::json!({
                        "name": test.symbol.name,
                        "relationship": "calls_target",
                    }));
                }
            }
        }

        // Section 4: Memories
        let memories_json = {
            let config = crate::memory::SearchConfig {
                limit: 5,
                current_only: true,
                ..Default::default()
            };
            match self.memory_manager.search(&path_str, &config, &[]).await {
                Ok(results) => {
                    let memory_budget = max_tokens * 15 / 100;
                    let mut mem_tokens = 0usize;
                    let mut seen_titles = std::collections::HashSet::new();
                    let mut mem_list = Vec::new();
                    for r in &results {
                        if mem_tokens >= memory_budget {
                            break;
                        }
                        if !seen_titles.insert(r.memory.title.clone()) {
                            continue;
                        }
                        let content_tokens = r.memory.content.len() / 4;
                        mem_tokens += content_tokens;
                        budget_remaining = budget_remaining.saturating_sub(content_tokens);
                        mem_list.push(serde_json::json!({
                            "id": r.memory.id,
                            "title": r.memory.title,
                            "content": r.memory.content,
                            "kind": r.memory.kind.discriminant_name(),
                        }));
                    }
                    mem_list
                }
                Err(_) => Vec::new(),
            }
        };

        // Section 5: Recent git changes
        let git_json = {
            let workspace = self.workspace_folders.read().await;
            let ws = workspace.first().cloned();
            drop(workspace);
            let file_path_clone = path_str.clone();
            match ws {
                Some(ws_path) => tokio::task::spawn_blocking(move || {
                    let executor = crate::git_mining::GitExecutor::new(&ws_path).ok()?;
                    let log_output = executor
                        .log(
                            "%H%x00%s%x00%an%x00%ai",
                            Some(10),
                            Some(std::path::Path::new(&file_path_clone)),
                        )
                        .ok()?;
                    let commits: Vec<serde_json::Value> = log_output
                        .lines()
                        .filter(|l| !l.is_empty())
                        .take(5)
                        .filter_map(|line| {
                            let parts: Vec<&str> = line.split('\0').collect();
                            if parts.len() >= 4 {
                                Some(serde_json::json!({
                                    "hash": &parts[0][..8.min(parts[0].len())],
                                    "subject": parts[1],
                                    "author": parts[2],
                                    "date": parts[3],
                                }))
                            } else {
                                None
                            }
                        })
                        .collect();
                    Some(commits)
                })
                .await
                .ok()
                .flatten()
                .unwrap_or_default(),
                None => Vec::new(),
            }
        };

        let query_time = start_time.elapsed().as_millis() as u64;
        let total_tokens = max_tokens.saturating_sub(budget_remaining);

        Ok(serde_json::json!({
            "symbol": symbol,
            "callers": callers_json,
            "tests": tests_json,
            "memories": memories_json,
            "recentChanges": git_json,
            "metadata": {
                "totalTokens": total_tokens,
                "maxTokens": max_tokens,
                "queryTime": query_time,
            }
        }))
    }

    /// Handle get_curated_context LSP request.
    ///
    /// Params: `{ query: string, uri?: string, maxTokens?: number, maxSymbols?: number }`
    pub async fn handle_get_curated_context(
        &self,
        params: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let query = params
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                tower_lsp::jsonrpc::Error::invalid_params("Missing 'query' parameter")
            })?;
        let anchor_uri = params.get("uri").and_then(|v| v.as_str());
        let max_tokens = params
            .get("maxTokens")
            .or_else(|| params.get("max_tokens"))
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(8000);
        let max_symbols = params
            .get("maxSymbols")
            .or_else(|| params.get("max_symbols"))
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(5);

        let start_time = std::time::Instant::now();
        let mut budget_remaining = max_tokens;

        // Step 1: Search for relevant symbols
        let options = crate::ai_query::SearchOptions {
            limit: max_symbols * 3,
            include_private: true,
            compact: false,
            ..Default::default()
        };
        let search_result = self.query_engine.symbol_search(query, &options).await;

        // Resolve anchor path for prioritization
        let anchor_path: Option<String> = anchor_uri.and_then(|uri| {
            tower_lsp::lsp_types::Url::parse(uri)
                .ok()
                .and_then(|u| u.to_file_path().ok())
                .map(|p| p.to_string_lossy().to_string())
        });

        let mut matches = search_result.results;
        if let Some(ref anchor) = anchor_path {
            matches.sort_by(|a, b| {
                let a_anchor = a.symbol.location.file == *anchor;
                let b_anchor = b.symbol.location.file == *anchor;
                b_anchor.cmp(&a_anchor).then(b.score.total_cmp(&a.score))
            });
        }
        let top_matches: Vec<_> = matches.into_iter().take(max_symbols).collect();

        if top_matches.is_empty() {
            return Ok(serde_json::json!({
                "error": format!("No symbols found matching '{}'", query),
                "query": query,
            }));
        }

        // Step 2: Resolve full source
        let symbol_budget = max_tokens * 40 / 100;
        let mut symbols_json = Vec::new();
        let mut primary_files = std::collections::HashSet::new();
        let mut symbols_tokens = 0usize;

        for m in &top_matches {
            if symbols_tokens >= symbol_budget {
                break;
            }
            let code = self.get_source_for_node(m.node_id).await;
            let code_tokens = code.as_ref().map(|c| c.len() / 4).unwrap_or(0);
            symbols_tokens += code_tokens;
            primary_files.insert(m.symbol.location.file.clone());

            symbols_json.push(serde_json::json!({
                "name": m.symbol.name,
                "kind": m.symbol.kind,
                "file": m.symbol.location.file,
                "line": m.symbol.location.line,
                "score": m.score,
                "code": code,
            }));
        }
        budget_remaining = budget_remaining.saturating_sub(symbols_tokens);

        // Step 3: Expand — callers of primary symbols
        let dep_budget = max_tokens * 25 / 100;
        let mut deps_json = Vec::new();
        let mut dep_tokens = 0usize;
        let mut seen_ids = std::collections::HashSet::new();
        for m in &top_matches {
            seen_ids.insert(m.node_id);
        }

        for m in &top_matches {
            if dep_tokens >= dep_budget {
                break;
            }
            let callers = self.query_engine.get_callers(m.node_id, 1).await;
            for caller in callers.iter().take(3) {
                if dep_tokens >= dep_budget || !seen_ids.insert(caller.node_id) {
                    continue;
                }
                let code = self.get_source_for_node(caller.node_id).await;
                let code_tokens = code.as_ref().map(|c| c.len() / 4).unwrap_or(0);
                if code_tokens > dep_budget / 3 {
                    deps_json.push(serde_json::json!({
                        "name": caller.symbol.name,
                        "kind": caller.symbol.kind,
                        "file": caller.symbol.location.file,
                        "relationship": "caller",
                    }));
                } else {
                    dep_tokens += code_tokens;
                    deps_json.push(serde_json::json!({
                        "name": caller.symbol.name,
                        "kind": caller.symbol.kind,
                        "file": caller.symbol.location.file,
                        "relationship": "caller",
                        "code": code,
                    }));
                }
            }
        }
        budget_remaining = budget_remaining.saturating_sub(dep_tokens);

        // Step 4: Memories for primary files
        let memory_budget = max_tokens * 15 / 100;
        let mut memories_json = Vec::new();
        let mut mem_tokens = 0usize;
        let mut seen_titles = std::collections::HashSet::new();

        for file in primary_files.iter().take(3) {
            if mem_tokens >= memory_budget {
                break;
            }
            let config = crate::memory::SearchConfig {
                limit: 3,
                current_only: true,
                ..Default::default()
            };
            if let Ok(results) = self.memory_manager.search(file, &config, &[]).await {
                for r in &results {
                    if mem_tokens >= memory_budget {
                        break;
                    }
                    if !seen_titles.insert(r.memory.title.clone()) {
                        continue;
                    }
                    mem_tokens += r.memory.content.len() / 4;
                    budget_remaining = budget_remaining.saturating_sub(r.memory.content.len() / 4);
                    memories_json.push(serde_json::json!({
                        "title": r.memory.title,
                        "content": r.memory.content,
                        "kind": r.memory.kind.discriminant_name(),
                        "relatedFile": file,
                    }));
                }
            }
        }

        let query_time = start_time.elapsed().as_millis() as u64;
        let total_tokens = max_tokens.saturating_sub(budget_remaining);

        Ok(serde_json::json!({
            "query": query,
            "symbols": symbols_json,
            "dependencies": deps_json,
            "memories": memories_json,
            "metadata": {
                "totalTokens": total_tokens,
                "maxTokens": max_tokens,
                "queryTime": query_time,
                "symbolsFound": search_result.total_matches,
                "symbolsIncluded": symbols_json.len(),
            }
        }))
    }

    /// Handle search_git_history LSP request.
    pub async fn handle_search_git_history(
        &self,
        params: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let query = params
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                tower_lsp::jsonrpc::Error::invalid_params("Missing 'query' parameter")
            })?;
        let since = params.get("since").and_then(|v| v.as_str());
        let max_results = params
            .get("maxResults")
            .or_else(|| params.get("max_results"))
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(10);

        let start_time = std::time::Instant::now();
        let mut results = Vec::new();
        let mut seen_hashes = std::collections::HashSet::new();

        // Strategy 1: Semantic search via memory embeddings
        let config = crate::memory::SearchConfig {
            limit: max_results,
            current_only: false,
            ..Default::default()
        };
        if let Ok(mem_results) = self.memory_manager.search(query, &config, &[]).await {
            for r in &mem_results {
                if let crate::memory::MemorySource::GitHistory { ref commit_hash } = r.memory.source
                {
                    if seen_hashes.insert(commit_hash.clone()) {
                        results.push(serde_json::json!({
                            "hash": &commit_hash[..8.min(commit_hash.len())],
                            "subject": r.memory.title.trim_start_matches("[Git] "),
                            "content": r.memory.content,
                            "kind": r.memory.kind.discriminant_name(),
                            "score": r.score,
                            "source": "semantic",
                        }));
                    }
                }
            }
        }

        // Strategy 2: Keyword search via git log --grep
        if results.len() < max_results {
            let workspace = self.workspace_folders.read().await;
            let ws = workspace.first().cloned();
            drop(workspace);
            let query_owned = query.to_string();
            let since_owned = since.map(|s| s.to_string());
            let remaining = max_results.saturating_sub(results.len());

            if let Some(ws_path) = ws {
                let git_results = tokio::task::spawn_blocking(move || {
                    let executor = crate::git_mining::GitExecutor::new(&ws_path).ok()?;
                    let mut cmd = std::process::Command::new("git");
                    cmd.current_dir(&ws_path);
                    cmd.args([
                        "log",
                        "--format=%H%x00%s%x00%an%x00%ai",
                        &format!("--grep={}", query_owned),
                        "-i",
                        &format!("-n{}", remaining * 2),
                    ]);
                    if let Some(ref s) = since_owned {
                        cmd.arg(format!("--since={}", s));
                    }
                    cmd.arg("--");
                    let output = cmd.output().ok()?;
                    if !output.status.success() {
                        return None;
                    }
                    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                    let commits: Vec<_> = stdout
                        .lines()
                        .filter(|l| !l.is_empty())
                        .take(remaining)
                        .filter_map(|line| {
                            let parts: Vec<&str> = line.split('\0').collect();
                            if parts.len() >= 4 {
                                let files = executor
                                    .show_files(parts[0])
                                    .unwrap_or_default()
                                    .into_iter()
                                    .take(10)
                                    .collect::<Vec<_>>();
                                Some((
                                    parts[0].to_string(),
                                    parts[1].to_string(),
                                    parts[2].to_string(),
                                    parts[3].to_string(),
                                    files,
                                ))
                            } else {
                                None
                            }
                        })
                        .collect();
                    Some(commits)
                })
                .await
                .ok()
                .flatten()
                .unwrap_or_default();

                for (hash, subject, author, date, files) in git_results {
                    if seen_hashes.insert(hash.clone()) {
                        results.push(serde_json::json!({
                            "hash": &hash[..8.min(hash.len())],
                            "subject": subject,
                            "author": author,
                            "date": date,
                            "files": files,
                            "source": "keyword",
                        }));
                    }
                }
            }
        }

        let query_time = start_time.elapsed().as_millis() as u64;

        Ok(serde_json::json!({
            "query": query,
            "results": results,
            "metadata": {
                "total": results.len(),
                "queryTime": query_time,
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph::{NodeType, PropertyMap};
    use std::path::Path;
    use tempfile::TempDir;

    /// Helper to create a test backend with an empty graph
    fn create_test_backend() -> CodeGraphBackend {
        let graph = Arc::new(RwLock::new(
            CodeGraph::in_memory().expect("Failed to create graph"),
        ));
        let query_engine = Arc::new(QueryEngine::new(Arc::clone(&graph)));
        CodeGraphBackend::new_for_test(graph, query_engine)
    }

    /// Helper to create a backend with a pre-populated graph
    async fn create_backend_with_nodes() -> (CodeGraphBackend, NodeId, NodeId) {
        use codegraph::PropertyValue;

        let graph = Arc::new(RwLock::new(
            CodeGraph::in_memory().expect("Failed to create graph"),
        ));

        let (func1_id, func2_id) = {
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
            props1.insert("line_start".to_string(), PropertyValue::Int(10));
            props1.insert("line_end".to_string(), PropertyValue::Int(20));
            props1.insert("col_start".to_string(), PropertyValue::Int(0));
            props1.insert("col_end".to_string(), PropertyValue::Int(50));
            props1.insert(
                "signature".to_string(),
                PropertyValue::String("fn test_function() -> bool".to_string()),
            );
            let func1_id = g.add_node(NodeType::Function, props1).unwrap();

            // Create another function that calls the first
            let mut props2 = PropertyMap::new();
            props2.insert(
                "name".to_string(),
                PropertyValue::String("caller_function".to_string()),
            );
            props2.insert(
                "path".to_string(),
                PropertyValue::String("/test/file.rs".to_string()),
            );
            props2.insert("line_start".to_string(), PropertyValue::Int(30));
            props2.insert("line_end".to_string(), PropertyValue::Int(40));
            props2.insert("col_start".to_string(), PropertyValue::Int(0));
            props2.insert("col_end".to_string(), PropertyValue::Int(50));
            props2.insert(
                "signature".to_string(),
                PropertyValue::String("fn caller_function()".to_string()),
            );
            let func2_id = g.add_node(NodeType::Function, props2).unwrap();

            // Create a call edge from func2 to func1
            g.add_edge(func2_id, func1_id, EdgeType::Calls, PropertyMap::new())
                .unwrap();

            (func1_id, func2_id)
        };

        let query_engine = Arc::new(QueryEngine::new(Arc::clone(&graph)));
        let backend = CodeGraphBackend::new_for_test(graph, query_engine);

        (backend, func1_id, func2_id)
    }

    /// Helper to add a function node to the symbol index
    fn add_func_to_index(
        backend: &CodeGraphBackend,
        path: &Path,
        func_id: NodeId,
        name: &str,
        start_line: u32,
        end_line: u32,
    ) {
        backend.symbol_index.add_node_for_test(
            path.to_path_buf(),
            func_id,
            name,
            "Function",
            start_line,
            end_line,
        );
    }

    #[test]
    fn test_backend_creation() {
        let backend = create_test_backend();
        assert!(backend.file_cache.is_empty());
    }

    #[tokio::test]
    async fn test_find_node_at_position_empty_graph() {
        let backend = create_test_backend();
        let graph = backend.graph.read().await;
        let path = Path::new("/test/file.rs");
        let position = Position {
            line: 10,
            character: 5,
        };

        let result = backend.find_node_at_position(&graph, path, position);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_find_node_at_position_with_indexed_symbol() {
        let (backend, func_id, _) = create_backend_with_nodes().await;

        // Add the node to the symbol index
        let path = Path::new("/test/file.rs");
        add_func_to_index(&backend, path, func_id, "test_function", 10, 20);

        let graph = backend.graph.read().await;
        // Position within the function (line 15, 0-indexed is 14, but we add 1 = 15)
        let position = Position {
            line: 14, // 0-indexed, will be converted to 15 (1-indexed)
            character: 10,
        };

        let result = backend.find_node_at_position(&graph, path, position);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some(func_id));
    }

    #[tokio::test]
    async fn test_find_nearest_node_exact_match() {
        let (backend, func_id, _) = create_backend_with_nodes().await;

        // Add the node to the symbol index
        let path = Path::new("/test/file.rs");
        add_func_to_index(&backend, path, func_id, "test_function", 10, 20);

        let graph = backend.graph.read().await;
        let position = Position {
            line: 14,
            character: 10,
        };

        let result = backend.find_nearest_node(&graph, path, position);
        assert!(result.is_ok());
        let (node_id, was_fallback) = result.unwrap().unwrap();
        assert_eq!(node_id, func_id);
        assert!(!was_fallback);
    }

    #[tokio::test]
    async fn test_find_nearest_node_fallback() {
        let (backend, func_id, _) = create_backend_with_nodes().await;

        // Add the node to the symbol index
        let path = Path::new("/test/file.rs");
        add_func_to_index(&backend, path, func_id, "test_function", 10, 20);

        let graph = backend.graph.read().await;
        // Position before the function
        let position = Position {
            line: 0,
            character: 0,
        };

        let result = backend.find_nearest_node(&graph, path, position);
        assert!(result.is_ok());
        let (node_id, was_fallback) = result.unwrap().unwrap();
        assert_eq!(node_id, func_id);
        assert!(was_fallback);
    }

    #[tokio::test]
    async fn test_get_connected_edges_outgoing() {
        let (backend, func1_id, func2_id) = create_backend_with_nodes().await;
        let graph = backend.graph.read().await;

        // func2 calls func1, so outgoing edges from func2 should include func1
        let edges = backend.get_connected_edges(&graph, func2_id, Direction::Outgoing);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].0, func2_id); // source
        assert_eq!(edges[0].1, func1_id); // target
        assert_eq!(edges[0].2, EdgeType::Calls);
    }

    #[tokio::test]
    async fn test_get_connected_edges_incoming() {
        let (backend, func1_id, func2_id) = create_backend_with_nodes().await;
        let graph = backend.graph.read().await;

        // func1 is called by func2, so incoming edges to func1 should include func2
        let edges = backend.get_connected_edges(&graph, func1_id, Direction::Incoming);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].0, func2_id); // source
        assert_eq!(edges[0].1, func1_id); // target
        assert_eq!(edges[0].2, EdgeType::Calls);
    }

    #[tokio::test]
    async fn test_get_connected_edges_both() {
        let (backend, func1_id, _func2_id) = create_backend_with_nodes().await;
        let graph = backend.graph.read().await;

        // Get both directions for func1
        let edges = backend.get_connected_edges(&graph, func1_id, Direction::Both);
        assert_eq!(edges.len(), 1);
    }

    #[tokio::test]
    async fn test_node_to_location() {
        let (backend, func_id, _) = create_backend_with_nodes().await;
        let graph = backend.graph.read().await;

        let result = backend.node_to_location(&graph, func_id);
        assert!(result.is_ok());

        let location = result.unwrap();
        assert!(location.uri.to_string().contains("file.rs"));
        // Line 10 (1-indexed) becomes 9 (0-indexed)
        assert_eq!(location.range.start.line, 9);
        assert_eq!(location.range.end.line, 19);
    }

    #[tokio::test]
    async fn test_node_to_location_missing_path() {
        use codegraph::PropertyValue;
        let backend = create_test_backend();

        let node_id = {
            let mut graph = backend.graph.write().await;
            let mut props = PropertyMap::new();
            props.insert(
                "name".to_string(),
                PropertyValue::String("orphan_node".to_string()),
            );
            // No path property set
            graph.add_node(NodeType::Function, props).unwrap()
        };

        let graph = backend.graph.read().await;
        let result = backend.node_to_location(&graph, node_id);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_node_source_code_from_property() {
        use codegraph::PropertyValue;
        let backend = create_test_backend();

        let node_id = {
            let mut graph = backend.graph.write().await;
            let mut props = PropertyMap::new();
            props.insert(
                "name".to_string(),
                PropertyValue::String("test_func".to_string()),
            );
            props.insert(
                "source".to_string(),
                PropertyValue::String("fn test_func() { }".to_string()),
            );
            graph.add_node(NodeType::Function, props).unwrap()
        };

        let result = backend.get_node_source_code(node_id).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some("fn test_func() { }".to_string()));
    }

    #[tokio::test]
    async fn test_get_node_source_code_from_file() {
        use codegraph::PropertyValue;
        let backend = create_test_backend();
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");

        // Write test file
        std::fs::write(&file_path, "line 1\nline 2\nline 3\nline 4\nline 5").unwrap();

        let node_id = {
            let mut graph = backend.graph.write().await;
            let mut props = PropertyMap::new();
            props.insert(
                "name".to_string(),
                PropertyValue::String("test_func".to_string()),
            );
            props.insert(
                "path".to_string(),
                PropertyValue::String(file_path.to_str().unwrap().to_string()),
            );
            props.insert("line_start".to_string(), PropertyValue::Int(2));
            props.insert("line_end".to_string(), PropertyValue::Int(4));
            graph.add_node(NodeType::Function, props).unwrap()
        };

        let result = backend.get_node_source_code(node_id).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some("line 2\nline 3\nline 4".to_string()));
    }

    #[tokio::test]
    async fn test_extract_node_id_from_item_valid() {
        let backend = create_test_backend();

        let item = CallHierarchyItem {
            name: "test".to_string(),
            kind: SymbolKind::FUNCTION,
            tags: None,
            detail: None,
            uri: Url::parse("file:///test.rs").unwrap(),
            range: Range::default(),
            selection_range: Range::default(),
            data: Some(serde_json::json!({ "nodeId": "123" })),
        };

        let result = backend.extract_node_id_from_item(&item);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 123);
    }

    #[tokio::test]
    async fn test_extract_node_id_from_item_missing_data() {
        let backend = create_test_backend();

        let item = CallHierarchyItem {
            name: "test".to_string(),
            kind: SymbolKind::FUNCTION,
            tags: None,
            detail: None,
            uri: Url::parse("file:///test.rs").unwrap(),
            range: Range::default(),
            selection_range: Range::default(),
            data: None,
        };

        let result = backend.extract_node_id_from_item(&item);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_extract_node_id_from_item_invalid_node_id() {
        let backend = create_test_backend();

        let item = CallHierarchyItem {
            name: "test".to_string(),
            kind: SymbolKind::FUNCTION,
            tags: None,
            detail: None,
            uri: Url::parse("file:///test.rs").unwrap(),
            range: Range::default(),
            selection_range: Range::default(),
            data: Some(serde_json::json!({ "nodeId": "not_a_number" })),
        };

        let result = backend.extract_node_id_from_item(&item);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_file_cache_operations() {
        let backend = create_test_backend();
        let uri = Url::parse("file:///test.rs").unwrap();

        assert!(backend.file_cache.is_empty());

        // Simulate inserting a file info
        let file_info = FileInfo {
            file_path: "/test.rs".into(),
            file_id: 1,
            functions: vec![],
            classes: vec![],
            traits: vec![],
            imports: vec![],
            parse_time: std::time::Duration::from_millis(0),
            line_count: 0,
            byte_count: 0,
        };
        backend.file_cache.insert(uri.clone(), file_info);

        assert!(!backend.file_cache.is_empty());
        assert!(backend.file_cache.contains_key(&uri));

        // Remove
        backend.file_cache.remove(&uri);
        assert!(backend.file_cache.is_empty());
    }

    #[tokio::test]
    async fn test_query_cache_invalidation() {
        let backend = create_test_backend();

        // Add something to cache
        let path = PathBuf::from("/test/file.rs");
        backend.query_cache.set_definition(path.clone(), 0, 0, 123);

        // Verify it's cached
        let cached = backend.query_cache.get_definition(&path, 0, 0);
        assert!(cached.is_some());
        assert_eq!(cached.unwrap(), 123);

        // Invalidate
        backend.query_cache.invalidate_file(&path);

        // Verify it's gone
        let cached = backend.query_cache.get_definition(&path, 0, 0);
        assert!(cached.is_none());
    }

    #[tokio::test]
    async fn test_symbol_index_integration() {
        let (backend, func_id, _) = create_backend_with_nodes().await;
        let path = Path::new("/test/file.rs");

        // Add to symbol index using test helper
        add_func_to_index(&backend, path, func_id, "test_function", 10, 20);

        // Search by name
        let results = backend.symbol_index.search_by_name("test_function");
        assert!(!results.is_empty());
        assert!(results.contains(&func_id));

        // Get file symbols
        let file_symbols = backend.symbol_index.get_file_symbols(path);
        assert!(!file_symbols.is_empty());

        // Remove file
        backend.symbol_index.remove_file(path);
        let file_symbols = backend.symbol_index.get_file_symbols(path);
        assert!(file_symbols.is_empty());
    }
}
