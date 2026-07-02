// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AI context assembly — transport-agnostic implementation.
//!
//! Unified get_ai_context used by both LSP and MCP transports.
//! Includes quality improvements over both prior implementations:
//! - Signature-only mode for related symbols (compact representation)
//! - File-level imports in context response
//! - Sibling functions (same file, signature only)
//! - Debug hints (control flow shape for debug intent)

use codegraph::{CodeGraph, Direction, EdgeType, NodeId, NodeType};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

use crate::domain::{complexity, node_props, node_resolution, source_code};

// ============================================================
// Result Types
// ============================================================

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AiContextResult {
    pub primary_context: PrimaryContext,
    pub related_symbols: Vec<RelatedSymbol>,
    pub dependencies: Vec<DependencyInfo>,
    /// File-level imports: modules/packages imported by the file containing this symbol.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub imports: Vec<String>,
    /// Other functions/methods in the same file (signature only, compact).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sibling_functions: Vec<SiblingInfo>,
    pub usage_examples: Option<Vec<UsageExample>>,
    pub architecture: Option<ArchitectureInfo>,
    /// Control flow shape hints — only present for debug intent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub debug_hints: Option<DebugHints>,
    pub metadata: ContextMetadata,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PrimaryContext {
    #[serde(rename = "type")]
    pub context_type: String,
    pub name: String,
    pub code: String,
    pub language: String,
    pub location: LocationInfo,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RelatedSymbol {
    pub name: String,
    pub relationship: String,
    pub code: String,
    pub location: LocationInfo,
    pub relevance_score: f64,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DependencyInfo {
    pub name: String,
    #[serde(rename = "type")]
    pub dep_type: String,
    pub code: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UsageExample {
    pub code: String,
    pub location: LocationInfo,
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ArchitectureInfo {
    pub module: String,
    pub layer: Option<String>,
    pub neighbors: Vec<NeighborInfo>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NeighborInfo {
    pub module: String,
    pub relationship: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ContextMetadata {
    pub total_tokens: usize,
    pub query_time: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_fallback: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_message: Option<String>,
    pub graph_stats: GraphStats,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GraphStats {
    pub entities_in_graph: usize,
    pub entities_traversed: usize,
    pub entities_kept: usize,
    pub output_tokens: usize,
}

/// Compact sibling function info — signature only, no full source.
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SiblingInfo {
    pub name: String,
    pub signature: String,
    pub visibility: String,
    pub line_start: u32,
}

/// Control flow shape hints for debug intent.
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DebugHints {
    pub complexity: u32,
    pub branches: u32,
    pub exception_handlers: u32,
    pub early_returns: u32,
    pub nesting_depth: u32,
    /// Names of callees with error/panic/fail patterns.
    pub error_paths: Vec<String>,
}

/// Transport-agnostic location (no tower_lsp dependency).
/// Serializes identically to tower_lsp's Location+Range types.
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LocationInfo {
    pub uri: String,
    pub range: RangeInfo,
}

#[derive(Debug, Serialize, Clone)]
pub struct RangeInfo {
    pub start: PosInfo,
    pub end: PosInfo,
}

#[derive(Debug, Serialize, Clone)]
pub struct PosInfo {
    pub line: u32,
    pub character: u32,
}

// ============================================================
// Token Budget
// ============================================================

struct TokenBudget {
    total: usize,
    used: usize,
}

impl TokenBudget {
    fn new(total: usize) -> Self {
        Self { total, used: 0 }
    }

    fn consume(&mut self, tokens: usize) -> bool {
        if self.used + tokens <= self.total {
            self.used += tokens;
            true
        } else {
            false
        }
    }

    fn has_budget(&self) -> bool {
        self.used < self.total
    }
}

fn estimate_tokens(s: &str) -> usize {
    s.len() / 4
}

// ============================================================
// Main Entry Point
// ============================================================

/// Assemble AI context for the symbol nearest to (file_path, line).
///
/// Returns None if no symbol is found for the given file.
pub(crate) fn get_ai_context(
    graph: &CodeGraph,
    file_path: &str,
    line: u32,
    intent: &str,
    max_tokens: usize,
) -> Option<AiContextResult> {
    let start_time = std::time::Instant::now();

    let (target, used_fallback) = node_resolution::find_nearest_node(graph, file_path, line)?;

    let node = graph.get_node(target).ok()?;

    let name = node_props::name(node).to_string();
    let node_type = format!("{}", node.node_type).to_lowercase();
    let language = {
        let l = node_props::language(node);
        if l.is_empty() {
            std::path::Path::new(file_path)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("unknown")
                .to_string()
        } else {
            l.to_string()
        }
    };
    let line_start = node_props::line_start(node);
    let line_end = {
        let e = node_props::line_end(node);
        if e == 0 {
            line_start
        } else {
            e
        }
    };

    let primary_code = source_code::get_symbol_source(graph, target)
        .unwrap_or_else(|| "<source not available>".to_string());

    let primary_context = PrimaryContext {
        context_type: node_type,
        name: name.clone(),
        code: primary_code.clone(),
        language,
        location: make_location(file_path, line_start, line_end),
    };

    let mut budget = TokenBudget::new(max_tokens);
    budget.consume(estimate_tokens(&primary_code));

    let mut seen = HashSet::new();
    seen.insert(target);

    let related_symbols =
        get_related_by_intent(graph, target, &name, intent, &mut budget, &mut seen);

    let dependencies = get_dependencies(graph, target);
    let imports = get_file_imports(graph, file_path);
    let sibling_functions = get_sibling_functions(graph, target, file_path);
    let usage_examples = get_usage_examples(graph, target, &name, &mut budget);
    let architecture = get_architecture_info(graph, target);
    let debug_hints = if intent == "debug" {
        get_debug_hints(graph, target)
    } else {
        None
    };

    let query_time = start_time.elapsed().as_millis() as u64;

    let fallback_message = if used_fallback {
        Some(format!(
            "No symbol at cursor position. Using nearest symbol '{name}' instead."
        ))
    } else {
        None
    };

    let entities_kept = 1 + related_symbols.len(); // primary + related

    Some(AiContextResult {
        primary_context,
        related_symbols,
        dependencies,
        imports,
        sibling_functions,
        usage_examples,
        architecture,
        debug_hints,
        metadata: ContextMetadata {
            total_tokens: budget.used,
            query_time,
            used_fallback: if used_fallback { Some(true) } else { None },
            fallback_message,
            graph_stats: GraphStats {
                entities_in_graph: graph.node_count(),
                entities_traversed: seen.len(),
                entities_kept,
                output_tokens: budget.used,
            },
        },
    })
}

// ============================================================
// Private Helpers
// ============================================================

fn get_related_by_intent(
    graph: &CodeGraph,
    node_id: NodeId,
    target_name: &str,
    intent: &str,
    budget: &mut TokenBudget,
    seen: &mut HashSet<NodeId>,
) -> Vec<RelatedSymbol> {
    let outgoing = get_edges(graph, node_id, Direction::Outgoing);
    let incoming = get_edges(graph, node_id, Direction::Incoming);
    let mut symbols = Vec::new();

    match intent {
        "explain" => {
            // Priority 1: Direct dependencies (things this symbol uses)
            for (_, target, _) in outgoing.iter().take(5) {
                if !budget.has_budget() {
                    break;
                }
                if seen.insert(*target) {
                    if let Some(sym) = make_related_symbol(graph, *target, "uses", 1.0, budget) {
                        symbols.push(sym);
                    }
                }
            }
            // Priority 2: Direct callers (truncated to call site for large functions)
            for (source, _, _) in incoming
                .iter()
                .filter(|(_, _, t)| *t == EdgeType::Calls)
                .take(3)
            {
                if !budget.has_budget() {
                    break;
                }
                if seen.insert(*source) {
                    if let Some(sym) = make_related_symbol_for(
                        graph,
                        *source,
                        "called_by",
                        0.8,
                        budget,
                        Some(target_name),
                    ) {
                        symbols.push(sym);
                    }
                }
            }
            // Priority 3: Parent type (for methods)
            for (source, _, _) in incoming.iter().filter(|(_, _, t)| *t == EdgeType::Extends) {
                if !budget.has_budget() {
                    break;
                }
                if seen.insert(*source) {
                    if let Some(sym) = make_related_symbol(graph, *source, "inherits", 0.9, budget)
                    {
                        symbols.push(sym);
                    }
                }
            }
        }
        "modify" => {
            // Priority 1: Tests for this symbol
            for (source, _, _) in incoming
                .iter()
                .filter(|(_, _, t)| *t == EdgeType::Calls)
                .take(5)
            {
                if !budget.has_budget() {
                    break;
                }
                if seen.insert(*source) {
                    if let Ok(n) = graph.get_node(*source) {
                        let n_name = node_props::name(n);
                        if n_name.starts_with("test_") || n_name.ends_with("_test") {
                            if let Some(sym) =
                                make_related_symbol(graph, *source, "tests", 1.0, budget)
                            {
                                symbols.push(sym);
                            }
                        }
                    }
                }
            }
            // Priority 2: Non-test callers (truncated to call site)
            for (source, _, _) in incoming
                .iter()
                .filter(|(_, _, t)| *t == EdgeType::Calls)
                .take(5)
            {
                if !budget.has_budget() {
                    break;
                }
                if seen.insert(*source) {
                    if let Ok(n) = graph.get_node(*source) {
                        let n_name = node_props::name(n);
                        if !n_name.starts_with("test_") && !n_name.ends_with("_test") {
                            if let Some(sym) = make_related_symbol_for(
                                graph,
                                *source,
                                "called_by",
                                0.9,
                                budget,
                                Some(target_name),
                            ) {
                                symbols.push(sym);
                            }
                        }
                    }
                }
            }
        }
        "debug" => {
            // Call chain up to entry point (truncated to call site)
            let mut current = node_id;
            let mut current_name = target_name.to_string();
            let mut depth = 0;
            while depth < 5 && budget.has_budget() {
                let cur_incoming = get_edges(graph, current, Direction::Incoming);
                let caller = cur_incoming
                    .iter()
                    .filter(|(_, _, t)| *t == EdgeType::Calls)
                    .find(|(source, _, _)| !seen.contains(source));
                if let Some((source, _, _)) = caller {
                    seen.insert(*source);
                    let relevance = 1.0 - (depth as f64 * 0.1);
                    let relationship = format!("call_chain_depth_{depth}");
                    if let Some(sym) = make_related_symbol_for(
                        graph,
                        *source,
                        &relationship,
                        relevance,
                        budget,
                        Some(&current_name),
                    ) {
                        symbols.push(sym);
                    }
                    // Track name for next level's truncation
                    if let Ok(n) = graph.get_node(*source) {
                        current_name = node_props::name(n).to_string();
                    }
                    current = *source;
                    depth += 1;
                } else {
                    break;
                }
            }
            // Data dependencies
            for (_, target, _) in outgoing.iter().take(3) {
                if !budget.has_budget() {
                    break;
                }
                if seen.insert(*target) {
                    if let Some(sym) = make_related_symbol(graph, *target, "data_flow", 0.8, budget)
                    {
                        symbols.push(sym);
                    }
                }
            }
        }
        "test" => {
            // Existing tests as examples
            for (source, _, _) in incoming
                .iter()
                .filter(|(_, _, t)| *t == EdgeType::Calls)
                .take(3)
            {
                if !budget.has_budget() {
                    break;
                }
                if seen.insert(*source) {
                    if let Ok(n) = graph.get_node(*source) {
                        let n_name = node_props::name(n);
                        if n_name.starts_with("test_") || n_name.ends_with("_test") {
                            if let Some(sym) =
                                make_related_symbol(graph, *source, "example_test", 0.9, budget)
                            {
                                symbols.push(sym);
                            }
                        }
                    }
                }
            }
            // Dependencies to mock
            for (_, target, _) in outgoing.iter().take(3) {
                if !budget.has_budget() {
                    break;
                }
                if seen.insert(*target) {
                    if let Some(sym) =
                        make_related_symbol(graph, *target, "dependency_to_mock", 0.7, budget)
                    {
                        symbols.push(sym);
                    }
                }
            }
        }
        _ => {}
    }

    symbols
}

fn get_file_imports(graph: &CodeGraph, file_path: &str) -> Vec<String> {
    let nodes = match graph.query().property("path", file_path).execute() {
        Ok(n) => n,
        Err(_) => return Vec::new(),
    };

    let mut imports = Vec::new();
    let mut seen = HashSet::new();
    for node_id in nodes {
        for (_, target, edge_type) in get_edges(graph, node_id, Direction::Outgoing) {
            if edge_type == EdgeType::Imports && seen.insert(target) {
                if let Ok(target_node) = graph.get_node(target) {
                    let name = node_props::name(target_node);
                    if !name.is_empty() {
                        imports.push(name.to_string());
                    }
                }
            }
        }
    }

    imports.truncate(20);
    imports
}

fn get_sibling_functions(graph: &CodeGraph, node_id: NodeId, file_path: &str) -> Vec<SiblingInfo> {
    let nodes = match graph.query().property("path", file_path).execute() {
        Ok(n) => n,
        Err(_) => return Vec::new(),
    };

    let mut siblings = Vec::new();
    for nid in nodes {
        if nid == node_id {
            continue;
        }
        if let Ok(node) = graph.get_node(nid) {
            if node.node_type != NodeType::Function {
                continue;
            }
            let name = node_props::name(node).to_string();
            if name.is_empty() {
                continue;
            }
            let signature = node
                .properties
                .get_string("signature")
                .map(|s| s.to_string())
                .unwrap_or_else(|| name.clone());
            let visibility = node_props::visibility(node).to_string();
            let line_start = node_props::line_start(node);
            siblings.push(SiblingInfo {
                name,
                signature,
                visibility,
                line_start,
            });
        }
    }

    siblings.sort_by_key(|s| s.line_start);
    siblings.truncate(10);
    siblings
}

fn get_debug_hints(graph: &CodeGraph, node_id: NodeId) -> Option<DebugHints> {
    let node = graph.get_node(node_id).ok()?;
    let (complexity_score, details, _) = complexity::get_complexity_from_node(node);

    let error_paths: Vec<String> = get_edges(graph, node_id, Direction::Outgoing)
        .into_iter()
        .filter_map(|(_, target, edge_type)| {
            if edge_type != EdgeType::Calls {
                return None;
            }
            let target_node = graph.get_node(target).ok()?;
            let name_lower = node_props::name(target_node).to_lowercase();
            if name_lower.contains("error")
                || name_lower.contains("err")
                || name_lower.contains("panic")
                || name_lower.contains("throw")
                || name_lower.contains("fail")
                || name_lower.contains("exception")
            {
                Some(node_props::name(target_node).to_string())
            } else {
                None
            }
        })
        .collect();

    Some(DebugHints {
        complexity: complexity_score,
        branches: details.complexity_branches,
        exception_handlers: details.complexity_exceptions,
        early_returns: details.complexity_early_returns,
        nesting_depth: details.complexity_nesting,
        error_paths,
    })
}

fn get_dependencies(graph: &CodeGraph, node_id: NodeId) -> Vec<DependencyInfo> {
    get_edges(graph, node_id, Direction::Outgoing)
        .into_iter()
        .filter(|(_, _, t)| *t == EdgeType::Imports)
        .take(10)
        .filter_map(|(_, target, _)| {
            let dep_node = graph.get_node(target).ok()?;
            let name = node_props::name(dep_node);
            if name.is_empty() {
                return None;
            }
            Some(DependencyInfo {
                name: name.to_string(),
                dep_type: "import".to_string(),
                code: None,
            })
        })
        .collect()
}

fn get_usage_examples(
    graph: &CodeGraph,
    node_id: NodeId,
    target_name: &str,
    budget: &mut TokenBudget,
) -> Option<Vec<UsageExample>> {
    let incoming = get_edges(graph, node_id, Direction::Incoming);
    let usages: Vec<_> = incoming
        .iter()
        .filter(|(_, _, t)| *t == EdgeType::Calls || *t == EdgeType::References)
        .collect();

    let mut examples = Vec::new();
    for (source, _, _) in usages.iter().take(3) {
        if !budget.has_budget() {
            break;
        }
        let usage_node = match graph.get_node(*source) {
            Ok(n) => n,
            Err(_) => continue,
        };
        let usage_name = node_props::name(usage_node);
        if usage_name.starts_with("test_") || usage_name.ends_with("_test") {
            continue;
        }
        if let Some(code) = source_code::get_symbol_source(graph, *source) {
            let tokens = estimate_tokens(&code);
            if !budget.consume(tokens) {
                break;
            }
            let path = node_props::path(usage_node).to_string();
            let start_line = node_props::line_start(usage_node);
            let end_line = {
                let e = node_props::line_end(usage_node);
                if e == 0 {
                    start_line
                } else {
                    e
                }
            };
            let description = generate_usage_description(usage_name, target_name, &code);
            examples.push(UsageExample {
                code,
                location: make_location(&path, start_line, end_line),
                description: Some(description),
            });
        }
    }

    if examples.is_empty() {
        None
    } else {
        Some(examples)
    }
}

fn get_architecture_info(graph: &CodeGraph, node_id: NodeId) -> Option<ArchitectureInfo> {
    let node = graph.get_node(node_id).ok()?;
    let path_str = node_props::path(node).to_string();
    if path_str.is_empty() {
        return None;
    }

    let module = std::path::Path::new(&path_str)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    let layer = detect_layer(&path_str);

    let mut neighbor_map: HashMap<String, HashSet<String>> = HashMap::new();
    let outgoing = get_edges(graph, node_id, Direction::Outgoing);
    let incoming = get_edges(graph, node_id, Direction::Incoming);

    for (source, target, edge_type) in outgoing.iter() {
        let _ = source; // outgoing: node_id -> target
        if let Ok(other_node) = graph.get_node(*target) {
            if let Some(other_path) = other_node.properties.get_string("path") {
                if let Some(other_module) = std::path::Path::new(other_path)
                    .file_stem()
                    .and_then(|s| s.to_str())
                {
                    if other_module != module {
                        let rel = match edge_type {
                            EdgeType::Calls => "calls",
                            EdgeType::Imports => "imports",
                            _ => "depends_on",
                        };
                        neighbor_map
                            .entry(other_module.to_string())
                            .or_default()
                            .insert(rel.to_string());
                    }
                }
            }
        }
    }

    for (source, _, edge_type) in incoming.iter() {
        // incoming: source -> node_id
        if let Ok(other_node) = graph.get_node(*source) {
            if let Some(other_path) = other_node.properties.get_string("path") {
                if let Some(other_module) = std::path::Path::new(other_path)
                    .file_stem()
                    .and_then(|s| s.to_str())
                {
                    if other_module != module {
                        let rel = match edge_type {
                            EdgeType::Calls => "called_by",
                            EdgeType::Imports => "imported_by",
                            _ => "depended_on_by",
                        };
                        neighbor_map
                            .entry(other_module.to_string())
                            .or_default()
                            .insert(rel.to_string());
                    }
                }
            }
        }
    }

    let neighbors: Vec<NeighborInfo> = neighbor_map
        .into_iter()
        .map(|(module, rels)| {
            let relationship = rels.into_iter().collect::<Vec<_>>().join(", ");
            NeighborInfo {
                module,
                relationship,
            }
        })
        .collect();

    Some(ArchitectureInfo {
        module,
        layer,
        neighbors,
    })
}

/// Maximum lines for a related symbol before truncation kicks in.
const MAX_RELATED_LINES: usize = 30;
/// Context lines around a call site when truncating.
const CALL_SITE_CONTEXT: usize = 5;

fn make_related_symbol(
    graph: &CodeGraph,
    node_id: NodeId,
    relationship: &str,
    relevance: f64,
    budget: &mut TokenBudget,
) -> Option<RelatedSymbol> {
    make_related_symbol_for(graph, node_id, relationship, relevance, budget, None)
}

/// Create a related symbol, optionally truncating large functions to the call site
/// where `target_name` is called (± CALL_SITE_CONTEXT lines).
fn make_related_symbol_for(
    graph: &CodeGraph,
    node_id: NodeId,
    relationship: &str,
    relevance: f64,
    budget: &mut TokenBudget,
    target_name: Option<&str>,
) -> Option<RelatedSymbol> {
    let full_code = source_code::get_symbol_source(graph, node_id)?;

    // If the symbol is large and we know what call to focus on, truncate
    let code = if full_code.lines().count() > MAX_RELATED_LINES {
        if let Some(target) = target_name {
            truncate_to_call_site(&full_code, target)
        } else {
            full_code
        }
    } else {
        full_code
    };

    let tokens = estimate_tokens(&code);
    if !budget.consume(tokens) {
        return None;
    }

    let node = graph.get_node(node_id).ok()?;
    let name = node_props::name(node).to_string();
    let path = node_props::path(node).to_string();
    let start_line = node_props::line_start(node);
    let end_line = {
        let e = node_props::line_end(node);
        if e == 0 {
            start_line
        } else {
            e
        }
    };

    Some(RelatedSymbol {
        name,
        relationship: relationship.to_string(),
        code,
        location: make_location(&path, start_line, end_line),
        relevance_score: relevance,
    })
}

/// Truncate a function body to the lines around where `target_name` is called.
/// Returns signature + call site ± context. Falls back to first MAX_RELATED_LINES if not found.
fn truncate_to_call_site(code: &str, target_name: &str) -> String {
    let lines: Vec<&str> = code.lines().collect();

    // Find the first line containing the target function call
    let call_line = lines.iter().position(|line| line.contains(target_name));

    if let Some(idx) = call_line {
        let start = idx.saturating_sub(CALL_SITE_CONTEXT);
        let end = (idx + CALL_SITE_CONTEXT + 1).min(lines.len());

        // Always include the function signature (first line)
        let mut result = String::new();
        if start > 0 {
            result.push_str(lines[0]);
            result.push('\n');
            if start > 1 {
                result.push_str(&format!("    // ... ({} lines omitted)\n", start - 1));
            }
        }

        for line in &lines[start..end] {
            result.push_str(line);
            result.push('\n');
        }

        if end < lines.len() {
            result.push_str(&format!(
                "    // ... ({} lines omitted)\n",
                lines.len() - end
            ));
        }

        result
    } else {
        // Target not found in source — return first MAX_RELATED_LINES
        lines
            .iter()
            .take(MAX_RELATED_LINES)
            .copied()
            .collect::<Vec<_>>()
            .join("\n")
            + &format!(
                "\n    // ... ({} lines omitted)",
                lines.len() - MAX_RELATED_LINES
            )
    }
}

fn make_location(path: &str, start_line: u32, end_line: u32) -> LocationInfo {
    let uri = if path.starts_with('/') {
        format!("file://{path}")
    } else {
        path.to_string()
    };
    LocationInfo {
        uri,
        range: RangeInfo {
            start: PosInfo {
                line: start_line,
                character: 0,
            },
            end: PosInfo {
                line: end_line,
                character: 0,
            },
        },
    }
}

fn get_edges(
    graph: &CodeGraph,
    node_id: NodeId,
    direction: Direction,
) -> Vec<(NodeId, NodeId, EdgeType)> {
    let neighbors = match graph.get_neighbors(node_id, direction) {
        Ok(n) => n,
        Err(_) => return Vec::new(),
    };

    let mut edges = Vec::new();
    for neighbor_id in neighbors {
        let (source, target) = match direction {
            Direction::Outgoing => (node_id, neighbor_id),
            Direction::Incoming => (neighbor_id, node_id),
            Direction::Both => {
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

// ============================================================
// Shared Utilities (pub(crate) for test access)
// ============================================================

/// Detect architectural layer from file path using common conventions.
pub(crate) fn detect_layer(path: &str) -> Option<String> {
    let path_lower = path.to_lowercase();

    let layer_patterns: &[(&[&str], &str)] = &[
        (
            &[
                "controllers",
                "controller",
                "routes",
                "router",
                "endpoints",
                "api/",
            ],
            "controller",
        ),
        (
            &["views", "view", "templates", "pages", "components", "ui/"],
            "presentation",
        ),
        (&["handlers", "handler"], "handler"),
        (
            &[
                "services",
                "service",
                "usecases",
                "use_cases",
                "application/",
            ],
            "service",
        ),
        (&["commands", "command"], "command"),
        (&["queries", "query"], "query"),
        (
            &["models", "model", "entities", "entity", "domain/"],
            "domain",
        ),
        (&["aggregates", "aggregate"], "aggregate"),
        (&["value_objects", "valueobjects"], "value_object"),
        (&["repositories", "repository", "repos"], "repository"),
        (&["database", "db/", "persistence"], "persistence"),
        (
            &["adapters", "adapter", "infrastructure/"],
            "infrastructure",
        ),
        (&["clients", "client"], "client"),
        (&["providers", "provider"], "provider"),
        (&["middleware", "middlewares"], "middleware"),
        (&["utils", "util", "helpers", "helper", "lib/"], "utility"),
        (&["config", "configuration", "settings"], "configuration"),
        (&["types", "interfaces", "contracts"], "contract"),
        (&["tests", "test", "__tests__", "spec", "specs"], "test"),
        (&["fixtures", "mocks", "stubs"], "test_support"),
    ];

    for (patterns, layer) in layer_patterns {
        for pattern in *patterns {
            if path_lower.contains(pattern) {
                return Some(layer.to_string());
            }
        }
    }

    // Fallback: infer from file name
    let file_name = std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();

    if file_name.ends_with("controller") || file_name.ends_with("_controller") {
        return Some("controller".to_string());
    }
    if file_name.ends_with("service") || file_name.ends_with("_service") {
        return Some("service".to_string());
    }
    if file_name.ends_with("repository")
        || file_name.ends_with("_repository")
        || file_name.ends_with("repo")
    {
        return Some("repository".to_string());
    }
    if file_name.ends_with("model")
        || file_name.ends_with("_model")
        || file_name.ends_with("entity")
    {
        return Some("domain".to_string());
    }
    if file_name.ends_with("handler") || file_name.ends_with("_handler") {
        return Some("handler".to_string());
    }
    if file_name.ends_with("middleware") {
        return Some("middleware".to_string());
    }
    if file_name.starts_with("test_")
        || file_name.ends_with("_test")
        || file_name.ends_with(".test")
        || file_name.ends_with(".spec")
    {
        return Some("test".to_string());
    }

    None
}

/// Generate a description for a usage example.
pub(crate) fn generate_usage_description(
    caller_name: &str,
    target_name: &str,
    code: &str,
) -> String {
    let is_async = code.contains("await") || code.contains("async");
    let is_error_handling = code.contains("try") || code.contains("catch") || code.contains('?');
    let is_conditional = code.contains("if") || code.contains("match") || code.contains("switch");

    let mut parts = Vec::new();

    if !caller_name.is_empty() {
        parts.push(format!("`{caller_name}` calls `{target_name}`"));
    } else {
        parts.push(format!("Usage of `{target_name}`"));
    }

    if is_async {
        parts.push("(async)".to_string());
    }
    if is_error_handling {
        parts.push("with error handling".to_string());
    }
    if is_conditional {
        parts.push("conditionally".to_string());
    }

    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_layer_controllers() {
        assert_eq!(
            detect_layer("/src/controllers/user.ts"),
            Some("controller".to_string())
        );
        assert_eq!(
            detect_layer("/src/api/users.ts"),
            Some("controller".to_string())
        );
        assert_eq!(
            detect_layer("/app/routes/index.ts"),
            Some("controller".to_string())
        );
    }

    #[test]
    fn test_detect_layer_services() {
        assert_eq!(
            detect_layer("/src/services/auth.ts"),
            Some("service".to_string())
        );
        assert_eq!(
            detect_layer("/src/usecases/login.ts"),
            Some("service".to_string())
        );
    }

    #[test]
    fn test_detect_layer_domain() {
        assert_eq!(
            detect_layer("/src/models/user.ts"),
            Some("domain".to_string())
        );
        assert_eq!(
            detect_layer("/src/entities/order.ts"),
            Some("domain".to_string())
        );
        assert_eq!(
            detect_layer("/src/domain/product.ts"),
            Some("domain".to_string())
        );
    }

    #[test]
    fn test_detect_layer_repository() {
        assert_eq!(
            detect_layer("/src/repositories/user_repo.ts"),
            Some("repository".to_string())
        );
        assert_eq!(
            detect_layer("/src/repos/order.ts"),
            Some("repository".to_string())
        );
    }

    #[test]
    fn test_detect_layer_infrastructure() {
        assert_eq!(
            detect_layer("/src/database/connection.ts"),
            Some("persistence".to_string())
        );
        assert_eq!(
            detect_layer("/src/adapters/redis.ts"),
            Some("infrastructure".to_string())
        );
    }

    #[test]
    fn test_detect_layer_utility() {
        assert_eq!(
            detect_layer("/src/utils/helpers.ts"),
            Some("utility".to_string())
        );
        assert_eq!(detect_layer("/lib/format.ts"), Some("utility".to_string()));
    }

    #[test]
    fn test_detect_layer_tests() {
        assert_eq!(
            detect_layer("/src/__tests__/user.test.ts"),
            Some("test".to_string())
        );
        assert_eq!(
            detect_layer("/tests/integration/api.ts"),
            Some("test".to_string())
        );
    }

    #[test]
    fn test_detect_layer_by_filename() {
        assert_eq!(
            detect_layer("/src/user_controller.ts"),
            Some("controller".to_string())
        );
        assert_eq!(
            detect_layer("/src/auth_service.ts"),
            Some("service".to_string())
        );
        assert_eq!(
            detect_layer("/src/user_repository.ts"),
            Some("repository".to_string())
        );
    }

    #[test]
    fn test_detect_layer_unknown() {
        assert_eq!(detect_layer("/src/main.ts"), None);
        assert_eq!(detect_layer("/app.ts"), None);
    }

    #[test]
    fn test_detect_layer_covers_remaining_pattern_arms() {
        // The prior detect_layer tests exercise only ~9 of the 20 ordered
        // pattern arms; these pin the remaining classifications, each with a
        // path whose earliest-matching pattern is the intended layer.
        assert_eq!(
            detect_layer("/src/views/home.ts"),
            Some("presentation".to_string())
        );
        assert_eq!(
            detect_layer("/src/handlers/event.ts"),
            Some("handler".to_string())
        );
        assert_eq!(
            detect_layer("/src/commands/create.ts"),
            Some("command".to_string())
        );
        assert_eq!(
            detect_layer("/src/queries/find.ts"),
            Some("query".to_string())
        );
        assert_eq!(
            detect_layer("/src/aggregates/cart.ts"),
            Some("aggregate".to_string())
        );
        assert_eq!(
            detect_layer("/src/value_objects/money.ts"),
            Some("value_object".to_string())
        );
        assert_eq!(
            detect_layer("/src/clients/http.ts"),
            Some("client".to_string())
        );
        assert_eq!(
            detect_layer("/src/providers/auth.ts"),
            Some("provider".to_string())
        );
        assert_eq!(
            detect_layer("/src/middleware/cors.ts"),
            Some("middleware".to_string())
        );
        assert_eq!(
            detect_layer("/src/config/app.ts"),
            Some("configuration".to_string())
        );
        assert_eq!(
            detect_layer("/src/types/index.ts"),
            Some("contract".to_string())
        );
        assert_eq!(
            detect_layer("/src/fixtures/data.ts"),
            Some("test_support".to_string())
        );
    }

    #[test]
    fn test_detect_layer_first_matching_pattern_wins() {
        // Patterns are scanned in declaration order and the first hit wins:
        // "controllers" (arm 1) precedes "services" (arm 4), so a path
        // containing both resolves to the earlier "controller" layer.
        assert_eq!(
            detect_layer("/src/controllers/services/user.ts"),
            Some("controller".to_string())
        );
    }

    #[test]
    fn test_generate_usage_description_basic() {
        let desc =
            generate_usage_description("process_order", "validate_data", "validate_data(input)");
        assert!(desc.contains("`process_order`"));
        assert!(desc.contains("`validate_data`"));
    }

    #[test]
    fn test_generate_usage_description_async() {
        let desc = generate_usage_description("handler", "fetch_user", "await fetch_user(id)");
        assert!(desc.contains("(async)"));
    }

    #[test]
    fn test_generate_usage_description_error_handling() {
        let desc = generate_usage_description(
            "process",
            "parse_config",
            "try { parse_config() } catch(e) { }",
        );
        assert!(desc.contains("error handling"));
    }

    #[test]
    fn test_generate_usage_description_conditional() {
        let desc = generate_usage_description("run", "check", "if (check(x)) { do_thing() }");
        assert!(desc.contains("conditionally"));
    }

    #[test]
    fn test_generate_usage_description_empty_caller() {
        let desc = generate_usage_description("", "my_function", "my_function()");
        assert!(desc.contains("Usage of `my_function`"));
    }

    // ============================================================
    // get_ai_context end-to-end tests (in-memory graph)
    // ============================================================

    use codegraph::{PropertyMap, PropertyValue};

    fn str_prop(v: &str) -> PropertyValue {
        PropertyValue::String(v.to_string())
    }

    fn int_prop(v: i64) -> PropertyValue {
        PropertyValue::Int(v)
    }

    /// Add a node carrying the given key/value properties, returning its id.
    fn add_node(graph: &mut CodeGraph, ty: NodeType, props: &[(&str, PropertyValue)]) -> NodeId {
        let mut map = PropertyMap::new();
        for (k, v) in props {
            map.insert(k.to_string(), v.clone());
        }
        graph.add_node(ty, map).expect("add_node")
    }

    fn edge(graph: &mut CodeGraph, from: NodeId, to: NodeId, ty: EdgeType) {
        graph
            .add_edge(from, to, ty, PropertyMap::new())
            .expect("add_edge");
    }

    /// A Function node with path, an inline source, and a [start, end] line range.
    fn add_fn(
        graph: &mut CodeGraph,
        name: &str,
        path: &str,
        start: i64,
        end: i64,
        source: &str,
    ) -> NodeId {
        add_node(
            graph,
            NodeType::Function,
            &[
                ("name", str_prop(name)),
                ("path", str_prop(path)),
                ("line_start", int_prop(start)),
                ("line_end", int_prop(end)),
                ("source", str_prop(source)),
            ],
        )
    }

    fn rels(result: &AiContextResult, relationship: &str) -> Vec<String> {
        result
            .related_symbols
            .iter()
            .filter(|s| s.relationship == relationship)
            .map(|s| s.name.clone())
            .collect()
    }

    #[test]
    fn ai_context_none_for_unknown_file() {
        let g = CodeGraph::in_memory().expect("in_memory");
        assert!(get_ai_context(&g, "/nope.rs", 1, "explain", 1000).is_none());
    }

    #[test]
    fn ai_context_assembles_primary_from_target() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        add_node(
            &mut g,
            NodeType::Function,
            &[
                ("name", str_prop("do_work")),
                ("path", str_prop("/src/app.rs")),
                ("language", str_prop("rust")),
                ("line_start", int_prop(10)),
                ("line_end", int_prop(20)),
                ("source", str_prop("fn do_work() {}")),
            ],
        );

        let r = get_ai_context(&g, "/src/app.rs", 12, "explain", 10_000).expect("context");
        assert_eq!(r.primary_context.name, "do_work");
        assert_eq!(r.primary_context.context_type, "function");
        assert_eq!(r.primary_context.language, "rust");
        assert_eq!(r.primary_context.code, "fn do_work() {}");
        assert_eq!(r.primary_context.location.range.start.line, 10);
        assert_eq!(r.primary_context.location.range.end.line, 20);
        assert_eq!(r.primary_context.location.uri, "file:///src/app.rs");
        // Exact containment — no fallback.
        assert!(r.metadata.used_fallback.is_none());
        assert!(r.metadata.fallback_message.is_none());
        assert_eq!(r.metadata.graph_stats.entities_in_graph, 1);
        assert_eq!(r.metadata.graph_stats.entities_kept, 1);
        assert!(r.related_symbols.is_empty());
    }

    #[test]
    fn ai_context_used_fallback_when_line_not_contained() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        add_fn(&mut g, "do_work", "/src/app.rs", 10, 20, "fn do_work() {}");

        // Request line 5 — before the only node's range → proximity fallback.
        let r = get_ai_context(&g, "/src/app.rs", 5, "explain", 10_000).expect("context");
        assert_eq!(r.metadata.used_fallback, Some(true));
        assert!(r
            .metadata
            .fallback_message
            .as_deref()
            .unwrap()
            .contains("do_work"));
    }

    #[test]
    fn ai_context_language_falls_back_to_extension() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        // No language property — must be derived from the file extension.
        add_node(
            &mut g,
            NodeType::Function,
            &[
                ("name", str_prop("do_work")),
                ("path", str_prop("/src/app.rs")),
                ("line_start", int_prop(10)),
                ("line_end", int_prop(20)),
                ("source", str_prop("fn do_work() {}")),
            ],
        );

        let r = get_ai_context(&g, "/src/app.rs", 12, "explain", 10_000).expect("context");
        assert_eq!(r.primary_context.language, "rs");
    }

    #[test]
    fn ai_context_line_end_zero_collapses_to_line_start() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        // No line_end property → line_end reads as 0 and collapses onto line_start.
        add_node(
            &mut g,
            NodeType::Function,
            &[
                ("name", str_prop("do_work")),
                ("path", str_prop("/src/app.rs")),
                ("line_start", int_prop(7)),
                ("source", str_prop("fn do_work() {}")),
            ],
        );

        let r = get_ai_context(&g, "/src/app.rs", 7, "explain", 10_000).expect("context");
        assert_eq!(r.primary_context.location.range.start.line, 7);
        assert_eq!(r.primary_context.location.range.end.line, 7);
    }

    #[test]
    fn ai_context_explain_surfaces_uses_and_called_by() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let target = add_fn(&mut g, "do_work", "/src/app.rs", 10, 20, "fn do_work() {}");
        let dep = add_fn(&mut g, "helper", "/src/dep.rs", 1, 3, "fn helper() {}");
        let caller = add_fn(
            &mut g,
            "caller",
            "/src/c.rs",
            1,
            3,
            "fn caller() { do_work(); }",
        );
        edge(&mut g, target, dep, EdgeType::Calls); // outgoing → "uses"
        edge(&mut g, caller, target, EdgeType::Calls); // incoming Calls → "called_by"

        let r = get_ai_context(&g, "/src/app.rs", 12, "explain", 100_000).expect("context");
        assert_eq!(rels(&r, "uses"), vec!["helper".to_string()]);
        assert_eq!(rels(&r, "called_by"), vec!["caller".to_string()]);
    }

    #[test]
    fn ai_context_explain_inherits_via_extends() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let target = add_fn(&mut g, "Derived", "/src/app.rs", 10, 20, "struct Derived;");
        let base = add_fn(&mut g, "Base", "/src/base.rs", 1, 3, "struct Base;");
        edge(&mut g, base, target, EdgeType::Extends); // incoming Extends → "inherits"

        let r = get_ai_context(&g, "/src/app.rs", 12, "explain", 100_000).expect("context");
        assert_eq!(rels(&r, "inherits"), vec!["Base".to_string()]);
        let sym = r
            .related_symbols
            .iter()
            .find(|s| s.relationship == "inherits")
            .unwrap();
        assert_eq!(sym.relevance_score, 0.9);
    }

    #[test]
    fn ai_context_modify_surfaces_tests_and_swallows_callers() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let target = add_fn(&mut g, "do_work", "/src/app.rs", 10, 20, "fn do_work() {}");
        let test_caller = add_fn(
            &mut g,
            "test_do_work",
            "/src/t.rs",
            1,
            3,
            "fn test_do_work() { do_work(); }",
        );
        let caller = add_fn(
            &mut g,
            "caller",
            "/src/c.rs",
            1,
            3,
            "fn caller() { do_work(); }",
        );
        edge(&mut g, test_caller, target, EdgeType::Calls);
        edge(&mut g, caller, target, EdgeType::Calls);

        let r = get_ai_context(&g, "/src/app.rs", 12, "modify", 100_000).expect("context");
        assert_eq!(rels(&r, "tests"), vec!["test_do_work".to_string()]);
        // Latent behavior: the modify Priority-1 "tests" loop calls seen.insert on
        // *every* Calls caller it visits (test or not), so the non-test "caller" is
        // marked seen without being emitted. Priority 2 iterates the same take(5)
        // set and finds nothing fresh, so "called_by" is never populated for callers
        // that appeared in Priority 1's window — the non-test caller is swallowed.
        assert!(rels(&r, "called_by").is_empty());
    }

    #[test]
    fn ai_context_debug_includes_hints_and_call_chain() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let target = add_fn(&mut g, "do_work", "/src/app.rs", 10, 20, "fn do_work() {}");
        let caller = add_fn(
            &mut g,
            "caller",
            "/src/c.rs",
            1,
            3,
            "fn caller() { do_work(); }",
        );
        edge(&mut g, caller, target, EdgeType::Calls);

        let r = get_ai_context(&g, "/src/app.rs", 12, "debug", 100_000).expect("context");
        assert!(r.debug_hints.is_some());
        assert_eq!(
            rels(&r, "call_chain_depth_0"),
            vec!["caller".to_string()],
            "debug intent walks the caller chain starting at depth 0"
        );
    }

    #[test]
    fn ai_context_debug_hints_absent_for_explain_intent() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        add_fn(&mut g, "do_work", "/src/app.rs", 10, 20, "fn do_work() {}");

        let r = get_ai_context(&g, "/src/app.rs", 12, "explain", 10_000).expect("context");
        assert!(r.debug_hints.is_none());
    }

    #[test]
    fn ai_context_test_intent_surfaces_example_test_and_mock() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let target = add_fn(&mut g, "do_work", "/src/app.rs", 10, 20, "fn do_work() {}");
        let example = add_fn(
            &mut g,
            "test_do_work",
            "/src/t.rs",
            1,
            3,
            "fn test_do_work() { do_work(); }",
        );
        let dep = add_fn(&mut g, "helper", "/src/dep.rs", 1, 3, "fn helper() {}");
        edge(&mut g, example, target, EdgeType::Calls); // incoming test → "example_test"
        edge(&mut g, target, dep, EdgeType::Calls); // outgoing → "dependency_to_mock"

        let r = get_ai_context(&g, "/src/app.rs", 12, "test", 100_000).expect("context");
        assert_eq!(rels(&r, "example_test"), vec!["test_do_work".to_string()]);
        assert_eq!(rels(&r, "dependency_to_mock"), vec!["helper".to_string()]);
    }

    #[test]
    fn ai_context_dependencies_list_imports_only() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let target = add_fn(&mut g, "do_work", "/src/app.rs", 10, 20, "fn do_work() {}");
        let module = add_node(
            &mut g,
            NodeType::Module,
            &[("name", str_prop("serde")), ("path", str_prop("serde"))],
        );
        let helper = add_fn(&mut g, "helper", "/src/dep.rs", 1, 3, "fn helper() {}");
        edge(&mut g, target, module, EdgeType::Imports);
        edge(&mut g, target, helper, EdgeType::Calls);

        let r = get_ai_context(&g, "/src/app.rs", 12, "explain", 100_000).expect("context");
        assert_eq!(r.dependencies.len(), 1);
        assert_eq!(r.dependencies[0].name, "serde");
        assert_eq!(r.dependencies[0].dep_type, "import");
        assert!(r.dependencies[0].code.is_none());
    }

    #[test]
    fn ai_context_imports_collected_from_file_nodes() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let target = add_fn(&mut g, "do_work", "/src/app.rs", 10, 20, "fn do_work() {}");
        let module = add_node(
            &mut g,
            NodeType::Module,
            &[("name", str_prop("serde")), ("path", str_prop("serde"))],
        );
        edge(&mut g, target, module, EdgeType::Imports);

        let r = get_ai_context(&g, "/src/app.rs", 12, "explain", 100_000).expect("context");
        assert_eq!(r.imports, vec!["serde".to_string()]);
    }

    #[test]
    fn ai_context_sibling_functions_exclude_target_and_sort() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        add_fn(&mut g, "do_work", "/src/app.rs", 10, 20, "fn do_work() {}");
        add_node(
            &mut g,
            NodeType::Function,
            &[
                ("name", str_prop("other")),
                ("path", str_prop("/src/app.rs")),
                ("line_start", int_prop(30)),
                ("line_end", int_prop(40)),
                ("signature", str_prop("fn other()")),
                ("visibility", str_prop("private")),
                ("source", str_prop("fn other() {}")),
            ],
        );

        let r = get_ai_context(&g, "/src/app.rs", 12, "explain", 100_000).expect("context");
        assert_eq!(r.sibling_functions.len(), 1);
        let sib = &r.sibling_functions[0];
        assert_eq!(sib.name, "other");
        assert_eq!(sib.signature, "fn other()");
        assert_eq!(sib.visibility, "private");
        assert_eq!(sib.line_start, 30);
    }

    #[test]
    fn ai_context_architecture_reports_module_and_neighbors() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let target = add_fn(
            &mut g,
            "doStuff",
            "/src/services/auth.rs",
            1,
            5,
            "fn doStuff() {}",
        );
        let neighbor = add_fn(&mut g, "Db", "/src/db/conn.rs", 1, 3, "struct Db;");
        edge(&mut g, target, neighbor, EdgeType::Calls);

        let r =
            get_ai_context(&g, "/src/services/auth.rs", 2, "explain", 100_000).expect("context");
        let arch = r.architecture.expect("architecture");
        assert_eq!(arch.module, "auth");
        assert_eq!(arch.layer, Some("service".to_string()));
        let conn = arch.neighbors.iter().find(|n| n.module == "conn").unwrap();
        assert!(conn.relationship.contains("calls"));
    }

    #[test]
    fn ai_context_usage_examples_from_non_test_caller() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let target = add_fn(&mut g, "do_work", "/src/app.rs", 10, 20, "fn do_work() {}");
        let runner = add_fn(
            &mut g,
            "runner",
            "/src/r.rs",
            1,
            3,
            "fn runner() { do_work(); }",
        );
        edge(&mut g, runner, target, EdgeType::Calls);

        let r = get_ai_context(&g, "/src/app.rs", 12, "explain", 100_000).expect("context");
        let examples = r.usage_examples.expect("usage examples");
        assert_eq!(examples.len(), 1);
        let desc = examples[0].description.as_deref().unwrap();
        assert!(desc.contains("runner"));
        assert!(desc.contains("do_work"));
    }

    #[test]
    fn estimate_tokens_is_len_over_four_floor() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abc"), 0); // 3/4 floors to 0
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcdefg"), 1); // 7/4 floors to 1
        assert_eq!(estimate_tokens("abcdefgh"), 2);
    }

    #[test]
    fn token_budget_consume_and_has_budget() {
        let mut b = TokenBudget::new(10);
        assert!(b.has_budget());
        // Partial consume within budget succeeds.
        assert!(b.consume(4));
        assert!(b.has_budget());
        // Consuming exactly up to the total succeeds and exhausts budget.
        assert!(b.consume(6));
        assert!(!b.has_budget()); // used == total is not < total
                                  // Any further consume is rejected and leaves `used` unchanged.
        assert!(!b.consume(1));
        assert!(!b.has_budget());
    }

    #[test]
    fn token_budget_over_budget_consume_does_not_mutate() {
        let mut b = TokenBudget::new(10);
        // A single request exceeding the total is rejected outright.
        assert!(!b.consume(11));
        // Budget was untouched, so a smaller request still fits.
        assert!(b.consume(10));
        // Zero-total budget has no budget from the start.
        assert!(!TokenBudget::new(0).has_budget());
    }

    #[test]
    fn make_location_prefixes_absolute_paths_with_file_scheme() {
        let loc = make_location("/src/a.rs", 3, 7);
        assert_eq!(loc.uri, "file:///src/a.rs");
        assert_eq!(loc.range.start.line, 3);
        assert_eq!(loc.range.start.character, 0);
        assert_eq!(loc.range.end.line, 7);
        assert_eq!(loc.range.end.character, 0);
    }

    #[test]
    fn make_location_leaves_relative_paths_unscheme() {
        let loc = make_location("src/a.rs", 0, 0);
        assert_eq!(loc.uri, "src/a.rs");
        assert_eq!(loc.range.start.line, 0);
        assert_eq!(loc.range.end.line, 0);
    }

    #[test]
    fn truncate_to_call_site_near_top_keeps_all_without_omission() {
        // Target on line 1 (idx=1) => start=0, so no signature-prepend and
        // no omitted markers for a short snippet fully inside the window.
        let code = "fn sig() {\n    do_work();\n    more();\n}";
        let out = truncate_to_call_site(code, "do_work");
        assert!(out.contains("fn sig() {"));
        assert!(out.contains("do_work();"));
        assert!(out.contains("more();"));
        assert!(!out.contains("lines omitted"));
    }

    #[test]
    fn truncate_to_call_site_deep_prepends_signature_and_omits_both_sides() {
        // 40 lines, target on line index 12 => start=7 (>1) prepends the
        // signature plus a leading "6 lines omitted" (start-1) marker, and
        // end=18 leaves a trailing "22 lines omitted" (40-18) marker.
        let mut lines: Vec<String> = vec!["fn signature() {".to_string()];
        for i in 1..40 {
            if i == 12 {
                lines.push("    target_call();".to_string());
            } else {
                lines.push(format!("    line_{i}();"));
            }
        }
        let code = lines.join("\n");
        let out = truncate_to_call_site(&code, "target_call");
        assert!(out.contains("fn signature() {"));
        assert!(out.contains("target_call();"));
        assert!(out.contains("// ... (6 lines omitted)"));
        assert!(out.contains("// ... (22 lines omitted)"));
    }

    #[test]
    fn truncate_to_call_site_start_one_prepends_signature_without_leading_omission() {
        // Target on line index 6 (CALL_SITE_CONTEXT=5) => start=1, exercising the
        // `start > 0` (prepend signature) branch WITHOUT the nested `start > 1`
        // leading-omission marker. With 20 lines, end=12 (<20) so exactly one
        // trailing "lines omitted" marker is emitted - never a leading one.
        let mut lines: Vec<String> = vec!["fn signature() {".to_string()];
        for i in 1..20 {
            if i == 6 {
                lines.push("    target_call();".to_string());
            } else {
                lines.push(format!("    line_{i}();"));
            }
        }
        let code = lines.join("\n");
        let out = truncate_to_call_site(&code, "target_call");
        assert!(out.contains("fn signature() {"));
        assert!(out.contains("target_call();"));
        // start=1 skips the leading marker, leaving only the trailing one.
        assert_eq!(out.matches("lines omitted").count(), 1);
        assert!(out.contains("// ... (8 lines omitted)"));
    }

    #[test]
    fn truncate_to_call_site_not_found_falls_back_to_max_related_lines() {
        // 40 lines, none containing the target => fallback takes the first
        // MAX_RELATED_LINES (30) and reports the remaining 10 as omitted.
        let lines: Vec<String> = (0..40).map(|i| format!("stmt_{i}();")).collect();
        let code = lines.join("\n");
        let out = truncate_to_call_site(&code, "never_present");
        assert!(out.contains("stmt_0();"));
        assert!(out.contains("stmt_29();"));
        assert!(!out.contains("stmt_30();"));
        assert!(out.contains("// ... (10 lines omitted)"));
    }

    // ============================================================
    // make_related_symbol / make_related_symbol_for
    // ============================================================

    #[test]
    fn make_related_symbol_maps_fields_and_consumes_budget() {
        // A short function (<= MAX_RELATED_LINES) is emitted verbatim; the
        // wrapper delegates to make_related_symbol_for with target None.
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let id = add_fn(&mut g, "helper", "/src/lib.rs", 5, 7, "fn helper() {}");
        let mut budget = TokenBudget::new(1000);

        let sym = make_related_symbol(&g, id, "callee", 0.75, &mut budget).expect("symbol");
        assert_eq!(sym.name, "helper");
        assert_eq!(sym.relationship, "callee");
        assert_eq!(sym.code, "fn helper() {}");
        assert_eq!(sym.relevance_score, 0.75);
        assert_eq!(sym.location.uri, "file:///src/lib.rs");
        assert_eq!(sym.location.range.start.line, 5);
        assert_eq!(sym.location.range.end.line, 7);
        // "fn helper() {}" is 14 bytes => 3 estimated tokens were charged.
        assert_eq!(budget.used, estimate_tokens("fn helper() {}"));
    }

    #[test]
    fn make_related_symbol_for_large_code_with_target_truncates_to_call_site() {
        // 41 lines (> MAX_RELATED_LINES) with a known call site at index 20 =>
        // the Some(target) arm routes through truncate_to_call_site.
        let mut lines: Vec<String> = (0..41).map(|i| format!("stmt_{i}();")).collect();
        lines[20] = "call_target();".to_string();
        let source = lines.join("\n");

        let mut g = CodeGraph::in_memory().expect("in_memory");
        let id = add_fn(&mut g, "big", "/src/big.rs", 1, 41, &source);
        let mut budget = TokenBudget::new(100_000);

        let sym = make_related_symbol_for(&g, id, "caller", 1.0, &mut budget, Some("call_target"))
            .expect("symbol");
        assert!(sym.code.contains("call_target();"));
        assert!(sym.code.contains("lines omitted"));
        assert!(sym.code.lines().count() < 41);
    }

    #[test]
    fn make_related_symbol_for_large_code_without_target_keeps_full_source() {
        // Same oversized body, but target None => no truncation, full source kept.
        let lines: Vec<String> = (0..41).map(|i| format!("stmt_{i}();")).collect();
        let source = lines.join("\n");

        let mut g = CodeGraph::in_memory().expect("in_memory");
        let id = add_fn(&mut g, "big", "/src/big.rs", 1, 41, &source);
        let mut budget = TokenBudget::new(100_000);

        let sym =
            make_related_symbol_for(&g, id, "caller", 0.5, &mut budget, None).expect("symbol");
        assert_eq!(sym.code, source);
        assert!(!sym.code.contains("lines omitted"));
    }

    #[test]
    fn make_related_symbol_returns_none_when_budget_exhausted() {
        // A zero budget can't cover the estimated tokens of a non-empty body.
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let id = add_fn(&mut g, "helper", "/src/lib.rs", 1, 1, "fn helper() {}");
        let mut budget = TokenBudget::new(0);

        assert!(make_related_symbol(&g, id, "callee", 0.5, &mut budget).is_none());
        assert_eq!(budget.used, 0);
    }

    #[test]
    fn make_related_symbol_for_end_line_zero_falls_back_to_start_line() {
        // line_end == 0 => the emitted range end collapses onto start_line.
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let id = add_fn(&mut g, "helper", "/src/lib.rs", 12, 0, "fn helper() {}");
        let mut budget = TokenBudget::new(1000);

        let sym = make_related_symbol(&g, id, "callee", 0.5, &mut budget).expect("symbol");
        assert_eq!(sym.location.range.start.line, 12);
        assert_eq!(sym.location.range.end.line, 12);
    }

    // ============================================================
    // get_file_imports / get_dependencies (Imports-edge collectors)
    // ============================================================

    #[test]
    fn file_imports_collects_named_import_targets_and_dedups() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        // Two source nodes sharing the same file path.
        let a = add_fn(&mut g, "a", "/src/app.rs", 1, 5, "");
        let b = add_fn(&mut g, "b", "/src/app.rs", 6, 10, "");
        // Two distinct import targets, one shared between both sources.
        let dep1 = add_node(&mut g, NodeType::Module, &[("name", str_prop("serde"))]);
        let dep2 = add_node(&mut g, NodeType::Module, &[("name", str_prop("tokio"))]);
        edge(&mut g, a, dep1, EdgeType::Imports);
        edge(&mut g, a, dep2, EdgeType::Imports);
        // b re-imports serde — must be de-duplicated, not double counted.
        edge(&mut g, b, dep1, EdgeType::Imports);

        let mut imports = get_file_imports(&g, "/src/app.rs");
        imports.sort();
        assert_eq!(imports, vec!["serde".to_string(), "tokio".to_string()]);
    }

    #[test]
    fn file_imports_ignores_non_import_edges_and_empty_names() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let a = add_fn(&mut g, "a", "/src/app.rs", 1, 5, "");
        // A Calls edge (not Imports) must be skipped.
        let callee = add_node(&mut g, NodeType::Function, &[("name", str_prop("helper"))]);
        edge(&mut g, a, callee, EdgeType::Calls);
        // An Imports edge to a node with an empty name must be skipped.
        let anon = add_node(&mut g, NodeType::Module, &[("name", str_prop(""))]);
        edge(&mut g, a, anon, EdgeType::Imports);

        assert!(get_file_imports(&g, "/src/app.rs").is_empty());
    }

    #[test]
    fn file_imports_empty_for_unknown_path() {
        let g = CodeGraph::in_memory().expect("in_memory");
        assert!(get_file_imports(&g, "/nope.rs").is_empty());
    }

    #[test]
    fn dependencies_returns_import_targets_only() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let f = add_fn(&mut g, "f", "/src/app.rs", 1, 5, "");
        let dep = add_node(&mut g, NodeType::Module, &[("name", str_prop("anyhow"))]);
        let called = add_node(&mut g, NodeType::Function, &[("name", str_prop("g"))]);
        edge(&mut g, f, dep, EdgeType::Imports);
        edge(&mut g, f, called, EdgeType::Calls);

        let deps = get_dependencies(&g, f);
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].name, "anyhow");
        assert_eq!(deps[0].dep_type, "import");
        assert!(deps[0].code.is_none());
    }

    #[test]
    fn dependencies_skips_empty_named_imports() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let f = add_fn(&mut g, "f", "/src/app.rs", 1, 5, "");
        let anon = add_node(&mut g, NodeType::Module, &[("name", str_prop(""))]);
        edge(&mut g, f, anon, EdgeType::Imports);
        assert!(get_dependencies(&g, f).is_empty());
    }

    // ============================================================
    // get_sibling_functions
    // ============================================================

    #[test]
    fn sibling_functions_excludes_self_and_sorts_by_line() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let target = add_fn(&mut g, "target", "/src/app.rs", 20, 30, "");
        add_fn(&mut g, "later", "/src/app.rs", 40, 50, "");
        add_fn(&mut g, "earlier", "/src/app.rs", 1, 10, "");

        let sibs = get_sibling_functions(&g, target, "/src/app.rs");
        let names: Vec<_> = sibs.iter().map(|s| s.name.as_str()).collect();
        // target itself excluded; remaining sorted ascending by line_start.
        assert_eq!(names, vec!["earlier", "later"]);
        assert_eq!(sibs[0].line_start, 1);
    }

    #[test]
    fn sibling_functions_skips_non_functions_and_empty_names() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let target = add_fn(&mut g, "target", "/src/app.rs", 20, 30, "");
        // A Class node at the same path — not a Function, must be skipped.
        add_node(
            &mut g,
            NodeType::Class,
            &[
                ("name", str_prop("Widget")),
                ("path", str_prop("/src/app.rs")),
            ],
        );
        // A Function with an empty name — skipped.
        add_node(
            &mut g,
            NodeType::Function,
            &[("name", str_prop("")), ("path", str_prop("/src/app.rs"))],
        );
        assert!(get_sibling_functions(&g, target, "/src/app.rs").is_empty());
    }

    #[test]
    fn sibling_functions_signature_falls_back_to_name() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let target = add_fn(&mut g, "target", "/src/app.rs", 20, 30, "");
        // Sibling with no explicit signature property → signature == name.
        add_node(
            &mut g,
            NodeType::Function,
            &[
                ("name", str_prop("plain")),
                ("path", str_prop("/src/app.rs")),
                ("line_start", int_prop(5)),
            ],
        );
        let sibs = get_sibling_functions(&g, target, "/src/app.rs");
        assert_eq!(sibs.len(), 1);
        assert_eq!(sibs[0].signature, "plain");
    }

    // ============================================================
    // get_debug_hints — error-path detection over Calls edges
    // ============================================================

    #[test]
    fn debug_hints_collects_error_named_callees_only() {
        let mut g = CodeGraph::in_memory().expect("in_memory");
        let f = add_fn(&mut g, "f", "/src/app.rs", 1, 20, "");
        // Callees whose names match the error/panic/fail patterns.
        let e1 = add_node(
            &mut g,
            NodeType::Function,
            &[("name", str_prop("handle_error"))],
        );
        let e2 = add_node(
            &mut g,
            NodeType::Function,
            &[("name", str_prop("do_panic"))],
        );
        // A normal callee — must NOT appear in error_paths.
        let ok = add_node(&mut g, NodeType::Function, &[("name", str_prop("compute"))]);
        // A References edge (not Calls) to an error name — excluded (Calls-only).
        let ref_err = add_node(
            &mut g,
            NodeType::Function,
            &[("name", str_prop("throw_it"))],
        );
        edge(&mut g, f, e1, EdgeType::Calls);
        edge(&mut g, f, e2, EdgeType::Calls);
        edge(&mut g, f, ok, EdgeType::Calls);
        edge(&mut g, f, ref_err, EdgeType::References);

        let hints = get_debug_hints(&g, f).expect("hints");
        let mut paths = hints.error_paths.clone();
        paths.sort();
        assert_eq!(
            paths,
            vec!["do_panic".to_string(), "handle_error".to_string()]
        );
    }

    #[test]
    fn debug_hints_none_for_unknown_node() {
        let g = CodeGraph::in_memory().expect("in_memory");
        // No node with this id exists → get_node fails → None.
        assert!(get_debug_hints(&g, 999_999).is_none());
    }
}
