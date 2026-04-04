// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Text Index with BM25 Scoring
//!
//! Provides fast text-based symbol search using an inverted index with BM25 ranking.
//!
//! Performance targets:
//! - Build time: < 5 seconds for 10K symbols
//! - Query time: < 5ms for typical queries
//! - Memory: ~50 bytes per token occurrence

use codegraph::NodeId;
use std::collections::HashMap;

/// BM25 parameter: term frequency saturation
const K1: f32 = 1.2;

/// BM25 parameter: length normalization factor
const B: f32 = 0.75;

/// Weight multipliers for different match types
const WEIGHT_SYMBOL_NAME: f32 = 3.0;
const WEIGHT_DOCSTRING: f32 = 2.0;
const WEIGHT_COMMENT: f32 = 1.0;

/// A posting in the inverted index, representing one occurrence of a term.
#[derive(Debug, Clone)]
pub struct Posting {
    /// The node that contains this term
    pub node_id: NodeId,
    /// Term frequency in this document
    pub term_frequency: f32,
    /// Weight based on field (name=3.0, docstring=2.0, comment=1.0)
    pub weight: f32,
    /// Position in the original text (for phrase queries, future use)
    pub position: usize,
}

/// Result of a text search query with scoring information.
#[derive(Debug, Clone)]
pub struct TextSearchResult {
    /// The matching node
    pub node_id: NodeId,
    /// BM25 score for ranking
    pub score: f32,
    /// Reason this result matched (for explainability)
    pub match_reason: MatchReason,
}

/// Why a result matched the query (for explainability).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchReason {
    /// Matched in symbol name
    SymbolName,
    /// Matched in docstring
    Docstring,
    /// Matched in comment
    Comment,
    /// Matched in multiple fields
    Multiple,
}

/// Text index using BM25 ranking for symbol search.
#[derive(Debug)]
pub struct TextIndex {
    /// Token -> List of postings (node_id, term_frequency, weight)
    inverted_index: HashMap<String, Vec<Posting>>,
    /// NodeId -> Document length (for BM25 normalization)
    doc_lengths: HashMap<NodeId, f32>,
    /// Average document length across all documents
    avg_document_length: f32,
    /// Total number of indexed documents
    total_docs: usize,
    /// NodeId -> Primary match reason (for explainability)
    node_match_types: HashMap<NodeId, MatchReason>,
}

impl TextIndex {
    /// Create a new empty text index.
    pub fn new() -> Self {
        Self {
            inverted_index: HashMap::new(),
            doc_lengths: HashMap::new(),
            avg_document_length: 0.0,
            total_docs: 0,
            node_match_types: HashMap::new(),
        }
    }

    /// Build an index from a list of documents.
    /// Each document is (node_id, name, docstring, comments).
    pub fn build(documents: &[(NodeId, String, Option<String>, Vec<String>)]) -> Self {
        let mut builder = TextIndexBuilder::new();
        for (node_id, name, docstring, comments) in documents {
            builder.add_document(*node_id, name, docstring.as_deref(), comments);
        }
        builder.build()
    }

    /// Search the index for matching documents.
    /// Returns results sorted by BM25 score (descending).
    pub fn search(&self, query: &str, limit: usize) -> Vec<TextSearchResult> {
        let tokens = tokenize(query);
        if tokens.is_empty() {
            return Vec::new();
        }

        let mut scores: HashMap<NodeId, f32> = HashMap::new();

        for token in &tokens {
            if let Some(postings) = self.inverted_index.get(token) {
                let idf = self.compute_idf(token);

                for posting in postings {
                    let doc_len = self
                        .doc_lengths
                        .get(&posting.node_id)
                        .copied()
                        .unwrap_or(1.0);
                    let tf = posting.term_frequency;

                    // BM25 formula
                    let numerator = tf * (K1 + 1.0);
                    let denominator =
                        tf + K1 * (1.0 - B + B * (doc_len / self.avg_document_length));
                    let score = idf * (numerator / denominator) * posting.weight;

                    *scores.entry(posting.node_id).or_insert(0.0) += score;
                }
            }
        }

        // Collect and sort results
        let mut results: Vec<_> = scores
            .into_iter()
            .map(|(node_id, score)| TextSearchResult {
                node_id,
                score,
                match_reason: self
                    .node_match_types
                    .get(&node_id)
                    .cloned()
                    .unwrap_or(MatchReason::SymbolName),
            })
            .collect();

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(limit);
        results
    }

    /// Compute inverse document frequency for a term.
    fn compute_idf(&self, term: &str) -> f32 {
        let doc_freq = self
            .inverted_index
            .get(term)
            .map(|postings| {
                // Count unique documents
                let mut unique_docs: Vec<NodeId> = postings.iter().map(|p| p.node_id).collect();
                unique_docs.sort();
                unique_docs.dedup();
                unique_docs.len()
            })
            .unwrap_or(0);

        if doc_freq == 0 {
            return 0.0;
        }

        // IDF formula: log((N - df + 0.5) / (df + 0.5) + 1)
        let n = self.total_docs as f32;
        let df = doc_freq as f32;
        ((n - df + 0.5) / (df + 0.5) + 1.0).ln()
    }

    /// Get the number of indexed documents.
    pub fn document_count(&self) -> usize {
        self.total_docs
    }

    /// Get the number of unique tokens.
    pub fn token_count(&self) -> usize {
        self.inverted_index.len()
    }

    /// Check if a term exists in the index.
    pub fn has_term(&self, term: &str) -> bool {
        self.inverted_index.contains_key(&term.to_lowercase())
    }
}

impl Default for TextIndex {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for constructing a TextIndex incrementally.
#[derive(Debug)]
pub struct TextIndexBuilder {
    inverted_index: HashMap<String, Vec<Posting>>,
    doc_lengths: HashMap<NodeId, f32>,
    node_match_types: HashMap<NodeId, MatchReason>,
    total_length: f32,
    total_docs: usize,
}

impl TextIndexBuilder {
    /// Create a new index builder.
    pub fn new() -> Self {
        Self {
            inverted_index: HashMap::new(),
            doc_lengths: HashMap::new(),
            node_match_types: HashMap::new(),
            total_length: 0.0,
            total_docs: 0,
        }
    }

    /// Add a document to the index.
    pub fn add_document(
        &mut self,
        node_id: NodeId,
        name: &str,
        docstring: Option<&str>,
        comments: &[String],
    ) {
        let mut doc_length = 0.0;
        let mut has_name_match = false;
        let mut has_docstring_match = false;
        let mut has_comment_match = false;

        // Index symbol name with high weight
        let name_tokens = tokenize(name);
        for (position, token) in name_tokens.iter().enumerate() {
            self.add_posting(node_id, token, WEIGHT_SYMBOL_NAME, position);
            doc_length += 1.0;
            has_name_match = true;
        }

        // Index docstring with medium weight
        if let Some(doc) = docstring {
            let doc_tokens = tokenize(doc);
            for (position, token) in doc_tokens.iter().enumerate() {
                self.add_posting(node_id, token, WEIGHT_DOCSTRING, position);
                doc_length += 1.0;
            }
            if !doc_tokens.is_empty() {
                has_docstring_match = true;
            }
        }

        // Index comments with lower weight
        for comment in comments {
            let comment_tokens = tokenize(comment);
            for (position, token) in comment_tokens.iter().enumerate() {
                self.add_posting(node_id, token, WEIGHT_COMMENT, position);
                doc_length += 1.0;
            }
            if !comment_tokens.is_empty() {
                has_comment_match = true;
            }
        }

        self.doc_lengths.insert(node_id, doc_length);
        self.total_length += doc_length;
        self.total_docs += 1;

        // Determine primary match type
        let match_reason = if has_name_match && (has_docstring_match || has_comment_match) {
            MatchReason::Multiple
        } else if has_docstring_match {
            MatchReason::Docstring
        } else if has_comment_match {
            MatchReason::Comment
        } else {
            MatchReason::SymbolName
        };
        self.node_match_types.insert(node_id, match_reason);
    }

    /// Add a posting to the inverted index.
    fn add_posting(&mut self, node_id: NodeId, token: &str, weight: f32, position: usize) {
        let postings = self.inverted_index.entry(token.to_string()).or_default();

        // Check if we already have a posting for this node
        if let Some(existing) = postings.iter_mut().find(|p| p.node_id == node_id) {
            // Update term frequency and use max weight
            existing.term_frequency += 1.0;
            existing.weight = existing.weight.max(weight);
        } else {
            postings.push(Posting {
                node_id,
                term_frequency: 1.0,
                weight,
                position,
            });
        }
    }

    /// Build the final TextIndex.
    pub fn build(self) -> TextIndex {
        let avg_document_length = if self.total_docs > 0 {
            self.total_length / self.total_docs as f32
        } else {
            1.0
        };

        TextIndex {
            inverted_index: self.inverted_index,
            doc_lengths: self.doc_lengths,
            avg_document_length,
            total_docs: self.total_docs,
            node_match_types: self.node_match_types,
        }
    }
}

impl Default for TextIndexBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Tokenize a string into lowercase tokens.
/// Splits on non-alphanumeric characters and handles camelCase/snake_case.
/// Handles acronyms like "VALIDATE" or "HTMLParser" correctly.
fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut prev_was_upper = false;

    for ch in text.chars() {
        if ch.is_alphanumeric() {
            // Handle camelCase: split before uppercase letters
            // BUT: don't split if previous char was also uppercase (handles acronyms like "HTML")
            // AND: don't split if we're at the start of a token
            if ch.is_uppercase() && !current.is_empty() && !prev_was_upper {
                tokens.push(current.to_lowercase());
                current = String::new();
            }
            current.push(ch);
            prev_was_upper = ch.is_uppercase();
        } else {
            // Split on non-alphanumeric
            if !current.is_empty() {
                tokens.push(current.to_lowercase());
                current = String::new();
            }
            prev_was_upper = false;
        }
    }

    if !current.is_empty() {
        tokens.push(current.to_lowercase());
    }

    // Filter out very short tokens (less than 2 chars) except for common programming terms
    tokens
        .into_iter()
        .filter(|t| t.len() >= 2 || matches!(t.as_str(), "id" | "io" | "ok"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================
    // Tokenization Tests
    // ============================================

    #[test]
    fn test_tokenize_simple() {
        let tokens = tokenize("hello world");
        assert_eq!(tokens, vec!["hello", "world"]);
    }

    #[test]
    fn test_tokenize_camel_case() {
        let tokens = tokenize("validateEmail");
        assert_eq!(tokens, vec!["validate", "email"]);
    }

    #[test]
    fn test_tokenize_snake_case() {
        let tokens = tokenize("validate_email");
        assert_eq!(tokens, vec!["validate", "email"]);
    }

    #[test]
    fn test_tokenize_mixed_case() {
        let tokens = tokenize("getUserById");
        assert_eq!(tokens, vec!["get", "user", "by", "id"]);
    }

    #[test]
    fn test_tokenize_filters_short_tokens() {
        let tokens = tokenize("a b c id ok io xy");
        // Should keep "id", "ok", "io" and filter out "a", "b", "c", "xy"
        assert!(tokens.contains(&"id".to_string()));
        assert!(tokens.contains(&"ok".to_string()));
        assert!(tokens.contains(&"io".to_string()));
        assert_eq!(tokens.len(), 4); // id, ok, io, xy (xy has 2 chars)
    }

    #[test]
    fn test_tokenize_empty() {
        let tokens = tokenize("");
        assert!(tokens.is_empty());
    }

    #[test]
    fn test_tokenize_special_chars() {
        let tokens = tokenize("hello-world_test.foo");
        assert_eq!(tokens, vec!["hello", "world", "test", "foo"]);
    }

    // ============================================
    // TextIndex Builder Tests
    // ============================================

    #[test]
    fn test_builder_new() {
        let builder = TextIndexBuilder::new();
        assert_eq!(builder.total_docs, 0);
    }

    #[test]
    fn test_builder_add_single_document() {
        let mut builder = TextIndexBuilder::new();
        builder.add_document(1, "validateEmail", None, &[]);

        let index = builder.build();
        assert_eq!(index.document_count(), 1);
        assert!(index.has_term("validate"));
        assert!(index.has_term("email"));
    }

    #[test]
    fn test_builder_add_document_with_docstring() {
        let mut builder = TextIndexBuilder::new();
        builder.add_document(
            1,
            "validateEmail",
            Some("Validates an email address format"),
            &[],
        );

        let index = builder.build();
        assert!(index.has_term("validates"));
        assert!(index.has_term("address"));
        assert!(index.has_term("format"));
    }

    #[test]
    fn test_builder_add_document_with_comments() {
        let mut builder = TextIndexBuilder::new();
        builder.add_document(
            1,
            "processData",
            None,
            &[
                "Handles incoming data".to_string(),
                "Returns processed result".to_string(),
            ],
        );

        let index = builder.build();
        assert!(index.has_term("handles"));
        assert!(index.has_term("incoming"));
        assert!(index.has_term("processed"));
        assert!(index.has_term("result"));
    }

    // ============================================
    // TextIndex Build Tests
    // ============================================

    #[test]
    fn test_build_empty() {
        let index = TextIndex::build(&[]);
        assert_eq!(index.document_count(), 0);
        assert_eq!(index.token_count(), 0);
    }

    #[test]
    fn test_build_single_document() {
        let index = TextIndex::build(&[(1, "processUserData".to_string(), None, vec![])]);

        assert_eq!(index.document_count(), 1);
        assert!(index.has_term("process"));
        assert!(index.has_term("user"));
        assert!(index.has_term("data"));
    }

    #[test]
    fn test_build_multiple_documents() {
        let index = TextIndex::build(&[
            (1, "processUserData".to_string(), None, vec![]),
            (2, "validateEmail".to_string(), None, vec![]),
            (3, "handleRequest".to_string(), None, vec![]),
        ]);

        assert_eq!(index.document_count(), 3);
    }

    // ============================================
    // Search Tests
    // ============================================

    #[test]
    fn test_search_empty_query() {
        let index = TextIndex::build(&[(1, "validateEmail".to_string(), None, vec![])]);

        let results = index.search("", 10);
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_no_match() {
        let index = TextIndex::build(&[(1, "validateEmail".to_string(), None, vec![])]);

        let results = index.search("database", 10);
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_exact_match() {
        let index = TextIndex::build(&[
            (1, "validateEmail".to_string(), None, vec![]),
            (2, "processData".to_string(), None, vec![]),
        ]);

        let results = index.search("validate", 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].node_id, 1);
    }

    #[test]
    fn test_search_partial_match() {
        let index = TextIndex::build(&[
            (1, "validateEmail".to_string(), None, vec![]),
            (2, "validatePhone".to_string(), None, vec![]),
            (3, "processData".to_string(), None, vec![]),
        ]);

        let results = index.search("validate", 10);
        assert_eq!(results.len(), 2);
        // Both validateEmail and validatePhone should match
        let node_ids: Vec<NodeId> = results.iter().map(|r| r.node_id).collect();
        assert!(node_ids.contains(&1));
        assert!(node_ids.contains(&2));
    }

    #[test]
    fn test_search_case_insensitive() {
        let index = TextIndex::build(&[(1, "ValidateEmail".to_string(), None, vec![])]);

        let results = index.search("VALIDATE", 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].node_id, 1);
    }

    #[test]
    fn test_search_ranking_by_term_frequency() {
        let index = TextIndex::build(&[
            (
                1,
                "validate".to_string(),
                Some("validate validate validate".to_string()),
                vec![],
            ),
            (2, "validate".to_string(), None, vec![]),
        ]);

        let results = index.search("validate", 10);
        assert_eq!(results.len(), 2);
        // Node 1 should rank higher due to more occurrences
        assert_eq!(results[0].node_id, 1);
    }

    #[test]
    fn test_search_limit() {
        let index = TextIndex::build(&[
            (1, "test".to_string(), None, vec![]),
            (2, "test".to_string(), None, vec![]),
            (3, "test".to_string(), None, vec![]),
            (4, "test".to_string(), None, vec![]),
            (5, "test".to_string(), None, vec![]),
        ]);

        let results = index.search("test", 3);
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_search_multi_term() {
        let index = TextIndex::build(&[
            (1, "validateEmail".to_string(), None, vec![]),
            (2, "processEmail".to_string(), None, vec![]),
            (3, "validatePhone".to_string(), None, vec![]),
        ]);

        // "validate email" should match validateEmail best
        let results = index.search("validate email", 10);
        assert!(!results.is_empty());
        assert_eq!(results[0].node_id, 1);
    }

    #[test]
    fn test_search_symbol_name_weight() {
        // Symbol name matches should rank higher than docstring matches
        let index = TextIndex::build(&[
            (
                1,
                "handleRequest".to_string(),
                Some("validate the request".to_string()),
                vec![],
            ),
            (2, "validateRequest".to_string(), None, vec![]),
        ]);

        let results = index.search("validate", 10);
        assert_eq!(results.len(), 2);
        // Node 2 (name match) should rank higher than Node 1 (docstring match)
        assert_eq!(results[0].node_id, 2);
    }

    // ============================================
    // Match Reason Tests
    // ============================================

    #[test]
    fn test_match_reason_symbol_name() {
        let index = TextIndex::build(&[(1, "validateEmail".to_string(), None, vec![])]);

        let results = index.search("validate", 10);
        assert_eq!(results[0].match_reason, MatchReason::SymbolName);
    }

    // ============================================
    // Performance Tests
    // ============================================

    #[test]
    fn test_performance_large_index() {
        // Build an index with 1000 documents
        let documents: Vec<_> = (0..1000)
            .map(|i| {
                (
                    i as NodeId,
                    format!("function{i}"),
                    Some(format!("Documentation for function {i}")),
                    vec![format!("Comment for function {i}")],
                )
            })
            .collect();

        let start = std::time::Instant::now();
        let index = TextIndex::build(&documents);
        let build_time = start.elapsed();

        // Build should be fast (< 100ms for 1000 docs)
        assert!(
            build_time.as_millis() < 100,
            "Build took too long: {build_time:?}"
        );
        assert_eq!(index.document_count(), 1000);

        // Search should be fast (< 5ms)
        let start = std::time::Instant::now();
        let results = index.search("function", 20);
        let search_time = start.elapsed();

        assert!(
            search_time.as_millis() < 5,
            "Search took too long: {search_time:?}"
        );
        assert_eq!(results.len(), 20);
    }

    #[test]
    fn test_idf_calculation() {
        // Rare terms should have higher IDF than common terms
        let index = TextIndex::build(&[
            (1, "validate".to_string(), None, vec![]),
            (2, "validate".to_string(), None, vec![]),
            (3, "validate".to_string(), None, vec![]),
            (4, "unique".to_string(), None, vec![]),
        ]);

        let common_idf = index.compute_idf("validate");
        let rare_idf = index.compute_idf("unique");

        // Rare term should have higher IDF
        assert!(
            rare_idf > common_idf,
            "Rare term IDF ({rare_idf}) should be > common term IDF ({common_idf})"
        );
    }

    // ============================================
    // Edge Case Tests
    // ============================================

    #[test]
    fn test_empty_symbol_name() {
        let index =
            TextIndex::build(&[(1, "".to_string(), Some("Has docstring".to_string()), vec![])]);

        let results = index.search("docstring", 10);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_unicode_handling() {
        // Unicode strings tokenize as whole units (no splitting within scripts)
        let index = TextIndex::build(&[
            (1, "handleユーザー".to_string(), None, vec![]),
            (2, "processДанные".to_string(), None, vec![]),
        ]);

        assert_eq!(index.document_count(), 2);

        // Mixed ASCII-Unicode tokens require full match or prefix match
        // This is a known limitation - tokens are not split at script boundaries
        let results = index.search("handleユーザー", 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].node_id, 1);

        // Pure ASCII still works with camelCase
        let index2 = TextIndex::build(&[(1, "handleUserRequest".to_string(), None, vec![])]);
        let results = index2.search("handle", 10);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_duplicate_terms_in_document() {
        let index = TextIndex::build(&[(1, "validateValidateValidate".to_string(), None, vec![])]);

        // Should handle repeated terms correctly
        let results = index.search("validate", 10);
        assert_eq!(results.len(), 1);
    }
}
