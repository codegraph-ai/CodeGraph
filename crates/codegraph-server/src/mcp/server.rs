// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! MCP Server Implementation
//!
//! Handles MCP protocol requests and routes them to CodeGraph functionality.

use super::protocol::*;
use super::resources::get_all_resources;
use super::tools::get_all_tools;
use super::transport::AsyncStdioTransport;
use crate::ai_query::QueryEngine;
use crate::domain::node_props;
use crate::index_state::IndexState;
use crate::indexer::{IndexConfig, Indexer};
use crate::memory::{self, MemoryManager};
use crate::parser_registry::ParserRegistry;
use codegraph::{CodeGraph, NamespacedBackend, RocksDBBackend, StorageBackend};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

const PROTOCOL_VERSION: &str = "2024-11-05";
const SERVER_NAME: &str = "codegraph";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// MCP Backend - wraps CodeGraph components for MCP access
#[derive(Clone)]
pub struct McpBackend {
    pub graph: Arc<RwLock<CodeGraph>>,
    pub parsers: Arc<ParserRegistry>,
    pub query_engine: Arc<QueryEngine>,
    pub memory_manager: Arc<MemoryManager>,
    pub workspace_folders: Vec<PathBuf>,
    /// Project slug used as namespace in the shared graph database
    pub project_slug: String,
    /// Additional directories to exclude from indexing
    pub exclude_dirs: Vec<String>,
    /// Maximum number of files to index
    pub max_files: usize,
    /// Shared indexer for directory walking and file parsing
    pub indexer: Arc<Indexer>,
    /// Index state for hash persistence (shared with indexer)
    index_state: Arc<Mutex<IndexState>>,
}

impl McpBackend {
    /// Create a new MCP backend for the given workspace.
    ///
    /// Starts with a fresh in-memory graph (re-indexes all files on startup).
    /// After indexing, persists to the shared database at `~/.codegraph/graph.db`
    /// (namespaced by project slug) for cross-project access.
    pub fn new(
        workspaces: Vec<PathBuf>,
        exclude_dirs: Vec<String>,
        max_files: usize,
        embedding_model: codegraph_memory::CodeGraphEmbeddingModel,
        full_body_embedding: bool,
    ) -> Self {
        let primary = workspaces.first().expect("At least one workspace required");
        let slug = memory::project_slug(primary);
        tracing::info!("Project slug: {}", slug);
        tracing::info!(
            "Workspace folders: {:?} ({} total)",
            workspaces,
            workspaces.len()
        );

        // Try to load persisted graph from previous session.
        // Falls back to empty in-memory graph if no prior data exists.
        let graph = match Self::open_persistent_graph(&slug) {
            Ok(g) if g.node_count() > 0 => {
                tracing::info!(
                    "Loaded persisted graph ({} nodes) from previous session",
                    g.node_count()
                );
                Arc::new(RwLock::new(g))
            }
            _ => {
                tracing::info!("No persisted graph found — starting fresh");
                Arc::new(RwLock::new(
                    CodeGraph::in_memory().expect("Failed to create in-memory graph"),
                ))
            }
        };

        // Resolve extension path from binary location for model discovery
        // In dev: target/debug/codegraph-server -> project root (go up 3 levels)
        // In prod: extension/bin/codegraph-server -> extension root (go up 2 levels)
        let extension_path = std::env::current_exe().ok().and_then(|exe| {
            let exe_dir = exe.parent()?;
            // Check if we're in target/debug or target/release
            if exe_dir.ends_with("debug") || exe_dir.ends_with("release") {
                // Dev environment: go up to project root (target -> project)
                exe_dir.parent()?.parent().map(|p| p.to_path_buf())
            } else {
                // Prod environment: assume bin/ -> extension root
                exe_dir.parent().map(|p| p.to_path_buf())
            }
        });

        tracing::info!("Extension path for models: {:?}", extension_path);

        let query_engine = QueryEngine::new(Arc::clone(&graph));
        query_engine.set_full_body_embedding(full_body_embedding);

        let parsers = Arc::new(ParserRegistry::new());
        let index_state = Arc::new(Mutex::new(IndexState::new(&slug)));
        let indexer = Arc::new(Indexer::new(Arc::clone(&parsers), Arc::clone(&index_state)));

        Self {
            query_engine: Arc::new(query_engine),
            graph,
            parsers,
            memory_manager: Arc::new(MemoryManager::with_model(extension_path, embedding_model)),
            workspace_folders: workspaces,
            project_slug: slug,
            exclude_dirs,
            max_files,
            indexer,
            index_state,
        }
    }

    /// Open the shared graph database with project-scoped namespacing.
    ///
    /// Opens RocksDB at `~/.codegraph/graph.db`, wraps with NamespacedBackend,
    /// loads all data into in-memory caches, then detaches storage to release
    /// the database lock. Used for cross-project graph access (T1-4).
    pub fn open_persistent_graph(slug: &str) -> Result<CodeGraph, String> {
        let db_path = memory::shared_graph_db_path().map_err(|e| format!("{e}"))?;

        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create ~/.codegraph: {e}"))?;
        }

        let rocks =
            RocksDBBackend::open(&db_path).map_err(|e| format!("Failed to open graph.db: {e}"))?;
        let namespaced = NamespacedBackend::new(Box::new(rocks), slug);
        let mut graph = CodeGraph::with_backend(Box::new(namespaced))
            .map_err(|e| format!("Failed to load graph: {e}"))?;

        // Detach to release the RocksDB lock — all data is now in memory
        graph
            .detach_storage()
            .map_err(|e| format!("Failed to detach storage: {e}"))?;

        Ok(graph)
    }

    /// Persist the current graph state to the shared database.
    ///
    /// Opens RocksDB briefly, writes registry entry + all data with namespace prefix, then closes.
    fn persist_graph(&self, graph: &CodeGraph) -> Result<(), String> {
        let db_path = memory::shared_graph_db_path().map_err(|e| format!("{e}"))?;

        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create ~/.codegraph: {e}"))?;
        }

        let mut rocks = RocksDBBackend::open(&db_path)
            .map_err(|e| format!("Failed to open graph.db for persist: {e}"))?;

        // Write project registry entry (un-namespaced, global key)
        let workspace_path = self
            .workspace_folders
            .first()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let registry_value = serde_json::json!({
            "slug": self.project_slug,
            "workspace": workspace_path,
            "node_count": graph.node_count(),
            "edge_count": graph.edge_count(),
            "last_indexed": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        });
        let registry_key = format!("_registry:{}", self.project_slug);
        rocks
            .put(
                registry_key.as_bytes(),
                registry_value.to_string().as_bytes(),
            )
            .map_err(|e| format!("Failed to write registry: {e}"))?;

        // Write graph data with namespace prefix
        let namespaced = NamespacedBackend::new(Box::new(rocks), &self.project_slug);

        graph
            .persist_to(Box::new(namespaced))
            .map_err(|e| format!("Failed to persist graph: {e}"))?;

        tracing::info!(
            "Persisted {} nodes, {} edges to graph.db (namespace: {})",
            graph.node_count(),
            graph.edge_count(),
            self.project_slug
        );
        Ok(())
    }

    /// List all projects indexed in the shared graph database.
    ///
    /// Scans `_registry:*` keys to discover project metadata without loading graphs.
    pub fn list_indexed_projects() -> Result<Vec<serde_json::Value>, String> {
        let db_path = memory::shared_graph_db_path().map_err(|e| format!("{e}"))?;

        if !db_path.exists() {
            return Ok(vec![]);
        }

        let rocks =
            RocksDBBackend::open(&db_path).map_err(|e| format!("Failed to open graph.db: {e}"))?;

        let entries = rocks
            .scan_prefix(b"_registry:")
            .map_err(|e| format!("Failed to scan registry: {e}"))?;

        let mut projects = Vec::new();
        for (_key, value) in entries {
            if let Ok(metadata) = serde_json::from_slice::<serde_json::Value>(&value) {
                projects.push(metadata);
            }
        }

        Ok(projects)
    }

    /// Search for symbols across all other indexed projects.
    ///
    /// Opens each project's graph from the shared DB (excluding the current project),
    /// searches for matching symbols by name substring, and returns aggregated results.
    pub fn cross_project_search(
        &self,
        query: &str,
        symbol_type: Option<&str>,
        limit: usize,
    ) -> Result<serde_json::Value, String> {
        let projects = Self::list_indexed_projects()?;
        let query_lower = query.to_lowercase();

        let mut all_results = Vec::new();
        let mut searched_projects = Vec::new();

        for project in &projects {
            let slug = project.get("slug").and_then(|v| v.as_str()).unwrap_or("");
            let workspace = project
                .get("workspace")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            // Skip the current project
            if slug == self.project_slug {
                continue;
            }

            // Open the project graph from shared DB
            let graph = match Self::open_persistent_graph(slug) {
                Ok(g) => g,
                Err(e) => {
                    tracing::warn!("Failed to open project {}: {}", slug, e);
                    continue;
                }
            };

            searched_projects.push(serde_json::json!({
                "slug": slug,
                "workspace": workspace,
                "node_count": graph.node_count(),
            }));

            // Search nodes by name substring match
            let type_filter: Option<codegraph::NodeType> = symbol_type.and_then(|st| match st {
                "function" | "method" => Some(codegraph::NodeType::Function),
                "class" => Some(codegraph::NodeType::Class),
                "variable" => Some(codegraph::NodeType::Variable),
                "interface" => Some(codegraph::NodeType::Interface),
                "type" => Some(codegraph::NodeType::Type),
                "module" => Some(codegraph::NodeType::Module),
                _ => None,
            });

            for (_id, node) in graph.iter_nodes() {
                if all_results.len() >= limit {
                    break;
                }

                // Apply type filter
                if let Some(ref tf) = type_filter {
                    if &node.node_type != tf {
                        continue;
                    }
                }

                // Skip CodeFile nodes
                if node.node_type == codegraph::NodeType::CodeFile {
                    continue;
                }

                let name = node_props::name(node);
                if !name.to_lowercase().contains(&query_lower) {
                    continue;
                }

                let file_path = node_props::path(node);
                let line_start = node_props::line_start(node);
                let line_end = node_props::line_end(node);
                let signature = node.properties.get_string("signature").unwrap_or("");

                let mut result = serde_json::json!({
                    "name": name,
                    "kind": format!("{}", node.node_type),
                    "project": slug,
                    "project_workspace": workspace,
                    "file": file_path,
                    "line_start": line_start,
                    "line_end": line_end,
                });

                if !signature.is_empty() {
                    result["signature"] = serde_json::Value::String(signature.to_string());
                }
                if let Some(route) = node.properties.get_string("route") {
                    result["route"] = serde_json::Value::String(route.to_string());
                    if let Some(method) = node.properties.get_string("http_method") {
                        result["http_method"] = serde_json::Value::String(method.to_string());
                    }
                }

                all_results.push(result);
            }
        }

        Ok(serde_json::json!({
            "query": query,
            "current_project": self.project_slug,
            "searched_projects": searched_projects,
            "results": all_results,
            "total": all_results.len(),
        }))
    }

    /// Search git history using semantic (memory embeddings) + keyword (git log --grep) matching.
    pub async fn search_git_history(
        &self,
        query: &str,
        since: Option<&str>,
        max_results: usize,
    ) -> serde_json::Value {
        use crate::git_mining::GitExecutor;
        let start_time = std::time::Instant::now();
        let mut results = Vec::new();
        let mut seen_hashes = std::collections::HashSet::new();

        // Strategy 1: Semantic search via memory embeddings
        let config = crate::memory::SearchConfig {
            limit: max_results,
            current_only: false,
            ..Default::default()
        };
        const MIN_SIMILARITY: f32 = 0.5;

        if let Ok(mem_results) = self.memory_manager.search(query, &config, &[]).await {
            for r in &mem_results {
                if r.score < MIN_SIMILARITY {
                    continue;
                }
                if let crate::memory::MemorySource::GitHistory { ref commit_hash } = r.memory.source
                {
                    if seen_hashes.insert(commit_hash.clone()) {
                        results.push(serde_json::json!({
                            "hash": &commit_hash[..8.min(commit_hash.len())],
                            "fullHash": commit_hash,
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
            let workspace = self.workspace_folders.first().cloned();
            let query_owned = query.to_string();
            let since_owned = since.map(|s| s.to_string());
            let remaining = max_results.saturating_sub(results.len());

            if let Some(ws) = workspace {
                let git_results = tokio::task::spawn_blocking(move || {
                    let executor = GitExecutor::new(&ws).ok()?;
                    let mut cmd = std::process::Command::new("git");
                    cmd.current_dir(&ws);
                    cmd.args([
                        "log",
                        "--format=%H%x00%s%x00%an%x00%ai",
                        &format!("--grep={}", query_owned),
                        "-i",
                        &format!("-n{}", remaining * 2),
                    ]);
                    if let Some(ref since_str) = since_owned {
                        cmd.arg(format!("--since={}", since_str));
                    }
                    cmd.arg("--");
                    let output = cmd.output().ok()?;
                    if !output.status.success() {
                        return None;
                    }
                    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                    let commits: Vec<(String, String, String, String, Vec<String>)> = stdout
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
                                    .collect();
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
                            "fullHash": hash,
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
        serde_json::json!({
            "query": query,
            "since": since,
            "results": results,
            "metadata": {
                "total": results.len(),
                "queryTime": query_time,
                "semanticMatches": results.iter().filter(|r| r.get("source").and_then(|s| s.as_str()) == Some("semantic")).count(),
                "keywordMatches": results.iter().filter(|r| r.get("source").and_then(|s| s.as_str()) == Some("keyword")).count(),
            }
        })
    }

    /// Save current file hashes to disk for persistence across restarts.
    pub async fn save_index_state(&self) {
        let state = self.index_state.lock().await;
        state.save();
    }

    /// Load saved file hashes from disk. Returns true if state was loaded.
    pub async fn load_index_state(&self) -> bool {
        let mut state = self.index_state.lock().await;
        let count = state.load();
        count > 0
    }

    /// Check if there is a saved index state (has been indexed before).
    pub fn has_index_state(&self) -> bool {
        IndexState::new(&self.project_slug).exists_on_disk()
    }

    /// Build an [`IndexConfig`] from this backend's settings.
    fn index_config(&self) -> IndexConfig {
        let mut exclude_dirs = IndexConfig::default_exclude_dirs();
        for dir in &self.exclude_dirs {
            if !exclude_dirs.contains(dir) {
                exclude_dirs.push(dir.clone());
            }
        }
        IndexConfig {
            exclude_dirs,
            max_files: self.max_files,
            ..IndexConfig::default()
        }
    }

    /// Index the workspace. Returns (total_files, files_actually_parsed).
    pub async fn index_workspace(&self) -> (usize, usize) {
        let config = self.index_config();

        // Initialize memory manager for each workspace folder
        for folder in &self.workspace_folders {
            if let Err(e) = self.memory_manager.initialize(folder).await {
                tracing::warn!("Failed to initialize memory manager: {:?}", e);
            }
        }

        // Delegate to the shared Indexer (handles dir walk, hashing, cross-file
        // imports, runtime deps, and index state persistence)
        let result = self
            .indexer
            .index_workspace(&self.graph, &self.workspace_folders, &config)
            .await;

        // Persist graph to shared database
        {
            let graph = self.graph.read().await;
            if let Err(e) = self.persist_graph(&graph) {
                tracing::warn!("Failed to persist graph: {}", e);
            }
        }

        // Rebuild indexes if files were parsed OR graph was loaded from persistence
        let graph_has_data = self.graph.read().await.node_count() > 0;
        if result.files_parsed > 0 || graph_has_data {
            self.query_engine.build_indexes().await;

            if let Some(engine) = self.memory_manager.get_vector_engine().await {
                self.query_engine.set_vector_engine(engine).await;

                // Load persisted vectors if no files changed; rebuild otherwise
                let loaded = if result.files_parsed == 0 {
                    self.query_engine
                        .load_symbol_vectors(&self.project_slug)
                        .await
                } else {
                    0
                };

                if loaded > 0 {
                    tracing::info!(
                        "Loaded {} persisted symbol vectors — semantic search ready",
                        loaded
                    );
                } else {
                    tracing::info!("Building semantic search index... This may take a moment.");
                    self.query_engine.build_symbol_vectors().await;
                    if let Err(e) = self
                        .query_engine
                        .save_symbol_vectors(&self.project_slug)
                        .await
                    {
                        tracing::warn!("Failed to persist symbol vectors: {}", e);
                    }
                    tracing::info!("Semantic search index ready");
                }
            }
        } else {
            tracing::info!("No files changed and no persisted data — skipping index rebuild");
        }

        (result.total_files, result.files_parsed)
    }

    /// Add or update specific files in the index without full reindex.
    /// Removes old nodes for each file before re-parsing (safe for updates).
    /// Also detects and re-indexes direct dependents (files that called or
    /// imported symbols from the updated files) to keep edges consistent.
    pub async fn add_files_to_index(&self, paths: &[PathBuf]) -> (usize, usize) {
        let mut indexed = 0;
        let mut failed = 0;
        let mut dependent_files: std::collections::HashSet<PathBuf> =
            std::collections::HashSet::new();

        for path in paths {
            if !path.exists() {
                tracing::warn!("File not found: {:?}", path);
                failed += 1;
                continue;
            }

            // Before deleting, find files that have edges INTO this file's nodes.
            // These dependents need re-indexing so their cross-file edges get re-resolved.
            {
                let graph = self.graph.read().await;
                let path_str = path.to_string_lossy().to_string();
                if let Ok(file_nodes) = graph.query().property("path", path_str.as_str()).execute()
                {
                    for node_id in &file_nodes {
                        if let Ok(neighbors) =
                            graph.get_neighbors(*node_id, codegraph::Direction::Incoming)
                        {
                            for neighbor_id in neighbors {
                                if let Ok(neighbor) = graph.get_node(neighbor_id) {
                                    if let Some(dep_path) = neighbor.properties.get_string("path") {
                                        let dep = PathBuf::from(dep_path);
                                        if dep != *path && dep.exists() {
                                            dependent_files.insert(dep);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Remove old nodes (and their connected edges) before re-parsing
            {
                let mut graph = self.graph.write().await;
                let path_str = path.to_string_lossy().to_string();
                if let Ok(old_nodes) = graph.query().property("path", path_str.as_str()).execute() {
                    for old_id in old_nodes {
                        let _ = graph.delete_node(old_id);
                    }
                }
            }

            // Clear hash so index_file doesn't skip (we already deleted old nodes above)
            {
                let mut state = self.index_state.lock().await;
                state.remove(path);
            }

            match self.indexer.index_file(&self.graph, path).await {
                Ok(_) => {
                    tracing::info!("Indexed: {:?}", path);
                    indexed += 1;
                }
                Err(e) => {
                    tracing::warn!("Failed to index {:?}: {}", path, e);
                    failed += 1;
                }
            }
        }

        // Re-index dependent files so their cross-file edges get re-resolved
        // against the updated symbol map
        if !dependent_files.is_empty() {
            let dep_count = dependent_files.len();
            tracing::info!(
                "Re-indexing {} dependent files for edge consistency",
                dep_count
            );
            for dep_path in &dependent_files {
                // Remove old nodes for dependent (index_file already removes old
                // nodes, but we do it explicitly here in case the file was not
                // previously indexed via the Indexer)
                {
                    let mut graph = self.graph.write().await;
                    let path_str = dep_path.to_string_lossy().to_string();
                    if let Ok(old_nodes) =
                        graph.query().property("path", path_str.as_str()).execute()
                    {
                        for old_id in old_nodes {
                            let _ = graph.delete_node(old_id);
                        }
                    }
                }
                if self.indexer.index_file(&self.graph, dep_path).await.is_ok() {
                    indexed += 1;
                }
            }
        }

        if indexed > 0 {
            // Resolve cross-file imports across all files
            {
                let mut graph = self.graph.write().await;
                crate::watcher::GraphUpdater::resolve_cross_file_imports(&mut graph);
            }
            // Rebuild query indexes
            self.query_engine.build_indexes().await;
            // Incrementally re-embed updated symbols
            for path in paths.iter().chain(dependent_files.iter()) {
                let path_str = path.to_string_lossy().to_string();
                self.query_engine.update_file_vectors(&path_str).await;
            }
        }

        (indexed, failed)
    }

    /// Add a directory to the index without clearing existing data.
    /// Recursively indexes all supported files, resolves imports, rebuilds indexes.
    pub async fn add_directory_to_index(&self, dir: &std::path::Path, embed: bool) -> usize {
        if !dir.exists() || !dir.is_dir() {
            tracing::warn!("Directory not found: {:?}", dir);
            return 0;
        }

        let config = self.index_config();
        let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (total, _parsed, _skipped) = self
            .indexer
            .index_directory(&self.graph, dir, &config, 0, counter)
            .await;

        if total > 0 {
            // Resolve cross-file imports
            {
                let mut graph = self.graph.write().await;
                crate::watcher::GraphUpdater::resolve_cross_file_imports(&mut graph);
            }
            // Rebuild indexes
            self.query_engine.build_indexes().await;

            if embed {
                tracing::info!("Embedding symbols from {:?}...", dir);
                self.query_engine.build_symbol_vectors().await;
            }

            tracing::info!(
                "Added directory {:?}: {} files indexed (embed={})",
                dir,
                total,
                embed
            );
        }

        total
    }
}

/// MCP Server - handles protocol messages
pub struct McpServer {
    backend: McpBackend,
    initialized: bool,
    indexed: bool,
    /// Filesystem watcher for auto-indexing on file changes
    _file_watcher: Option<super::file_watcher::McpFileWatcher>,
    /// Extension point for pro tools (community edition uses NoopProProvider)
    pro_provider: Arc<dyn super::pro_hooks::ProToolProvider>,
}

impl McpServer {
    pub fn new(
        workspaces: Vec<PathBuf>,
        exclude_dirs: Vec<String>,
        max_files: usize,
        embedding_model: codegraph_memory::CodeGraphEmbeddingModel,
        full_body_embedding: bool,
    ) -> Self {
        Self::with_pro_provider(
            workspaces,
            exclude_dirs,
            max_files,
            embedding_model,
            full_body_embedding,
            Arc::new(super::pro_hooks::NoopProProvider),
        )
    }

    /// Create a new MCP server with a custom pro tool provider.
    pub fn with_pro_provider(
        workspaces: Vec<PathBuf>,
        exclude_dirs: Vec<String>,
        max_files: usize,
        embedding_model: codegraph_memory::CodeGraphEmbeddingModel,
        full_body_embedding: bool,
        pro_provider: Arc<dyn super::pro_hooks::ProToolProvider>,
    ) -> Self {
        Self {
            backend: McpBackend::new(
                workspaces,
                exclude_dirs,
                max_files,
                embedding_model,
                full_body_embedding,
            ),
            initialized: false,
            indexed: false,
            _file_watcher: None,
            pro_provider,
        }
    }

    /// Ensure workspace is indexed (lazy — runs on first tool call)
    async fn ensure_indexed(&mut self) {
        if self.indexed {
            return;
        }
        self.indexed = true;

        // Load saved index state from previous session for incremental indexing
        let had_previous_state = self.backend.load_index_state().await;
        if had_previous_state {
            tracing::info!("Resuming from previous index state — incremental reindex");
        }

        tracing::info!("Indexing workspace: {:?}", self.backend.workspace_folders);
        let (total, parsed) = self.backend.index_workspace().await;
        tracing::info!(
            "Indexed {} files ({} parsed, {} skipped)",
            total,
            parsed,
            total - parsed
        );

        // Start filesystem watcher for auto-indexing on file changes
        if self._file_watcher.is_none() {
            match super::file_watcher::McpFileWatcher::start(
                Arc::clone(&self.backend.graph),
                Arc::clone(&self.backend.parsers),
                Arc::clone(&self.backend.query_engine),
                &self.backend.workspace_folders,
            ) {
                Ok(watcher) => {
                    self._file_watcher = Some(watcher);
                }
                Err(e) => {
                    tracing::warn!("Failed to start file watcher: {}", e);
                }
            }
        }
    }

    /// Run the MCP server event loop
    pub async fn run(&mut self) -> std::io::Result<()> {
        let mut transport = AsyncStdioTransport::new();

        tracing::info!("MCP server starting...");

        loop {
            match transport.read_request().await {
                Ok(Some(request)) => {
                    // JSON-RPC 2.0: notifications have no id and must not receive a response
                    let is_notification = request.id.is_none();
                    let response = self.handle_request(request).await;
                    if !is_notification {
                        transport.write_response(&response).await?;
                    }
                }
                Ok(None) => {
                    // Empty line, keep reading
                    continue;
                }
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::UnexpectedEof {
                        tracing::info!("Client disconnected");
                        break;
                    }
                    let response = JsonRpcResponse::error(
                        None,
                        JsonRpcError::parse_error(format!("Parse error: {}", e)),
                    );
                    transport.write_response(&response).await?;
                }
            }
        }

        Ok(())
    }

    /// Handle a JSON-RPC request
    async fn handle_request(&mut self, request: JsonRpcRequest) -> JsonRpcResponse {
        tracing::debug!("Handling request: {}", request.method);

        match request.method.as_str() {
            "initialize" => self.handle_initialize(request.id, request.params).await,
            "initialized" => {
                // This is a notification (no id) — response suppressed by run()
                tracing::debug!("Client initialized");
                JsonRpcResponse::success(request.id, Value::Null)
            }
            "ping" => {
                JsonRpcResponse::success(request.id, serde_json::to_value(PingResult {}).unwrap())
            }
            "tools/list" => self.handle_tools_list(request.id).await,
            "tools/call" => self.handle_tools_call(request.id, request.params).await,
            "resources/list" => self.handle_resources_list(request.id).await,
            "resources/read" => self.handle_resources_read(request.id, request.params).await,
            // Notifications — handled silently, response is suppressed by run()
            "notifications/initialized"
            | "notifications/cancelled"
            | "notifications/roots/list_changed" => {
                tracing::debug!("Received notification: {}", request.method);
                JsonRpcResponse::success(request.id, Value::Null)
            }
            _ => {
                JsonRpcResponse::error(request.id, JsonRpcError::method_not_found(&request.method))
            }
        }
    }

    async fn handle_initialize(
        &mut self,
        id: Option<Value>,
        params: Option<Value>,
    ) -> JsonRpcResponse {
        let init_params: InitializeParams = params
            .map(|p| serde_json::from_value(p).unwrap_or_default())
            .unwrap_or_default();

        // If the client provides roots, use them as workspace folders.
        // This allows a globally-configured MCP server to index the
        // correct project without per-project .mcp.json or --workspace.
        if let Some(roots) = &init_params.roots {
            let root_paths: Vec<PathBuf> = roots
                .iter()
                .filter_map(|r| {
                    r.uri
                        .strip_prefix("file://")
                        .map(PathBuf::from)
                        .or_else(|| {
                            // Accept bare paths too
                            let p = PathBuf::from(&r.uri);
                            if p.is_absolute() {
                                Some(p)
                            } else {
                                None
                            }
                        })
                })
                .filter(|p| p.is_dir())
                .collect();

            if !root_paths.is_empty() {
                tracing::info!(
                    "Using {} workspace root(s) from client: {:?}",
                    root_paths.len(),
                    root_paths
                );
                self.backend.workspace_folders = root_paths;
                // Recompute project slug from first root
                self.backend.project_slug =
                    crate::memory::project_slug(&self.backend.workspace_folders[0]);
            }
        }

        if let Some(ref client_info) = init_params.client_info {
            tracing::info!(
                "Client: {} {}",
                client_info.name,
                client_info.version.as_deref().unwrap_or("(unknown)")
            );
        }

        self.initialized = true;

        let result = InitializeResult {
            protocol_version: PROTOCOL_VERSION.to_string(),
            capabilities: ServerCapabilities {
                experimental: None,
                logging: Some(LoggingCapability {}),
                prompts: None,
                resources: Some(ResourcesCapability {
                    subscribe: Some(false),
                    list_changed: Some(false),
                }),
                tools: Some(ToolsCapability {
                    list_changed: Some(false),
                }),
            },
            server_info: ServerInfo {
                name: SERVER_NAME.to_string(),
                version: Some(SERVER_VERSION.to_string()),
            },
        };

        JsonRpcResponse::success(id, serde_json::to_value(result).unwrap())
    }

    async fn handle_tools_list(&self, id: Option<Value>) -> JsonRpcResponse {
        let tools = get_all_tools();
        let mut tools_json: Vec<Value> = tools
            .iter()
            .map(|t| serde_json::to_value(t).unwrap())
            .collect();

        // Add pro tools (if any are registered by the pro provider)
        for pro_tool in self.pro_provider.tools() {
            tools_json.push(serde_json::json!({
                "name": pro_tool.name,
                "description": pro_tool.description,
                "inputSchema": pro_tool.schema,
            }));
        }

        JsonRpcResponse::success(id, serde_json::json!({ "tools": tools_json }))
    }

    async fn handle_tools_call(
        &mut self,
        id: Option<Value>,
        params: Option<Value>,
    ) -> JsonRpcResponse {
        self.ensure_indexed().await;

        let params: ToolCallParams = match params {
            Some(p) => match serde_json::from_value(p) {
                Ok(p) => p,
                Err(e) => {
                    return JsonRpcResponse::error(
                        id,
                        JsonRpcError::invalid_params(format!("Invalid params: {}", e)),
                    );
                }
            },
            None => {
                return JsonRpcResponse::error(id, JsonRpcError::invalid_params("Missing params"));
            }
        };

        match self.execute_tool(&params.name, params.arguments).await {
            Ok(result) => {
                let tool_result = ToolCallResult {
                    content: vec![ToolResultContent::Text {
                        text: serde_json::to_string_pretty(&result)
                            .unwrap_or_else(|_| result.to_string()),
                    }],
                    is_error: None,
                };
                JsonRpcResponse::success(id, serde_json::to_value(tool_result).unwrap())
            }
            Err(e) => {
                let tool_result = ToolCallResult {
                    content: vec![ToolResultContent::Text {
                        text: format!("Error: {}", e),
                    }],
                    is_error: Some(true),
                };
                JsonRpcResponse::success(id, serde_json::to_value(tool_result).unwrap())
            }
        }
    }

    async fn handle_resources_list(&self, id: Option<Value>) -> JsonRpcResponse {
        let result = ResourcesListResult {
            resources: get_all_resources(),
        };
        JsonRpcResponse::success(id, serde_json::to_value(result).unwrap())
    }

    async fn handle_resources_read(
        &self,
        id: Option<Value>,
        params: Option<Value>,
    ) -> JsonRpcResponse {
        let params: ResourceReadParams = match params {
            Some(p) => match serde_json::from_value(p) {
                Ok(p) => p,
                Err(e) => {
                    return JsonRpcResponse::error(
                        id,
                        JsonRpcError::invalid_params(format!("Invalid params: {}", e)),
                    );
                }
            },
            None => {
                return JsonRpcResponse::error(id, JsonRpcError::invalid_params("Missing params"));
            }
        };

        match super::resources::read_resource(
            &params.uri,
            Arc::clone(&self.backend.graph),
            &self.backend.memory_manager,
            &self.backend.workspace_folders,
        )
        .await
        {
            Some(result) => JsonRpcResponse::success(id, serde_json::to_value(result).unwrap()),
            None => JsonRpcResponse::error(
                id,
                JsonRpcError::invalid_params(format!("Resource not found: {}", params.uri)),
            ),
        }
    }

    /// Execute a tool by name - delegates to query engine and other components
    async fn execute_tool(&self, name: &str, args: Option<Value>) -> Result<Value, String> {
        let args = args.unwrap_or(Value::Object(serde_json::Map::new()));

        match name {
            // ==================== Search Tools ====================
            "codegraph_symbol_search" => {
                let query = args
                    .get("query")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing 'query' parameter")?;
                let limit = args
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize)
                    .unwrap_or(20);
                let compact = args
                    .get("compact")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                // Parse symbolType filter
                let symbol_types: Vec<crate::ai_query::SymbolType> = args
                    .get("symbolType")
                    .or_else(|| args.get("symbol_type"))
                    .and_then(|v| {
                        // Accept either a single string or "any"
                        v.as_str().and_then(|s| {
                            if s == "any" {
                                None
                            } else {
                                Self::parse_symbol_type(s).map(|st| vec![st])
                            }
                        })
                    })
                    .unwrap_or_default();

                let include_private = args
                    .get("includePrivate")
                    .or_else(|| args.get("include_private"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);

                let options = crate::ai_query::SearchOptions::new()
                    .with_limit(limit)
                    .with_compact(compact)
                    .with_symbol_types(symbol_types)
                    .with_include_private(include_private);
                let mut result = self
                    .backend
                    .query_engine
                    .symbol_search(query, &options)
                    .await;

                // Deduplicate by node_id
                let mut seen = std::collections::HashSet::new();
                result.results.retain(|m| seen.insert(m.node_id));

                Ok(serde_json::to_value(result).map_err(|e| e.to_string())?)
            }

            "codegraph_find_entry_points" => {
                let entry_type = args
                    .get("entryType")
                    .or_else(|| args.get("entry_type"))
                    .and_then(|v| v.as_str());

                let entry_types = match entry_type {
                    Some("http") | Some("http_handler") | Some("HttpHandler") => {
                        vec![crate::ai_query::EntryType::HttpHandler]
                    }
                    Some("cli") | Some("cli_command") | Some("CliCommand") => {
                        vec![crate::ai_query::EntryType::CliCommand]
                    }
                    Some("public") | Some("public_api") | Some("PublicApi") => {
                        vec![crate::ai_query::EntryType::PublicApi]
                    }
                    Some("event") | Some("event_handler") | Some("EventHandler") => {
                        vec![crate::ai_query::EntryType::EventHandler]
                    }
                    Some("test") | Some("TestEntry") => vec![crate::ai_query::EntryType::TestEntry],
                    Some("main") | Some("Main") => vec![crate::ai_query::EntryType::Main],
                    Some("all") => vec![
                        crate::ai_query::EntryType::HttpHandler,
                        crate::ai_query::EntryType::CliCommand,
                        crate::ai_query::EntryType::PublicApi,
                        crate::ai_query::EntryType::Main,
                        crate::ai_query::EntryType::EventHandler,
                        crate::ai_query::EntryType::TestEntry,
                    ],
                    // Default: architectural entry points only (no tests/public API noise)
                    None => vec![
                        crate::ai_query::EntryType::HttpHandler,
                        crate::ai_query::EntryType::CliCommand,
                        crate::ai_query::EntryType::Main,
                        crate::ai_query::EntryType::EventHandler,
                    ],
                    _ => vec![
                        crate::ai_query::EntryType::HttpHandler,
                        crate::ai_query::EntryType::CliCommand,
                        crate::ai_query::EntryType::PublicApi,
                        crate::ai_query::EntryType::Main,
                    ],
                };

                let compact = args
                    .get("compact")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                let limit = args
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize)
                    .unwrap_or(50);

                let result = self
                    .backend
                    .query_engine
                    .find_entry_points_opts(&entry_types, compact, Some(limit))
                    .await;

                // Deduplicate by node_id
                let mut seen = std::collections::HashSet::new();
                let deduped: Vec<_> = result
                    .into_iter()
                    .filter(|e| seen.insert(e.node_id))
                    .collect();

                Ok(serde_json::to_value(deduped).map_err(|e| e.to_string())?)
            }

            "codegraph_find_hot_paths" => {
                let limit = args
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize)
                    .unwrap_or(20);

                let result = {
                    let graph = self.backend.graph.read().await;
                    crate::domain::hot_paths::find_hot_paths(&graph, limit)
                };

                Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
            }

            "codegraph_find_by_imports" => {
                let module_name = args
                    .get("moduleName")
                    .or_else(|| args.get("module_name"))
                    .and_then(|v| v.as_str());
                let libraries = args
                    .get("libraries")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let match_mode_str = args
                    .get("matchMode")
                    .or_else(|| args.get("match_mode"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("contains");
                let limit = args
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize)
                    .unwrap_or(50);

                // Determine which library to search for
                let library = if let Some(name) = module_name {
                    name.to_string()
                } else if let Some(first) = libraries.first() {
                    first.clone()
                } else {
                    return Err("Missing 'moduleName' or 'libraries' parameter".to_string());
                };

                let match_mode = match match_mode_str {
                    "exact" => crate::ai_query::ImportMatchMode::Exact,
                    "prefix" => crate::ai_query::ImportMatchMode::Prefix,
                    _ => crate::ai_query::ImportMatchMode::Fuzzy,
                };

                let options = crate::ai_query::ImportSearchOptions {
                    match_mode,
                    ..Default::default()
                };

                let result = self
                    .backend
                    .query_engine
                    .find_by_imports(&library, &options)
                    .await;

                // Deduplicate by node_id and apply limit
                let mut seen = std::collections::HashSet::new();
                let deduped: Vec<_> = result
                    .into_iter()
                    .filter(|m| seen.insert(m.node_id))
                    .take(limit)
                    .collect();

                Ok(serde_json::to_value(deduped).map_err(|e| e.to_string())?)
            }

            "codegraph_find_by_signature" => {
                let name_pattern = args
                    .get("namePattern")
                    .or_else(|| args.get("name_pattern"))
                    .and_then(|v| v.as_str())
                    .map(String::from);
                let return_type = args
                    .get("returnType")
                    .or_else(|| args.get("return_type"))
                    .and_then(|v| v.as_str())
                    .map(String::from);
                let exact_param_count = args
                    .get("paramCount")
                    .or_else(|| args.get("param_count"))
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize);
                let min_params = args
                    .get("minParams")
                    .or_else(|| args.get("min_params"))
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize);
                let max_params = args
                    .get("maxParams")
                    .or_else(|| args.get("max_params"))
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize);
                let modifiers = args
                    .get("modifiers")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let limit = args
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize);

                // param_count is Option<(min, max)>
                let param_count = if let Some(exact) = exact_param_count {
                    Some((exact, exact))
                } else if min_params.is_some() || max_params.is_some() {
                    Some((min_params.unwrap_or(0), max_params.unwrap_or(usize::MAX)))
                } else {
                    None
                };

                let pattern = crate::ai_query::SignaturePattern {
                    name_pattern,
                    return_type,
                    param_count,
                    modifiers,
                };

                let result = self
                    .backend
                    .query_engine
                    .find_by_signature(&pattern, limit)
                    .await;

                // Deduplicate by node_id
                let mut seen = std::collections::HashSet::new();
                let deduped: Vec<_> = result
                    .into_iter()
                    .filter(|m| seen.insert(m.node_id))
                    .collect();

                Ok(serde_json::to_value(deduped).map_err(|e| e.to_string())?)
            }

            // ==================== Graph Traversal Tools ====================
            "codegraph_get_callers" => {
                let uri = args.get("uri").and_then(|v| v.as_str());
                let line = args.get("line").and_then(|v| v.as_u64()).map(|v| v as u32);
                let node_id = args
                    .get("nodeId")
                    .or_else(|| args.get("node_id"))
                    .and_then(|v| v.as_str());
                let depth = args
                    .get("depth")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32)
                    .unwrap_or(1);

                // Use fallback for uri+line, exact match for node_id
                let (start_node, used_fallback) = if let Some(id_str) = node_id {
                    (parse_node_id(id_str), false)
                } else if let (Some(u), Some(l)) = (uri, line) {
                    match self.find_nearest_node_with_fallback(u, l).await {
                        Some((id, fallback)) => (Some(id), fallback),
                        None => (None, false),
                    }
                } else {
                    (None, false)
                };

                if let Some(start) = start_node {
                    let result = crate::domain::callers::get_callers(
                        &self.backend.graph,
                        &self.backend.query_engine,
                        start,
                        depth,
                        used_fallback,
                        line,
                    )
                    .await;
                    Ok(serde_json::to_value(&result).unwrap_or_default())
                } else {
                    Ok(serde_json::json!({
                        "callers": [],
                        "message": "Could not find starting node. Provide either nodeId or uri+line."
                    }))
                }
            }

            "codegraph_get_callees" => {
                let uri = args.get("uri").and_then(|v| v.as_str());
                let line = args.get("line").and_then(|v| v.as_u64()).map(|v| v as u32);
                let node_id = args
                    .get("nodeId")
                    .or_else(|| args.get("node_id"))
                    .and_then(|v| v.as_str());
                let depth = args
                    .get("depth")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32)
                    .unwrap_or(1);

                // Use fallback for uri+line, exact match for node_id
                let (start_node, used_fallback) = if let Some(id_str) = node_id {
                    (parse_node_id(id_str), false)
                } else if let (Some(u), Some(l)) = (uri, line) {
                    match self.find_nearest_node_with_fallback(u, l).await {
                        Some((id, fallback)) => (Some(id), fallback),
                        None => (None, false),
                    }
                } else {
                    (None, false)
                };

                if let Some(start) = start_node {
                    let result = crate::domain::callers::get_callees(
                        &self.backend.graph,
                        &self.backend.query_engine,
                        start,
                        depth,
                        used_fallback,
                        line,
                    )
                    .await;
                    Ok(serde_json::to_value(&result).unwrap_or_default())
                } else {
                    Ok(serde_json::json!({
                        "callees": [],
                        "message": "Could not find starting node. Provide either nodeId or uri+line."
                    }))
                }
            }

            "codegraph_traverse_graph" => {
                let uri = args.get("uri").and_then(|v| v.as_str());
                let line = args.get("line").and_then(|v| v.as_u64()).map(|v| v as u32);
                let node_id = args
                    .get("startNodeId")
                    .or_else(|| args.get("nodeId"))
                    .or_else(|| args.get("node_id"))
                    .and_then(|v| v.as_str());
                let direction_str = args
                    .get("direction")
                    .and_then(|v| v.as_str())
                    .unwrap_or("outgoing");
                let max_depth = args
                    .get("maxDepth")
                    .or_else(|| args.get("max_depth"))
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32)
                    .unwrap_or(3);
                let limit = args
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize)
                    .unwrap_or(100);

                // Use fallback for uri+line, exact match for node_id
                let (start_node, used_fallback) = if let Some(id_str) = node_id {
                    (parse_node_id(id_str), false)
                } else if let (Some(u), Some(l)) = (uri, line) {
                    match self.find_nearest_node_with_fallback(u, l).await {
                        Some((id, fallback)) => (Some(id), fallback),
                        None => (None, false),
                    }
                } else {
                    (None, false)
                };

                // Parse edgeTypes filter
                let edge_types: Vec<String> = args
                    .get("edgeTypes")
                    .or_else(|| args.get("edge_types"))
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();

                // Parse nodeTypes filter
                let node_types: Vec<crate::ai_query::SymbolType> = args
                    .get("nodeTypes")
                    .or_else(|| args.get("node_types"))
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str())
                            .filter_map(Self::parse_symbol_type)
                            .collect()
                    })
                    .unwrap_or_default();

                let summary = args
                    .get("summary")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                if let Some(start) = start_node {
                    let direction = match direction_str {
                        "incoming" => crate::ai_query::TraversalDirection::Incoming,
                        "both" => crate::ai_query::TraversalDirection::Both,
                        _ => crate::ai_query::TraversalDirection::Outgoing,
                    };

                    let filter = crate::ai_query::TraversalFilter {
                        symbol_types: node_types,
                        edge_types,
                        max_nodes: limit,
                    };

                    let result = self
                        .backend
                        .query_engine
                        .traverse_graph(start, direction, max_depth, &filter)
                        .await;

                    if summary {
                        let node_count = result.len();
                        let edge_types_seen: Vec<String> = result
                            .iter()
                            .filter(|n| !n.edge_type.is_empty())
                            .map(|n| n.edge_type.clone())
                            .collect::<std::collections::HashSet<_>>()
                            .into_iter()
                            .collect();
                        Ok(serde_json::json!({
                            "summary": {
                                "node_count": node_count,
                                "max_depth": max_depth,
                                "direction": direction_str,
                                "edge_types_seen": edge_types_seen,
                            }
                        }))
                    } else {
                        // Add fallback metadata if used
                        let mut response =
                            serde_json::to_value(result).map_err(|e| e.to_string())?;
                        if used_fallback {
                            if let Some(obj) = response.as_object_mut() {
                                let symbol_name = {
                                    let graph = self.backend.graph.read().await;
                                    graph
                                        .get_node(start)
                                        .ok()
                                        .and_then(|n| {
                                            n.properties.get_string("name").map(|s| s.to_string())
                                        })
                                        .unwrap_or_default()
                                };
                                obj.insert("used_fallback".to_string(), serde_json::json!(true));
                                obj.insert(
                                    "fallback_message".to_string(),
                                    serde_json::json!(format!(
                                        "No symbol at line {}. Using nearest symbol '{}' instead.",
                                        line.unwrap_or(0),
                                        symbol_name
                                    )),
                                );
                            }
                        }
                        Ok(response)
                    }
                } else {
                    Ok(serde_json::json!({
                        "nodes": [],
                        "edges": [],
                        "message": "Could not find starting node. Provide either startNodeId or uri+line."
                    }))
                }
            }

            "codegraph_get_symbol_info" => {
                let uri = args.get("uri").and_then(|v| v.as_str());
                let line = args.get("line").and_then(|v| v.as_u64()).map(|v| v as u32);
                let node_id = args
                    .get("nodeId")
                    .or_else(|| args.get("node_id"))
                    .and_then(|v| v.as_str());
                let include_refs = args
                    .get("includeReferences")
                    .or_else(|| args.get("include_references"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                // Use fallback for uri+line, exact match for node_id
                let (target_node, used_fallback) = if let Some(id_str) = node_id {
                    (parse_node_id(id_str), false)
                } else if let (Some(u), Some(l)) = (uri, line) {
                    match self.find_nearest_node_with_fallback(u, l).await {
                        Some((id, fallback)) => (Some(id), fallback),
                        None => (None, false),
                    }
                } else {
                    (None, false)
                };

                if let Some(node_id) = target_node {
                    match crate::domain::symbol_info::get_symbol_info(
                        &self.backend.graph,
                        &self.backend.query_engine,
                        node_id,
                        include_refs,
                        used_fallback,
                        line,
                    )
                    .await
                    {
                        Some(response) => Ok(serde_json::to_value(&response).unwrap_or_default()),
                        None => Ok(serde_json::json!({
                            "error": "Symbol not found"
                        })),
                    }
                } else {
                    Ok(serde_json::json!({
                        "error": "Could not find symbol. Provide either nodeId or uri+line."
                    }))
                }
            }

            "codegraph_get_detailed_symbol" => {
                let uri = args.get("uri").and_then(|v| v.as_str());
                let line = args.get("line").and_then(|v| v.as_u64()).map(|v| v as u32);
                let node_id = args
                    .get("nodeId")
                    .or_else(|| args.get("node_id"))
                    .and_then(|v| v.as_str());
                let include_source = args
                    .get("includeSource")
                    .or_else(|| args.get("include_source"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                let include_callers = args
                    .get("includeCallers")
                    .or_else(|| args.get("include_callers"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                let include_callees = args
                    .get("includeCallees")
                    .or_else(|| args.get("include_callees"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);

                // Use fallback for uri+line, exact match for node_id
                let (target_node, used_fallback) = if let Some(id_str) = node_id {
                    (parse_node_id(id_str), false)
                } else if let (Some(u), Some(l)) = (uri, line) {
                    match self.find_nearest_node_with_fallback(u, l).await {
                        Some((id, fallback)) => (Some(id), fallback),
                        None => (None, false),
                    }
                } else {
                    (None, false)
                };

                if let Some(node_id) = target_node {
                    let result = crate::domain::symbol_info::get_detailed_symbol(
                        &self.backend.graph,
                        &self.backend.query_engine,
                        node_id,
                        include_source,
                        include_callers,
                        include_callees,
                        used_fallback,
                        line,
                    )
                    .await;
                    Ok(serde_json::to_value(&result).unwrap_or_default())
                } else {
                    Ok(serde_json::json!({
                        "error": "Could not find symbol. Provide either nodeId or uri+line."
                    }))
                }
            }

            // ==================== Dependency Analysis Tools ====================
            "codegraph_get_dependency_graph" => {
                let uri = args
                    .get("uri")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing 'uri' parameter")?;
                let depth = args
                    .get("depth")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize)
                    .unwrap_or(3);
                let direction = args
                    .get("direction")
                    .and_then(|v| v.as_str())
                    .unwrap_or("both");
                let _include_external = args
                    .get("includeExternal")
                    .or_else(|| args.get("include_external"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let summary = args
                    .get("summary")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                let typed_result = {
                    let url = tower_lsp::lsp_types::Url::parse(uri)
                        .map_err(|_| "Invalid URI".to_string())?;
                    let path = url
                        .to_file_path()
                        .map_err(|_| "Invalid file path".to_string())?;
                    let path_str = path.to_string_lossy().to_string();
                    let graph = self.backend.graph.read().await;
                    crate::domain::dependency_graph::get_dependency_graph(
                        &graph, &path_str, depth, direction,
                    )
                };

                if summary {
                    let node_count = typed_result.nodes.len();
                    let edge_count = typed_result.edges.len();
                    Ok(serde_json::json!({
                        "summary": {
                            "node_count": node_count,
                            "edge_count": edge_count,
                            "depth": depth,
                            "direction": direction,
                        }
                    }))
                } else {
                    Ok(serde_json::to_value(&typed_result).unwrap_or_default())
                }
            }

            "codegraph_get_call_graph" => {
                let uri = args
                    .get("uri")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing 'uri' parameter")?;
                let line = args
                    .get("line")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32)
                    .unwrap_or(0);
                let depth = args
                    .get("depth")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32)
                    .unwrap_or(3);
                let direction = args
                    .get("direction")
                    .and_then(|v| v.as_str())
                    .unwrap_or("both");
                let summary = args
                    .get("summary")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                let (start_node, used_fallback) =
                    match self.find_nearest_node_with_fallback(uri, line).await {
                        Some((id, fallback)) => (Some(id), fallback),
                        None => (None, false),
                    };

                let result = match start_node {
                    Some(start) => {
                        let typed = crate::domain::call_graph::get_call_graph(
                            &self.backend.graph,
                            &self.backend.query_engine,
                            start,
                            depth,
                            direction,
                            used_fallback,
                            Some(line),
                        )
                        .await;
                        serde_json::to_value(&typed).unwrap_or_default()
                    }
                    None => serde_json::json!({
                        "nodes": [],
                        "edges": [],
                        "message": "Could not find symbol at location"
                    }),
                };

                if summary {
                    // Count callers/callees from nodes array (each has a "direction" field)
                    let nodes = result.get("nodes").and_then(|v| v.as_array());
                    let caller_count = nodes
                        .map(|a| {
                            a.iter()
                                .filter(|n| {
                                    n.get("direction").and_then(|d| d.as_str()) == Some("caller")
                                })
                                .count()
                        })
                        .unwrap_or(0);
                    let callee_count = nodes
                        .map(|a| {
                            a.iter()
                                .filter(|n| {
                                    n.get("direction").and_then(|d| d.as_str()) == Some("callee")
                                })
                                .count()
                        })
                        .unwrap_or(0);
                    let symbol = result
                        .get("root_node")
                        .or_else(|| result.get("symbol_name"))
                        .cloned()
                        .unwrap_or(serde_json::json!(null));
                    Ok(serde_json::json!({
                        "symbol": symbol,
                        "summary": {
                            "caller_count": caller_count,
                            "callee_count": callee_count,
                            "depth": depth,
                            "direction": direction,
                        }
                    }))
                } else {
                    Ok(result)
                }
            }

            "codegraph_analyze_impact" => {
                let uri = args
                    .get("uri")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing 'uri' parameter")?;
                let line = args
                    .get("line")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32)
                    .unwrap_or(0);
                let change_type = args
                    .get("changeType")
                    .or_else(|| args.get("change_type"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("modify");
                let summary = args
                    .get("summary")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                let (start_node, used_fallback) =
                    match self.find_nearest_node_with_fallback(uri, line).await {
                        Some((id, fallback)) => (Some(id), fallback),
                        None => (None, false),
                    };

                let result = match start_node {
                    Some(start) => {
                        let typed = crate::domain::impact::analyze_impact(
                            &self.backend.graph,
                            &self.backend.query_engine,
                            start,
                            change_type,
                            used_fallback,
                            Some(line),
                            Some(&self.backend.project_slug),
                        )
                        .await;
                        serde_json::to_value(&typed).unwrap_or_default()
                    }
                    None => serde_json::json!({
                        "impacted": [],
                        "risk_level": "unknown",
                        "message": "Could not find symbol at location"
                    }),
                };

                if summary {
                    let total_impacted = result
                        .get("total_impacted")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let direct_impacted = result
                        .get("direct_impacted")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let risk_level = result
                        .get("risk_level")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    let symbol_name = result
                        .get("symbol_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let symbol_id = result
                        .get("symbol_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    Ok(serde_json::json!({
                        "symbol": symbol_name,
                        "symbol_id": symbol_id,
                        "summary": {
                            "total_impacted": total_impacted,
                            "direct_impacted": direct_impacted,
                            "risk_level": risk_level,
                            "change_type": change_type,
                        }
                    }))
                } else {
                    Ok(result)
                }
            }

            // ==================== Analysis Tools ====================
            "codegraph_get_ai_context" => {
                let uri = args
                    .get("uri")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing 'uri' parameter")?;
                let line = args
                    .get("line")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32)
                    .unwrap_or(0);
                let intent = args
                    .get("intent")
                    .or_else(|| args.get("context_type"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("explain");
                let max_tokens = args
                    .get("maxTokens")
                    .or_else(|| args.get("max_tokens"))
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize)
                    .unwrap_or(4000);

                let url =
                    tower_lsp::lsp_types::Url::parse(uri).map_err(|_| "Invalid URI".to_string())?;
                let path = url
                    .to_file_path()
                    .map_err(|_| "Invalid file path".to_string())?;
                let path_str = path.to_string_lossy().to_string();

                let graph = self.backend.graph.read().await;
                let result = crate::domain::ai_context::get_ai_context(
                    &graph, &path_str, line, intent, max_tokens,
                )
                .ok_or_else(|| {
                    format!("No symbols found in '{uri}'. Try indexing the workspace first.")
                })?;

                Ok(serde_json::to_value(result).map_err(|e| e.to_string())?)
            }

            "codegraph_get_edit_context" => {
                let uri = args
                    .get("uri")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing 'uri' parameter")?;
                let line = args
                    .get("line")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32)
                    .ok_or("Missing 'line' parameter")?;
                let max_tokens = args
                    .get("maxTokens")
                    .or_else(|| args.get("max_tokens"))
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize)
                    .unwrap_or(8000);

                let file_path = tower_lsp::lsp_types::Url::parse(uri)
                    .ok()
                    .and_then(|u| u.to_file_path().ok())
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();
                let result = crate::domain::edit_context::get_edit_context(
                    &self.backend.graph,
                    &self.backend.query_engine,
                    &self.backend.memory_manager,
                    &self.backend.workspace_folders,
                    &file_path,
                    uri,
                    line,
                    max_tokens,
                )
                .await;
                Ok(match result {
                    Ok(ctx) => serde_json::to_value(&ctx).unwrap_or_default(),
                    Err(e) => serde_json::to_value(&e).unwrap_or_default(),
                })
            }

            "codegraph_get_curated_context" => {
                let query = args
                    .get("query")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing 'query' parameter")?;
                let uri = args.get("uri").and_then(|v| v.as_str());
                let max_tokens = args
                    .get("maxTokens")
                    .or_else(|| args.get("max_tokens"))
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize)
                    .unwrap_or(8000);
                let max_symbols = args
                    .get("maxSymbols")
                    .or_else(|| args.get("max_symbols"))
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize)
                    .unwrap_or(5);

                let anchor_path: Option<String> = uri.and_then(|u| {
                    tower_lsp::lsp_types::Url::parse(u)
                        .ok()
                        .and_then(|parsed| parsed.to_file_path().ok())
                        .map(|p| p.to_string_lossy().to_string())
                });
                let result = crate::domain::curated_context::get_curated_context(
                    &self.backend.graph,
                    &self.backend.query_engine,
                    &self.backend.memory_manager,
                    query,
                    anchor_path.as_deref(),
                    max_tokens,
                    max_symbols,
                )
                .await;
                Ok(match result {
                    Ok(ctx) => serde_json::to_value(&ctx).unwrap_or_default(),
                    Err(e) => serde_json::to_value(&e).unwrap_or_default(),
                })
            }

            "codegraph_find_related_tests" => {
                let uri = args
                    .get("uri")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing 'uri' parameter")?;
                let line = args
                    .get("line")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32)
                    .unwrap_or(0);
                let limit = args
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize)
                    .unwrap_or(10);

                // Resolve file path
                let url = match tower_lsp::lsp_types::Url::parse(uri) {
                    Ok(u) => u,
                    Err(_) => {
                        return Ok(serde_json::json!({
                            "tests": [],
                            "message": "Invalid URI"
                        }))
                    }
                };
                let file_path = match url.to_file_path() {
                    Ok(p) => p,
                    Err(_) => {
                        return Ok(serde_json::json!({
                            "tests": [],
                            "message": "Invalid file path"
                        }))
                    }
                };
                let path_str = file_path.to_string_lossy().to_string();

                // Resolve target node (with fallback to nearest symbol)
                let (target_node_id, used_fallback, symbol_name) =
                    match self.find_nearest_node_with_fallback(uri, line).await {
                        Some((id, fallback)) => {
                            let name = {
                                let graph = self.backend.graph.read().await;
                                graph
                                    .get_node(id)
                                    .ok()
                                    .map(|n| node_props::name(n).to_string())
                                    .unwrap_or_default()
                            };
                            (Some(id), fallback, name)
                        }
                        None => (None, false, String::new()),
                    };

                let params = crate::domain::related_tests::FindRelatedTestsParams {
                    path: path_str.clone(),
                    target_node_id,
                    limit,
                };

                let graph = self.backend.graph.read().await;
                let result = crate::domain::related_tests::find_related_tests(
                    &graph,
                    &self.backend.query_engine,
                    params,
                )
                .await;

                let tests: Vec<_> = result
                    .tests
                    .iter()
                    .map(|t| {
                        serde_json::json!({
                            "name": t.name,
                            "id": t.node_id.to_string(),
                            "relationship": t.relationship,
                        })
                    })
                    .collect();

                let mut response = if let Some(target_id) = target_node_id {
                    serde_json::json!({
                        "target_id": target_id.to_string(),
                        "symbol_name": symbol_name,
                        "tests": tests,
                        "total": tests.len(),
                    })
                } else {
                    serde_json::json!({
                        "file": path_str,
                        "tests": tests,
                        "total": tests.len(),
                    })
                };

                if used_fallback {
                    if let Some(obj) = response.as_object_mut() {
                        obj.insert("used_fallback".to_string(), serde_json::json!(true));
                        obj.insert(
                            "fallback_message".to_string(),
                            serde_json::json!(format!(
                                "No symbol at line {}. Using nearest symbol '{}' instead.",
                                line, symbol_name
                            )),
                        );
                    }
                }

                Ok(serde_json::to_value(response).map_err(|e| e.to_string())?)
            }

            "codegraph_analyze_complexity" => {
                let uri = args
                    .get("uri")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing 'uri' parameter")?;
                let line = args.get("line").and_then(|v| v.as_u64()).map(|v| v as u32);
                let threshold = args
                    .get("threshold")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32)
                    .unwrap_or(10);
                let summary_only = args
                    .get("summary")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                let url =
                    tower_lsp::lsp_types::Url::parse(uri).map_err(|_| "Invalid URI".to_string())?;
                let path = url
                    .to_file_path()
                    .map_err(|_| "Invalid file path".to_string())?;
                let graph = self.backend.graph.read().await;
                let path_str = path.to_string_lossy().to_string();
                let file_nodes = graph
                    .query()
                    .property("path", path_str)
                    .execute()
                    .unwrap_or_default();
                let result = crate::handlers::metrics::analyze_file_complexity(
                    &graph,
                    &file_nodes,
                    line,
                    threshold,
                );

                let functions: Vec<serde_json::Value> = result
                    .functions
                    .iter()
                    .map(|f| {
                        serde_json::json!({
                            "name": f.name,
                            "complexity": f.complexity,
                            "grade": f.grade.to_string(),
                            "node_id": f.node_id.to_string(),
                            "line_start": f.line_start,
                            "line_end": f.line_end,
                            "details": {
                                "complexity_branches": f.details.complexity_branches,
                                "complexity_loops": f.details.complexity_loops,
                                "complexity_logical_ops": f.details.complexity_logical_ops,
                                "complexity_nesting": f.details.complexity_nesting,
                                "complexity_exceptions": f.details.complexity_exceptions,
                                "complexity_early_returns": f.details.complexity_early_returns,
                                "lines_of_code": f.details.lines_of_code,
                            }
                        })
                    })
                    .collect();

                let summary = serde_json::json!({
                    "total_functions": result.functions.len(),
                    "average_complexity": result.average_complexity,
                    "max_complexity": result.max_complexity,
                    "above_threshold": result.functions_above_threshold,
                    "threshold": result.threshold,
                    "overall_grade": result.overall_grade.to_string(),
                });

                if summary_only {
                    Ok(serde_json::json!({ "summary": summary }))
                } else if functions.is_empty() {
                    Ok(serde_json::json!({
                        "functions": [],
                        "summary": summary,
                        "recommendations": [],
                        "note": "No functions found in this file. This may indicate: (1) the language parser doesn't extract function-level details for this file type, (2) the file doesn't contain any functions, or (3) the workspace needs to be re-indexed."
                    }))
                } else {
                    Ok(serde_json::json!({
                        "functions": functions,
                        "summary": summary,
                        "recommendations": result.recommendations,
                    }))
                }
            }

            // ==================== Memory Tools ====================
            "codegraph_memory_search" => {
                let query = args
                    .get("query")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing 'query' parameter")?;
                let limit = args
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize)
                    .unwrap_or(10);
                let current_only = args
                    .get("currentOnly")
                    .or_else(|| args.get("current_only"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                let kinds = Self::parse_kinds_filter(&args);
                let tags = Self::parse_tags_filter(&args);

                let config = crate::memory::SearchConfig {
                    limit,
                    current_only,
                    kinds,
                    tags,
                    ..Default::default()
                };

                let results = self
                    .backend
                    .memory_manager
                    .search(query, &config, &[])
                    .await
                    .map_err(|e| format!("Memory search failed: {:?}", e))?;

                // Deduplicate by title and commit hash (git-mined commits create duplicates)
                let mut seen_titles = std::collections::HashSet::new();
                let mut seen_commits = std::collections::HashSet::new();
                let results_json: Vec<serde_json::Value> = results
                    .iter()
                    .filter(|r| {
                        // Skip if commit hash already seen
                        if let crate::memory::MemorySource::GitHistory { ref commit_hash } =
                            r.memory.source
                        {
                            if !seen_commits.insert(commit_hash.clone()) {
                                return false;
                            }
                        }
                        seen_titles.insert(r.memory.title.clone())
                    })
                    .map(|r| {
                        serde_json::json!({
                            "id": r.memory.id,
                            "title": r.memory.title,
                            "content": r.memory.content,
                            "kind": r.memory.kind.discriminant_name(),
                            "score": r.score,
                            "created_at": r.memory.temporal.created_at.to_rfc3339(),
                            "tags": r.memory.tags,
                        })
                    })
                    .collect();

                Ok(serde_json::json!({
                    "results": results_json,
                    "total": results_json.len()
                }))
            }

            "codegraph_memory_stats" => {
                let result = self
                    .backend
                    .memory_manager
                    .stats()
                    .await
                    .map_err(|e| format!("Failed to get memory stats: {:?}", e))?;

                Ok(result)
            }

            "codegraph_memory_store" => {
                let kind = args
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing 'kind' parameter")?;
                let title = args
                    .get("title")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing 'title' parameter")?;
                let content = args
                    .get("content")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing 'content' parameter")?;
                let tags = args
                    .get("tags")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();

                let memory = self.build_memory_node(kind, title, content, &tags, &args)?;

                let id = self
                    .backend
                    .memory_manager
                    .put(memory)
                    .await
                    .map_err(|e| format!("Failed to store memory: {:?}", e))?;

                Ok(serde_json::json!({
                    "id": id,
                    "status": "stored"
                }))
            }

            "codegraph_memory_get" => {
                let id = args
                    .get("id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing 'id' parameter")?;

                let result = self
                    .backend
                    .memory_manager
                    .get(id)
                    .await
                    .map_err(|e| format!("Failed to get memory: {:?}", e))?;

                match result {
                    Some(memory) => Ok(serde_json::json!({
                        "id": memory.id,
                        "title": memory.title,
                        "content": memory.content,
                        "kind": memory.kind.discriminant_name(),
                        "tags": memory.tags,
                        "created_at": memory.temporal.created_at.to_rfc3339(),
                        "invalidated": memory.temporal.invalid_at.is_some(),
                    })),
                    None => Ok(serde_json::json!({
                        "error": "Memory not found"
                    })),
                }
            }

            "codegraph_memory_context" => {
                let uri = args
                    .get("uri")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing 'uri' parameter")?;
                let limit = args
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize)
                    .unwrap_or(5);

                // Find code nodes at the given location and search for related memories
                let url =
                    tower_lsp::lsp_types::Url::parse(uri).map_err(|_| "Invalid URI".to_string())?;
                let path = url
                    .to_file_path()
                    .map_err(|_| "Invalid file path".to_string())?;
                let path_str = path.to_string_lossy().to_string();

                // Search for memories related to this file
                let current_only = args
                    .get("currentOnly")
                    .or_else(|| args.get("current_only"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                let kinds = Self::parse_kinds_filter(&args);
                let tags = Self::parse_tags_filter(&args);
                let config = crate::memory::SearchConfig {
                    limit,
                    current_only,
                    kinds,
                    tags,
                    ..Default::default()
                };

                let results = self
                    .backend
                    .memory_manager
                    .search(&path_str, &config, &[])
                    .await
                    .map_err(|e| format!("Memory search failed: {:?}", e))?;

                // Deduplicate by title and commit hash (git-mined commits create duplicates)
                let mut seen_titles = std::collections::HashSet::new();
                let mut seen_commits = std::collections::HashSet::new();
                let results_json: Vec<serde_json::Value> = results
                    .iter()
                    .filter(|r| {
                        if let crate::memory::MemorySource::GitHistory { ref commit_hash } =
                            r.memory.source
                        {
                            if !seen_commits.insert(commit_hash.clone()) {
                                return false;
                            }
                        }
                        seen_titles.insert(r.memory.title.clone())
                    })
                    .map(|r| {
                        serde_json::json!({
                            "id": r.memory.id,
                            "title": r.memory.title,
                            "content": r.memory.content,
                            "kind": r.memory.kind.discriminant_name(),
                            "score": r.score,
                            "tags": r.memory.tags,
                        })
                    })
                    .collect();

                Ok(serde_json::json!({
                    "uri": uri,
                    "memories": results_json,
                    "total": results_json.len()
                }))
            }

            "codegraph_memory_invalidate" => {
                let id = args
                    .get("id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing 'id' parameter")?;

                // Try to invalidate — idempotent: re-invalidating an already-invalidated
                // memory succeeds silently (returns "already_invalidated" status).
                match self
                    .backend
                    .memory_manager
                    .invalidate(id, "Invalidated via MCP")
                    .await
                {
                    Ok(()) => Ok(serde_json::json!({
                        "id": id,
                        "status": "invalidated"
                    })),
                    Err(e) => {
                        let err_str = format!("{:?}", e);
                        // If the memory doesn't exist in the primary index, check if it's
                        // already invalidated (visible via get_all_memories with currentOnly=false)
                        if err_str.contains("not found") || err_str.contains("Not found") {
                            // Check if it exists as an invalidated memory
                            let all_memories = self
                                .backend
                                .memory_manager
                                .get_all_memories(false)
                                .await
                                .unwrap_or_default();
                            let is_already_invalidated =
                                all_memories.iter().any(|m| m.id.to_string() == id);
                            if is_already_invalidated {
                                Ok(serde_json::json!({
                                    "id": id,
                                    "status": "already_invalidated"
                                }))
                            } else {
                                Err(format!("Memory not found: {}", id))
                            }
                        } else {
                            Err(format!("Failed to invalidate memory: {}", err_str))
                        }
                    }
                }
            }

            "codegraph_memory_list" => {
                let current_only = args
                    .get("currentOnly")
                    .or_else(|| args.get("current_only"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                let limit = args
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize)
                    .unwrap_or(50);
                let offset = args
                    .get("offset")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize)
                    .unwrap_or(0);
                let kinds = Self::parse_kinds_filter(&args);
                let tags = Self::parse_tags_filter(&args);

                let all_memories = self
                    .backend
                    .memory_manager
                    .get_all_memories(current_only)
                    .await
                    .map_err(|e| format!("Failed to list memories: {:?}", e))?;

                // Apply kinds/tags filters and deduplicate by title + commit hash
                let mut seen_titles = std::collections::HashSet::new();
                let mut seen_commits = std::collections::HashSet::new();
                let filtered: Vec<&crate::memory::MemoryNode> = all_memories
                    .iter()
                    .filter(|m| {
                        if !kinds.is_empty()
                            && !kinds.iter().any(|k| Self::kind_matches_filter(k, &m.kind))
                        {
                            return false;
                        }
                        if !tags.is_empty() && !tags.iter().any(|t| m.tags.contains(t)) {
                            return false;
                        }
                        // Deduplicate by commit hash (git-mined commits create duplicates)
                        if let crate::memory::MemorySource::GitHistory { ref commit_hash } =
                            m.source
                        {
                            if !seen_commits.insert(commit_hash.clone()) {
                                return false;
                            }
                        }
                        seen_titles.insert(m.title.clone())
                    })
                    .collect();

                let total = filtered.len();
                let memories_json: Vec<serde_json::Value> = filtered
                    .into_iter()
                    .skip(offset)
                    .take(limit)
                    .map(|m| {
                        serde_json::json!({
                            "id": m.id,
                            "title": m.title,
                            "kind": m.kind.discriminant_name(),
                            "tags": m.tags,
                            "created_at": m.temporal.created_at.to_rfc3339(),
                            "invalidated": m.temporal.invalid_at.is_some(),
                        })
                    })
                    .collect();

                Ok(serde_json::json!({
                    "memories": memories_json,
                    "total": total,
                    "offset": offset,
                    "limit": limit,
                }))
            }

            // ==================== Admin Tools ====================
            "codegraph_reindex_workspace" => {
                let force = args.get("force").and_then(|v| v.as_bool()).unwrap_or(false);
                tracing::info!("Reindexing workspace (force={})...", force);

                if force {
                    // Force: clear graph and hash cache for full rebuild
                    {
                        let mut graph = self.backend.graph.write().await;
                        *graph = codegraph::CodeGraph::in_memory()
                            .map_err(|e| format!("Failed to create new graph: {}", e))?;
                    }
                    self.backend.index_state.lock().await.clear();
                }
                // else: incremental — index_file skips unchanged files via hash cache

                // Reindex the workspace
                let (total, parsed) = self.backend.index_workspace().await;
                tracing::info!(
                    "Reindexed: {} total, {} parsed, {} skipped",
                    total,
                    parsed,
                    total - parsed
                );

                Ok(serde_json::json!({
                    "status": "success",
                    "message": format!("Reindexed {} files ({} changed, {} skipped)", total, parsed, total - parsed),
                    "files_indexed": total,
                    "files_parsed": parsed,
                    "files_skipped": total - parsed
                }))
            }

            // ==================== Index File(s) ====================
            "codegraph_index_files" => {
                // Accept both "paths" (MCP convention) and "files" (VS Code LM tools)
                let raw: Vec<String> = args
                    .get("paths")
                    .or_else(|| args.get("files"))
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();

                if raw.is_empty() {
                    return Err("paths parameter is required (array of file paths)".to_string());
                }

                // Convert file:// URIs to plain paths
                let paths: Vec<PathBuf> = raw
                    .iter()
                    .map(|s| {
                        if let Some(p) = s.strip_prefix("file://") {
                            PathBuf::from(p)
                        } else {
                            PathBuf::from(s)
                        }
                    })
                    .collect();

                let (indexed, failed) = self.backend.add_files_to_index(&paths).await;

                Ok(serde_json::json!({
                    "status": if failed == 0 { "success" } else { "partial" },
                    "files_indexed": indexed,
                    "files_failed": failed,
                    "message": format!("Indexed {} files ({} failed)", indexed, failed)
                }))
            }

            // ==================== Index Directory ====================
            "codegraph_index_directory" => {
                let path = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or("path parameter is required")?;
                let embed = args.get("embed").and_then(|v| v.as_bool()).unwrap_or(false);
                let dir = PathBuf::from(path);

                let count = self.backend.add_directory_to_index(&dir, embed).await;

                Ok(serde_json::json!({
                    "status": "success",
                    "files_indexed": count,
                    "directory": path,
                    "embedded": embed,
                    "message": format!("Added {} files from {}{}", count, path,
                        if embed { " (with embeddings)" } else { "" })
                }))
            }

            // ==================== Circular Dependencies ====================
            "codegraph_find_circular_deps" => {
                let max_cycle_length = args
                    .get("max_cycle_length")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize)
                    .unwrap_or(10);
                let compact = args
                    .get("compact")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                let graph = self.backend.graph.read().await;
                let result =
                    crate::domain::circular_deps::find_circular_deps(&graph, max_cycle_length);

                if compact {
                    Ok(serde_json::json!({
                        "has_circular_deps": result.has_circular_deps,
                        "total_cycles": result.total_cycles,
                    }))
                } else {
                    Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
                }
            }

            // ==================== Find Implementors ====================
            "codegraph_find_implementors" => {
                let struct_type = args.get("structType").and_then(|v| v.as_str());
                let field_name = args.get("fieldName").and_then(|v| v.as_str());
                let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;

                let results = self
                    .backend
                    .query_engine
                    .find_implementors(struct_type, field_name)
                    .await;

                let total = results.len();
                let truncated = results.into_iter().take(limit).collect::<Vec<_>>();

                Ok(serde_json::json!({
                    "implementors": truncated,
                    "total": total,
                    "filters": {
                        "struct_type": struct_type,
                        "field_name": field_name,
                    }
                }))
            }

            // ==================== Module Summary ====================
            "codegraph_get_module_summary" => {
                let directory = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing 'path' parameter")?;
                let top_n = args
                    .get("top_n")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize)
                    .unwrap_or(5);

                let graph = self.backend.graph.read().await;
                let result = crate::domain::module_summary::get_module_summary(
                    &graph, directory, top_n,
                );

                Ok(serde_json::to_value(result).map_err(|e| e.to_string())?)
            }

            // ==================== Dead Import Analysis ====================
            "codegraph_find_dead_imports" => {
                let file_path: Option<String> = args
                    .get("uri")
                    .and_then(|v| v.as_str())
                    .and_then(|uri| tower_lsp::lsp_types::Url::parse(uri).ok())
                    .and_then(|url| url.to_file_path().ok())
                    .map(|p| p.to_string_lossy().to_string());
                let limit = args
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(100) as usize;

                let typed_result = {
                    let graph = self.backend.graph.read().await;
                    crate::domain::dead_imports::find_dead_imports(
                        &graph,
                        file_path.as_deref(),
                    )
                };

                let dead_count = typed_result.dead_count;
                let total_imports = typed_result.total_imports;
                let unresolved_count = typed_result.unresolved_imports.len();
                let dead_imports: Vec<_> = typed_result
                    .dead_imports
                    .into_iter()
                    .take(limit)
                    .collect();

                Ok(serde_json::json!({
                    "dead_imports": dead_imports,
                    "unresolved_imports": typed_result.unresolved_imports,
                    "total_imports": total_imports,
                    "dead_count": dead_count,
                    "unresolved_count": unresolved_count,
                    "scanned_file": file_path,
                }))
            }

            "codegraph_search_by_pattern" => {
                let pattern = match args.get("pattern").and_then(|v| v.as_str()) {
                    Some(p) => p.to_string(),
                    None => {
                        return Ok(serde_json::json!({
                            "error": "Missing required argument: pattern"
                        }))
                    }
                };

                let scope = args
                    .get("scope")
                    .and_then(|v| v.as_str())
                    .map(String::from);

                let node_type = args
                    .get("node_type")
                    .and_then(|v| v.as_str())
                    .map(String::from);

                let limit = args
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize)
                    .unwrap_or(50);

                let result = {
                    let graph = self.backend.graph.read().await;
                    crate::domain::pattern_search::search_by_pattern(
                        &graph,
                        &pattern,
                        scope.as_deref(),
                        node_type.as_deref(),
                        limit,
                    )
                };

                Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
            }

            "codegraph_search_by_error" => {
                let error_type = args
                    .get("error_type")
                    .or_else(|| args.get("errorType"))
                    .and_then(|v| v.as_str())
                    .map(String::from);
                let mode = args
                    .get("mode")
                    .and_then(|v| v.as_str())
                    .unwrap_or("any")
                    .to_string();
                let limit = args
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize)
                    .unwrap_or(50);

                let result = {
                    let graph = self.backend.graph.read().await;
                    crate::domain::error_search::search_by_error(
                        &graph,
                        error_type.as_deref(),
                        &mode,
                        limit,
                    )
                };

                Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
            }

            // ==================== Pro / Unknown Tool ====================
            other => {
                // Fall through to pro tool provider
                if let Some(future) =
                    self.pro_provider
                        .handle_tool(other, args.clone(), &self.backend)
                {
                    future.await
                } else {
                    Err(format!("Unknown tool: {}", other))
                }
            }
        }
    }

    /// Find a node at location with broader fallback, returning whether fallback was used.
    ///
    /// Strategy:
    /// 1. First try exact match (line within symbol's range)
    /// 2. If no exact match, find the closest symbol in the file (no distance limit)
    ///
    /// Returns (node_id, used_fallback) where used_fallback is true if not an exact match.
    async fn find_nearest_node_with_fallback(
        &self,
        uri: &str,
        line: u32,
    ) -> Option<(codegraph::NodeId, bool)> {
        let url = tower_lsp::lsp_types::Url::parse(uri).ok()?;
        let path = url.to_file_path().ok()?;
        let path_str = path.to_string_lossy().to_string();
        let graph = self.backend.graph.read().await;
        crate::domain::node_resolution::find_nearest_node(&graph, &path_str, line)
    }

    /// Build a memory node from parameters
    fn build_memory_node(
        &self,
        kind: &str,
        title: &str,
        content: &str,
        tags: &[String],
        args: &Value,
    ) -> Result<crate::memory::MemoryNode, String> {
        let mut builder = crate::memory::MemoryNodeBuilder::new()
            .title(title)
            .content(content);

        for tag in tags {
            builder = builder.tag(tag);
        }

        // Set kind-specific fields
        builder = match kind {
            "debug_context" => {
                let problem = args
                    .get("problem")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown problem");
                let solution = args
                    .get("solution")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown solution");
                builder.debug_context(problem, solution)
            }
            "architectural_decision" => {
                let decision = args
                    .get("decision")
                    .and_then(|v| v.as_str())
                    .unwrap_or(title);
                let rationale = args
                    .get("rationale")
                    .and_then(|v| v.as_str())
                    .unwrap_or(content);
                builder.architectural_decision(decision, rationale)
            }
            "known_issue" => {
                let description = args
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or(content);
                let severity = args
                    .get("severity")
                    .and_then(|v| v.as_str())
                    .unwrap_or("medium");
                let severity_enum = match severity {
                    "critical" => crate::memory::IssueSeverity::Critical,
                    "high" => crate::memory::IssueSeverity::High,
                    "low" => crate::memory::IssueSeverity::Low,
                    _ => crate::memory::IssueSeverity::Medium,
                };
                builder.known_issue(description, severity_enum)
            }
            "convention" => {
                let name = args.get("name").and_then(|v| v.as_str()).unwrap_or(title);
                let description = args
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or(content);
                builder.convention(name, description)
            }
            "project_context" => {
                let topic = args.get("topic").and_then(|v| v.as_str()).unwrap_or(title);
                let description = args
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or(content);
                builder.project_context(topic, description)
            }
            _ => {
                return Err(format!(
                    "Unknown memory kind: {}. Use: debug_context, architectural_decision, known_issue, convention, project_context",
                    kind
                ));
            }
        };

        builder
            .build()
            .map_err(|e| format!("Failed to build memory: {:?}", e))
    }

    /// Parse a string into a SymbolType
    fn parse_symbol_type(s: &str) -> Option<crate::ai_query::SymbolType> {
        match s.to_lowercase().as_str() {
            "function" | "method" => Some(crate::ai_query::SymbolType::Function),
            "class" | "struct" => Some(crate::ai_query::SymbolType::Class),
            "variable" | "constant" => Some(crate::ai_query::SymbolType::Variable),
            "module" | "namespace" => Some(crate::ai_query::SymbolType::Module),
            "interface" | "trait" => Some(crate::ai_query::SymbolType::Interface),
            "type" | "enum" => Some(crate::ai_query::SymbolType::Type),
            _ => None,
        }
    }

    /// Parse `kinds` filter from MCP args into MemoryKindFilter vec
    fn parse_kinds_filter(args: &serde_json::Value) -> Vec<crate::memory::MemoryKindFilter> {
        args.get("kinds")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .filter_map(Self::parse_kind_str)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Parse `tags` filter from MCP args
    fn parse_tags_filter(args: &serde_json::Value) -> Vec<String> {
        args.get("tags")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Parse a kind string into a MemoryKindFilter
    fn parse_kind_str(s: &str) -> Option<crate::memory::MemoryKindFilter> {
        match s {
            "debug_context" | "DebugContext" => Some(crate::memory::MemoryKindFilter::DebugContext),
            "architectural_decision" | "ArchitecturalDecision" => {
                Some(crate::memory::MemoryKindFilter::ArchitecturalDecision)
            }
            "known_issue" | "KnownIssue" => Some(crate::memory::MemoryKindFilter::KnownIssue),
            "convention" | "Convention" => Some(crate::memory::MemoryKindFilter::Convention),
            "project_context" | "ProjectContext" => {
                Some(crate::memory::MemoryKindFilter::ProjectContext)
            }
            _ => None,
        }
    }

    /// Check if a MemoryKindFilter matches a MemoryKind
    fn kind_matches_filter(
        filter: &crate::memory::MemoryKindFilter,
        kind: &crate::memory::MemoryKind,
    ) -> bool {
        matches!(
            (filter, kind),
            (
                crate::memory::MemoryKindFilter::ArchitecturalDecision,
                crate::memory::MemoryKind::ArchitecturalDecision { .. }
            ) | (
                crate::memory::MemoryKindFilter::DebugContext,
                crate::memory::MemoryKind::DebugContext { .. }
            ) | (
                crate::memory::MemoryKindFilter::KnownIssue,
                crate::memory::MemoryKind::KnownIssue { .. }
            ) | (
                crate::memory::MemoryKindFilter::Convention,
                crate::memory::MemoryKind::Convention { .. }
            ) | (
                crate::memory::MemoryKindFilter::ProjectContext,
                crate::memory::MemoryKind::ProjectContext { .. }
            )
        )
    }
}

/// Parse a string into a NodeId
fn parse_node_id(s: &str) -> Option<codegraph::NodeId> {
    // NodeId is u64 in codegraph
    s.parse::<codegraph::NodeId>().ok()
}
