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
}
