// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Query caching for performance optimization.

use codegraph::NodeId;
use dashmap::DashMap;
use lru::LruCache;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tower_lsp::lsp_types::{Location, Range};

/// Cache for definition lookups.
type DefinitionCache = DashMap<(PathBuf, u32, u32), NodeId>;

/// Cache for references lookups.
type ReferencesCache = DashMap<NodeId, Vec<Location>>;

/// Caches for expensive queries.
pub struct QueryCache {
    /// Fast lookup cache for definitions.
    definitions: DefinitionCache,

    /// Fast lookup cache for references.
    references: ReferencesCache,

    /// LRU cache for call hierarchy results.
    call_hierarchies: Mutex<LruCache<NodeId, CallHierarchyCache>>,

    /// LRU cache for dependency graph results.
    dependency_graphs: Mutex<LruCache<(PathBuf, usize), DependencyGraphCache>>,

    /// LRU cache for AI context results.
    ai_contexts: Mutex<LruCache<(NodeId, String), AIContextCache>>,

    /// LRU cache for AI agent symbol search results.
    symbol_searches: Mutex<LruCache<String, SymbolSearchCache>>,

    /// LRU cache for AI agent graph traversal results.
    traversals: Mutex<LruCache<(NodeId, String, u32), TraversalCache>>,
}

/// Cached call hierarchy data.
#[derive(Clone)]
pub struct CallHierarchyCache {
    pub incoming: Vec<(NodeId, Vec<Range>)>,
    pub outgoing: Vec<(NodeId, Vec<Range>)>,
}

/// Cached dependency graph data.
#[derive(Clone)]
pub struct DependencyGraphCache {
    pub nodes: Vec<NodeId>,
    pub edges: Vec<(NodeId, NodeId, String)>,
}

/// Cached AI context data.
#[derive(Clone)]
pub struct AIContextCache {
    pub primary_code: String,
    pub related_symbols: Vec<(NodeId, String, f64)>,
}

/// Cached AI query symbol search results.
#[derive(Clone)]
pub struct SymbolSearchCache {
    pub results: Vec<(NodeId, f32, String)>, // (node_id, score, match_reason)
    pub total_matches: usize,
    pub query_time_ms: u64,
}

/// Cached AI query traversal results.
#[derive(Clone)]
pub struct TraversalCache {
    pub nodes: Vec<(NodeId, u32, String)>, // (node_id, depth, edge_type)
    pub query_time_ms: u64,
}

impl QueryCache {
    /// Create a new cache with the specified capacity.
    pub fn new(capacity: usize) -> Self {
        let capacity = NonZeroUsize::new(capacity).unwrap_or(NonZeroUsize::new(100).unwrap());

        Self {
            definitions: DashMap::new(),
            references: DashMap::new(),
            call_hierarchies: Mutex::new(LruCache::new(capacity)),
            dependency_graphs: Mutex::new(LruCache::new(
                NonZeroUsize::new(capacity.get() / 2).unwrap_or(NonZeroUsize::new(50).unwrap()),
            )),
            ai_contexts: Mutex::new(LruCache::new(capacity)),
            symbol_searches: Mutex::new(LruCache::new(capacity)),
            traversals: Mutex::new(LruCache::new(capacity)),
        }
    }

    // ==========================================
    // Definition Cache
    // ==========================================

    /// Get cached definition.
    pub fn get_definition(&self, path: &Path, line: u32, character: u32) -> Option<NodeId> {
        self.definitions
            .get(&(path.to_path_buf(), line, character))
            .map(|v| *v)
    }

    /// Store definition in cache.
    pub fn set_definition(&self, path: PathBuf, line: u32, character: u32, node_id: NodeId) {
        self.definitions.insert((path, line, character), node_id);
    }

    // ==========================================
    // References Cache
    // ==========================================

    /// Get cached references.
    pub fn get_references(&self, node_id: NodeId) -> Option<Vec<Location>> {
        self.references.get(&node_id).map(|v| v.clone())
    }

    /// Store references in cache.
    pub fn set_references(&self, node_id: NodeId, locations: Vec<Location>) {
        self.references.insert(node_id, locations);
    }

    // ==========================================
    // Call Hierarchy Cache
    // ==========================================

    /// Get cached call hierarchy.
    pub fn get_call_hierarchy(&self, node_id: NodeId) -> Option<CallHierarchyCache> {
        self.call_hierarchies.lock().ok()?.get(&node_id).cloned()
    }

    /// Store call hierarchy in cache.
    pub fn set_call_hierarchy(&self, node_id: NodeId, cache: CallHierarchyCache) {
        if let Ok(mut guard) = self.call_hierarchies.lock() {
            guard.put(node_id, cache);
        }
    }

    // ==========================================
    // Dependency Graph Cache
    // ==========================================

    /// Get cached dependency graph.
    pub fn get_dependency_graph(&self, path: &Path, depth: usize) -> Option<DependencyGraphCache> {
        self.dependency_graphs
            .lock()
            .ok()?
            .get(&(path.to_path_buf(), depth))
            .cloned()
    }

    /// Store dependency graph in cache.
    pub fn set_dependency_graph(&self, path: PathBuf, depth: usize, cache: DependencyGraphCache) {
        if let Ok(mut guard) = self.dependency_graphs.lock() {
            guard.put((path, depth), cache);
        }
    }

    // ==========================================
    // AI Context Cache
    // ==========================================

    /// Get cached AI context.
    pub fn get_ai_context(&self, node_id: NodeId, context_type: &str) -> Option<AIContextCache> {
        self.ai_contexts
            .lock()
            .ok()?
            .get(&(node_id, context_type.to_string()))
            .cloned()
    }

    /// Store AI context in cache.
    pub fn set_ai_context(&self, node_id: NodeId, context_type: String, cache: AIContextCache) {
        if let Ok(mut guard) = self.ai_contexts.lock() {
            guard.put((node_id, context_type), cache);
        }
    }

    // ==========================================
    // AI Agent Symbol Search Cache
    // ==========================================

    /// Get cached symbol search results.
    /// The key is a normalized query string (e.g., "query:user scope:workspace limit:10").
    pub fn get_symbol_search(&self, query_key: &str) -> Option<SymbolSearchCache> {
        self.symbol_searches.lock().ok()?.get(query_key).cloned()
    }

    /// Store symbol search results in cache.
    pub fn set_symbol_search(&self, query_key: String, cache: SymbolSearchCache) {
        if let Ok(mut guard) = self.symbol_searches.lock() {
            guard.put(query_key, cache);
        }
    }

    // ==========================================
    // AI Agent Traversal Cache
    // ==========================================

    /// Get cached traversal results.
    /// Key is (start_node_id, direction, depth).
    pub fn get_traversal(
        &self,
        node_id: NodeId,
        direction: &str,
        depth: u32,
    ) -> Option<TraversalCache> {
        self.traversals
            .lock()
            .ok()?
            .get(&(node_id, direction.to_string(), depth))
            .cloned()
    }

    /// Store traversal results in cache.
    pub fn set_traversal(
        &self,
        node_id: NodeId,
        direction: String,
        depth: u32,
        cache: TraversalCache,
    ) {
        if let Ok(mut guard) = self.traversals.lock() {
            guard.put((node_id, direction, depth), cache);
        }
    }

    // ==========================================
    // Invalidation
    // ==========================================

    /// Invalidate all cache entries for a file.
    pub fn invalidate_file(&self, path: &PathBuf) {
        // Remove definition entries for this file
        self.definitions.retain(|(p, _, _), _| p != path);

        // Clear references cache (could be more selective)
        self.references.clear();

        // Clear call hierarchies
        if let Ok(mut guard) = self.call_hierarchies.lock() {
            guard.clear();
        }

        // Remove dependency graphs for this file
        if let Ok(mut guard) = self.dependency_graphs.lock() {
            // Note: LRU doesn't have retain, so we need to work around this
            // For now, just clear if the path matches the first in key
            guard.clear();
        }
    }

    /// Invalidate entire cache.
    pub fn invalidate_all(&self) {
        self.definitions.clear();
        self.references.clear();

        if let Ok(mut guard) = self.call_hierarchies.lock() {
            guard.clear();
        }

        if let Ok(mut guard) = self.dependency_graphs.lock() {
            guard.clear();
        }

        if let Ok(mut guard) = self.ai_contexts.lock() {
            guard.clear();
        }

        if let Ok(mut guard) = self.symbol_searches.lock() {
            guard.clear();
        }

        if let Ok(mut guard) = self.traversals.lock() {
            guard.clear();
        }
    }

    /// Get cache statistics.
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            definitions_count: self.definitions.len(),
            references_count: self.references.len(),
            call_hierarchies_count: self.call_hierarchies.lock().map(|g| g.len()).unwrap_or(0),
            dependency_graphs_count: self.dependency_graphs.lock().map(|g| g.len()).unwrap_or(0),
            ai_contexts_count: self.ai_contexts.lock().map(|g| g.len()).unwrap_or(0),
            symbol_searches_count: self.symbol_searches.lock().map(|g| g.len()).unwrap_or(0),
            traversals_count: self.traversals.lock().map(|g| g.len()).unwrap_or(0),
        }
    }
}

/// Cache statistics.
pub struct CacheStats {
    pub definitions_count: usize,
    pub references_count: usize,
    pub call_hierarchies_count: usize,
    pub dependency_graphs_count: usize,
    pub ai_contexts_count: usize,
    pub symbol_searches_count: usize,
    pub traversals_count: usize,
}

impl Default for QueryCache {
    fn default() -> Self {
        Self::new(1000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::Url;

    fn make_location(line: u32) -> Location {
        Location {
            uri: Url::parse("file:///test.rs").unwrap(),
            range: Range {
                start: tower_lsp::lsp_types::Position { line, character: 0 },
                end: tower_lsp::lsp_types::Position {
                    line,
                    character: 10,
                },
            },
        }
    }

    #[test]
    fn test_cache_new_with_valid_capacity() {
        let cache = QueryCache::new(50);
        let stats = cache.stats();
        assert_eq!(stats.definitions_count, 0);
        assert_eq!(stats.references_count, 0);
        assert_eq!(stats.call_hierarchies_count, 0);
    }

    #[test]
    fn test_cache_new_with_zero_capacity_uses_default() {
        let cache = QueryCache::new(0);
        // Should use default capacity of 100
        let stats = cache.stats();
        assert_eq!(stats.definitions_count, 0);
    }

    #[test]
    fn test_cache_default() {
        let cache = QueryCache::default();
        let stats = cache.stats();
        assert_eq!(stats.definitions_count, 0);
    }

    #[test]
    fn test_definition_cache_set_and_get() {
        let cache = QueryCache::new(100);
        let path = PathBuf::from("/test/file.rs");
        let node_id: NodeId = 42;

        cache.set_definition(path.clone(), 10, 5, node_id);
        let result = cache.get_definition(&path, 10, 5);

        assert_eq!(result, Some(node_id));
    }

    #[test]
    fn test_definition_cache_miss() {
        let cache = QueryCache::new(100);
        let path = PathBuf::from("/test/file.rs");

        let result = cache.get_definition(&path, 10, 5);
        assert_eq!(result, None);
    }

    #[test]
    fn test_definition_cache_different_positions() {
        let cache = QueryCache::new(100);
        let path = PathBuf::from("/test/file.rs");
        let node_id1: NodeId = 1;
        let node_id2: NodeId = 2;

        cache.set_definition(path.clone(), 10, 5, node_id1);
        cache.set_definition(path.clone(), 20, 10, node_id2);

        assert_eq!(cache.get_definition(&path, 10, 5), Some(node_id1));
        assert_eq!(cache.get_definition(&path, 20, 10), Some(node_id2));
        assert_eq!(cache.get_definition(&path, 10, 6), None);
    }

    #[test]
    fn test_references_cache_set_and_get() {
        let cache = QueryCache::new(100);
        let node_id: NodeId = 42;
        let locations = vec![make_location(1), make_location(5), make_location(10)];

        cache.set_references(node_id, locations.clone());
        let result = cache.get_references(node_id);

        assert!(result.is_some());
        let result = result.unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].range.start.line, 1);
        assert_eq!(result[1].range.start.line, 5);
    }

    #[test]
    fn test_references_cache_miss() {
        let cache = QueryCache::new(100);
        let node_id: NodeId = 42;

        let result = cache.get_references(node_id);
        assert!(result.is_none());
    }

    #[test]
    fn test_call_hierarchy_cache_set_and_get() {
        let cache = QueryCache::new(100);
        let node_id: NodeId = 42;

        let incoming: Vec<(NodeId, Vec<Range>)> = vec![(1, vec![])];
        let outgoing: Vec<(NodeId, Vec<Range>)> = vec![(2, vec![]), (3, vec![])];

        cache.set_call_hierarchy(
            node_id,
            CallHierarchyCache {
                incoming: incoming.clone(),
                outgoing: outgoing.clone(),
            },
        );

        let result = cache.get_call_hierarchy(node_id);
        assert!(result.is_some());
        let result = result.unwrap();
        assert_eq!(result.incoming.len(), 1);
        assert_eq!(result.outgoing.len(), 2);
    }

    #[test]
    fn test_call_hierarchy_cache_miss() {
        let cache = QueryCache::new(100);
        let node_id: NodeId = 42;

        let result = cache.get_call_hierarchy(node_id);
        assert!(result.is_none());
    }

    #[test]
    fn test_dependency_graph_cache_set_and_get() {
        let cache = QueryCache::new(100);
        let path = PathBuf::from("/test/file.rs");
        let depth = 3;

        let graph_cache = DependencyGraphCache {
            nodes: vec![1, 2],
            edges: vec![(1, 2, "imports".to_string())],
        };

        cache.set_dependency_graph(path.clone(), depth, graph_cache);

        let result = cache.get_dependency_graph(&path, depth);
        assert!(result.is_some());
        let result = result.unwrap();
        assert_eq!(result.nodes.len(), 2);
        assert_eq!(result.edges.len(), 1);
    }

    #[test]
    fn test_dependency_graph_cache_different_depths() {
        let cache = QueryCache::new(100);
        let path = PathBuf::from("/test/file.rs");

        let graph1 = DependencyGraphCache {
            nodes: vec![1],
            edges: vec![],
        };
        let graph2 = DependencyGraphCache {
            nodes: vec![1, 2, 3],
            edges: vec![],
        };

        cache.set_dependency_graph(path.clone(), 1, graph1);
        cache.set_dependency_graph(path.clone(), 3, graph2);

        let result1 = cache.get_dependency_graph(&path, 1);
        let result3 = cache.get_dependency_graph(&path, 3);

        assert!(result1.is_some());
        assert!(result3.is_some());
        assert_eq!(result1.unwrap().nodes.len(), 1);
        assert_eq!(result3.unwrap().nodes.len(), 3);
    }

    #[test]
    fn test_ai_context_cache_set_and_get() {
        let cache = QueryCache::new(100);
        let node_id: NodeId = 42;
        let context_type = "function";

        let ai_cache = AIContextCache {
            primary_code: "fn test() {}".to_string(),
            related_symbols: vec![(1, "helper".to_string(), 0.9), (2, "util".to_string(), 0.7)],
        };

        cache.set_ai_context(node_id, context_type.to_string(), ai_cache);

        let result = cache.get_ai_context(node_id, context_type);
        assert!(result.is_some());
        let result = result.unwrap();
        assert_eq!(result.primary_code, "fn test() {}");
        assert_eq!(result.related_symbols.len(), 2);
    }

    #[test]
    fn test_ai_context_cache_different_types() {
        let cache = QueryCache::new(100);
        let node_id: NodeId = 42;

        let cache1 = AIContextCache {
            primary_code: "fn test() {}".to_string(),
            related_symbols: vec![],
        };
        let cache2 = AIContextCache {
            primary_code: "struct Test {}".to_string(),
            related_symbols: vec![],
        };

        cache.set_ai_context(node_id, "function".to_string(), cache1);
        cache.set_ai_context(node_id, "struct".to_string(), cache2);

        let result1 = cache.get_ai_context(node_id, "function");
        let result2 = cache.get_ai_context(node_id, "struct");

        assert_eq!(result1.unwrap().primary_code, "fn test() {}");
        assert_eq!(result2.unwrap().primary_code, "struct Test {}");
    }

    #[test]
    fn test_invalidate_file_removes_definitions() {
        let cache = QueryCache::new(100);
        let path1 = PathBuf::from("/test/file1.rs");
        let path2 = PathBuf::from("/test/file2.rs");

        cache.set_definition(path1.clone(), 10, 5, 1);
        cache.set_definition(path2.clone(), 20, 10, 2);

        cache.invalidate_file(&path1);

        assert!(cache.get_definition(&path1, 10, 5).is_none());
        assert!(cache.get_definition(&path2, 20, 10).is_some());
    }

    #[test]
    fn test_invalidate_file_clears_references() {
        let cache = QueryCache::new(100);
        let path = PathBuf::from("/test/file.rs");
        let node_id: NodeId = 42;

        cache.set_references(node_id, vec![make_location(1)]);
        cache.invalidate_file(&path);

        // References are cleared entirely when a file is invalidated
        assert!(cache.get_references(node_id).is_none());
    }

    #[test]
    fn test_invalidate_all() {
        let cache = QueryCache::new(100);
        let path = PathBuf::from("/test/file.rs");
        let node_id: NodeId = 42;

        cache.set_definition(path.clone(), 10, 5, node_id);
        cache.set_references(node_id, vec![make_location(1)]);
        cache.set_call_hierarchy(
            node_id,
            CallHierarchyCache {
                incoming: vec![],
                outgoing: vec![],
            },
        );
        cache.set_dependency_graph(
            path.clone(),
            3,
            DependencyGraphCache {
                nodes: vec![],
                edges: vec![],
            },
        );
        cache.set_ai_context(
            node_id,
            "test".to_string(),
            AIContextCache {
                primary_code: "".to_string(),
                related_symbols: vec![],
            },
        );

        let stats_before = cache.stats();
        assert!(stats_before.definitions_count > 0);

        cache.invalidate_all();

        let stats_after = cache.stats();
        assert_eq!(stats_after.definitions_count, 0);
        assert_eq!(stats_after.references_count, 0);
        assert_eq!(stats_after.call_hierarchies_count, 0);
        assert_eq!(stats_after.dependency_graphs_count, 0);
        assert_eq!(stats_after.ai_contexts_count, 0);
    }

    #[test]
    fn test_stats_reflects_cache_contents() {
        let cache = QueryCache::new(100);
        let path = PathBuf::from("/test/file.rs");
        let node_id1: NodeId = 1;
        let node_id2: NodeId = 2;

        cache.set_definition(path.clone(), 10, 5, node_id1);
        cache.set_definition(path.clone(), 20, 10, node_id2);
        cache.set_references(node_id1, vec![make_location(1)]);

        let stats = cache.stats();
        assert_eq!(stats.definitions_count, 2);
        assert_eq!(stats.references_count, 1);
    }

    #[test]
    fn test_lru_eviction_for_call_hierarchies() {
        // Create cache with small capacity
        let cache = QueryCache::new(2);

        // Add 3 items to trigger LRU eviction
        for i in 0..3u64 {
            cache.set_call_hierarchy(
                i,
                CallHierarchyCache {
                    incoming: vec![],
                    outgoing: vec![],
                },
            );
        }

        // The least recently used item (node 0) should be evicted
        assert!(cache.get_call_hierarchy(0).is_none());
        assert!(cache.get_call_hierarchy(1).is_some());
        assert!(cache.get_call_hierarchy(2).is_some());
    }

    #[test]
    fn test_concurrent_access_definitions() {
        use std::sync::Arc;
        use std::thread;

        let cache = Arc::new(QueryCache::new(1000));
        let mut handles = vec![];

        // Spawn multiple threads writing to the cache
        for i in 0..10u64 {
            let cache = Arc::clone(&cache);
            let handle = thread::spawn(move || {
                let path = PathBuf::from(format!("/test/file{i}.rs"));
                for j in 0..100u32 {
                    cache.set_definition(path.clone(), j, 0, (i * 100 + j as u64) as NodeId);
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        // Verify we can read all entries
        let stats = cache.stats();
        assert_eq!(stats.definitions_count, 1000);
    }
}
