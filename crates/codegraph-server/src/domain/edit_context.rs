// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Edit context assembly — transport-agnostic.
//!
//! Extracts get_edit_context from MCP server.
//! Composes: symbol source, callers, tests, memories, recent git changes.

use crate::ai_query::{EntryType, QueryEngine};
use crate::domain::{node_props, source_code};
use crate::git_mining::GitExecutor;
use crate::memory::MemoryManager;
use codegraph::{CodeGraph, NodeId, NodeType};
use serde::Serialize;
use std::path::PathBuf;
use tokio::sync::RwLock;

// ============================================================
// Response Types
// ============================================================

/// A position within a file (line + character).
#[derive(Debug, Serialize)]
pub(crate) struct EditContextPosition {
    pub line: i64,
    pub character: i64,
}

/// A range within a file.
#[derive(Debug, Serialize)]
pub(crate) struct EditContextRange {
    pub start: EditContextPosition,
    pub end: EditContextPosition,
}

/// Location of the target symbol.
#[derive(Debug, Serialize)]
pub(crate) struct EditContextLocation {
    pub uri: String,
    pub range: EditContextRange,
}

/// The primary symbol section of the edit context.
#[derive(Debug, Serialize)]
pub(crate) struct EditContextSymbol {
    pub name: String,
    #[serde(rename = "type")]
    pub symbol_type: String,
    pub code: String,
    pub language: String,
    pub location: EditContextLocation,
}

/// A caller entry.
#[derive(Debug, Serialize)]
pub(crate) struct EditContextCaller {
    pub name: String,
    pub code: Option<String>,
    pub file: String,
    pub line: u32,
}

/// A related test entry.
#[derive(Debug, Serialize)]
pub(crate) struct EditContextTest {
    pub name: String,
    pub relationship: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

/// A memory entry.
#[derive(Debug, Serialize)]
pub(crate) struct EditContextMemory {
    pub id: String,
    pub title: String,
    pub content: String,
    pub kind: String,
    pub score: f32,
}

/// A recent git change entry.
#[derive(Debug, Serialize)]
pub(crate) struct EditContextGitChange {
    pub hash: String,
    pub subject: String,
    pub author: String,
    pub date: String,
}

/// Section presence flags in metadata.
#[derive(Debug, Serialize)]
pub(crate) struct EditContextSections {
    pub symbol: bool,
    pub callers: bool,
    pub tests: bool,
    pub memories: bool,
    #[serde(rename = "recentChanges")]
    pub recent_changes: bool,
}

/// Metadata about the edit context response.
#[derive(Debug, Serialize)]
pub(crate) struct EditContextMetadata {
    #[serde(rename = "totalTokens")]
    pub total_tokens: usize,
    #[serde(rename = "maxTokens")]
    pub max_tokens: usize,
    #[serde(rename = "queryTime")]
    pub query_time: u64,
    pub sections: EditContextSections,
    #[serde(rename = "usedFallback", skip_serializing_if = "Option::is_none")]
    pub used_fallback: Option<bool>,
    #[serde(rename = "fallbackMessage", skip_serializing_if = "Option::is_none")]
    pub fallback_message: Option<String>,
}

/// Successful edit context result.
#[derive(Debug, Serialize)]
pub(crate) struct EditContextResult {
    pub symbol: EditContextSymbol,
    pub callers: Vec<EditContextCaller>,
    pub tests: Vec<EditContextTest>,
    pub memories: Vec<EditContextMemory>,
    #[serde(rename = "recentChanges")]
    pub recent_changes: Vec<EditContextGitChange>,
    pub metadata: EditContextMetadata,
}

/// Error result for edit context (symbol not found, etc.).
#[derive(Debug, Serialize)]
pub(crate) struct EditContextError {
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
}

// ============================================================
// Domain Function
// ============================================================

/// Assemble comprehensive edit context for a file + line in a single call.
///
/// `file_path` should be the resolved filesystem path (not a URI).
/// `uri` is used in the response for location references.
///
/// Composes: symbol source, callers, tests, memories, and recent git changes.
/// Token budget is allocated with priority: symbol > callers > tests > memories > git.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn get_edit_context(
    graph: &RwLock<CodeGraph>,
    query_engine: &QueryEngine,
    memory_manager: &MemoryManager,
    workspace_folders: &[PathBuf],
    file_path: &str,
    uri: &str,
    line: u32,
    max_tokens: usize,
) -> Result<EditContextResult, EditContextError> {
    use crate::domain::node_resolution;

    let start_time = std::time::Instant::now();

    // --- Resolve target symbol ---
    let (target, used_fallback) = {
        let g = graph.read().await;
        match node_resolution::find_nearest_node(&g, file_path, line) {
            Some(result) => result,
            None => {
                return Err(EditContextError {
                    error: "No symbols found at this location. Try indexing the workspace first."
                        .to_string(),
                    uri: Some(uri.to_string()),
                    line: Some(line),
                });
            }
        }
    };

    // Extract symbol metadata
    let (name, node_type, language, sym_path, line_start, line_end) = {
        let g = graph.read().await;
        let node = match g.get_node(target) {
            Ok(n) => n,
            Err(_) => {
                return Err(EditContextError {
                    error: "Could not load node".to_string(),
                    uri: None,
                    line: None,
                });
            }
        };
        let name = node_props::name(node).to_string();
        let node_type = format!("{:?}", node.node_type).to_lowercase();
        let language = node
            .properties
            .get_string("language")
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                node.properties
                    .get_string("path")
                    .and_then(|p| {
                        std::path::Path::new(p)
                            .extension()
                            .and_then(|e| e.to_str())
                            .map(|e| e.to_string())
                    })
                    .unwrap_or_else(|| "unknown".to_string())
            });
        let sym_path = node_props::path(node).to_string();
        let line_start = node_props::line_start(node) as i64;
        let line_end = {
            let e = node_props::line_end(node) as i64;
            if e == 0 {
                line_start
            } else {
                e
            }
        };
        (name, node_type, language, sym_path, line_start, line_end)
    };

    // --- Section 1: Symbol source code (budget: up to 30%) ---
    let source_code_str = {
        let g = graph.read().await;
        source_code::get_symbol_source(&g, target)
            .unwrap_or_else(|| "<source not available>".to_string())
    };
    let source_tokens = source_code_str.len() / 4;
    let mut budget_remaining = max_tokens.saturating_sub(source_tokens);

    let symbol = EditContextSymbol {
        name: name.clone(),
        symbol_type: node_type,
        code: source_code_str.clone(),
        language,
        location: EditContextLocation {
            uri: uri.to_string(),
            range: EditContextRange {
                start: EditContextPosition {
                    line: line_start,
                    character: 0,
                },
                end: EditContextPosition {
                    line: line_end,
                    character: 0,
                },
            },
        },
    };

    // --- Section 2: Callers (budget: up to 25% of original) ---
    let caller_budget = max_tokens / 4;
    let callers: Vec<EditContextCaller> = {
        let callers = query_engine.get_callers(target, 1).await;
        let mut caller_tokens_used = 0usize;
        let mut caller_list = Vec::new();

        for caller in callers.iter().take(10) {
            if caller_tokens_used >= caller_budget {
                break;
            }
            let code = {
                let g = graph.read().await;
                source_code::get_symbol_source(&g, caller.node_id)
            };
            let code_tokens = code.as_ref().map(|c| c.len() / 4).unwrap_or(0);
            caller_tokens_used += code_tokens;

            caller_list.push(EditContextCaller {
                name: caller.symbol.name.clone(),
                code,
                file: caller.symbol.location.file.clone(),
                line: caller.symbol.location.line,
            });
        }
        budget_remaining = budget_remaining.saturating_sub(caller_tokens_used);
        caller_list
    };

    // --- Section 3: Related tests (budget: up to 20% of original) ---
    let test_budget = max_tokens / 5;
    let tests: Vec<EditContextTest> = {
        let mut test_list = Vec::new();
        let mut test_tokens_used = 0usize;
        let mut seen_ids = std::collections::HashSet::<NodeId>::new();
        seen_ids.insert(target);

        // Stage 1: Tests that call target
        let entry_types = vec![EntryType::TestEntry];
        let tests = query_engine.find_entry_points(&entry_types).await;

        for test in tests.iter().take(20) {
            if test_list.len() >= 5 || test_tokens_used >= test_budget {
                break;
            }
            let callees = query_engine.get_callees(test.node_id, 3).await;
            if callees.iter().any(|c| c.node_id == target) && seen_ids.insert(test.node_id) {
                let code = {
                    let g = graph.read().await;
                    source_code::get_symbol_source(&g, test.node_id)
                };
                let code_tokens = code.as_ref().map(|c| c.len() / 4).unwrap_or(0);
                test_tokens_used += code_tokens;

                test_list.push(EditContextTest {
                    name: test.symbol.name.clone(),
                    relationship: "calls_target".to_string(),
                    code,
                });
            }
        }

        // Stage 2: Same-file test functions (if room)
        if test_list.len() < 5 {
            let g = graph.read().await;
            if let Ok(file_nodes) = g.query().property("path", sym_path.clone()).execute() {
                for node_id in file_nodes {
                    if test_list.len() >= 5 || test_tokens_used >= test_budget {
                        break;
                    }
                    if !seen_ids.insert(node_id) {
                        continue;
                    }
                    if let Ok(node) = g.get_node(node_id) {
                        if node.node_type == NodeType::Function
                            && crate::domain::unused_code::is_test_node(node)
                        {
                            let test_name = node_props::name(node).to_string();
                            test_list.push(EditContextTest {
                                name: test_name,
                                relationship: "same_file".to_string(),
                                code: None,
                            });
                        }
                    }
                }
            }
        }

        budget_remaining = budget_remaining.saturating_sub(test_tokens_used);
        test_list
    };

    // --- Section 4: Memories (budget: up to 15% of original) ---
    let memories: Vec<EditContextMemory> = {
        let search_query = if sym_path.is_empty() {
            name.clone()
        } else {
            sym_path.clone()
        };

        let config = crate::memory::SearchConfig {
            limit: 5,
            current_only: true,
            ..Default::default()
        };

        match memory_manager.search(&search_query, &config, &[]).await {
            Ok(results) => {
                let memory_budget = max_tokens * 15 / 100;
                let mut mem_tokens_used = 0usize;
                let mut mem_list = Vec::new();
                let mut seen_titles = std::collections::HashSet::new();

                for r in &results {
                    if mem_tokens_used >= memory_budget {
                        break;
                    }
                    if !seen_titles.insert(r.memory.title.clone()) {
                        continue;
                    }
                    let content_tokens = r.memory.content.len() / 4;
                    mem_tokens_used += content_tokens;

                    mem_list.push(EditContextMemory {
                        id: r.memory.id.to_string(),
                        title: r.memory.title.clone(),
                        content: r.memory.content.clone(),
                        kind: r.memory.kind.discriminant_name().to_string(),
                        score: r.score,
                    });
                }

                budget_remaining = budget_remaining.saturating_sub(mem_tokens_used);
                mem_list
            }
            Err(_) => Vec::new(),
        }
    };

    // --- Section 5: Recent git changes (budget: up to 10% of original) ---
    let recent_changes: Vec<EditContextGitChange> = {
        match workspace_folders.first().cloned() {
            Some(ws) => {
                let file_path_clone = sym_path.clone();
                let git_result = tokio::task::spawn_blocking(move || {
                    let executor = GitExecutor::new(&ws).ok()?;
                    let log_output = executor
                        .log(
                            "%H%x00%s%x00%an%x00%ai",
                            Some(10),
                            Some(std::path::Path::new(&file_path_clone)),
                        )
                        .ok()?;

                    let commits: Vec<EditContextGitChange> = log_output
                        .lines()
                        .filter(|l| !l.is_empty())
                        .take(5)
                        .filter_map(|line| {
                            let parts: Vec<&str> = line.split('\0').collect();
                            if parts.len() >= 4 {
                                Some(EditContextGitChange {
                                    hash: parts[0][..8.min(parts[0].len())].to_string(),
                                    subject: parts[1].to_string(),
                                    author: parts[2].to_string(),
                                    date: parts[3].to_string(),
                                })
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
                git_result
            }
            None => Vec::new(),
        }
    };

    let query_time = start_time.elapsed().as_millis() as u64;
    let total_tokens = max_tokens.saturating_sub(budget_remaining);

    let (fallback_used, fallback_message) = if used_fallback {
        (
            Some(true),
            Some(format!(
                "No symbol at line {}. Using nearest symbol '{}' instead.",
                line, name
            )),
        )
    } else {
        (None, None)
    };

    // Capture section presence booleans before moving the Vecs.
    let has_symbol = !source_code_str.is_empty();
    let has_callers = !callers.is_empty();
    let has_tests = !tests.is_empty();
    let has_memories = !memories.is_empty();
    let has_recent_changes = !recent_changes.is_empty();

    Ok(EditContextResult {
        symbol,
        callers,
        tests,
        memories,
        recent_changes,
        metadata: EditContextMetadata {
            total_tokens,
            max_tokens,
            query_time,
            sections: EditContextSections {
                symbol: has_symbol,
                callers: has_callers,
                tests: has_tests,
                memories: has_memories,
                recent_changes: has_recent_changes,
            },
            used_fallback: fallback_used,
            fallback_message,
        },
    })
}
