// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Branch-aware graph indexing via `.git/HEAD` monitoring.
//!
//! Watches `.git/HEAD` for branch switches and performs differential re-indexing
//! of only the files that changed between the old and new branch.

use crate::ai_query::QueryEngine;
use crate::cache::QueryCache;
use crate::git_mining::GitExecutor;
use crate::index::SymbolIndex;
use crate::memory::MemoryManager;
use crate::parser_registry::ParserRegistry;
use crate::watcher::{FileWatcher, GraphUpdater};
use codegraph::CodeGraph;
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};
use tower_lsp::lsp_types::MessageType;
use tower_lsp::Client;

/// Debounce interval for `.git/HEAD` changes (2 seconds).
/// Longer than the file watcher's 300ms because interactive rebase
/// writes HEAD multiple times in rapid succession.
const BRANCH_DEBOUNCE_MS: u64 = 2000;

/// Watches `.git/HEAD` for branch switches and triggers differential re-indexing.
pub struct BranchWatcher {
    _watcher: RecommendedWatcher,
}

/// Shared context passed to the branch watcher's async task and re-index functions.
struct BranchWatcherCtx {
    graph: Arc<RwLock<CodeGraph>>,
    parsers: Arc<ParserRegistry>,
    symbol_index: Arc<SymbolIndex>,
    query_engine: Arc<QueryEngine>,
    query_cache: Arc<QueryCache>,
    memory_manager: Arc<MemoryManager>,
    workspace_root: PathBuf,
}

/// Tracks the current git branch/commit state for change detection.
struct BranchState {
    branch: String,
    commit: String,
}

impl BranchWatcher {
    /// Create a new branch watcher for the given workspace.
    ///
    /// Watches `.git/HEAD` for modifications, debounces rapid changes (2s),
    /// then diffs changed files and batch re-indexes them.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        graph: Arc<RwLock<CodeGraph>>,
        parsers: Arc<ParserRegistry>,
        symbol_index: Arc<SymbolIndex>,
        query_engine: Arc<QueryEngine>,
        query_cache: Arc<QueryCache>,
        client: Client,
        memory_manager: Arc<MemoryManager>,
        workspace_root: PathBuf,
    ) -> Result<Self, notify::Error> {
        let (tx, mut rx) = mpsc::channel::<Event>(10);

        let mut watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                if let Ok(event) = res {
                    let _ = tx.blocking_send(event);
                }
            },
            Config::default(),
        )?;

        // Resolve the actual .git directory (handles worktrees)
        let git_head_path = resolve_git_head(&workspace_root);
        if let Some(ref head_path) = git_head_path {
            // Watch the parent directory of HEAD (the .git dir) non-recursively
            if let Some(git_dir) = head_path.parent() {
                watcher.watch(git_dir, RecursiveMode::NonRecursive)?;
            }
        }

        // Build context for the async task
        let ctx = BranchWatcherCtx {
            graph,
            parsers,
            symbol_index,
            query_engine,
            query_cache,
            memory_manager,
            workspace_root,
        };

        tokio::spawn(async move {
            // Initialize branch state from current git HEAD
            let mut state = match read_branch_state(&ctx.workspace_root).await {
                Some(s) => s,
                None => {
                    tracing::warn!("Could not read initial branch state, branch watcher inactive");
                    return;
                }
            };

            tracing::info!(
                "Branch watcher initialized: branch={}, commit={}",
                state.branch,
                &state.commit[..8.min(state.commit.len())]
            );

            let debounce_duration = Duration::from_millis(BRANCH_DEBOUNCE_MS);
            let mut last_event: Option<Instant> = None;

            loop {
                tokio::select! {
                    event = rx.recv() => {
                        match event {
                            Some(event) => {
                                // Only care about modifications to HEAD
                                if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                                    let is_head = event.paths.iter().any(|p| {
                                        p.file_name().is_some_and(|n| n == "HEAD")
                                    });
                                    if is_head {
                                        last_event = Some(Instant::now());
                                    }
                                }
                            }
                            None => break,
                        }
                    }
                    _ = tokio::time::sleep(Duration::from_millis(200)) => {
                        // Check if debounce period has elapsed
                        if let Some(ts) = last_event {
                            if Instant::now().duration_since(ts) >= debounce_duration {
                                last_event = None;

                                // Read new branch state
                                let new_state = match read_branch_state(&ctx.workspace_root).await {
                                    Some(s) => s,
                                    None => continue,
                                };

                                // Skip if nothing changed
                                if new_state.commit == state.commit {
                                    state = new_state;
                                    continue;
                                }

                                let old_branch = state.branch.clone();
                                let old_commit = state.commit.clone();
                                let new_branch = new_state.branch.clone();
                                let new_commit = new_state.commit.clone();

                                // Perform differential re-index
                                let result = handle_branch_switch(
                                    &ctx,
                                    &old_commit,
                                    &new_commit,
                                )
                                .await;

                                match result {
                                    Ok((modified, deleted)) => {
                                        let total = modified + deleted;
                                        if total > 0 {
                                            client.log_message(
                                                MessageType::INFO,
                                                format!(
                                                    "Branch switch: {} → {}, re-indexed {} files ({} modified, {} deleted)",
                                                    old_branch, new_branch, total, modified, deleted
                                                ),
                                            ).await;
                                        } else {
                                            client.log_message(
                                                MessageType::INFO,
                                                format!(
                                                    "Branch switch: {} → {} (no parseable files changed)",
                                                    old_branch, new_branch
                                                ),
                                            ).await;
                                        }
                                    }
                                    Err(e) => {
                                        client.log_message(
                                            MessageType::WARNING,
                                            format!(
                                                "Branch switch detected ({} → {}) but re-index failed: {}",
                                                old_branch, new_branch, e
                                            ),
                                        ).await;
                                    }
                                }

                                state = new_state;
                            }
                        }
                    }
                }
            }
        });

        Ok(Self { _watcher: watcher })
    }
}

/// Resolve the path to `.git/HEAD`, handling both normal repos and worktrees.
fn resolve_git_head(workspace_root: &Path) -> Option<PathBuf> {
    let dot_git = workspace_root.join(".git");

    if dot_git.is_dir() {
        // Normal repo: .git is a directory
        Some(dot_git.join("HEAD"))
    } else if dot_git.is_file() {
        // Worktree: .git is a file containing "gitdir: /path/to/actual/.git/worktrees/name"
        // Use git rev-parse --git-dir to resolve
        let output = std::process::Command::new("git")
            .current_dir(workspace_root)
            .args(["rev-parse", "--git-dir"])
            .output()
            .ok()?;

        if output.status.success() {
            let git_dir = String::from_utf8(output.stdout).ok()?.trim().to_string();
            let path = PathBuf::from(&git_dir);
            let resolved = if path.is_relative() {
                workspace_root.join(path)
            } else {
                path
            };
            Some(resolved.join("HEAD"))
        } else {
            None
        }
    } else {
        // Not a git repo
        None
    }
}

/// Read the current branch name and commit hash.
async fn read_branch_state(workspace_root: &Path) -> Option<BranchState> {
    let root = workspace_root.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let executor = GitExecutor::new(&root).ok()?;
        let branch = executor.current_branch().ok()?;
        let commit = executor.head_commit().ok()?;
        Some(BranchState { branch, commit })
    })
    .await
    .ok()?
}

/// Handle a branch switch by diffing changed files and re-indexing them.
///
/// Returns `(modified_count, deleted_count)` on success.
async fn handle_branch_switch(
    ctx: &BranchWatcherCtx,
    old_commit: &str,
    new_commit: &str,
) -> Result<(usize, usize), Box<dyn std::error::Error + Send + Sync>> {
    // Get the list of changed files
    let root = ctx.workspace_root.clone();
    let old = old_commit.to_string();
    let new = new_commit.to_string();

    let changes = tokio::task::spawn_blocking(move || {
        let executor = GitExecutor::new(&root)?;
        executor.diff_name_status(&old, &new)
    })
    .await??;

    if changes.is_empty() {
        return Ok((0, 0));
    }

    // Classify files
    let mut deleted_files: Vec<PathBuf> = Vec::new();
    let mut modified_files: Vec<PathBuf> = Vec::new();

    for (status, rel_path) in &changes {
        let abs_path = ctx.workspace_root.join(rel_path);
        match status {
            'D' => deleted_files.push(abs_path),
            'A' | 'M' | 'R' | 'C' => {
                // Only process files that exist on disk and are parseable
                if abs_path.exists() && ctx.parsers.can_parse(&abs_path) {
                    modified_files.push(abs_path);
                }
            }
            _ => {} // Ignore unknown statuses
        }
    }

    // Handle deleted files
    let deleted_count = deleted_files.len();
    if !deleted_files.is_empty() {
        let mut graph_guard = ctx.graph.write().await;
        let mut all_node_ids = Vec::new();

        for path in &deleted_files {
            all_node_ids.extend(FileWatcher::collect_file_node_ids(&graph_guard, path));
            let _ = FileWatcher::remove_file_nodes(&mut graph_guard, path);
            ctx.symbol_index.remove_file(path);
        }

        drop(graph_guard);

        // Invalidate memories for deleted nodes
        if !all_node_ids.is_empty() {
            let _ = ctx
                .memory_manager
                .invalidate_for_code_nodes(&all_node_ids, "Branch switch: files deleted")
                .await;
        }
    }

    // Handle modified/added files
    let modified_count = modified_files.len();
    if !modified_files.is_empty() {
        // Read file contents
        let mut files_with_content: Vec<(PathBuf, String)> = Vec::new();
        for path in &modified_files {
            match tokio::fs::read_to_string(path).await {
                Ok(content) => files_with_content.push((path.clone(), content)),
                Err(e) => {
                    tracing::warn!(
                        "Failed to read {} during branch switch: {}",
                        path.display(),
                        e
                    );
                }
            }
        }

        // Collect node IDs for memory invalidation before batch update
        let node_ids_to_invalidate: Vec<String> = {
            let graph_guard = ctx.graph.read().await;
            files_with_content
                .iter()
                .flat_map(|(path, _)| FileWatcher::collect_file_node_ids(&graph_guard, path))
                .collect()
        };

        // Remove old symbol index entries
        for (path, _) in &files_with_content {
            ctx.symbol_index.remove_file(path);
        }

        // Batch update: remove old nodes + parse new + resolve imports
        let result =
            GraphUpdater::update_files(&ctx.graph, &ctx.parsers, &files_with_content).await;

        // Re-add to symbol index
        {
            let graph_guard = ctx.graph.read().await;
            for (path, info) in &result.succeeded {
                ctx.symbol_index.add_file(path.clone(), info, &graph_guard);
            }
        }

        // Log failures
        for (path, err) in &result.failed {
            tracing::warn!(
                "Failed to re-index {} during branch switch: {}",
                path.display(),
                err
            );
        }

        // Invalidate memories
        if !node_ids_to_invalidate.is_empty() {
            let _ = ctx
                .memory_manager
                .invalidate_for_code_nodes(&node_ids_to_invalidate, "Branch switch: files modified")
                .await;
        }
    }

    // Invalidate query cache and rebuild indexes
    ctx.query_cache.invalidate_all();
    ctx.query_engine.build_indexes().await;

    // Incrementally re-embed changed files' symbols
    for path in &modified_files {
        let path_str = path.to_string_lossy().to_string();
        ctx.query_engine.update_file_vectors(&path_str).await;
    }

    Ok((modified_count, deleted_count))
}
