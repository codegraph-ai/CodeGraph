// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Memory-related LSP command handlers for the CodeGraph memory system.

use serde::{Deserialize, Serialize};

// ==========================================
// Memory Store Request
// ==========================================

/// Parameters for storing a memory.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryStoreParams {
    /// Memory kind: "debug_context", "architectural_decision", "known_issue", "convention", "project_context"
    pub kind: String,
    /// Title of the memory
    pub title: String,
    /// Content/description
    pub content: String,
    /// Optional tags for categorization
    #[serde(default)]
    pub tags: Vec<String>,
    /// Optional code node IDs to link to
    #[serde(default)]
    pub code_links: Vec<CodeLinkParam>,
    /// Optional confidence score (0.0-1.0)
    pub confidence: Option<f32>,
    /// Kind-specific fields (e.g., problem/solution for debug_context)
    #[serde(flatten)]
    pub kind_data: serde_json::Value,
}

/// Code link parameter for associating memories with graph nodes.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeLinkParam {
    /// The node ID in the code graph
    pub node_id: String,
    /// The type of the node (e.g., "function", "class", "file")
    pub node_type: String,
}

/// Response after storing a memory.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryStoreResponse {
    /// The ID of the newly created memory
    pub id: String,
    /// Whether the operation was successful
    pub success: bool,
}

// ==========================================
// Memory Search Request
// ==========================================

/// Parameters for searching memories.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySearchParams {
    /// The search query string
    pub query: String,
    /// Maximum number of results to return
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// Filter by tags
    #[serde(default)]
    pub tags: Vec<String>,
    /// Filter by memory kinds
    #[serde(default)]
    pub kinds: Vec<String>,
    /// Only return current (non-invalidated) memories
    #[serde(default = "default_true")]
    pub current_only: bool,
    /// Code context for graph proximity scoring (node IDs)
    #[serde(default)]
    pub code_context: Vec<String>,
}

fn default_limit() -> usize {
    10
}

fn default_true() -> bool {
    true
}

/// A single memory search result.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySearchResult {
    /// The memory ID
    pub id: String,
    /// The kind of memory
    pub kind: String,
    /// Title of the memory
    pub title: String,
    /// Content/description
    pub content: String,
    /// Tags associated with this memory
    pub tags: Vec<String>,
    /// Relevance score (0.0-1.0)
    pub score: f32,
    /// Whether the memory is still current (not invalidated)
    pub is_current: bool,
}

/// Response for memory search.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySearchResponse {
    /// The search results
    pub results: Vec<MemorySearchResult>,
    /// Total number of matching memories (before limit)
    pub total: usize,
}

// ==========================================
// Memory Get Request
// ==========================================

/// Parameters for getting a memory by ID.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryGetParams {
    /// The memory ID to retrieve
    pub id: String,
}

/// Full memory details response.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryGetResponse {
    /// The memory ID
    pub id: String,
    /// The kind of memory (may include kind-specific data)
    pub kind: serde_json::Value,
    /// Title of the memory
    pub title: String,
    /// Content/description
    pub content: String,
    /// Tags associated with this memory
    pub tags: Vec<String>,
    /// Code links to graph nodes
    pub code_links: Vec<CodeLinkResponse>,
    /// Confidence score (0.0-1.0)
    pub confidence: f32,
    /// Whether the memory is still current (not invalidated)
    pub is_current: bool,
    /// ISO 8601 timestamp when the memory was created
    pub created_at: String,
    /// ISO 8601 timestamp when the memory became valid (if temporal)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valid_from: Option<String>,
}

/// Code link response for graph node associations.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeLinkResponse {
    /// The node ID in the code graph
    pub node_id: String,
    /// The type of the node (e.g., "function", "class", "file")
    pub node_type: String,
}

// ==========================================
// Memory Invalidate Request
// ==========================================

/// Parameters for invalidating a memory.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryInvalidateParams {
    /// The memory ID to invalidate
    pub id: String,
}

/// Response for memory invalidation.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryInvalidateResponse {
    /// Whether the operation was successful
    pub success: bool,
}

// ==========================================
// Memory List Request
// ==========================================

/// Parameters for listing memories.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryListParams {
    /// Filter by memory kinds
    #[serde(default)]
    pub kinds: Vec<String>,
    /// Filter by tags
    #[serde(default)]
    pub tags: Vec<String>,
    /// Only return current (non-invalidated) memories
    #[serde(default = "default_true")]
    pub current_only: bool,
    /// Maximum number of results to return
    #[serde(default = "default_list_limit")]
    pub limit: usize,
    /// Offset for pagination
    #[serde(default)]
    pub offset: usize,
}

fn default_list_limit() -> usize {
    50
}

/// Response for listing memories.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryListResponse {
    /// The memories matching the criteria
    pub memories: Vec<MemorySearchResult>,
    /// Total number of matching memories (before limit/offset)
    pub total: usize,
    /// Whether there are more results available
    pub has_more: bool,
}

// ==========================================
// Memory Update Request
// ==========================================

/// Parameters for updating a memory.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryUpdateParams {
    /// The memory ID to update
    pub id: String,
    /// New title (optional, keeps existing if not provided)
    #[serde(default)]
    pub title: Option<String>,
    /// New content (optional, keeps existing if not provided)
    #[serde(default)]
    pub content: Option<String>,
    /// New tags (optional, keeps existing if not provided)
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    /// New confidence score (optional, keeps existing if not provided)
    #[serde(default)]
    pub confidence: Option<f32>,
    /// Code links to add
    #[serde(default)]
    pub add_code_links: Vec<CodeLinkParam>,
    /// Code link node IDs to remove
    #[serde(default)]
    pub remove_code_links: Vec<String>,
}

/// Response for memory update.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryUpdateResponse {
    /// Whether the operation was successful
    pub success: bool,
    /// The updated memory (if successful)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryGetResponse>,
}

// ==========================================
// Memory Context Request
// ==========================================

/// Parameters for getting memories relevant to a code context.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryContextParams {
    /// File URI for context
    pub uri: String,
    /// Optional position in the file
    #[serde(default)]
    pub position: Option<PositionParam>,
    /// Maximum number of memories to return
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// Filter by memory kinds
    #[serde(default)]
    pub kinds: Vec<String>,
}

/// Position parameter for file locations.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PositionParam {
    /// Line number (0-indexed)
    pub line: u32,
    /// Character/column number (0-indexed)
    pub character: u32,
}

/// Response for memory context query.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryContextResponse {
    /// Memories relevant to the code context
    pub memories: Vec<ContextMemory>,
}

/// A memory with context-specific relevance information.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextMemory {
    /// The memory ID
    pub id: String,
    /// The kind of memory
    pub kind: String,
    /// Title of the memory
    pub title: String,
    /// Content/description
    pub content: String,
    /// Tags associated with this memory
    pub tags: Vec<String>,
    /// Relevance score to the current context (0.0-1.0)
    pub relevance_score: f32,
    /// Why this memory is relevant
    pub relevance_reason: String,
}

// ==========================================
// Git Mining Request Types
// ==========================================

/// Parameters for mining git history for memories.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MineGitHistoryParams {
    /// Maximum number of commits to process
    #[serde(default = "default_max_commits")]
    pub max_commits: Option<usize>,
    /// Minimum confidence score for creating memories (0.0-1.0)
    #[serde(default = "default_min_confidence")]
    pub min_confidence: Option<f64>,
}

fn default_max_commits() -> Option<usize> {
    Some(500)
}

fn default_min_confidence() -> Option<f64> {
    Some(0.7)
}

/// Parameters for mining git history for a specific file.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MineGitFileParams {
    /// The file URI to mine history for
    pub uri: String,
    /// Maximum number of commits to process
    #[serde(default = "default_file_max_commits")]
    pub max_commits: Option<usize>,
    /// Minimum confidence score for creating memories (0.0-1.0)
    #[serde(default = "default_min_confidence")]
    pub min_confidence: Option<f64>,
}

fn default_file_max_commits() -> Option<usize> {
    Some(100)
}

// ==========================================
// Symbol Info Request (for MCP)
// ==========================================

/// Parameters for getting symbol information.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SymbolInfoParams {
    /// The file URI containing the symbol
    pub uri: String,
    /// Line number of the symbol (0-indexed)
    pub line: u32,
    /// Character position (0-indexed)
    #[serde(default)]
    pub character: Option<u32>,
    /// Whether to include all references to the symbol
    #[serde(default)]
    pub include_references: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_store_params_deserialize() {
        let json = r#"{
            "kind": "debug_context",
            "title": "Fix null pointer",
            "content": "Found issue with null check",
            "tags": ["bug", "critical"],
            "codeLinks": [{"nodeId": "123", "nodeType": "function"}],
            "confidence": 0.9,
            "problem": "null pointer",
            "solution": "add null check"
        }"#;

        let params: MemoryStoreParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.kind, "debug_context");
        assert_eq!(params.title, "Fix null pointer");
        assert_eq!(params.tags.len(), 2);
        assert_eq!(params.code_links.len(), 1);
        assert_eq!(params.confidence, Some(0.9));
        assert!(params.kind_data.get("problem").is_some());
    }

    #[test]
    fn test_memory_store_params_minimal() {
        let json = r#"{
            "kind": "convention",
            "title": "Use snake_case",
            "content": "All functions should use snake_case"
        }"#;

        let params: MemoryStoreParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.kind, "convention");
        assert!(params.tags.is_empty());
        assert!(params.code_links.is_empty());
        assert!(params.confidence.is_none());
    }

    #[test]
    fn test_memory_search_params_defaults() {
        let json = r#"{
            "query": "authentication"
        }"#;

        let params: MemorySearchParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.query, "authentication");
        assert_eq!(params.limit, 10);
        assert!(params.current_only);
        assert!(params.tags.is_empty());
        assert!(params.kinds.is_empty());
    }

    #[test]
    fn test_memory_search_params_full() {
        let json = r#"{
            "query": "security",
            "limit": 20,
            "tags": ["auth"],
            "kinds": ["architectural_decision", "known_issue"],
            "currentOnly": false,
            "codeContext": ["node1", "node2"]
        }"#;

        let params: MemorySearchParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.limit, 20);
        assert!(!params.current_only);
        assert_eq!(params.tags.len(), 1);
        assert_eq!(params.kinds.len(), 2);
        assert_eq!(params.code_context.len(), 2);
    }

    #[test]
    fn test_memory_store_response_serialize() {
        let response = MemoryStoreResponse {
            id: "mem_123".to_string(),
            success: true,
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"id\":\"mem_123\""));
        assert!(json.contains("\"success\":true"));
    }

    #[test]
    fn test_memory_search_response_serialize() {
        let response = MemorySearchResponse {
            results: vec![MemorySearchResult {
                id: "mem_1".to_string(),
                kind: "debug_context".to_string(),
                title: "Bug fix".to_string(),
                content: "Fixed the bug".to_string(),
                tags: vec!["bug".to_string()],
                score: 0.95,
                is_current: true,
            }],
            total: 1,
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"isCurrent\":true"));
        assert!(json.contains("\"score\":0.95"));
    }

    #[test]
    fn test_memory_get_response_serialize() {
        let response = MemoryGetResponse {
            id: "mem_123".to_string(),
            kind: serde_json::json!({"type": "debug_context", "problem": "null ptr"}),
            title: "Null pointer fix".to_string(),
            content: "Description".to_string(),
            tags: vec![],
            code_links: vec![CodeLinkResponse {
                node_id: "42".to_string(),
                node_type: "function".to_string(),
            }],
            confidence: 0.8,
            is_current: true,
            created_at: "2025-01-21T10:00:00Z".to_string(),
            valid_from: None,
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"createdAt\":\"2025-01-21T10:00:00Z\""));
        assert!(!json.contains("validFrom")); // Should be skipped when None
    }

    #[test]
    fn test_memory_invalidate_params() {
        let json = r#"{"id": "mem_456"}"#;

        let params: MemoryInvalidateParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.id, "mem_456");
    }

    #[test]
    fn test_memory_list_params_defaults() {
        let json = r#"{}"#;

        let params: MemoryListParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.limit, 50);
        assert_eq!(params.offset, 0);
        assert!(params.current_only);
    }

    #[test]
    fn test_memory_update_params() {
        let json = r#"{
            "id": "mem_789",
            "title": "Updated title",
            "confidence": 0.95,
            "addCodeLinks": [{"nodeId": "new_node", "nodeType": "class"}],
            "removeCodeLinks": ["old_node"]
        }"#;

        let params: MemoryUpdateParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.id, "mem_789");
        assert_eq!(params.title, Some("Updated title".to_string()));
        assert!(params.content.is_none());
        assert_eq!(params.add_code_links.len(), 1);
        assert_eq!(params.remove_code_links.len(), 1);
    }

    #[test]
    fn test_memory_context_params() {
        let json = r#"{
            "uri": "file:///test/main.rs",
            "position": {"line": 10, "character": 5},
            "limit": 5,
            "kinds": ["debug_context"]
        }"#;

        let params: MemoryContextParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.uri, "file:///test/main.rs");
        assert!(params.position.is_some());
        let pos = params.position.unwrap();
        assert_eq!(pos.line, 10);
        assert_eq!(pos.character, 5);
    }

    #[test]
    fn test_code_link_param_camel_case() {
        let json = r#"{"nodeId": "123", "nodeType": "function"}"#;

        let param: CodeLinkParam = serde_json::from_str(json).unwrap();
        assert_eq!(param.node_id, "123");
        assert_eq!(param.node_type, "function");
    }

    #[test]
    fn test_context_memory_serialize() {
        let memory = ContextMemory {
            id: "mem_1".to_string(),
            kind: "known_issue".to_string(),
            title: "Race condition".to_string(),
            content: "Watch out for concurrent access".to_string(),
            tags: vec!["concurrency".to_string()],
            relevance_score: 0.85,
            relevance_reason: "Same file context".to_string(),
        };

        let json = serde_json::to_string(&memory).unwrap();
        assert!(json.contains("\"relevanceScore\":0.85"));
        assert!(json.contains("\"relevanceReason\":\"Same file context\""));
    }
}
