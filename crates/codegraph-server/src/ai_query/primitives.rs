// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Query Primitives for AI Agents
//!
//! Core query primitives that AI agents can compose into complex workflows:
//! - symbol_search: Fast text-based symbol search with BM25 ranking
//! - find_by_imports: Discover code by imported libraries/modules
//! - find_by_signature: Pattern matching on function signatures
//! - find_entry_points: Detect architectural entry points
//! - traverse_graph: Custom graph traversal with filters
//! - get_callers/callees: Fast relationship queries
//! - get_symbol_info: Rich metadata retrieval

use codegraph::NodeId;
use serde::{Deserialize, Serialize};

/// Maximum length for signatures before truncation (default: 500 chars)
pub const MAX_SIGNATURE_LENGTH: usize = 500;

/// Truncate a string to max_len characters, adding "..." if truncated
pub fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

/// Truncate an optional string
pub fn truncate_optional(s: Option<&str>, max_len: usize) -> Option<String> {
    s.map(|s| truncate_string(s, max_len))
}

/// Search scope for queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SearchScope {
    /// Search entire workspace
    #[default]
    Workspace,
    /// Search within a single module/directory
    Module,
    /// Search within a single file
    File,
}

/// Symbol types to filter by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SymbolType {
    Function,
    Class,
    Variable,
    Module,
    Interface,
    Type,
}

/// Options for symbol search queries.
#[derive(Debug, Clone, Default)]
pub struct SearchOptions {
    /// Search scope (workspace, module, file)
    pub scope: SearchScope,
    /// Filter by symbol types
    pub symbol_types: Vec<SymbolType>,
    /// Filter by programming languages
    pub languages: Vec<String>,
    /// Maximum results to return
    pub limit: usize,
    /// Include private/internal symbols
    pub include_private: bool,
    /// Compact mode: omit signatures and docstrings for smaller responses
    pub compact: bool,
}

impl SearchOptions {
    /// Create default search options.
    pub fn new() -> Self {
        Self {
            scope: SearchScope::Workspace,
            symbol_types: Vec::new(),
            languages: Vec::new(),
            limit: 20,
            include_private: false,
            compact: false,
        }
    }

    /// Set the limit.
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }

    /// Set the scope.
    pub fn with_scope(mut self, scope: SearchScope) -> Self {
        self.scope = scope;
        self
    }

    /// Filter by symbol types.
    pub fn with_symbol_types(mut self, types: Vec<SymbolType>) -> Self {
        self.symbol_types = types;
        self
    }

    /// Filter by languages.
    pub fn with_languages(mut self, languages: Vec<String>) -> Self {
        self.languages = languages;
        self
    }

    /// Include private symbols.
    pub fn include_private(mut self) -> Self {
        self.include_private = true;
        self
    }

    /// Enable compact mode (omit signatures and docstrings).
    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
    }

    /// Set compact mode explicitly.
    pub fn with_compact(mut self, compact: bool) -> Self {
        self.compact = compact;
        self
    }

    /// Set include_private flag.
    pub fn with_include_private(mut self, include_private: bool) -> Self {
        self.include_private = include_private;
        self
    }
}

/// Location information for a symbol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolLocation {
    /// File path
    pub file: String,
    /// Line number (1-indexed)
    pub line: u32,
    /// Column number (0-indexed)
    pub column: u32,
    /// End line number (1-indexed)
    pub end_line: u32,
    /// End column number (0-indexed)
    pub end_column: u32,
}

/// Basic symbol information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolInfo {
    /// Symbol name
    pub name: String,
    /// Symbol type (function, class, etc.)
    pub kind: String,
    /// Location in source code
    pub location: SymbolLocation,
    /// Function signature if applicable
    pub signature: Option<String>,
    /// Documentation string
    pub docstring: Option<String>,
    /// Whether the symbol is exported/public
    pub is_public: bool,
    /// Visibility level: "public", "private", "protected", "pub", "pub(crate)", etc.
    pub visibility: String,
}

/// A match result from symbol search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolMatch {
    /// The node ID in the graph
    pub node_id: NodeId,
    /// Symbol information
    pub symbol: SymbolInfo,
    /// BM25 relevance score
    pub score: f32,
    /// Why this result matched
    pub match_reason: String,
}

/// Context information about a symbol's relationships.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolContext {
    /// Direct callers of this symbol
    pub callers: Vec<String>,
    /// Direct callees from this symbol
    pub callees: Vec<String>,
    /// Imported modules/libraries
    pub imports: Vec<String>,
    /// Whether this symbol has tests
    pub has_tests: bool,
    /// Cyclomatic complexity if available
    pub complexity: Option<u32>,
    /// Number of references
    pub reference_count: usize,
}

/// Complete symbol search result with context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolSearchResult {
    /// Matching symbols
    pub results: Vec<SymbolMatch>,
    /// Total number of matches (before limit)
    pub total_matches: usize,
    /// Query execution time
    pub query_time_ms: u64,
    /// Embedding status message (set when embeddings are still building)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedding_status: Option<String>,
}

/// A pair of similar/duplicate functions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicatePair {
    /// First symbol
    pub symbol_a: SymbolInfo,
    pub node_id_a: NodeId,
    /// Second symbol
    pub symbol_b: SymbolInfo,
    pub node_id_b: NodeId,
    /// Cosine similarity score (0.0-1.0)
    pub similarity: f32,
}

/// Result of duplicate/similar code detection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateResult {
    pub pairs: Vec<DuplicatePair>,
    pub total_symbols_compared: usize,
    pub threshold: f32,
    pub query_time_ms: u64,
}

/// A cluster of semantically related functions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolCluster {
    /// Cluster label (auto-generated from most representative member)
    pub label: String,
    /// Members of this cluster
    pub members: Vec<ClusterMember>,
    /// Number of members
    pub size: usize,
}

/// A member of a symbol cluster.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterMember {
    pub node_id: NodeId,
    pub name: String,
    pub file: String,
    pub line: u32,
    pub similarity_to_centroid: f32,
}

/// Result of symbol clustering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterResult {
    pub clusters: Vec<SymbolCluster>,
    pub total_symbols: usize,
    pub unclustered: usize,
    pub query_time_ms: u64,
}

/// Comparison between two functions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolComparison {
    pub symbol_a: SymbolInfo,
    pub symbol_b: SymbolInfo,
    pub similarity: f32,
    pub verdict: String,
    pub structural: StructuralComparison,
    pub shared_callers: Vec<String>,
    pub shared_callees: Vec<String>,
}

/// Structural comparison between two symbols.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuralComparison {
    pub same_file: bool,
    pub same_language: bool,
    pub complexity_a: u32,
    pub complexity_b: u32,
    pub lines_a: u32,
    pub lines_b: u32,
    pub param_count_a: usize,
    pub param_count_b: usize,
}

/// Import match mode for find_by_imports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ImportMatchMode {
    /// Exact library name match (e.g., "re" matches only "re")
    #[default]
    Exact,
    /// Prefix match (e.g., "email" matches "email", "email.utils")
    Prefix,
    /// Fuzzy match for related libraries (e.g., "regex" matches "re", "regex")
    Fuzzy,
}

/// Options for find_by_imports query.
#[derive(Debug, Clone, Default)]
pub struct ImportSearchOptions {
    /// How to match library names
    pub match_mode: ImportMatchMode,
    /// Search scope
    pub scope: SearchScope,
    /// Filter by languages
    pub languages: Vec<String>,
    /// Include code that transitively imports these libraries
    pub include_transitive: bool,
}

impl ImportSearchOptions {
    /// Create new import search options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set match mode.
    pub fn with_match_mode(mut self, mode: ImportMatchMode) -> Self {
        self.match_mode = mode;
        self
    }

    /// Set scope.
    pub fn with_scope(mut self, scope: SearchScope) -> Self {
        self.scope = scope;
        self
    }

    /// Include transitive imports.
    pub fn include_transitive(mut self) -> Self {
        self.include_transitive = true;
        self
    }
}

/// Entry point types for architectural discovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryType {
    /// HTTP/REST handlers (e.g., Express routes, FastAPI endpoints)
    HttpHandler,
    /// CLI command handlers
    CliCommand,
    /// Exported/public API functions
    PublicApi,
    /// Event handlers and callbacks
    EventHandler,
    /// Test functions
    TestEntry,
    /// Program main entry points
    Main,
}

/// An entry point in the codebase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryPoint {
    /// Node ID in the graph
    pub node_id: NodeId,
    /// Entry point type
    pub entry_type: EntryType,
    /// HTTP route if applicable (e.g., "/api/users")
    pub route: Option<String>,
    /// HTTP method if applicable (e.g., "GET", "POST")
    pub method: Option<String>,
    /// Description or docstring
    pub description: Option<String>,
    /// Symbol information
    pub symbol: SymbolInfo,
}

/// Direction for graph traversal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TraversalDirection {
    /// Follow outgoing edges (calls, dependencies)
    Outgoing,
    /// Follow incoming edges (callers, dependents)
    Incoming,
    /// Bidirectional traversal
    Both,
}

/// Filter options for graph traversal.
#[derive(Debug, Clone, Default)]
pub struct TraversalFilter {
    /// Filter by symbol types (node types to include in results)
    pub symbol_types: Vec<SymbolType>,
    /// Filter by edge types (only traverse edges of these types)
    pub edge_types: Vec<String>,
    /// Maximum number of nodes to return
    pub max_nodes: usize,
}

impl TraversalFilter {
    /// Create a new traversal filter.
    pub fn new() -> Self {
        Self {
            symbol_types: Vec::new(),
            edge_types: Vec::new(),
            max_nodes: 1000,
        }
    }

    /// Set maximum nodes.
    pub fn with_max_nodes(mut self, max: usize) -> Self {
        self.max_nodes = max;
        self
    }

    /// Filter by symbol types.
    pub fn with_symbol_types(mut self, types: Vec<SymbolType>) -> Self {
        self.symbol_types = types;
        self
    }

    /// Filter by edge types (e.g., "calls", "imports").
    pub fn with_edge_types(mut self, types: Vec<String>) -> Self {
        self.edge_types = types;
        self
    }
}

/// A node in a graph traversal result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraversalNode {
    /// Node ID
    pub node_id: NodeId,
    /// Depth from starting node
    pub depth: u32,
    /// Path from start (list of node IDs)
    pub path: Vec<NodeId>,
    /// Edge type that led to this node
    pub edge_type: String,
    /// Symbol information
    pub symbol: SymbolInfo,
}

/// Information about a caller/callee relationship.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallInfo {
    /// The caller or callee node
    pub node_id: NodeId,
    /// Symbol information
    pub symbol: SymbolInfo,
    /// Location of the call site
    pub call_site: SymbolLocation,
    /// Depth in the call chain (1 = direct)
    pub depth: u32,
    /// For ops struct registrations: the struct type (e.g., "net_device_ops")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub via_ops_struct: Option<String>,
    /// For ops struct registrations: the field name (e.g., "ndo_open")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ops_field: Option<String>,
}

/// A function that implements an ops struct field.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImplementorInfo {
    /// The implementor node ID
    pub node_id: NodeId,
    /// Symbol information
    pub symbol: SymbolInfo,
    /// The ops struct type (e.g., "net_device_ops")
    pub struct_type: String,
    /// The field name (e.g., "ndo_open")
    pub field_name: String,
}

/// Detailed information about a symbol (get_symbol_info result).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetailedSymbolInfo {
    /// Basic symbol information
    pub symbol: SymbolInfo,
    /// Direct callers
    pub callers: Vec<CallInfo>,
    /// Direct callees
    pub callees: Vec<CallInfo>,
    /// Imported dependencies
    pub dependencies: Vec<String>,
    /// Modules that import this
    pub dependents: Vec<String>,
    /// Code complexity metrics
    pub complexity: Option<u32>,
    /// Lines of code
    pub lines_of_code: usize,
    /// Whether this symbol has tests
    pub has_tests: bool,
    /// Whether this symbol is exported/public
    pub is_public: bool,
    /// Whether this symbol is deprecated
    pub is_deprecated: bool,
    /// Number of references to this symbol
    pub reference_count: usize,
}

/// Function signature pattern for find_by_signature.
#[derive(Debug, Clone, Default)]
pub struct SignaturePattern {
    /// Regex pattern for function name
    pub name_pattern: Option<String>,
    /// Expected return type
    pub return_type: Option<String>,
    /// Parameter count range (min, max)
    pub param_count: Option<(usize, usize)>,
    /// Required modifiers (async, public, static, etc.)
    pub modifiers: Vec<String>,
}

impl SignaturePattern {
    /// Create a new signature pattern.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set name pattern.
    pub fn with_name_pattern(mut self, pattern: &str) -> Self {
        self.name_pattern = Some(pattern.to_string());
        self
    }

    /// Set return type.
    pub fn with_return_type(mut self, return_type: &str) -> Self {
        self.return_type = Some(return_type.to_string());
        self
    }

    /// Set parameter count range.
    pub fn with_param_count(mut self, min: usize, max: usize) -> Self {
        self.param_count = Some((min, max));
        self
    }

    /// Add required modifier.
    pub fn with_modifier(mut self, modifier: &str) -> Self {
        self.modifiers.push(modifier.to_string());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_options_builder() {
        let options = SearchOptions::new()
            .with_limit(50)
            .with_scope(SearchScope::Module)
            .with_symbol_types(vec![SymbolType::Function])
            .with_languages(vec!["rust".to_string()])
            .include_private();

        assert_eq!(options.limit, 50);
        assert_eq!(options.scope, SearchScope::Module);
        assert_eq!(options.symbol_types, vec![SymbolType::Function]);
        assert_eq!(options.languages, vec!["rust"]);
        assert!(options.include_private);
    }

    #[test]
    fn test_search_options_defaults() {
        let options = SearchOptions::new();
        assert_eq!(options.scope, SearchScope::Workspace);
        assert_eq!(options.limit, 20);
        assert!(!options.include_private);
        assert!(options.symbol_types.is_empty());
        assert!(options.languages.is_empty());
    }

    #[test]
    fn test_traversal_filter_builder() {
        let filter = TraversalFilter::new()
            .with_max_nodes(500)
            .with_symbol_types(vec![SymbolType::Function, SymbolType::Class]);

        assert_eq!(filter.max_nodes, 500);
        assert_eq!(filter.symbol_types.len(), 2);
    }

    #[test]
    fn test_signature_pattern_builder() {
        let pattern = SignaturePattern::new()
            .with_name_pattern(".*validate.*")
            .with_return_type("bool")
            .with_param_count(1, 3)
            .with_modifier("async")
            .with_modifier("public");

        assert_eq!(pattern.name_pattern, Some(".*validate.*".to_string()));
        assert_eq!(pattern.return_type, Some("bool".to_string()));
        assert_eq!(pattern.param_count, Some((1, 3)));
        assert_eq!(pattern.modifiers, vec!["async", "public"]);
    }

    #[test]
    fn test_import_match_mode_default() {
        let mode = ImportMatchMode::default();
        assert_eq!(mode, ImportMatchMode::Exact);
    }

    #[test]
    fn test_search_scope_default() {
        let scope = SearchScope::default();
        assert_eq!(scope, SearchScope::Workspace);
    }
}
