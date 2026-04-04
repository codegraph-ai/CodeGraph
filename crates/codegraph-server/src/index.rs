// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Symbol indexing for fast lookups.

use crate::domain::node_props;
use codegraph::{CodeGraph, NodeId, PropertyMap};
use codegraph_parser_api::FileInfo;
use dashmap::DashMap;
use std::path::{Path, PathBuf};
use tower_lsp::lsp_types::{Position, Range};

/// Secondary indexes for fast symbol lookups.
pub struct SymbolIndex {
    /// Name -> NodeIds (for workspace symbol search).
    by_name: DashMap<String, Vec<NodeId>>,

    /// File path -> NodeIds (for file-scoped queries).
    by_file: DashMap<PathBuf, Vec<NodeId>>,

    /// Node type -> NodeIds (for type-filtered queries).
    by_type: DashMap<String, Vec<NodeId>>,

    /// Position index for fast position lookups.
    /// Maps file path to sorted list of (range, node_id).
    by_position: DashMap<PathBuf, Vec<(IndexRange, NodeId)>>,

    /// Reverse index: NodeId -> File path (for getting the file path of a node).
    node_to_file: DashMap<NodeId, PathBuf>,
}

/// Internal range representation for indexing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexRange {
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

impl IndexRange {
    /// Check if this range contains the given position.
    pub fn contains(&self, line: u32, col: u32) -> bool {
        if line < self.start_line || line > self.end_line {
            return false;
        }
        if line == self.start_line && col < self.start_col {
            return false;
        }
        if line == self.end_line && col > self.end_col {
            return false;
        }
        true
    }

    /// Convert to LSP Range (0-indexed).
    pub fn to_lsp_range(&self) -> Range {
        Range {
            start: Position {
                line: self.start_line.saturating_sub(1),
                character: self.start_col,
            },
            end: Position {
                line: self.end_line.saturating_sub(1),
                character: self.end_col,
            },
        }
    }
}

impl SymbolIndex {
    /// Create a new empty index.
    pub fn new() -> Self {
        Self {
            by_name: DashMap::new(),
            by_file: DashMap::new(),
            by_type: DashMap::new(),
            by_position: DashMap::new(),
            node_to_file: DashMap::new(),
        }
    }

    /// Add a file's symbols to the index.
    pub fn add_file(&self, path: PathBuf, file_info: &FileInfo, graph: &CodeGraph) {
        let mut file_nodes = Vec::new();
        let mut positions = Vec::new();

        // Index all symbols from the file
        let all_node_ids: Vec<NodeId> = file_info
            .functions
            .iter()
            .chain(file_info.classes.iter())
            .chain(file_info.traits.iter())
            .copied()
            .collect();

        for node_id in all_node_ids {
            if let Ok(node) = graph.get_node(node_id) {
                // Index by name
                if let Some(name) = node.properties.get_string("name") {
                    self.by_name
                        .entry(name.to_string())
                        .or_default()
                        .push(node_id);
                }

                // Index by type
                let type_str = format!("{}", node.node_type);
                self.by_type.entry(type_str).or_default().push(node_id);

                file_nodes.push(node_id);

                // Index by position
                if let Some(range) = extract_range(&node.properties) {
                    positions.push((range, node_id));
                }

                // Add to reverse index (NodeId -> PathBuf)
                self.node_to_file.insert(node_id, path.clone());
            }
        }

        // Store file index
        self.by_file.insert(path.clone(), file_nodes);

        // Sort positions for binary search (by start line, then start col)
        positions.sort_by(|a, b| {
            a.0.start_line
                .cmp(&b.0.start_line)
                .then(a.0.start_col.cmp(&b.0.start_col))
        });
        self.by_position.insert(path, positions);
    }

    /// Remove a file's symbols from the index.
    pub fn remove_file(&self, path: &Path) {
        let path_buf = path.to_path_buf();

        // Get nodes to remove
        if let Some((_, nodes)) = self.by_file.remove(&path_buf) {
            // Remove from name index
            for &node_id in &nodes {
                self.by_name.retain(|_, v| {
                    v.retain(|&id| id != node_id);
                    !v.is_empty()
                });

                // Remove from type index
                self.by_type.retain(|_, v| {
                    v.retain(|&id| id != node_id);
                    !v.is_empty()
                });

                // Remove from reverse index
                self.node_to_file.remove(&node_id);
            }
        }

        // Remove from position index
        self.by_position.remove(&path_buf);
    }

    /// Find node at the given position in a file.
    /// Position is 1-indexed (as stored in graph properties).
    pub fn find_at_position(&self, path: &Path, line: u32, col: u32) -> Option<NodeId> {
        let positions = self.by_position.get(&path.to_path_buf())?;

        // Find the smallest range containing the position
        // (innermost symbol at that position)
        let mut best_match: Option<(usize, NodeId)> = None;

        for (range, node_id) in positions.iter() {
            if range.contains(line, col) {
                let size = ((range.end_line - range.start_line) as usize) * 10000
                    + (range.end_col - range.start_col) as usize;

                match &best_match {
                    Some((best_size, _)) if size < *best_size => {
                        best_match = Some((size, *node_id));
                    }
                    None => {
                        best_match = Some((size, *node_id));
                    }
                    _ => {}
                }
            }
        }

        best_match.map(|(_, id)| id)
    }

    /// Search symbols by name pattern.
    pub fn search_by_name(&self, pattern: &str) -> Vec<NodeId> {
        let pattern_lower = pattern.to_lowercase();
        let mut results = Vec::new();

        for entry in self.by_name.iter() {
            if entry.key().to_lowercase().contains(&pattern_lower) {
                results.extend(entry.value().iter().copied());
            }
        }

        results
    }

    /// Get all symbols in a file.
    pub fn get_file_symbols(&self, path: &Path) -> Vec<NodeId> {
        self.by_file
            .get(&path.to_path_buf())
            .map(|v| v.clone())
            .unwrap_or_default()
    }

    /// Get all symbols of a specific type.
    pub fn get_by_type(&self, node_type: &str) -> Vec<NodeId> {
        self.by_type
            .get(node_type)
            .map(|v| v.clone())
            .unwrap_or_default()
    }

    /// Get the range for a node if it was indexed.
    pub fn get_node_range(&self, path: &Path, node_id: NodeId) -> Option<IndexRange> {
        let positions = self.by_position.get(&path.to_path_buf())?;

        for (range, id) in positions.iter() {
            if *id == node_id {
                return Some(range.clone());
            }
        }

        None
    }

    /// Find the file path for a given node ID by reverse lookup.
    /// This is useful when nodes don't have a `path` property set directly.
    pub fn find_file_for_node(&self, node_id: NodeId) -> Option<PathBuf> {
        self.node_to_file.get(&node_id).map(|entry| entry.clone())
    }

    /// Clear all indexes.
    /// Add a single node to the index (for testing purposes).
    #[cfg(test)]
    pub fn add_node_for_test(
        &self,
        path: PathBuf,
        node_id: NodeId,
        name: &str,
        node_type: &str,
        start_line: u32,
        end_line: u32,
    ) {
        // Index by name
        self.by_name
            .entry(name.to_string())
            .or_default()
            .push(node_id);

        // Index by type
        self.by_type
            .entry(node_type.to_string())
            .or_default()
            .push(node_id);

        // Add to file index
        self.by_file.entry(path.clone()).or_default().push(node_id);

        // Add to reverse index
        self.node_to_file.insert(node_id, path.clone());

        // Add to position index
        let range = IndexRange {
            start_line,
            start_col: 0,
            end_line,
            end_col: 100,
        };
        self.by_position
            .entry(path)
            .or_default()
            .push((range, node_id));
    }

    pub fn clear(&self) {
        self.by_name.clear();
        self.by_file.clear();
        self.by_type.clear();
        self.by_position.clear();
        self.node_to_file.clear();
    }

    /// Get index statistics.
    pub fn stats(&self) -> IndexStats {
        IndexStats {
            total_symbols: self.by_name.iter().map(|e| e.value().len()).sum(),
            total_files: self.by_file.len(),
            unique_names: self.by_name.len(),
        }
    }
}

impl Default for SymbolIndex {
    fn default() -> Self {
        Self::new()
    }
}

/// Index statistics.
pub struct IndexStats {
    pub total_symbols: usize,
    pub total_files: usize,
    pub unique_names: usize,
}

/// Extract range from node properties.
/// Note: codegraph parsers use line_start/line_end, not start_line/end_line
fn extract_range(properties: &PropertyMap) -> Option<IndexRange> {
    let start_line = node_props::line_start_opt_from_props(properties)?;
    let end_line = node_props::line_end_opt_from_props(properties)?;
    let start_col = node_props::col_start_from_props(properties);
    let end_col = node_props::col_end_from_props(properties);

    Some(IndexRange {
        start_line,
        start_col,
        end_line,
        end_col,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // IndexRange tests
    #[test]
    fn test_range_contains_inside() {
        let range = IndexRange {
            start_line: 10,
            start_col: 5,
            end_line: 15,
            end_col: 10,
        };

        // Inside range
        assert!(range.contains(12, 0));
        assert!(range.contains(12, 100));
        assert!(range.contains(10, 5)); // Exact start
        assert!(range.contains(15, 10)); // Exact end
        assert!(range.contains(10, 100)); // Start line, after start col
        assert!(range.contains(15, 5)); // End line, before end col
    }

    #[test]
    fn test_range_contains_outside() {
        let range = IndexRange {
            start_line: 10,
            start_col: 5,
            end_line: 15,
            end_col: 10,
        };

        // Outside range
        assert!(!range.contains(9, 0)); // Before start line
        assert!(!range.contains(16, 0)); // After end line
        assert!(!range.contains(10, 4)); // Before start col on start line
        assert!(!range.contains(15, 11)); // After end col on end line
    }

    #[test]
    fn test_range_contains_single_line() {
        let range = IndexRange {
            start_line: 5,
            start_col: 10,
            end_line: 5,
            end_col: 20,
        };

        assert!(range.contains(5, 10));
        assert!(range.contains(5, 15));
        assert!(range.contains(5, 20));
        assert!(!range.contains(5, 9));
        assert!(!range.contains(5, 21));
        assert!(!range.contains(4, 15));
        assert!(!range.contains(6, 15));
    }

    #[test]
    fn test_range_to_lsp_range() {
        let range = IndexRange {
            start_line: 10,
            start_col: 5,
            end_line: 15,
            end_col: 20,
        };

        let lsp_range = range.to_lsp_range();
        assert_eq!(lsp_range.start.line, 9); // 1-indexed to 0-indexed
        assert_eq!(lsp_range.start.character, 5);
        assert_eq!(lsp_range.end.line, 14);
        assert_eq!(lsp_range.end.character, 20);
    }

    #[test]
    fn test_range_to_lsp_range_saturating_sub() {
        // Test that line 0 doesn't underflow
        let range = IndexRange {
            start_line: 0,
            start_col: 0,
            end_line: 1,
            end_col: 10,
        };

        let lsp_range = range.to_lsp_range();
        assert_eq!(lsp_range.start.line, 0); // saturating_sub prevents underflow
        assert_eq!(lsp_range.end.line, 0);
    }

    // SymbolIndex tests
    #[test]
    fn test_symbol_index_new() {
        let index = SymbolIndex::new();
        let stats = index.stats();
        assert_eq!(stats.total_symbols, 0);
        assert_eq!(stats.total_files, 0);
        assert_eq!(stats.unique_names, 0);
    }

    #[test]
    fn test_symbol_index_default() {
        let index = SymbolIndex::default();
        let stats = index.stats();
        assert_eq!(stats.total_symbols, 0);
    }

    #[test]
    fn test_symbol_index_clear() {
        let index = SymbolIndex::new();

        // Add some data manually
        index.by_name.insert("test".to_string(), vec![1, 2]);
        index.by_file.insert(PathBuf::from("/test.rs"), vec![1, 2]);
        index.by_type.insert("Function".to_string(), vec![1, 2]);

        assert!(!index.by_name.is_empty());
        assert!(!index.by_file.is_empty());
        assert!(!index.by_type.is_empty());

        index.clear();

        assert!(index.by_name.is_empty());
        assert!(index.by_file.is_empty());
        assert!(index.by_type.is_empty());
        assert!(index.by_position.is_empty());
    }

    #[test]
    fn test_symbol_index_search_by_name() {
        let index = SymbolIndex::new();

        // Manually add entries
        index.by_name.insert("process_data".to_string(), vec![1]);
        index.by_name.insert("ProcessHandler".to_string(), vec![2]);
        index.by_name.insert("validate".to_string(), vec![3]);
        index
            .by_name
            .insert("process_request".to_string(), vec![4, 5]);

        // Case-insensitive search
        let results = index.search_by_name("process");
        assert_eq!(results.len(), 4); // process_data, ProcessHandler, process_request (2)

        let results = index.search_by_name("PROCESS");
        assert_eq!(results.len(), 4);

        let results = index.search_by_name("validate");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], 3);

        let results = index.search_by_name("nonexistent");
        assert!(results.is_empty());
    }

    #[test]
    fn test_symbol_index_get_file_symbols() {
        let index = SymbolIndex::new();

        let path1 = PathBuf::from("/src/main.rs");
        let path2 = PathBuf::from("/src/lib.rs");

        index.by_file.insert(path1.clone(), vec![1, 2, 3]);
        index.by_file.insert(path2.clone(), vec![4, 5]);

        let symbols = index.get_file_symbols(&path1);
        assert_eq!(symbols, vec![1, 2, 3]);

        let symbols = index.get_file_symbols(&path2);
        assert_eq!(symbols, vec![4, 5]);

        let symbols = index.get_file_symbols(Path::new("/nonexistent"));
        assert!(symbols.is_empty());
    }

    #[test]
    fn test_symbol_index_get_by_type() {
        let index = SymbolIndex::new();

        index.by_type.insert("Function".to_string(), vec![1, 2, 3]);
        index.by_type.insert("Class".to_string(), vec![4, 5]);
        index.by_type.insert("Trait".to_string(), vec![6]);

        let functions = index.get_by_type("Function");
        assert_eq!(functions, vec![1, 2, 3]);

        let classes = index.get_by_type("Class");
        assert_eq!(classes, vec![4, 5]);

        let traits = index.get_by_type("Trait");
        assert_eq!(traits, vec![6]);

        let modules = index.get_by_type("Module");
        assert!(modules.is_empty());
    }

    #[test]
    fn test_symbol_index_find_at_position_none() {
        let index = SymbolIndex::new();

        // No data
        assert!(index
            .find_at_position(Path::new("/test.rs"), 10, 5)
            .is_none());
    }

    #[test]
    fn test_symbol_index_find_at_position_single() {
        let index = SymbolIndex::new();

        let path = PathBuf::from("/test.rs");
        let range = IndexRange {
            start_line: 5,
            start_col: 0,
            end_line: 10,
            end_col: 1,
        };

        index.by_position.insert(path.clone(), vec![(range, 42)]);

        // Find within range
        assert_eq!(index.find_at_position(&path, 7, 5), Some(42));

        // Outside range
        assert!(index.find_at_position(&path, 3, 0).is_none());
        assert!(index.find_at_position(&path, 15, 0).is_none());
    }

    #[test]
    fn test_symbol_index_find_at_position_nested() {
        let index = SymbolIndex::new();

        let path = PathBuf::from("/test.rs");

        // Outer range (larger)
        let outer = IndexRange {
            start_line: 1,
            start_col: 0,
            end_line: 100,
            end_col: 0,
        };

        // Inner range (smaller - should be preferred)
        let inner = IndexRange {
            start_line: 10,
            start_col: 4,
            end_line: 20,
            end_col: 5,
        };

        index
            .by_position
            .insert(path.clone(), vec![(outer, 1), (inner, 2)]);

        // Position inside inner should return inner (smallest containing range)
        assert_eq!(index.find_at_position(&path, 15, 10), Some(2));

        // Position outside inner but inside outer should return outer
        assert_eq!(index.find_at_position(&path, 50, 0), Some(1));
    }

    #[test]
    fn test_symbol_index_get_node_range() {
        let index = SymbolIndex::new();

        let path = PathBuf::from("/test.rs");
        let range1 = IndexRange {
            start_line: 5,
            start_col: 0,
            end_line: 10,
            end_col: 1,
        };
        let range2 = IndexRange {
            start_line: 15,
            start_col: 0,
            end_line: 20,
            end_col: 1,
        };

        index
            .by_position
            .insert(path.clone(), vec![(range1.clone(), 1), (range2.clone(), 2)]);

        // Find existing node range
        let found = index.get_node_range(&path, 1);
        assert!(found.is_some());
        assert_eq!(found.unwrap().start_line, 5);

        let found = index.get_node_range(&path, 2);
        assert!(found.is_some());
        assert_eq!(found.unwrap().start_line, 15);

        // Non-existent node
        assert!(index.get_node_range(&path, 999).is_none());

        // Non-existent file
        assert!(index.get_node_range(Path::new("/other.rs"), 1).is_none());
    }

    #[test]
    fn test_symbol_index_stats() {
        let index = SymbolIndex::new();

        // Empty stats
        let stats = index.stats();
        assert_eq!(stats.total_symbols, 0);
        assert_eq!(stats.total_files, 0);
        assert_eq!(stats.unique_names, 0);

        // Add some data
        index.by_name.insert("foo".to_string(), vec![1, 2, 3]);
        index.by_name.insert("bar".to_string(), vec![4]);
        index.by_file.insert(PathBuf::from("/a.rs"), vec![1, 2]);
        index.by_file.insert(PathBuf::from("/b.rs"), vec![3, 4]);

        let stats = index.stats();
        assert_eq!(stats.total_symbols, 4); // 3 + 1 symbols
        assert_eq!(stats.total_files, 2);
        assert_eq!(stats.unique_names, 2);
    }

    #[test]
    fn test_symbol_index_remove_file() {
        let index = SymbolIndex::new();

        let path = PathBuf::from("/test.rs");

        // Setup: add file with nodes
        index.by_file.insert(path.clone(), vec![1, 2]);
        index.by_name.insert("func1".to_string(), vec![1, 10]); // node 1 is from our file, 10 from another
        index.by_name.insert("func2".to_string(), vec![2]);
        index.by_type.insert("Function".to_string(), vec![1, 2, 10]);
        index.by_position.insert(
            path.clone(),
            vec![(
                IndexRange {
                    start_line: 1,
                    start_col: 0,
                    end_line: 10,
                    end_col: 0,
                },
                1,
            )],
        );

        // Remove file
        index.remove_file(&path);

        // Verify file removed
        assert!(index.by_file.get(&path).is_none());
        assert!(index.by_position.get(&path).is_none());

        // Verify node 1 and 2 removed from name index
        let func1_nodes = index.by_name.get("func1");
        assert!(func1_nodes.is_some());
        assert_eq!(func1_nodes.unwrap().len(), 1); // Only node 10 remains

        // func2 entry should be completely removed (was only node 2)
        assert!(index.by_name.get("func2").is_none());

        // Type index should only have node 10
        let type_nodes = index.by_type.get("Function");
        assert!(type_nodes.is_some());
        assert_eq!(type_nodes.unwrap().len(), 1);
    }

    #[test]
    fn test_symbol_index_remove_nonexistent_file() {
        let index = SymbolIndex::new();

        // Should not panic when removing non-existent file
        index.remove_file(Path::new("/nonexistent.rs"));

        // Index should still be empty
        assert!(index.by_file.is_empty());
    }

    // IndexRange equality tests
    #[test]
    fn test_index_range_equality() {
        let range1 = IndexRange {
            start_line: 10,
            start_col: 5,
            end_line: 20,
            end_col: 15,
        };

        let range2 = IndexRange {
            start_line: 10,
            start_col: 5,
            end_line: 20,
            end_col: 15,
        };

        let range3 = IndexRange {
            start_line: 10,
            start_col: 5,
            end_line: 20,
            end_col: 16, // Different
        };

        assert_eq!(range1, range2);
        assert_ne!(range1, range3);
    }

    #[test]
    fn test_index_range_clone() {
        let range = IndexRange {
            start_line: 1,
            start_col: 2,
            end_line: 3,
            end_col: 4,
        };

        let cloned = range.clone();
        assert_eq!(range, cloned);
    }

    #[test]
    fn test_index_range_debug() {
        let range = IndexRange {
            start_line: 1,
            start_col: 2,
            end_line: 3,
            end_col: 4,
        };

        let debug_str = format!("{range:?}");
        assert!(debug_str.contains("IndexRange"));
        assert!(debug_str.contains("start_line: 1"));
    }

    #[test]
    fn test_find_file_for_node() {
        let index = SymbolIndex::new();

        let path1 = PathBuf::from("/src/main.rs");
        let path2 = PathBuf::from("/src/lib.rs");

        // Insert into both by_file and node_to_file maps
        index.by_file.insert(path1.clone(), vec![1, 2, 3]);
        index.by_file.insert(path2.clone(), vec![4, 5]);

        // Also populate the reverse index (node_to_file)
        index.node_to_file.insert(1, path1.clone());
        index.node_to_file.insert(2, path1.clone());
        index.node_to_file.insert(3, path1.clone());
        index.node_to_file.insert(4, path2.clone());
        index.node_to_file.insert(5, path2.clone());

        // Find node in first file
        assert_eq!(index.find_file_for_node(1), Some(path1.clone()));
        assert_eq!(index.find_file_for_node(2), Some(path1.clone()));
        assert_eq!(index.find_file_for_node(3), Some(path1.clone()));

        // Find node in second file
        assert_eq!(index.find_file_for_node(4), Some(path2.clone()));
        assert_eq!(index.find_file_for_node(5), Some(path2.clone()));

        // Non-existent node
        assert!(index.find_file_for_node(99).is_none());
    }
}
