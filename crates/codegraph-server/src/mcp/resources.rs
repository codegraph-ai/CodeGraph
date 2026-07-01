// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! MCP Resource Definitions
//!
//! Defines resources available through the MCP protocol.

use super::protocol::{Resource, ResourceContent, ResourceReadResult};
use codegraph::CodeGraph;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Get all available resources
pub fn get_all_resources() -> Vec<Resource> {
    vec![
        Resource {
            uri: "codegraph://graph/stats".to_string(),
            name: "Graph Statistics".to_string(),
            description: Some(
                "Statistics about the code graph including node and edge counts by type"
                    .to_string(),
            ),
            mime_type: Some("application/json".to_string()),
        },
        Resource {
            uri: "codegraph://memory/stats".to_string(),
            name: "Memory Statistics".to_string(),
            description: Some(
                "Statistics about stored memories including counts by kind and status".to_string(),
            ),
            mime_type: Some("application/json".to_string()),
        },
        Resource {
            uri: "codegraph://index/status".to_string(),
            name: "Index Status".to_string(),
            description: Some("Current indexing status and workspace configuration".to_string()),
            mime_type: Some("application/json".to_string()),
        },
    ]
}

/// Read a resource by URI
pub async fn read_resource(
    uri: &str,
    graph: Arc<RwLock<CodeGraph>>,
    memory_manager: &crate::memory::MemoryManager,
    workspace_folders: &[std::path::PathBuf],
) -> Option<ResourceReadResult> {
    match uri {
        "codegraph://graph/stats" => {
            let stats = get_graph_stats(graph).await;
            Some(ResourceReadResult {
                contents: vec![ResourceContent {
                    uri: uri.to_string(),
                    mime_type: Some("application/json".to_string()),
                    text: Some(serde_json::to_string_pretty(&stats).unwrap_or_default()),
                    blob: None,
                }],
            })
        }
        "codegraph://memory/stats" => {
            let stats = get_memory_stats(memory_manager).await;
            Some(ResourceReadResult {
                contents: vec![ResourceContent {
                    uri: uri.to_string(),
                    mime_type: Some("application/json".to_string()),
                    text: Some(serde_json::to_string_pretty(&stats).unwrap_or_default()),
                    blob: None,
                }],
            })
        }
        "codegraph://index/status" => {
            let status = get_index_status(graph, workspace_folders).await;
            Some(ResourceReadResult {
                contents: vec![ResourceContent {
                    uri: uri.to_string(),
                    mime_type: Some("application/json".to_string()),
                    text: Some(serde_json::to_string_pretty(&status).unwrap_or_default()),
                    blob: None,
                }],
            })
        }
        _ => None,
    }
}

async fn get_graph_stats(graph: Arc<RwLock<CodeGraph>>) -> serde_json::Value {
    let graph = graph.read().await;

    // Count nodes by type
    let mut node_counts = std::collections::HashMap::new();
    let mut total_nodes = 0u64;

    // Get all nodes
    if let Ok(nodes) = graph.query().execute() {
        total_nodes = nodes.len() as u64;
        for node_id in nodes {
            if let Ok(node) = graph.get_node(node_id) {
                let type_name = format!("{:?}", node.node_type);
                *node_counts.entry(type_name).or_insert(0u64) += 1;
            }
        }
    }

    serde_json::json!({
        "totalNodes": total_nodes,
        "nodesByType": node_counts,
    })
}

async fn get_memory_stats(memory_manager: &crate::memory::MemoryManager) -> serde_json::Value {
    match memory_manager.stats().await {
        Ok(stats) => stats,
        Err(e) => serde_json::json!({
            "error": format!("Failed to get memory stats: {}", e)
        }),
    }
}

async fn get_index_status(
    graph: Arc<RwLock<CodeGraph>>,
    workspace_folders: &[std::path::PathBuf],
) -> serde_json::Value {
    let graph = graph.read().await;
    let total_nodes = graph.query().count().unwrap_or(0);

    serde_json::json!({
        "indexed": true,
        "totalSymbols": total_nodes,
        "workspaceFolders": workspace_folders.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph::{NodeType, PropertyMap};

    #[test]
    fn test_get_all_resources() {
        let resources = get_all_resources();
        assert_eq!(resources.len(), 3);
        assert!(resources.iter().any(|r| r.uri == "codegraph://graph/stats"));
        assert!(resources
            .iter()
            .any(|r| r.uri == "codegraph://memory/stats"));
        assert!(resources
            .iter()
            .any(|r| r.uri == "codegraph://index/status"));
    }

    /// Build an in-memory graph with one Function and one Class node.
    fn graph_with_nodes() -> Arc<RwLock<CodeGraph>> {
        let mut g = CodeGraph::in_memory().unwrap();
        let mut f = PropertyMap::new();
        f.insert("name", "foo");
        g.add_node(NodeType::Function, f).unwrap();
        let mut c = PropertyMap::new();
        c.insert("name", "Bar");
        g.add_node(NodeType::Class, c).unwrap();
        Arc::new(RwLock::new(g))
    }

    #[tokio::test]
    async fn test_get_graph_stats_counts_nodes_by_type() {
        let stats = get_graph_stats(graph_with_nodes()).await;
        assert_eq!(stats["totalNodes"], 2);
        assert_eq!(stats["nodesByType"]["Function"], 1);
        assert_eq!(stats["nodesByType"]["Class"], 1);
    }

    #[tokio::test]
    async fn test_get_graph_stats_empty_graph() {
        let graph = Arc::new(RwLock::new(CodeGraph::in_memory().unwrap()));
        let stats = get_graph_stats(graph).await;
        assert_eq!(stats["totalNodes"], 0);
        assert!(stats["nodesByType"].as_object().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_get_index_status_reports_symbols_and_folders() {
        let folders = vec![std::path::PathBuf::from("/tmp/workspace-a")];
        let status = get_index_status(graph_with_nodes(), &folders).await;
        assert_eq!(status["indexed"], true);
        assert_eq!(status["totalSymbols"], 2);
        assert_eq!(status["workspaceFolders"][0], "/tmp/workspace-a");
    }

    #[tokio::test]
    async fn test_read_resource_graph_stats_branch() {
        let mm = crate::memory::MemoryManager::new(None);
        let result = read_resource("codegraph://graph/stats", graph_with_nodes(), &mm, &[])
            .await
            .expect("graph/stats should return Some");
        let content = &result.contents[0];
        assert_eq!(content.uri, "codegraph://graph/stats");
        assert_eq!(content.mime_type.as_deref(), Some("application/json"));
        let text = content.text.as_ref().unwrap();
        assert!(text.contains("totalNodes"));
    }

    #[tokio::test]
    async fn test_read_resource_index_status_branch() {
        let mm = crate::memory::MemoryManager::new(None);
        let folders = vec![std::path::PathBuf::from("/tmp/ws")];
        let result = read_resource(
            "codegraph://index/status",
            graph_with_nodes(),
            &mm,
            &folders,
        )
        .await
        .expect("index/status should return Some");
        let text = result.contents[0].text.as_ref().unwrap();
        assert!(text.contains("totalSymbols"));
        assert!(text.contains("/tmp/ws"));
    }

    #[tokio::test]
    async fn test_read_resource_memory_stats_branch_returns_some() {
        // An uninitialized MemoryManager cannot open a store, so get_memory_stats
        // surfaces an error object - but the branch still returns Some.
        let mm = crate::memory::MemoryManager::new(None);
        let graph = Arc::new(RwLock::new(CodeGraph::in_memory().unwrap()));
        let result = read_resource("codegraph://memory/stats", graph, &mm, &[]).await;
        assert!(result.is_some());
        let content = result.unwrap().contents.into_iter().next().unwrap();
        assert_eq!(content.uri, "codegraph://memory/stats");
    }

    #[tokio::test]
    async fn test_read_resource_unknown_uri_returns_none() {
        let mm = crate::memory::MemoryManager::new(None);
        let graph = Arc::new(RwLock::new(CodeGraph::in_memory().unwrap()));
        let result = read_resource("codegraph://does/not/exist", graph, &mm, &[]).await;
        assert!(result.is_none());
    }
}
