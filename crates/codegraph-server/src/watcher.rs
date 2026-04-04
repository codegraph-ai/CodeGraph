// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! File system watcher for incremental updates.

use crate::memory::MemoryManager;
use crate::parser_registry::ParserRegistry;
use codegraph::CodeGraph;
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};
use tower_lsp::lsp_types::MessageType;
use tower_lsp::Client;

/// Default debounce interval in milliseconds.
const DEFAULT_DEBOUNCE_MS: u64 = 300;

/// File system watcher that triggers re-parsing on changes.
pub struct FileWatcher {
    _watcher: RecommendedWatcher,
}

impl FileWatcher {
    /// Create a new file watcher with debouncing.
    pub fn new(
        graph: Arc<RwLock<CodeGraph>>,
        parsers: Arc<ParserRegistry>,
        client: Client,
        memory_manager: Arc<MemoryManager>,
    ) -> Result<Self, notify::Error> {
        let (tx, mut rx) = mpsc::channel::<Event>(100);

        let watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                if let Ok(event) = res {
                    // Use blocking_send since this is called from a sync context
                    let _ = tx.blocking_send(event);
                }
            },
            Config::default(),
        )?;

        // Spawn event handler task with debouncing
        let graph_clone = Arc::clone(&graph);
        let parsers_clone = Arc::clone(&parsers);
        let client_clone = client.clone();
        let memory_clone = Arc::clone(&memory_manager);

        tokio::spawn(async move {
            let debounce_duration = Duration::from_millis(DEFAULT_DEBOUNCE_MS);
            let mut pending_events: HashMap<PathBuf, (EventKind, Instant)> = HashMap::new();

            loop {
                // Use tokio::select to handle both incoming events and debounce timeouts
                tokio::select! {
                    event = rx.recv() => {
                        match event {
                            Some(event) => {
                                // Accumulate events with debouncing
                                for path in event.paths {
                                    pending_events.insert(path, (event.kind, Instant::now()));
                                }
                            }
                            None => break, // Channel closed
                        }
                    }
                    _ = tokio::time::sleep(Duration::from_millis(50)) => {
                        // Process any events that have been pending long enough
                        let now = Instant::now();
                        let mut to_process = Vec::new();

                        pending_events.retain(|path, (kind, timestamp)| {
                            if now.duration_since(*timestamp) >= debounce_duration {
                                to_process.push((path.clone(), *kind));
                                false
                            } else {
                                true
                            }
                        });

                        // Process debounced events
                        for (path, kind) in to_process {
                            let event = Event {
                                kind,
                                paths: vec![path],
                                attrs: Default::default(),
                            };
                            Self::handle_event(&graph_clone, &parsers_clone, &client_clone, &memory_clone, event).await;
                        }
                    }
                }
            }
        });

        Ok(Self { _watcher: watcher })
    }

    /// Start watching a directory.
    pub fn watch(&mut self, path: &Path) -> Result<(), notify::Error> {
        self._watcher.watch(path, RecursiveMode::Recursive)
    }

    /// Stop watching a directory.
    pub fn unwatch(&mut self, path: &Path) -> Result<(), notify::Error> {
        self._watcher.unwatch(path)
    }

    /// Handle a file system event.
    async fn handle_event(
        graph: &Arc<RwLock<CodeGraph>>,
        parsers: &Arc<ParserRegistry>,
        client: &Client,
        memory_manager: &Arc<MemoryManager>,
        event: Event,
    ) {
        match event.kind {
            EventKind::Create(_) | EventKind::Modify(_) => {
                for path in event.paths {
                    // Skip non-parseable files
                    if !parsers.can_parse(&path) {
                        continue;
                    }

                    if let Err(e) =
                        Self::handle_file_change(graph, parsers, memory_manager, &path).await
                    {
                        client
                            .log_message(
                                MessageType::WARNING,
                                format!("Error processing {}: {}", path.display(), e),
                            )
                            .await;
                    } else {
                        tracing::debug!("Re-indexed: {}", path.display());
                    }
                }
            }
            EventKind::Remove(_) => {
                for path in event.paths {
                    if let Err(e) = Self::handle_file_remove(graph, memory_manager, &path).await {
                        client
                            .log_message(
                                MessageType::WARNING,
                                format!("Error removing {}: {}", path.display(), e),
                            )
                            .await;
                    } else {
                        tracing::debug!("Removed from index: {}", path.display());
                    }
                }
            }
            _ => {}
        }
    }

    /// Handle a file creation or modification.
    async fn handle_file_change(
        graph: &Arc<RwLock<CodeGraph>>,
        parsers: &Arc<ParserRegistry>,
        memory_manager: &Arc<MemoryManager>,
        path: &Path,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Skip non-parseable files
        let parser = match parsers.parser_for_path(path) {
            Some(p) => p,
            None => return Ok(()),
        };

        // Read file content
        let content = tokio::fs::read_to_string(path).await?;

        let path_str = path.to_string_lossy().to_string();
        let node_id_strings: Vec<String>;

        // Scope the graph lock
        {
            let mut graph = graph.write().await;

            // Collect node IDs before removal for memory invalidation
            node_id_strings = Self::collect_file_node_ids(&graph, path);

            // Remove existing nodes for this file
            Self::remove_file_nodes(&mut graph, path)?;

            // Parse and add new nodes
            parser.parse_source(&content, path, &mut graph)?;

            // Resolve cross-file imports after parsing
            GraphUpdater::resolve_cross_file_imports(&mut graph);
        }

        // Auto-invalidate memories linked to changed nodes (after releasing graph lock)
        if !node_id_strings.is_empty() {
            let reason = format!("File modified: {}", path_str);
            if let Err(e) = memory_manager
                .invalidate_for_code_nodes(&node_id_strings, &reason)
                .await
            {
                tracing::warn!("Failed to invalidate memories for {}: {}", path_str, e);
            }
        }

        Ok(())
    }

    /// Handle a file removal.
    async fn handle_file_remove(
        graph: &Arc<RwLock<CodeGraph>>,
        memory_manager: &Arc<MemoryManager>,
        path: &Path,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let path_str = path.to_string_lossy().to_string();
        let node_id_strings: Vec<String>;

        // Scope the graph lock
        {
            let mut graph = graph.write().await;

            // Collect node IDs before removal for memory invalidation
            node_id_strings = Self::collect_file_node_ids(&graph, path);

            Self::remove_file_nodes(&mut graph, path)?;
        }

        // Auto-invalidate memories linked to deleted nodes
        if !node_id_strings.is_empty() {
            let reason = format!("File deleted: {}", path_str);
            if let Err(e) = memory_manager
                .invalidate_for_code_nodes(&node_id_strings, &reason)
                .await
            {
                tracing::warn!("Failed to invalidate memories for {}: {}", path_str, e);
            }
        }

        Ok(())
    }

    /// Collect all node IDs for a file (for memory invalidation).
    pub(crate) fn collect_file_node_ids(graph: &CodeGraph, path: &Path) -> Vec<String> {
        let path_str = path.to_string_lossy().to_string();
        if let Ok(nodes) = graph.query().property("path", path_str).execute() {
            nodes.iter().map(|n| n.to_string()).collect()
        } else {
            Vec::new()
        }
    }

    /// Remove all nodes associated with a file from the graph.
    pub(crate) fn remove_file_nodes(
        graph: &mut CodeGraph,
        path: &Path,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let path_str = path.to_string_lossy().to_string();

        // Query for all nodes with this file path
        if let Ok(nodes) = graph.query().property("path", path_str).execute() {
            for node_id in nodes {
                // Remove the node (edges are typically removed automatically)
                let _ = graph.delete_node(node_id);
            }
        }

        Ok(())
    }
}

/// Graph updater for batch operations.
pub struct GraphUpdater;

impl GraphUpdater {
    /// Batch update multiple files.
    pub async fn update_files(
        graph: &Arc<RwLock<CodeGraph>>,
        parsers: &Arc<ParserRegistry>,
        files: &[(PathBuf, String)],
    ) -> BatchUpdateResult {
        let mut succeeded = Vec::new();
        let mut failed = Vec::new();

        let mut graph_guard = graph.write().await;

        for (path, content) in files {
            if let Some(parser) = parsers.parser_for_path(path) {
                // Remove old nodes
                let path_str = path.to_string_lossy().to_string();
                if let Ok(nodes) = graph_guard.query().property("path", path_str).execute() {
                    for node_id in nodes {
                        let _ = graph_guard.delete_node(node_id);
                    }
                }

                // Parse new content
                match parser.parse_source(content, path, &mut graph_guard) {
                    Ok(info) => succeeded.push((path.clone(), info)),
                    Err(e) => failed.push((path.clone(), e.to_string())),
                }
            }
        }

        // Post-process: resolve cross-file import edges to actual symbol nodes
        Self::resolve_cross_file_imports(&mut graph_guard);

        BatchUpdateResult { succeeded, failed }
    }

    /// Resolve import edges to actual symbol nodes across files.
    ///
    /// After parsing all files, import edges may point to placeholder module nodes
    /// with relative path strings like "./toolManager". This function:
    /// 1. Finds all import edges with a `symbols` property
    /// 2. Looks up the actual symbol nodes by name in the graph
    /// 3. Creates direct import edges from the importing file to the symbol nodes
    ///
    /// This should be called after any parse operation to ensure cross-file
    /// imports are properly resolved.
    pub fn resolve_cross_file_imports(graph: &mut CodeGraph) {
        use codegraph::{Direction, EdgeType, NodeType, PropertyMap};

        // Collect all file nodes and their import edges
        let file_nodes: Vec<_> = graph
            .query()
            .node_type(NodeType::CodeFile)
            .execute()
            .unwrap_or_default();

        // Build a map of symbol name -> node ID for quick lookup.
        // When multiple symbols share a name (e.g. platform-specific stubs),
        // prefer the non-stub version (higher complexity / more lines).
        let mut symbol_map: std::collections::HashMap<String, codegraph::NodeId> =
            std::collections::HashMap::new();

        // Index all functions — prefer non-stub implementations over stubs
        if let Ok(functions) = graph.query().node_type(NodeType::Function).execute() {
            for func_id in functions {
                if let Ok(node) = graph.get_node(func_id) {
                    if let Some(name) = node.properties.get_string("name") {
                        let name = name.to_string();
                        if let Some(&existing_id) = symbol_map.get(&name) {
                            // Prefer the version with higher complexity (non-stub)
                            let existing_score = symbol_weight(graph, existing_id);
                            let new_score = symbol_weight(graph, func_id);
                            if new_score > existing_score {
                                symbol_map.insert(name, func_id);
                            }
                        } else {
                            symbol_map.insert(name, func_id);
                        }
                    }
                }
            }
        }

        // Index all classes
        if let Ok(classes) = graph.query().node_type(NodeType::Class).execute() {
            for class_id in classes {
                if let Ok(node) = graph.get_node(class_id) {
                    if let Some(name) = node.properties.get_string("name") {
                        symbol_map.insert(name.to_string(), class_id);
                    }
                }
            }
        }

        // Index all interfaces
        if let Ok(interfaces) = graph.query().node_type(NodeType::Interface).execute() {
            for interface_id in interfaces {
                if let Ok(node) = graph.get_node(interface_id) {
                    if let Some(name) = node.properties.get_string("name") {
                        symbol_map.insert(name.to_string(), interface_id);
                    }
                }
            }
        }

        // Process each file's outgoing import edges
        let mut edges_to_add: Vec<(codegraph::NodeId, codegraph::NodeId, PropertyMap)> = Vec::new();

        for file_id in file_nodes {
            // Get outgoing edges from this file
            if let Ok(neighbors) = graph.get_neighbors(file_id, Direction::Outgoing) {
                for neighbor_id in neighbors {
                    // Check if this is an import edge
                    if let Ok(edge_ids) = graph.get_edges_between(file_id, neighbor_id) {
                        for edge_id in edge_ids {
                            if let Ok(edge) = graph.get_edge(edge_id) {
                                if edge.edge_type == EdgeType::Imports {
                                    // Check if this edge has symbols that we can resolve
                                    if let Some(symbols) =
                                        edge.properties.get_string_list_compat("symbols")
                                    {
                                        for symbol in &symbols {
                                            let symbol = symbol.as_str();
                                            // Look up the symbol in our map
                                            if let Some(&symbol_id) = symbol_map.get(symbol) {
                                                // Check if we already have an edge to this symbol
                                                let already_linked = graph
                                                    .get_edges_between(file_id, symbol_id)
                                                    .map(|edges| {
                                                        edges.iter().any(|e| {
                                                            graph
                                                                .get_edge(*e)
                                                                .map(|edge| {
                                                                    edge.edge_type
                                                                        == EdgeType::Imports
                                                                })
                                                                .unwrap_or(false)
                                                        })
                                                    })
                                                    .unwrap_or(false);

                                                if !already_linked {
                                                    let props = PropertyMap::new()
                                                        .with("imported_symbol", symbol)
                                                        .with(
                                                            "resolved_by",
                                                            "cross_file_resolution",
                                                        );
                                                    edges_to_add.push((file_id, symbol_id, props));
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Add the resolved import edges
        for (from_id, to_id, props) in edges_to_add {
            let _ = graph.add_edge(from_id, to_id, EdgeType::Imports, props);
        }

        // Phase 2: Resolve cross-file calls
        // Find all function nodes with unresolved_calls property and resolve them
        let mut call_edges_to_add: Vec<(codegraph::NodeId, codegraph::NodeId, PropertyMap)> =
            Vec::new();

        if let Ok(functions) = graph.query().node_type(NodeType::Function).execute() {
            for func_id in functions {
                if let Ok(node) = graph.get_node(func_id) {
                    if let Some(unresolved) =
                        node.properties.get_string_list_compat("unresolved_calls")
                    {
                        for callee_name in &unresolved {
                            let callee_name = callee_name.as_str();
                            if !callee_name.is_empty() {
                                if let Some(&callee_id) = symbol_map.get(callee_name) {
                                    // Check if we already have a call edge
                                    let already_linked = graph
                                        .get_edges_between(func_id, callee_id)
                                        .map(|edges| {
                                            edges.iter().any(|e| {
                                                graph
                                                    .get_edge(*e)
                                                    .map(|edge| edge.edge_type == EdgeType::Calls)
                                                    .unwrap_or(false)
                                            })
                                        })
                                        .unwrap_or(false);

                                    if !already_linked {
                                        let props = PropertyMap::new()
                                            .with("resolved_by", "cross_file_resolution")
                                            .with("is_direct", "true");
                                        call_edges_to_add.push((func_id, callee_id, props));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Add the resolved call edges
        let resolved_call_count = call_edges_to_add.len();
        for (from_id, to_id, props) in call_edges_to_add {
            let _ = graph.add_edge(from_id, to_id, EdgeType::Calls, props);
        }

        if resolved_call_count > 0 {
            tracing::info!(
                "[resolve_cross_file] Phase 2: resolved {} cross-file call edges",
                resolved_call_count
            );
        }
        tracing::info!("[resolve_cross_file] Phase 2 complete: call edges added");

        // Phase 3: Resolve cross-file type references
        // Find function/class nodes with unresolved_type_refs and create References edges
        let mut ref_edges_to_add: Vec<(codegraph::NodeId, codegraph::NodeId, PropertyMap)> =
            Vec::new();

        if let Ok(functions) = graph.query().node_type(NodeType::Function).execute() {
            for func_id in functions {
                if let Ok(node) = graph.get_node(func_id) {
                    if let Some(unresolved) = node
                        .properties
                        .get_string_list_compat("unresolved_type_refs")
                    {
                        for type_name in &unresolved {
                            let type_name = type_name.as_str();
                            if let Some(&type_id) = symbol_map.get(type_name) {
                                let already_linked = graph
                                    .get_edges_between(func_id, type_id)
                                    .map(|edges| {
                                        edges.iter().any(|e| {
                                            graph
                                                .get_edge(*e)
                                                .map(|edge| edge.edge_type == EdgeType::References)
                                                .unwrap_or(false)
                                        })
                                    })
                                    .unwrap_or(false);

                                if !already_linked {
                                    let props = PropertyMap::new()
                                        .with("resolved_by", "cross_file_resolution");
                                    ref_edges_to_add.push((func_id, type_id, props));
                                }
                            }
                        }
                    }
                }
            }
        }

        // Add the resolved type reference edges
        for (from_id, to_id, props) in ref_edges_to_add {
            let _ = graph.add_edge(from_id, to_id, EdgeType::References, props);
        }
    }
}

/// Weight a symbol for deduplication: prefer non-stub implementations.
/// Returns a score based on complexity and line count — stubs (empty body,
/// single-line return) get low scores, real implementations get high scores.
fn symbol_weight(graph: &codegraph::CodeGraph, node_id: codegraph::NodeId) -> u32 {
    let node = match graph.get_node(node_id) {
        Ok(n) => n,
        Err(_) => return 0,
    };
    let complexity = node.properties.get_int("complexity").unwrap_or(1) as u32;
    let lines = {
        let start = node.properties.get_int("line_start").unwrap_or(0) as u32;
        let end = node.properties.get_int("line_end").unwrap_or(0) as u32;
        end.saturating_sub(start) + 1
    };
    // Complexity is the primary signal, lines break ties
    complexity * 100 + lines
}

/// Result of a batch update operation.
pub struct BatchUpdateResult {
    pub succeeded: Vec<(PathBuf, codegraph_parser_api::FileInfo)>,
    pub failed: Vec<(PathBuf, String)>,
}

impl BatchUpdateResult {
    /// Check if all files were updated successfully.
    pub fn all_succeeded(&self) -> bool {
        self.failed.is_empty()
    }

    /// Get the success rate.
    pub fn success_rate(&self) -> f64 {
        let total = self.succeeded.len() + self.failed.len();
        if total == 0 {
            1.0
        } else {
            self.succeeded.len() as f64 / total as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::node_props;

    // Helper to create a mock FileInfo using GraphUpdater
    async fn create_file_info_via_update(
        path: &str,
        content: &str,
    ) -> codegraph_parser_api::FileInfo {
        let graph = Arc::new(RwLock::new(CodeGraph::in_memory().unwrap()));
        let parsers = Arc::new(ParserRegistry::new());
        let files = vec![(PathBuf::from(path), content.to_string())];
        let result = GraphUpdater::update_files(&graph, &parsers, &files).await;
        result.succeeded.into_iter().next().unwrap().1
    }

    #[test]
    fn test_batch_update_result_all_succeeded_empty() {
        let result = BatchUpdateResult {
            succeeded: vec![],
            failed: vec![],
        };
        assert!(result.all_succeeded());
    }

    #[test]
    fn test_batch_update_result_has_failures() {
        let result = BatchUpdateResult {
            succeeded: vec![],
            failed: vec![(PathBuf::from("bad.py"), "parse error".to_string())],
        };
        assert!(!result.all_succeeded());
    }

    #[test]
    fn test_success_rate_empty() {
        let result = BatchUpdateResult {
            succeeded: vec![],
            failed: vec![],
        };
        assert!((result.success_rate() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_success_rate_all_failure() {
        let result = BatchUpdateResult {
            succeeded: vec![],
            failed: vec![
                (PathBuf::from("a.py"), "error".to_string()),
                (PathBuf::from("b.py"), "error".to_string()),
            ],
        };
        assert!((result.success_rate() - 0.0).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn test_batch_update_result_all_succeeded_with_files() {
        let info = create_file_info_via_update("a.py", "def foo(): pass").await;
        let result = BatchUpdateResult {
            succeeded: vec![(PathBuf::from("a.py"), info)],
            failed: vec![],
        };
        assert!(result.all_succeeded());
    }

    #[tokio::test]
    async fn test_batch_update_result_mixed() {
        let info = create_file_info_via_update("a.py", "def foo(): pass").await;
        let result = BatchUpdateResult {
            succeeded: vec![(PathBuf::from("a.py"), info)],
            failed: vec![(PathBuf::from("bad.py"), "parse error".to_string())],
        };
        assert!(!result.all_succeeded());
    }

    #[tokio::test]
    async fn test_success_rate_all_success() {
        let info1 = create_file_info_via_update("a.py", "def a(): pass").await;
        let info2 = create_file_info_via_update("b.py", "def b(): pass").await;
        let result = BatchUpdateResult {
            succeeded: vec![
                (PathBuf::from("a.py"), info1),
                (PathBuf::from("b.py"), info2),
            ],
            failed: vec![],
        };
        assert!((result.success_rate() - 1.0).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn test_success_rate_half() {
        let info = create_file_info_via_update("a.py", "def foo(): pass").await;
        let result = BatchUpdateResult {
            succeeded: vec![(PathBuf::from("a.py"), info)],
            failed: vec![(PathBuf::from("b.py"), "error".to_string())],
        };
        assert!((result.success_rate() - 0.5).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn test_success_rate_two_thirds() {
        let info1 = create_file_info_via_update("a.py", "def a(): pass").await;
        let info2 = create_file_info_via_update("b.py", "def b(): pass").await;
        let result = BatchUpdateResult {
            succeeded: vec![
                (PathBuf::from("a.py"), info1),
                (PathBuf::from("b.py"), info2),
            ],
            failed: vec![(PathBuf::from("c.py"), "error".to_string())],
        };
        // 2/3 = 0.6666...
        assert!((result.success_rate() - (2.0 / 3.0)).abs() < 0.0001);
    }

    #[tokio::test]
    async fn test_graph_updater_update_files_python() {
        let graph = Arc::new(RwLock::new(CodeGraph::in_memory().unwrap()));
        let parsers = Arc::new(ParserRegistry::new());

        let files = vec![(
            PathBuf::from("test.py"),
            "def foo():\n    pass\n".to_string(),
        )];

        let result = GraphUpdater::update_files(&graph, &parsers, &files).await;

        assert!(result.all_succeeded());
        assert_eq!(result.succeeded.len(), 1);
        assert!(result.failed.is_empty());
    }

    #[tokio::test]
    async fn test_graph_updater_update_multiple_files() {
        let graph = Arc::new(RwLock::new(CodeGraph::in_memory().unwrap()));
        let parsers = Arc::new(ParserRegistry::new());

        let files = vec![
            (PathBuf::from("a.py"), "def a(): pass".to_string()),
            (PathBuf::from("b.rs"), "fn b() {}".to_string()),
            (PathBuf::from("c.ts"), "function c() {}".to_string()),
        ];

        let result = GraphUpdater::update_files(&graph, &parsers, &files).await;

        assert!(result.all_succeeded());
        assert_eq!(result.succeeded.len(), 3);
    }

    #[tokio::test]
    async fn test_graph_updater_unsupported_file_skipped() {
        let graph = Arc::new(RwLock::new(CodeGraph::in_memory().unwrap()));
        let parsers = Arc::new(ParserRegistry::new());

        let files = vec![(PathBuf::from("readme.txt"), "hello world".to_string())];

        let result = GraphUpdater::update_files(&graph, &parsers, &files).await;

        // Unsupported files are silently skipped, not considered failures
        assert!(result.all_succeeded());
        assert!(result.succeeded.is_empty());
        assert!(result.failed.is_empty());
    }

    #[tokio::test]
    async fn test_graph_updater_mixed_files() {
        let graph = Arc::new(RwLock::new(CodeGraph::in_memory().unwrap()));
        let parsers = Arc::new(ParserRegistry::new());

        let files = vec![
            (PathBuf::from("valid.py"), "def foo(): pass".to_string()),
            (PathBuf::from("readme.md"), "# Hello".to_string()), // unsupported
            (PathBuf::from("valid.rs"), "fn bar() {}".to_string()),
        ];

        let result = GraphUpdater::update_files(&graph, &parsers, &files).await;

        // Only parseable files are tracked
        assert!(result.all_succeeded());
        assert_eq!(result.succeeded.len(), 2); // Python and Rust
    }

    #[tokio::test]
    async fn test_graph_updater_updates_remove_old_nodes() {
        let graph = Arc::new(RwLock::new(CodeGraph::in_memory().unwrap()));
        let parsers = Arc::new(ParserRegistry::new());

        // First update
        let files1 = vec![(
            PathBuf::from("test.py"),
            "def old_function(): pass".to_string(),
        )];
        GraphUpdater::update_files(&graph, &parsers, &files1).await;

        // Second update with new content (should replace old)
        let files2 = vec![(
            PathBuf::from("test.py"),
            "def new_function(): pass".to_string(),
        )];
        let result = GraphUpdater::update_files(&graph, &parsers, &files2).await;

        assert!(result.all_succeeded());
    }

    #[tokio::test]
    async fn test_cross_file_import_resolution() {
        use codegraph::{Direction, EdgeType, NodeType};

        let graph = Arc::new(RwLock::new(CodeGraph::in_memory().unwrap()));
        let parsers = Arc::new(ParserRegistry::new());

        // Create two TypeScript files:
        // 1. utils.ts defines a class MyClass
        // 2. main.ts imports { MyClass } from './utils'
        let files = vec![
            (
                PathBuf::from("/src/utils.ts"),
                r#"
                export class MyClass {
                    constructor() {}
                    doSomething() {}
                }
                "#
                .to_string(),
            ),
            (
                PathBuf::from("/src/main.ts"),
                r#"
                import { MyClass } from './utils';

                const instance = new MyClass();
                instance.doSomething();
                "#
                .to_string(),
            ),
        ];

        let result = GraphUpdater::update_files(&graph, &parsers, &files).await;
        assert!(result.all_succeeded());

        // Now check if the import edge was resolved correctly
        let graph_guard = graph.read().await;

        // Find the MyClass node
        let class_nodes: Vec<_> = graph_guard
            .query()
            .node_type(NodeType::Class)
            .execute()
            .unwrap_or_default();

        let my_class_id = class_nodes.iter().find(|&id| {
            graph_guard
                .get_node(*id)
                .map(|n| node_props::name(n) == "MyClass")
                .unwrap_or(false)
        });

        assert!(my_class_id.is_some(), "MyClass should exist in the graph");

        let my_class_id = my_class_id.unwrap();

        // Check if MyClass has incoming import edges
        let incoming_neighbors = graph_guard
            .get_neighbors(*my_class_id, Direction::Incoming)
            .unwrap_or_default();

        let has_import_edge = incoming_neighbors.iter().any(|neighbor_id| {
            graph_guard
                .get_edges_between(*neighbor_id, *my_class_id)
                .map(|edges| {
                    edges.iter().any(|e| {
                        graph_guard
                            .get_edge(*e)
                            .map(|edge| edge.edge_type == EdgeType::Imports)
                            .unwrap_or(false)
                    })
                })
                .unwrap_or(false)
        });

        assert!(
            has_import_edge,
            "MyClass should have an incoming import edge from main.ts"
        );
    }

    #[test]
    fn test_same_file_calls_edges_preserved() {
        use codegraph::{Direction, EdgeType};
        // Verify that the mapper creates same-file Calls edges
        // and they survive through parse_file
        let source = r#"
struct Foo;

impl Foo {
    fn caller(&self) {
        Self::target();
        self.instance_target();
        standalone();
    }
    fn target() {}
    fn instance_target(&self) {}
}

fn standalone() {}
"#;
        let mut graph = CodeGraph::in_memory().unwrap();
        let parsers = ParserRegistry::new();
        let result = parsers.parse_source(source, std::path::Path::new("test.rs"), &mut graph);
        assert!(result.is_ok(), "Parse failed: {:?}", result.err());

        // Find the "caller" function node
        let all_funcs = graph
            .query()
            .node_type(codegraph::NodeType::Function)
            .execute()
            .unwrap();
        let caller_id = all_funcs
            .iter()
            .find(|&&id| {
                graph
                    .get_node(id)
                    .map(|n| node_props::name(n) == "caller")
                    .unwrap_or(false)
            })
            .expect("Should find 'caller' function");

        // Collect callees via Calls edges
        let callees: Vec<String> = graph
            .get_neighbors(*caller_id, Direction::Outgoing)
            .unwrap_or_default()
            .iter()
            .filter(|&&nid| {
                graph
                    .get_edges_between(*caller_id, nid)
                    .unwrap_or_default()
                    .iter()
                    .any(|&eid| {
                        graph
                            .get_edge(eid)
                            .map(|e| e.edge_type == EdgeType::Calls)
                            .unwrap_or(false)
                    })
            })
            .filter_map(|&nid| {
                graph
                    .get_node(nid)
                    .ok()
                    .and_then(|n| n.properties.get_string("name").map(|s| s.to_string()))
            })
            .collect();

        eprintln!("Callees of 'caller': {:?}", callees);
        assert!(
            callees.contains(&"target".to_string()),
            "Missing Calls edge: caller -> target (Self::target())"
        );
        assert!(
            callees.contains(&"instance_target".to_string()),
            "Missing Calls edge: caller -> instance_target (self.instance_target())"
        );
        assert!(
            callees.contains(&"standalone".to_string()),
            "Missing Calls edge: caller -> standalone"
        );
    }

    #[test]
    fn test_parse_real_file_calls_edges() {
        use codegraph::{Direction, EdgeType};
        // Parse the actual watcher.rs and check if new -> handle_event edge exists
        let mut graph = CodeGraph::in_memory().unwrap();
        let parsers = ParserRegistry::new();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/watcher.rs");
        if !path.exists() {
            eprintln!("Skipping: {:?} not found", path);
            return;
        }
        let result = parsers.parse_file(&path, &mut graph);
        assert!(result.is_ok(), "Parse failed: {:?}", result.err());

        let all_funcs = graph
            .query()
            .node_type(codegraph::NodeType::Function)
            .execute()
            .unwrap();

        // Print all functions and their Calls edges
        for &fid in &all_funcs {
            let node = graph.get_node(fid).unwrap();
            let name = node.properties.get_string("name").unwrap_or("?");

            let callees: Vec<String> = graph
                .get_neighbors(fid, Direction::Outgoing)
                .unwrap_or_default()
                .iter()
                .filter_map(|&nid| {
                    let has_call = graph
                        .get_edges_between(fid, nid)
                        .unwrap_or_default()
                        .iter()
                        .any(|&eid| {
                            graph
                                .get_edge(eid)
                                .map(|e| e.edge_type == EdgeType::Calls)
                                .unwrap_or(false)
                        });
                    if has_call {
                        graph
                            .get_node(nid)
                            .ok()
                            .and_then(|n| n.properties.get_string("name").map(|s| s.to_string()))
                    } else {
                        None
                    }
                })
                .collect();

            let unresolved = node.properties.get_string("unresolved_calls").unwrap_or("");

            if !callees.is_empty() || !unresolved.is_empty() {
                eprintln!(
                    "  {} -> resolved: {:?}, unresolved: [{}]",
                    name, callees, unresolved
                );
            }
        }

        // Check that new -> handle_event exists
        let new_id = all_funcs
            .iter()
            .find(|&&id| {
                graph
                    .get_node(id)
                    .map(|n| node_props::name(n) == "new")
                    .unwrap_or(false)
            })
            .expect("Should find 'new' function");

        let new_callees: Vec<String> = graph
            .get_neighbors(*new_id, Direction::Outgoing)
            .unwrap_or_default()
            .iter()
            .filter_map(|&nid| {
                let has_call = graph
                    .get_edges_between(*new_id, nid)
                    .unwrap_or_default()
                    .iter()
                    .any(|&eid| {
                        graph
                            .get_edge(eid)
                            .map(|e| e.edge_type == EdgeType::Calls)
                            .unwrap_or(false)
                    });
                if has_call {
                    graph
                        .get_node(nid)
                        .ok()
                        .and_then(|n| n.properties.get_string("name").map(|s| s.to_string()))
                } else {
                    None
                }
            })
            .collect();

        eprintln!("new callees: {:?}", new_callees);
        assert!(
            new_callees.contains(&"handle_event".to_string()),
            "new -> handle_event Calls edge should exist"
        );
    }

    #[test]
    fn test_typescript_this_method_calls_edges() {
        use codegraph::{Direction, EdgeType};

        let source = r#"
export class ToolManager {
    private formatResult(data: any): string {
        return JSON.stringify(data);
    }

    private formatCallGraph(response: any, summary: boolean): string {
        return this.formatResult(response);
    }

    async handleTool(name: string): Promise<string> {
        const result = await this.getResult(name);
        return this.formatCallGraph(result, false);
    }

    private async getResult(name: string): Promise<any> {
        return { name };
    }
}
"#;

        let mut graph = CodeGraph::in_memory().unwrap();
        let parsers = ParserRegistry::new();
        let result = parsers.parse_source(source, std::path::Path::new("test.ts"), &mut graph);
        assert!(result.is_ok(), "Parse failed: {:?}", result.err());

        // Print all functions and their Calls edges for diagnosis
        let all_funcs = graph
            .query()
            .node_type(codegraph::NodeType::Function)
            .execute()
            .unwrap();

        eprintln!("=== TypeScript this.method() Calls edges ===");
        for &fid in &all_funcs {
            let node = graph.get_node(fid).unwrap();
            let name = node.properties.get_string("name").unwrap_or("?");

            let callees: Vec<String> = graph
                .get_neighbors(fid, Direction::Outgoing)
                .unwrap_or_default()
                .iter()
                .filter_map(|&nid| {
                    let has_call = graph
                        .get_edges_between(fid, nid)
                        .unwrap_or_default()
                        .iter()
                        .any(|&eid| {
                            graph
                                .get_edge(eid)
                                .map(|e| e.edge_type == EdgeType::Calls)
                                .unwrap_or(false)
                        });
                    if has_call {
                        graph
                            .get_node(nid)
                            .ok()
                            .and_then(|n| n.properties.get_string("name").map(|s| s.to_string()))
                    } else {
                        None
                    }
                })
                .collect();

            let unresolved = node.properties.get_string("unresolved_calls").unwrap_or("");
            eprintln!(
                "  {} -> resolved: {:?}, unresolved: [{}]",
                name, callees, unresolved
            );
        }

        // Check: handleTool should call formatCallGraph and getResult
        let handle_id = all_funcs
            .iter()
            .find(|&&id| {
                graph
                    .get_node(id)
                    .map(|n| node_props::name(n) == "handleTool")
                    .unwrap_or(false)
            })
            .expect("Should find 'handleTool' function");

        let handle_callees: Vec<String> = graph
            .get_neighbors(*handle_id, Direction::Outgoing)
            .unwrap_or_default()
            .iter()
            .filter_map(|&nid| {
                let has_call = graph
                    .get_edges_between(*handle_id, nid)
                    .unwrap_or_default()
                    .iter()
                    .any(|&eid| {
                        graph
                            .get_edge(eid)
                            .map(|e| e.edge_type == EdgeType::Calls)
                            .unwrap_or(false)
                    });
                if has_call {
                    graph
                        .get_node(nid)
                        .ok()
                        .and_then(|n| n.properties.get_string("name").map(|s| s.to_string()))
                } else {
                    None
                }
            })
            .collect();

        eprintln!("handleTool callees: {:?}", handle_callees);
        assert!(
            handle_callees.contains(&"formatCallGraph".to_string()),
            "handleTool -> formatCallGraph Calls edge should exist (this.formatCallGraph())"
        );
        assert!(
            handle_callees.contains(&"getResult".to_string()),
            "handleTool -> getResult Calls edge should exist (this.getResult())"
        );

        // Check: formatCallGraph should call formatResult
        let format_id = all_funcs
            .iter()
            .find(|&&id| {
                graph
                    .get_node(id)
                    .map(|n| node_props::name(n) == "formatCallGraph")
                    .unwrap_or(false)
            })
            .expect("Should find 'formatCallGraph' function");

        let format_callees: Vec<String> = graph
            .get_neighbors(*format_id, Direction::Outgoing)
            .unwrap_or_default()
            .iter()
            .filter_map(|&nid| {
                let has_call = graph
                    .get_edges_between(*format_id, nid)
                    .unwrap_or_default()
                    .iter()
                    .any(|&eid| {
                        graph
                            .get_edge(eid)
                            .map(|e| e.edge_type == EdgeType::Calls)
                            .unwrap_or(false)
                    });
                if has_call {
                    graph
                        .get_node(nid)
                        .ok()
                        .and_then(|n| n.properties.get_string("name").map(|s| s.to_string()))
                } else {
                    None
                }
            })
            .collect();

        eprintln!("formatCallGraph callees: {:?}", format_callees);
        assert!(
            format_callees.contains(&"formatResult".to_string()),
            "formatCallGraph -> formatResult Calls edge should exist (this.formatResult())"
        );
    }

    #[test]
    fn test_typescript_type_references_edges() {
        use codegraph::{Direction, EdgeType};

        let source = r#"
interface DependencyGraphParams {
    uri: string;
    depth: number;
}

interface DependencyNode {
    id: string;
    label: string;
}

function buildGraph(params: DependencyGraphParams): DependencyNode[] {
    return [];
}
"#;

        let mut graph = CodeGraph::in_memory().unwrap();
        let parsers = ParserRegistry::new();
        let result = parsers.parse_source(source, std::path::Path::new("test.ts"), &mut graph);
        assert!(result.is_ok(), "Parse failed: {:?}", result.err());

        let all_funcs = graph
            .query()
            .node_type(codegraph::NodeType::Function)
            .execute()
            .unwrap();

        let build_id = all_funcs
            .iter()
            .find(|&&id| {
                graph
                    .get_node(id)
                    .map(|n| node_props::name(n) == "buildGraph")
                    .unwrap_or(false)
            })
            .expect("Should find 'buildGraph'");

        // Check References edges from buildGraph
        let refs: Vec<String> = graph
            .get_neighbors(*build_id, Direction::Outgoing)
            .unwrap_or_default()
            .iter()
            .filter_map(|&nid| {
                let has_ref = graph
                    .get_edges_between(*build_id, nid)
                    .unwrap_or_default()
                    .iter()
                    .any(|&eid| {
                        graph
                            .get_edge(eid)
                            .map(|e| e.edge_type == EdgeType::References)
                            .unwrap_or(false)
                    });
                if has_ref {
                    graph
                        .get_node(nid)
                        .ok()
                        .and_then(|n| n.properties.get_string("name").map(|s| s.to_string()))
                } else {
                    None
                }
            })
            .collect();

        eprintln!("buildGraph References: {:?}", refs);
        assert!(
            refs.contains(&"DependencyGraphParams".to_string()),
            "buildGraph should reference DependencyGraphParams via param type"
        );
        assert!(
            refs.contains(&"DependencyNode".to_string()),
            "buildGraph should reference DependencyNode via return type"
        );

        // DependencyGraphParams should have incoming References edge
        let all_ifaces = graph
            .query()
            .node_type(codegraph::NodeType::Interface)
            .execute()
            .unwrap();

        let dgp_id = all_ifaces
            .iter()
            .find(|&&id| {
                graph
                    .get_node(id)
                    .map(|n| node_props::name(n) == "DependencyGraphParams")
                    .unwrap_or(false)
            })
            .expect("Should find DependencyGraphParams");

        let incoming: Vec<String> = graph
            .get_neighbors(*dgp_id, Direction::Incoming)
            .unwrap_or_default()
            .iter()
            .filter_map(|&nid| {
                let has_ref = graph
                    .get_edges_between(nid, *dgp_id)
                    .unwrap_or_default()
                    .iter()
                    .any(|&eid| {
                        graph
                            .get_edge(eid)
                            .map(|e| e.edge_type == EdgeType::References)
                            .unwrap_or(false)
                    });
                if has_ref {
                    graph
                        .get_node(nid)
                        .ok()
                        .and_then(|n| n.properties.get_string("name").map(|s| s.to_string()))
                } else {
                    None
                }
            })
            .collect();

        eprintln!("DependencyGraphParams referenced by: {:?}", incoming);
        assert!(
            incoming.contains(&"buildGraph".to_string()),
            "DependencyGraphParams should have incoming References from buildGraph"
        );
    }

    #[test]
    fn test_parse_real_ts_file_calls_edges() {
        use codegraph::{Direction, EdgeType};

        let mut graph = CodeGraph::in_memory().unwrap();
        let parsers = ParserRegistry::new();

        // Parse the actual toolManager.ts
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("src/ai/toolManager.ts");
        if !path.exists() {
            eprintln!("Skipping: {:?} not found", path);
            return;
        }

        let result = parsers.parse_file(&path, &mut graph);
        assert!(result.is_ok(), "Parse failed: {:?}", result.err());

        let all_funcs = graph
            .query()
            .node_type(codegraph::NodeType::Function)
            .execute()
            .unwrap();

        eprintln!("=== toolManager.ts functions with Calls edges ===");
        let mut format_call_graph_has_callers = false;
        for &fid in &all_funcs {
            let node = graph.get_node(fid).unwrap();
            let name = node.properties.get_string("name").unwrap_or("?");

            // Check outgoing Calls
            let callees: Vec<String> = graph
                .get_neighbors(fid, Direction::Outgoing)
                .unwrap_or_default()
                .iter()
                .filter_map(|&nid| {
                    let has_call = graph
                        .get_edges_between(fid, nid)
                        .unwrap_or_default()
                        .iter()
                        .any(|&eid| {
                            graph
                                .get_edge(eid)
                                .map(|e| e.edge_type == EdgeType::Calls)
                                .unwrap_or(false)
                        });
                    if has_call {
                        graph
                            .get_node(nid)
                            .ok()
                            .and_then(|n| n.properties.get_string("name").map(|s| s.to_string()))
                    } else {
                        None
                    }
                })
                .collect();

            // Check incoming Calls (callers)
            let callers: Vec<String> = graph
                .get_neighbors(fid, Direction::Incoming)
                .unwrap_or_default()
                .iter()
                .filter_map(|&nid| {
                    let has_call = graph
                        .get_edges_between(nid, fid)
                        .unwrap_or_default()
                        .iter()
                        .any(|&eid| {
                            graph
                                .get_edge(eid)
                                .map(|e| e.edge_type == EdgeType::Calls)
                                .unwrap_or(false)
                        });
                    if has_call {
                        graph
                            .get_node(nid)
                            .ok()
                            .and_then(|n| n.properties.get_string("name").map(|s| s.to_string()))
                    } else {
                        None
                    }
                })
                .collect();

            if name == "formatCallGraph" {
                format_call_graph_has_callers = !callers.is_empty();
            }

            let unresolved = node.properties.get_string("unresolved_calls").unwrap_or("");
            if !callees.is_empty() || !callers.is_empty() || !unresolved.is_empty() {
                eprintln!(
                    "  {} -> callees: {:?}, callers: {:?}, unresolved: [{}]",
                    name, callees, callers, unresolved
                );
            }
        }

        eprintln!(
            "\nformatCallGraph has callers: {}",
            format_call_graph_has_callers
        );
    }

    /// Integration test that simulates the full server pipeline for cross-file resolution.
    ///
    /// Scenario (using inline C source):
    /// 1. Batch-parse caller.c and callee.c into the graph
    /// 2. resolve_cross_file_imports → Calls edges created
    /// 3. Simulate did_open(caller.c): remove + re-parse + resolve
    /// 4. Verify cross-file Calls edges survive the did_open cycle
    ///
    #[test]
    fn test_cross_file_resolution_after_didopen() {
        let caller_source = r#"
void main_func(void) {
    int val = 42;
    if (val > 0) {
        helper_func(val);
    }
}
"#;
        let callee_source = r#"
void helper_func(int x) {
    int result = x * 2;
}
"#;
        let caller_path = std::path::Path::new("/test/caller.c");
        let callee_path = std::path::Path::new("/test/callee.c");

        let parsers = ParserRegistry::new();
        let parser = parsers
            .parser_for_path(caller_path)
            .expect("No C parser found");
        let mut graph = CodeGraph::in_memory().unwrap();

        // --- Phase 1: Batch-parse both files ---
        let r = parser.parse_source(caller_source, caller_path, &mut graph);
        assert!(r.is_ok(), "Failed to parse caller.c: {:?}", r.err());
        let r = parser.parse_source(callee_source, callee_path, &mut graph);
        assert!(r.is_ok(), "Failed to parse callee.c: {:?}", r.err());

        let main_id = find_function_node(&graph, "main_func");
        assert!(main_id.is_some(), "main_func not found after batch parse");
        let main_id = main_id.unwrap();

        let helper_id = find_function_node(&graph, "helper_func");
        assert!(
            helper_id.is_some(),
            "helper_func not found after batch parse"
        );

        // --- Phase 2: resolve_cross_file_imports ---
        GraphUpdater::resolve_cross_file_imports(&mut graph);

        let edge_count_after_batch = count_calls_edges_from(&graph, main_id);
        assert!(
            edge_count_after_batch > 0,
            "Expected Calls edges after batch resolve, got 0"
        );
        assert!(
            has_calls_edge(&graph, main_id, helper_id.unwrap()),
            "Expected Calls edge main_func -> helper_func after batch resolve"
        );

        // --- Phase 3: Simulate did_open(caller.c) ---
        // 3a. Remove caller.c nodes
        let path_str = caller_path.to_string_lossy().to_string();
        if let Ok(nodes) = graph.query().property("path", path_str).execute() {
            for node_id in nodes {
                let _ = graph.delete_node(node_id);
            }
        }
        assert!(
            find_function_node(&graph, "main_func").is_none(),
            "main_func should be gone after removal"
        );

        // 3b. Re-parse caller.c
        let r = parser.parse_source(caller_source, caller_path, &mut graph);
        assert!(r.is_ok(), "Failed to re-parse caller.c: {:?}", r.err());
        let fi = r.unwrap();
        // graph.query() doesn't see nodes added after deletion in the
        // in-memory backend, so use FileInfo directly for the new ID.
        assert!(
            !fi.functions.is_empty(),
            "Re-parse should produce at least one function"
        );
        let main_id_new = fi.functions[0];

        // 3c. resolve after re-parse
        GraphUpdater::resolve_cross_file_imports(&mut graph);

        let helper_id_final = find_function_node(&graph, "helper_func");
        assert!(
            helper_id_final.is_some(),
            "helper_func not found after re-resolve"
        );
        assert!(
            has_calls_edge(&graph, main_id_new, helper_id_final.unwrap()),
            "Expected Calls edge main_func -> helper_func after did_open cycle"
        );
    }

    fn find_function_node(graph: &CodeGraph, name: &str) -> Option<codegraph::NodeId> {
        graph
            .query()
            .node_type(codegraph::NodeType::Function)
            .execute()
            .unwrap_or_default()
            .into_iter()
            .find(|&id| {
                graph
                    .get_node(id)
                    .ok()
                    .map(|n| node_props::name(n) == name)
                    .unwrap_or(false)
            })
    }

    fn count_calls_edges_from(graph: &CodeGraph, node_id: codegraph::NodeId) -> usize {
        use codegraph::{Direction, EdgeType};
        graph
            .get_neighbors(node_id, Direction::Outgoing)
            .unwrap_or_default()
            .iter()
            .filter(|&&nid| {
                graph
                    .get_edges_between(node_id, nid)
                    .unwrap_or_default()
                    .iter()
                    .any(|&eid| {
                        graph
                            .get_edge(eid)
                            .map(|e| e.edge_type == EdgeType::Calls)
                            .unwrap_or(false)
                    })
            })
            .count()
    }

    fn has_calls_edge(graph: &CodeGraph, from: codegraph::NodeId, to: codegraph::NodeId) -> bool {
        use codegraph::EdgeType;
        graph
            .get_edges_between(from, to)
            .unwrap_or_default()
            .iter()
            .any(|&eid| {
                graph
                    .get_edge(eid)
                    .map(|e| e.edge_type == EdgeType::Calls)
                    .unwrap_or(false)
            })
    }
}
