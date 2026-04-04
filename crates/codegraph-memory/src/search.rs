// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Hybrid search engine
//!
//! Combines BM25 text search, semantic search, and graph proximity
//! for comprehensive memory retrieval.

use std::collections::HashMap;
use std::sync::Arc;

use crate::error::Result;
use crate::node::{MemoryKind, MemoryNode};
use crate::storage::MemoryStore;

/// Search configuration
#[derive(Debug, Clone)]
pub struct SearchConfig {
    /// Maximum results to return
    pub limit: usize,
    /// Weight for BM25 text search (default: 0.3)
    pub bm25_weight: f32,
    /// Weight for semantic search (default: 0.5)
    pub semantic_weight: f32,
    /// Weight for graph proximity (default: 0.2)
    pub graph_weight: f32,
    /// Only return current (non-invalidated) memories
    pub current_only: bool,
    /// Filter by tags
    pub tags: Vec<String>,
    /// Filter by memory kinds
    pub kinds: Vec<MemoryKindFilter>,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            limit: 10,
            bm25_weight: 0.3,
            semantic_weight: 0.5,
            graph_weight: 0.2,
            current_only: true,
            tags: vec![],
            kinds: vec![],
        }
    }
}

/// Filter for memory kinds
#[derive(Debug, Clone, PartialEq)]
pub enum MemoryKindFilter {
    ArchitecturalDecision,
    DebugContext,
    KnownIssue,
    Convention,
    ProjectContext,
}

impl MemoryKindFilter {
    fn matches(&self, kind: &MemoryKind) -> bool {
        matches!(
            (self, kind),
            (
                Self::ArchitecturalDecision,
                MemoryKind::ArchitecturalDecision { .. }
            ) | (Self::DebugContext, MemoryKind::DebugContext { .. })
                | (Self::KnownIssue, MemoryKind::KnownIssue { .. })
                | (Self::Convention, MemoryKind::Convention { .. })
                | (Self::ProjectContext, MemoryKind::ProjectContext { .. })
        )
    }
}

/// Why a memory matched the search
#[derive(Debug, Clone)]
pub enum MatchReason {
    TextMatch { score: f32 },
    SemanticSimilarity { score: f32 },
    CodeProximity { score: f32 },
}

/// Search result with scores
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// The matched memory
    pub memory: MemoryNode,
    /// Combined score
    pub score: f32,
    /// Individual match reasons
    pub match_reasons: Vec<MatchReason>,
}

/// BM25 index for text search
pub struct BM25Index {
    /// Inverted index: term -> [(memory_id, tf-idf score)]
    inverted: HashMap<String, Vec<(String, f32)>>,
    /// Document lengths
    doc_lengths: HashMap<String, f32>,
    /// Average document length
    avg_doc_length: f32,
    /// Number of documents
    num_docs: usize,
    /// BM25 k1 parameter
    k1: f32,
    /// BM25 b parameter
    b: f32,
}

impl BM25Index {
    /// Build BM25 index from memories
    pub fn build(memories: &[MemoryNode]) -> Self {
        let mut inverted: HashMap<String, Vec<(String, f32)>> = HashMap::new();
        let mut doc_lengths: HashMap<String, f32> = HashMap::new();
        let mut total_length = 0.0;

        for memory in memories {
            let id = memory.id.to_string();
            let text = memory.searchable_text();
            let tokens = Self::tokenize(&text);
            let doc_length = tokens.len() as f32;

            doc_lengths.insert(id.clone(), doc_length);
            total_length += doc_length;

            // Count term frequencies
            let mut term_freqs: HashMap<String, usize> = HashMap::new();
            for token in &tokens {
                *term_freqs.entry(token.clone()).or_insert(0) += 1;
            }

            // Add to inverted index
            for (term, freq) in term_freqs {
                let tf = freq as f32;
                inverted.entry(term).or_default().push((id.clone(), tf));
            }
        }

        let num_docs = memories.len();
        let avg_doc_length = if num_docs > 0 {
            total_length / num_docs as f32
        } else {
            0.0
        };

        Self {
            inverted,
            doc_lengths,
            avg_doc_length,
            num_docs,
            k1: 1.2,
            b: 0.75,
        }
    }

    /// Tokenize text into terms
    fn tokenize(text: &str) -> Vec<String> {
        text.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|s| s.len() > 2)
            .map(String::from)
            .collect()
    }

    /// Search with BM25 scoring
    pub fn search(&self, query: &str, limit: usize) -> Vec<(String, f32)> {
        let query_tokens = Self::tokenize(query);
        let mut scores: HashMap<String, f32> = HashMap::new();

        for token in &query_tokens {
            if let Some(postings) = self.inverted.get(token) {
                let idf = self.idf(postings.len());

                for (doc_id, tf) in postings {
                    let doc_length = self.doc_lengths.get(doc_id).copied().unwrap_or(1.0);
                    let score = self.bm25_score(*tf, doc_length, idf);
                    *scores.entry(doc_id.clone()).or_insert(0.0) += score;
                }
            }
        }

        let mut results: Vec<_> = scores.into_iter().collect();
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(limit);
        results
    }

    /// Calculate IDF
    fn idf(&self, doc_freq: usize) -> f32 {
        let n = self.num_docs as f32;
        let df = doc_freq as f32;
        ((n - df + 0.5) / (df + 0.5) + 1.0).ln()
    }

    /// Calculate BM25 score for a term
    fn bm25_score(&self, tf: f32, doc_length: f32, idf: f32) -> f32 {
        let numerator = tf * (self.k1 + 1.0);
        let denominator = tf + self.k1 * (1.0 - self.b + self.b * doc_length / self.avg_doc_length);
        idf * numerator / denominator
    }
}

/// Hybrid search engine
pub struct MemorySearch {
    store: Arc<MemoryStore>,
    bm25_index: BM25Index,
}

impl MemorySearch {
    /// Create new search engine
    pub fn new(store: Arc<MemoryStore>) -> Result<Self> {
        let memories = store.get_all_current();
        let bm25_index = BM25Index::build(&memories);

        Ok(Self { store, bm25_index })
    }

    /// Rebuild the search index
    pub fn rebuild_index(&mut self) -> Result<()> {
        let memories = self.store.get_all_current();
        self.bm25_index = BM25Index::build(&memories);
        Ok(())
    }

    /// Hybrid search combining BM25 + semantic + graph proximity
    pub fn search(
        &self,
        query: &str,
        code_context: &[String],
        config: &SearchConfig,
    ) -> Result<Vec<SearchResult>> {
        let candidate_limit = config.limit * 3;

        // 1. BM25 text search
        let bm25_results = self.bm25_index.search(query, candidate_limit);

        // 2. Semantic search
        let query_embedding = self.store.engine().embed(query)?;
        let semantic_results = self
            .store
            .semantic_search(&query_embedding, candidate_limit);

        // 3. Merge candidates
        let mut candidate_scores: HashMap<String, (f32, f32, f32)> = HashMap::new();

        for (id, score) in bm25_results {
            candidate_scores.entry(id).or_insert((0.0, 0.0, 0.0)).0 = score;
        }

        for (id, score) in semantic_results {
            candidate_scores.entry(id).or_insert((0.0, 0.0, 0.0)).1 = score;
        }

        // 4. Calculate graph proximity for candidates
        for id in candidate_scores.keys().cloned().collect::<Vec<_>>() {
            if let Some(memory) = self.store.get(&id) {
                let graph_score = self.calculate_graph_score(&memory, code_context);
                candidate_scores.get_mut(&id).unwrap().2 = graph_score;
            }
        }

        // 5. Calculate final scores and build results
        let mut results: Vec<SearchResult> = Vec::new();

        for (id, (bm25, semantic, graph)) in candidate_scores {
            if let Some(memory) = self.store.get(&id) {
                // Apply filters
                if config.current_only && !memory.is_current() {
                    continue;
                }

                if !config.tags.is_empty() && !config.tags.iter().any(|t| memory.tags.contains(t)) {
                    continue;
                }

                if !config.kinds.is_empty() && !config.kinds.iter().any(|k| k.matches(&memory.kind))
                {
                    continue;
                }

                // Calculate weighted score
                let score = bm25 * config.bm25_weight
                    + semantic * config.semantic_weight
                    + graph * config.graph_weight;

                let mut match_reasons = Vec::new();
                if bm25 > 0.0 {
                    match_reasons.push(MatchReason::TextMatch { score: bm25 });
                }
                if semantic > 0.0 {
                    match_reasons.push(MatchReason::SemanticSimilarity { score: semantic });
                }
                if graph > 0.0 {
                    match_reasons.push(MatchReason::CodeProximity { score: graph });
                }

                results.push(SearchResult {
                    memory,
                    score,
                    match_reasons,
                });
            }
        }

        // 6. Sort by score and limit
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(config.limit);

        Ok(results)
    }

    /// Calculate graph proximity score
    fn calculate_graph_score(&self, memory: &MemoryNode, code_context: &[String]) -> f32 {
        if code_context.is_empty() || memory.code_links.is_empty() {
            return 0.0;
        }

        let mut max_score = 0.0_f32;
        for link in &memory.code_links {
            if code_context.contains(&link.node_id) {
                max_score = max_score.max(link.relevance);
            }
        }
        max_score
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bm25_tokenize() {
        let tokens = BM25Index::tokenize("Hello, World! This is a test.");
        assert!(tokens.contains(&"hello".to_string()));
        assert!(tokens.contains(&"world".to_string()));
        assert!(tokens.contains(&"test".to_string()));
        // Short words should be filtered
        assert!(!tokens.contains(&"is".to_string()));
        assert!(!tokens.contains(&"a".to_string()));
    }

    #[test]
    fn test_search_config_default() {
        let config = SearchConfig::default();
        assert_eq!(config.limit, 10);
        assert_eq!(config.bm25_weight, 0.3);
        assert_eq!(config.semantic_weight, 0.5);
        assert_eq!(config.graph_weight, 0.2);
        assert!(config.current_only);
    }

    #[test]
    fn test_memory_kind_filter_matches() {
        let kind = MemoryKind::DebugContext {
            problem_description: "test".to_string(),
            root_cause: None,
            solution: "fix".to_string(),
            symptoms: vec![],
            related_errors: vec![],
        };

        assert!(MemoryKindFilter::DebugContext.matches(&kind));
        assert!(!MemoryKindFilter::ArchitecturalDecision.matches(&kind));
    }
}
