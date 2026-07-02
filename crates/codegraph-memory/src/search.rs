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

    /// Build a Convention memory whose searchable_text is title + content + tags.
    fn mem(title: &str, content: &str, tags: &[&str]) -> MemoryNode {
        let mut b = MemoryNode::builder()
            .convention(title, content)
            .title(title);
        b = b.content(content);
        for t in tags {
            b = b.tag(*t);
        }
        b.build().unwrap()
    }

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
    fn test_tokenize_keeps_alphanumeric_and_lowercases() {
        let tokens = BM25Index::tokenize("Rust123 SPLIT-on/punct");
        // Digits are alphanumeric so tokens containing them survive.
        assert!(tokens.contains(&"rust123".to_string()));
        // Split boundaries include - and /.
        assert!(tokens.contains(&"split".to_string()));
        assert!(tokens.contains(&"punct".to_string()));
        // Everything is lowercased.
        assert!(tokens.iter().all(|t| t == &t.to_lowercase()));
    }

    #[test]
    fn test_tokenize_filters_len_le_two() {
        // Exactly-two-char tokens are dropped; three-char kept.
        let tokens = BM25Index::tokenize("ab abc abcd");
        assert!(!tokens.contains(&"ab".to_string()));
        assert!(tokens.contains(&"abc".to_string()));
        assert!(tokens.contains(&"abcd".to_string()));
    }

    #[test]
    fn test_build_empty_corpus() {
        let index = BM25Index::build(&[]);
        assert_eq!(index.num_docs, 0);
        assert_eq!(index.avg_doc_length, 0.0);
        assert!(index.inverted.is_empty());
        assert!(index.doc_lengths.is_empty());
        // Searching an empty index yields nothing and does not panic.
        assert!(index.search("anything", 10).is_empty());
    }

    #[test]
    fn test_build_indexes_terms_and_lengths() {
        let m = mem("Alpha Beta", "gamma delta", &["epsilon"]);
        let id = m.id.to_string();
        let index = BM25Index::build(&[m]);

        assert_eq!(index.num_docs, 1);
        // Five tokens all > 2 chars: alpha beta gamma delta epsilon.
        assert_eq!(index.doc_lengths.get(&id).copied(), Some(5.0));
        assert_eq!(index.avg_doc_length, 5.0);
        // Each term maps to this single document.
        let postings = index.inverted.get("gamma").expect("gamma indexed");
        assert_eq!(postings, &vec![(id.clone(), 1.0)]);
        assert!(index.inverted.contains_key("epsilon"));
    }

    #[test]
    fn test_build_counts_term_frequency() {
        let m = mem("repeat repeat repeat", "once", &[]);
        let id = m.id.to_string();
        let index = BM25Index::build(&[m]);
        let postings = index.inverted.get("repeat").expect("repeat indexed");
        assert_eq!(postings, &vec![(id, 3.0)]);
    }

    #[test]
    fn test_avg_doc_length_across_docs() {
        // Doc A has 2 tokens, Doc B has 4 -> avg 3.0.
        let a = mem("alpha beta", "", &[]);
        let b = mem("gamma delta epsilon zeta", "", &[]);
        let index = BM25Index::build(&[a, b]);
        assert_eq!(index.num_docs, 2);
        assert_eq!(index.avg_doc_length, 3.0);
    }

    #[test]
    fn test_search_matches_relevant_doc() {
        let hit = mem("database migration", "schema change", &[]);
        let miss = mem("frontend styling", "css layout", &[]);
        let hit_id = hit.id.to_string();
        let index = BM25Index::build(&[hit, miss]);

        let results = index.search("migration", 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, hit_id);
        assert!(results[0].1 > 0.0);
    }

    #[test]
    fn test_search_no_match_returns_empty() {
        let m = mem("hello world", "foo bar", &[]);
        let index = BM25Index::build(&[m]);
        assert!(index.search("nonexistentterm", 10).is_empty());
    }

    #[test]
    fn test_search_respects_limit_and_sorts_desc() {
        let a = mem("shared token", "extra shared token filler", &[]);
        let b = mem("shared", "one occurrence only here", &[]);
        let index = BM25Index::build(&[a, b]);

        let all = index.search("shared", 10);
        assert_eq!(all.len(), 2);
        // Descending by score.
        assert!(all[0].1 >= all[1].1);

        let limited = index.search("shared", 1);
        assert_eq!(limited.len(), 1);
        assert_eq!(limited[0].0, all[0].0);
    }

    #[test]
    fn test_idf_rarer_term_scores_higher() {
        // 3 documents; idf(1) (rare) should exceed idf(3) (in every doc).
        let docs = vec![
            mem("apple", "", &[]),
            mem("banana", "", &[]),
            mem("cherry", "", &[]),
        ];
        let index = BM25Index::build(&docs);
        assert!(index.idf(1) > index.idf(3));
        // With Robertson-Sparck-Jones +1 smoothing idf stays positive.
        assert!(index.idf(3) > 0.0);
    }

    #[test]
    fn test_bm25_score_monotonic_in_tf_and_idf() {
        let docs = vec![mem("alpha beta gamma", "", &[])];
        let index = BM25Index::build(&docs);
        let dl = index.avg_doc_length;

        // Higher term frequency yields a higher (saturating) score.
        let low = index.bm25_score(1.0, dl, 2.0);
        let high = index.bm25_score(5.0, dl, 2.0);
        assert!(high > low);
        // Score scales linearly with idf.
        let doubled = index.bm25_score(1.0, dl, 4.0);
        assert!((doubled - 2.0 * low).abs() < 1e-4);
    }

    #[test]
    fn test_memory_kind_filter_matches_all_variants() {
        use crate::node::{IssueSeverity, MemoryKind};

        let arch = MemoryKind::ArchitecturalDecision {
            decision: "d".into(),
            rationale: "r".into(),
            alternatives_considered: None,
            stakeholders: vec![],
        };
        let known = MemoryKind::KnownIssue {
            description: "bug".into(),
            severity: IssueSeverity::High,
            workaround: None,
            tracking_id: None,
        };
        let conv = MemoryKind::Convention {
            name: "n".into(),
            description: "d".into(),
            pattern: None,
            anti_pattern: None,
        };
        let proj = MemoryKind::ProjectContext {
            topic: "t".into(),
            description: "d".into(),
            tags: vec![],
        };

        assert!(MemoryKindFilter::ArchitecturalDecision.matches(&arch));
        assert!(MemoryKindFilter::KnownIssue.matches(&known));
        assert!(MemoryKindFilter::Convention.matches(&conv));
        assert!(MemoryKindFilter::ProjectContext.matches(&proj));

        // Cross-variant mismatches all reject.
        assert!(!MemoryKindFilter::ArchitecturalDecision.matches(&known));
        assert!(!MemoryKindFilter::Convention.matches(&proj));
        assert!(!MemoryKindFilter::ProjectContext.matches(&arch));
    }

    #[test]
    fn test_memory_kind_filter_partial_eq_and_clone() {
        let f = MemoryKindFilter::DebugContext;
        assert_eq!(f.clone(), MemoryKindFilter::DebugContext);
        assert_ne!(f, MemoryKindFilter::KnownIssue);
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

    fn search_engine() -> MemorySearch {
        use crate::embedding::VectorEngine;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("temp dir");
        let engine = Arc::new(VectorEngine::new(None).expect("create engine"));
        let store = Arc::new(MemoryStore::new(temp_dir.path(), engine).expect("create store"));
        // Keep the temp dir alive for the store's lifetime by leaking it: the
        // store holds an open handle and we only need the engine for scoring.
        std::mem::forget(temp_dir);
        MemorySearch::new(store).expect("create search")
    }

    fn mem_with_link(node_id: &str, relevance: f32) -> MemoryNode {
        use crate::node::{CodeLink, LinkedNodeType};
        let mut m = mem("linked", "content", &[]);
        m.code_links =
            vec![CodeLink::new(node_id, LinkedNodeType::Function).with_relevance(relevance)];
        m
    }

    #[test]
    fn test_graph_score_zero_when_context_or_links_empty() {
        let search = search_engine();

        // Empty code_context short-circuits to 0.0 even with links present.
        let linked = mem_with_link("node_a", 0.9);
        assert_eq!(search.calculate_graph_score(&linked, &[]), 0.0);

        // Non-empty context but a memory with no code_links also yields 0.0.
        let unlinked = mem("no links", "content", &[]);
        assert!(unlinked.code_links.is_empty());
        assert_eq!(
            search.calculate_graph_score(&unlinked, &["node_a".to_string()]),
            0.0
        );
    }

    #[test]
    fn test_graph_score_matches_max_relevance_or_zero() {
        use crate::node::{CodeLink, LinkedNodeType};
        let search = search_engine();

        // Two links; context overlaps both, so the higher relevance wins.
        let mut multi = mem("multi", "content", &[]);
        multi.code_links = vec![
            CodeLink::new("node_a", LinkedNodeType::Function).with_relevance(0.4),
            CodeLink::new("node_b", LinkedNodeType::Class).with_relevance(0.8),
        ];
        let ctx = vec!["node_a".to_string(), "node_b".to_string()];
        assert!((search.calculate_graph_score(&multi, &ctx) - 0.8).abs() < 1e-6);

        // Non-empty context and links that never overlap fall through to 0.0.
        let disjoint = mem_with_link("node_x", 1.0);
        let other_ctx = vec!["node_y".to_string()];
        assert_eq!(search.calculate_graph_score(&disjoint, &other_ctx), 0.0);
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

    /// Build a store backed by a real (cached) engine and seed it with memories.
    async fn store_with_memories(mems: Vec<MemoryNode>) -> Arc<MemoryStore> {
        use crate::embedding::VectorEngine;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("temp dir");
        let engine = Arc::new(VectorEngine::new(None).expect("create engine"));
        let store = Arc::new(MemoryStore::new(temp_dir.path(), engine).expect("create store"));
        std::mem::forget(temp_dir);
        for m in mems {
            store.put(m).await.expect("put memory");
        }
        store
    }

    #[tokio::test]
    async fn test_search_ranks_matches_and_reports_reasons() {
        // The full hybrid `search` path had no coverage: prior tests only
        // exercised the BM25 index and the private graph-score helper in
        // isolation. Two clearly distinct memories plus a query overlapping the
        // first should rank it above the second and attach both a text and a
        // semantic match reason.
        let store = store_with_memories(vec![
            mem(
                "database migration",
                "how to run the schema migration tool",
                &[],
            ),
            mem(
                "holiday recipe",
                "baking cookies with sugar and butter",
                &[],
            ),
        ])
        .await;
        let search = MemorySearch::new(store).expect("create search");

        let results = search
            .search("database migration schema", &[], &SearchConfig::default())
            .expect("search");

        assert!(!results.is_empty(), "expected at least one result");
        assert_eq!(
            results[0].memory.title, "database migration",
            "the text-overlapping memory should rank first"
        );
        // The top result matched both on text (BM25 index built from the
        // corpus) and semantically, so both reasons fire.
        assert!(results[0]
            .match_reasons
            .iter()
            .any(|r| matches!(r, MatchReason::TextMatch { .. })));
        assert!(results[0]
            .match_reasons
            .iter()
            .any(|r| matches!(r, MatchReason::SemanticSimilarity { .. })));
        // A weighted score is strictly positive once any reason fired.
        assert!(results[0].score > 0.0);
    }

    #[tokio::test]
    async fn test_rebuild_index_registers_new_memory_for_text_match() {
        // `rebuild_index` had no direct coverage. Its sole effect is refreshing
        // the BM25 index from the store: the semantic (HNSW) side updates on
        // `put`, but BM25 stays stale until rebuilt. Insert a memory after the
        // index was built, then confirm a text match only appears post-rebuild.
        let store = store_with_memories(vec![]).await;
        let mut search = MemorySearch::new(store.clone()).expect("create search");

        store
            .put(mem(
                "rustlang parser",
                "tree-sitter incremental parsing engine",
                &[],
            ))
            .await
            .expect("put");

        let query = "rustlang parser tree-sitter";
        let before = search
            .search(query, &[], &SearchConfig::default())
            .expect("search before rebuild");
        // Found semantically, but with no TextMatch since BM25 is stale.
        assert_eq!(before.len(), 1);
        assert!(
            !before[0]
                .match_reasons
                .iter()
                .any(|r| matches!(r, MatchReason::TextMatch { .. })),
            "BM25 index should not know the memory before rebuild"
        );

        search.rebuild_index().expect("rebuild");

        let after = search
            .search(query, &[], &SearchConfig::default())
            .expect("search after rebuild");
        assert_eq!(after.len(), 1);
        assert!(
            after[0]
                .match_reasons
                .iter()
                .any(|r| matches!(r, MatchReason::TextMatch { .. })),
            "rebuilt BM25 index should now yield a text match"
        );
    }
}
