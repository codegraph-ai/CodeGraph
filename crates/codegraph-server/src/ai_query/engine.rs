// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AI Query Engine
//!
//! Main query engine that provides fast, composable query primitives for AI agents.
//! Integrates with CodeGraph for graph-based code intelligence.

use super::primitives::{
    truncate_string, CallInfo, ClusterMember, ClusterResult, DetailedSymbolInfo, DuplicatePair,
    DuplicateResult, EntryPoint, EntryType, ImplementorInfo, ImportMatchMode, ImportSearchOptions,
    SearchOptions, SignaturePattern, StructuralComparison, SymbolCluster, SymbolComparison,
    SymbolInfo, SymbolLocation, SymbolMatch, SymbolSearchResult, SymbolType, TraversalDirection,
    TraversalFilter, TraversalNode, MAX_SIGNATURE_LENGTH,
};
use super::text_index::{TextIndex, TextIndexBuilder};
use crate::domain::node_props;
use codegraph::{CodeGraph, Direction, EdgeType, NamespacedBackend, NodeId, NodeType};
use codegraph_memory::VectorEngine;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

/// Weight for BM25 score in hybrid search (0-1)
const BM25_WEIGHT: f32 = 0.4;
/// Weight for semantic similarity score in hybrid search (0-1)
const SEMANTIC_WEIGHT: f32 = 0.6;

/// AI Query Engine for fast code exploration.
pub struct QueryEngine {
    /// Reference to the code graph
    graph: Arc<RwLock<CodeGraph>>,
    /// Text index for fast symbol search
    text_index: Arc<RwLock<TextIndex>>,
    /// Import index: library name -> importing files
    import_index: Arc<RwLock<HashMap<String, Vec<NodeId>>>>,
    /// Caller index: function -> list of callers
    caller_index: Arc<RwLock<HashMap<NodeId, Vec<NodeId>>>>,
    /// Callee index: function -> list of callees
    callee_index: Arc<RwLock<HashMap<NodeId, Vec<NodeId>>>>,
    /// Shared vector engine for semantic embedding (set after memory init)
    vector_engine: Arc<RwLock<Option<Arc<VectorEngine>>>>,
    /// Symbol embeddings: NodeId -> 768-dim vector (Jina Code V2)
    symbol_vectors: Arc<RwLock<HashMap<NodeId, Vec<f32>>>>,
    /// Symbol text used for embedding (for rebuilding)
    symbol_texts: Arc<RwLock<HashMap<NodeId, String>>>,
    /// Embed full function body (true) or just name+signature (false, default)
    full_body_embedding: std::sync::atomic::AtomicBool,
    /// Prepend split-identifier words to the embed text (helps static
    /// embedders; off by default to leave the transformer path unchanged).
    split_identifiers: std::sync::atomic::AtomicBool,
    /// Set when the watcher daemon this session attached to holds vectors
    /// built with different embed settings. No embed run will follow, so the
    /// search status must not say embeddings are building.
    daemon_vectors_mismatched: std::sync::atomic::AtomicBool,
}

/// Search status when the attached watcher daemon's vectors were built with
/// different embed settings than this session's.
const DAEMON_VECTORS_MISMATCHED_STATUS: &str = "The --watch daemon's stored vectors were built \
    with different embedding settings, so semantic matching is unavailable this session - results \
    are from name/text search only. Restart the daemon with the same --full-body-embedding / \
    --split-identifiers / --embedding-model flags as this session.";

/// Max characters of function body for full-body embedding.
/// ~512 tokens ≈ first 40-50 lines of code.
const FULL_BODY_MAX_CHARS: usize = 2048;

/// Persist accumulated vectors at least every this many symbols during a
/// checkpointed embed run, so an OOM-kill mid-marathon (telemetry: linux
/// SIGKILL ~25 min into a big first index) loses minutes, not the run.
const EMBED_CHECKPOINT_SYMBOLS: usize = 20_000;

/// Available-RAM floor (MB). Below it the embed loop halves the ONNX batch
/// and forces a checkpoint — degrade to slow instead of being OOM-killed.
const EMBED_LOW_MEM_MB: u64 = 1_536;

/// How often (in chunks) the embed loop polls available memory.
const EMBED_MEM_CHECK_CHUNKS: usize = 25;

/// Available system memory in MB, for the embed loop's backpressure check.
fn available_memory_mb() -> u64 {
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_memory();
    sys.available_memory() / (1024 * 1024)
}

/// Whether an available-memory reading counts as real pressure for the embed
/// loop. A 0 MB reading is a detection failure, not genuine pressure — some
/// macOS `sysinfo` versions report 0 available because reclaimable memory is
/// not counted as free (issue #13) — so it must not ratchet the batch size
/// down or force checkpoints.
fn embed_memory_pressured(avail_mb: u64) -> bool {
    avail_mb > 0 && avail_mb < EMBED_LOW_MEM_MB
}

/// Version of the code that builds embedding text, bumped whenever that code
/// changes what it emits.
///
/// 1 = raw name (through 0.20.1)
/// 2 = delimiter-free identifiers also embedded word-split
/// 3 = leading and trailing `_`/`-` no longer count as word delimiters, so
///     `_handleClick` is split like `handleClick`
const EMBED_TEXT_SCHEMA: u32 = 3;

/// Identifies the embedding text a set of vectors was built from: the version
/// of the code that builds it, plus the settings that change what it emits.
///
/// Vectors are only comparable to others built the same way. `getUserById` and
/// `get user by id getUserById` describe the same symbol but land in different
/// places, so ranking one against the other is worse than either scheme alone -
/// and the same holds for a signature-only vector against a full-body one.
/// The model comes first because it decides comparability most bluntly of all:
/// bge-small emits 384 dimensions and jina-code-v2 emits 768, and
/// `cosine_similarity` zips two vectors of different length down to the shorter
/// one while dividing by the longer norm, so mixing them scores every symbol
/// wrong without erroring.
fn embed_text_id(model: &str, full_body: bool, split_identifiers: bool) -> String {
    let flag = |on: bool| if on { "on" } else { "off" };
    format!(
        "{EMBED_TEXT_SCHEMA}-model={model}-body={}-split={}",
        flag(full_body),
        flag(split_identifiers)
    )
}

/// What [`QueryEngine::load_symbol_vectors`] found in the store.
///
/// The three empty outcomes are not interchangeable. A caller that treats
/// "nothing loaded" as "re-embed the repo" does a whole-corpus ONNX run because
/// a watcher daemon had not reached its first persist yet, or because the store
/// was locked for the moment it took the daemon to write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VectorLoad {
    /// Vectors built from the same embed text this engine produces.
    Loaded(usize),
    /// Nothing is stored for this project yet.
    Absent,
    /// The stored vectors were built from different embed text - another
    /// configuration, another model, or 0.20.1, which stamped nothing.
    Mismatched,
    /// The store could not be read. Says nothing about what it holds.
    Unreadable,
}

impl VectorLoad {
    /// Vectors actually loaded; zero for every outcome but [`Self::Loaded`].
    pub fn count(&self) -> usize {
        match self {
            Self::Loaded(n) => *n,
            _ => 0,
        }
    }
}

/// Key prefix for a project's persisted symbol vectors. One set per project.
const VECTOR_KEY_PREFIX: &str = "vec:";

/// Key holding the [`embed_text_id`] the persisted vectors were built from.
///
/// It is also the ownership marker. A project belongs to whichever embed text
/// last claimed it, and only a rebuild that is about to write a full set may
/// claim - see [`claim_project`].
const EMBED_STAMP_KEY: &[u8] = b"embed_text_stamp";

/// The node a `vec:` key belongs to, if it is one.
fn vector_key_node(key: &[u8]) -> Option<NodeId> {
    std::str::from_utf8(key)
        .ok()?
        .strip_prefix(VECTOR_KEY_PREFIX)?
        .parse::<NodeId>()
        .ok()
}

/// Whether a project's vectors belong to a watcher daemon other than this
/// process.
///
/// The daemon's own indexing runs before it publishes a heartbeat and matches
/// this pid afterwards, so it is never locked out of its own project; a daemon
/// that died leaves a stale heartbeat that `live_daemon_for` discards, so a
/// crash does not strand the vectors either.
fn owned_by_another_process(daemon: Option<&crate::daemon::DaemonHeartbeat>) -> bool {
    daemon.is_some_and(|d| d.pid != std::process::id())
}

/// The embed text a project's stored vectors belong to, if any.
fn project_owner(backend: &NamespacedBackend) -> std::result::Result<Option<Vec<u8>>, String> {
    use codegraph::StorageBackend;

    backend
        .get(EMBED_STAMP_KEY)
        .map_err(|e| format!("Failed to read embed-text stamp: {e}"))
}

/// Take a project's vector set for `stamp`: drop whatever is stored and stamp
/// the project, in one batch. Returns whether the project was taken.
///
/// This is the only destructive step in the lifecycle, and the process taking
/// it is about to write a full set. Everything after it - every checkpoint, the
/// final save - only adds to a set this run owns, so a crash mid-rebuild leaves
/// a partial set under its own stamp that the next start loads and the fill
/// pass completes. Losing a marathon first index to an OOM-kill is the failure
/// checkpointing exists for; it only works if the checkpoints are loadable,
/// which means the stamp has to be there from the start.
///
/// A live watcher daemon owns its workspace's vectors, so no other process may
/// take them - not a session that attached to it, not the reindex tool inside
/// one, not a path added later. The refusal lives here, at the delete, rather
/// than in the callers that would otherwise each have to remember it.
fn claim_project(
    backend: &mut NamespacedBackend,
    slug: &str,
    stamp: &str,
) -> std::result::Result<bool, String> {
    use codegraph::storage::BatchOperation;
    use codegraph::StorageBackend;

    if owned_by_another_process(crate::daemon::live_daemon_for(slug).as_ref()) {
        return Ok(false);
    }

    let mut ops: Vec<BatchOperation> = backend
        .scan_prefix_keys(VECTOR_KEY_PREFIX.as_bytes())
        .map_err(|e| format!("Failed to scan stored vector keys: {e}"))?
        .into_iter()
        .map(|key| BatchOperation::Delete { key })
        .collect();
    ops.push(BatchOperation::Put {
        key: EMBED_STAMP_KEY.to_vec(),
        value: stamp.as_bytes().to_vec(),
    });

    backend
        .write_batch(ops)
        .map_err(|e| format!("Failed to claim symbol vector set: {e}"))?;
    Ok(true)
}

/// Vectors per write batch.
///
/// Each batch is copied into the backend's own buffer, so writing a large set
/// as one batch peaks at several times the set's size. The checkpoint that runs
/// when [`embed_memory_pressured`] fires is exactly the write that must not do
/// that: it exists to survive low memory, not to triple the footprint at the
/// moment memory is already short.
const STORE_BATCH_VECTORS: usize = 2048;

/// Store `vecs` for a project, returning whether anything was written.
///
/// Writes only into a set this stamp already owns. A project nobody has claimed
/// is not adopted, and a set belonging to different embed text is left alone:
/// stamping is [`claim_project`]'s job, reserved for a process about to write
/// the whole set. Without that rule an unstamped set - everything a pre-0.21
/// binary wrote, including a `--watch` daemon still running across an upgrade -
/// would be adopted as current by the first complete save to come along, and
/// then served as if it matched.
///
/// Only ever adds. [`claim_project`] is the sole path that deletes, so it is
/// the sole place ownership has to be enforced, and a save can never destroy
/// work another writer - a checkpointing rebuild, a watcher daemon, a session
/// indexing a narrower set of paths - has already stored. Vectors for nodes
/// that no longer exist are dropped by the next claim, which clears the set
/// wholesale before the rebuild that follows refills it.
///
/// Written in chunks rather than one batch. An interrupted write leaves a
/// subset of the set under its own stamp, which is the same state a checkpoint
/// leaves and which the fill pass completes on the next start.
fn store_vectors(
    backend: &mut NamespacedBackend,
    vecs: &HashMap<NodeId, Vec<f32>>,
    stamp: &str,
) -> std::result::Result<bool, String> {
    use codegraph::storage::BatchOperation;
    use codegraph::StorageBackend;

    if project_owner(backend)?.as_deref() != Some(stamp.as_bytes()) {
        return Ok(false);
    }

    let mut batch: Vec<BatchOperation> = Vec::with_capacity(STORE_BATCH_VECTORS.min(vecs.len()));
    for (&node_id, vec) in vecs.iter() {
        batch.push(BatchOperation::Put {
            key: format!("{VECTOR_KEY_PREFIX}{node_id}").into_bytes(),
            value: vec.iter().flat_map(|f| f.to_le_bytes()).collect(),
        });
        if batch.len() == STORE_BATCH_VECTORS {
            backend
                .write_batch(std::mem::take(&mut batch))
                .map_err(|e| format!("Failed to store symbol vectors: {e}"))?;
            batch.reserve(STORE_BATCH_VECTORS);
        }
    }
    if !batch.is_empty() {
        backend
            .write_batch(batch)
            .map_err(|e| format!("Failed to store symbol vectors: {e}"))?;
    }
    Ok(true)
}

/// Read a project's persisted vectors, but only when they were built from
/// `stamp`.
///
/// A set built from other text is left exactly where it is rather than deleted:
/// the process that wrote it may still be using it, and the next complete save
/// replaces it wholesale anyway. `vec:` keys carrying no stamp at all are the
/// 0.20.1 layout and take the same path.
///
/// The error tells the caller *why* it got nothing, which is not one question
/// but three - see [`VectorLoad`].
fn read_vectors(
    backend: &NamespacedBackend,
    stamp: &str,
) -> std::result::Result<Vec<(NodeId, Vec<f32>)>, VectorLoad> {
    use codegraph::StorageBackend;

    let stored_stamp = backend.get(EMBED_STAMP_KEY).map_err(|e| {
        tracing::warn!("[QueryEngine] Failed to read embed-text stamp: {e}");
        VectorLoad::Unreadable
    })?;

    if stored_stamp.as_deref() != Some(stamp.as_bytes()) {
        let stored = backend
            .scan_prefix_keys(VECTOR_KEY_PREFIX.as_bytes())
            .map_err(|e| {
                tracing::warn!("[QueryEngine] Failed to scan vector keys: {e}");
                VectorLoad::Unreadable
            })?;
        return Err(if stored.is_empty() {
            VectorLoad::Absent
        } else {
            VectorLoad::Mismatched
        });
    }

    let entries = backend
        .scan_prefix(VECTOR_KEY_PREFIX.as_bytes())
        .map_err(|e| {
            tracing::warn!("[QueryEngine] Failed to scan vectors: {e}");
            VectorLoad::Unreadable
        })?;

    Ok(entries
        .into_iter()
        .filter_map(|(key, value)| {
            let node_id = vector_key_node(&key)?;
            if value.len() % 4 != 0 {
                return None;
            }
            let vec = value
                .chunks_exact(4)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect();
            Some((node_id, vec))
        })
        .collect())
}

/// Whether splitting an identifier into words tells the embedder anything it
/// cannot already see.
///
/// A `_` or `-` *between* words is a delimiter every tokenizer already splits
/// on, so prepending the split form of `get_user_by_id` just repeats the name.
/// One at the start or the end separates nothing: `_handleClick` reaches the
/// embedder as the same single rare token `handleClick` does, and the
/// private/member conventions that produce it (`_privateField`, `type_`) belong
/// to exactly the camelCase languages splitting was measured to help.
///
/// Measured on the doc->symbol retrieval eval (pure semantic, R@1, BGE-small):
/// camelCase +79% (compressor, 377 symbols), PascalCase +23% (this repo, 200),
/// snake_case +1.3% (this repo, 723) and -2.5% (SystemVerilog, 696). The Rust
/// snake-vs-Pascal pair is the controlled comparison - same repo, same docs,
/// only the casing differs - so splitting is applied where it pays and skipped
/// where it is a wash or a small loss.
fn needs_word_split(name: &str) -> bool {
    let between_words = name.trim_matches(|c| c == '_' || c == '-');
    !between_words.contains('_') && !between_words.contains('-')
}

/// Split an identifier into camelCase/snake_case words (deduped, lowercased),
/// reusing the BM25 tokenizer: `getUserById` -> "get user by id".
fn split_identifier_words(name: &str) -> String {
    let mut seen = std::collections::HashSet::new();
    super::text_index::tokenize(name)
        .into_iter()
        .filter(|t| seen.insert(t.clone()))
        .collect::<Vec<_>>()
        .join(" ")
}

impl QueryEngine {
    /// Create a new query engine with the given graph.
    pub fn new(graph: Arc<RwLock<CodeGraph>>) -> Self {
        Self {
            graph,
            text_index: Arc::new(RwLock::new(TextIndex::new())),
            import_index: Arc::new(RwLock::new(HashMap::new())),
            caller_index: Arc::new(RwLock::new(HashMap::new())),
            callee_index: Arc::new(RwLock::new(HashMap::new())),
            vector_engine: Arc::new(RwLock::new(None)),
            symbol_vectors: Arc::new(RwLock::new(HashMap::new())),
            symbol_texts: Arc::new(RwLock::new(HashMap::new())),
            full_body_embedding: std::sync::atomic::AtomicBool::new(true),
            // On by default: run-together identifiers are the common case in
            // TypeScript, Java, C# and Go, and are exactly where the embedder
            // cannot recover the words on its own. needs_word_split() keeps it
            // off for snake_case, where it does not help.
            split_identifiers: std::sync::atomic::AtomicBool::new(true),
            daemon_vectors_mismatched: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Record that the attached watcher daemon's vectors were built with
    /// different embed settings, so they will not load and nothing in this
    /// session will build replacements.
    pub fn set_daemon_vectors_mismatched(&self) {
        self.daemon_vectors_mismatched
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// Enable or disable full-body embedding mode.
    pub fn set_full_body_embedding(&self, enabled: bool) {
        self.full_body_embedding
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
    }

    /// Enable or disable prepending split-identifier words to the embed text.
    ///
    /// On by default, and applied only to identifiers whose words are not
    /// already separated by `_`/`-` - see `needs_word_split`. Turning it off
    /// reverts to embedding the raw name, which releases up to 0.20.1 did.
    pub fn set_split_identifiers(&self, enabled: bool) {
        self.split_identifiers
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
    }

    fn split_identifiers_enabled(&self) -> bool {
        self.split_identifiers
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    fn full_body_enabled(&self) -> bool {
        self.full_body_embedding
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Build the embedding text for a symbol node.
    /// In signature mode: "name: signature — docstring"
    /// In full-body mode: "name: signature\n<truncated source body>"
    fn build_embed_text(
        node: &codegraph::Node,
        node_id: NodeId,
        name: &str,
        full_body: bool,
        split_identifiers: bool,
        graph: &CodeGraph,
    ) -> String {
        let signature = node.properties.get_string("signature").unwrap_or("");
        let docstring = node.properties.get_string("doc").unwrap_or("");

        // Base: name + signature + docstring (always included)
        let base = if !docstring.is_empty() && !signature.is_empty() {
            format!("{name}: {signature} — {docstring}")
        } else if !signature.is_empty() {
            format!("{name}: {signature}")
        } else if !docstring.is_empty() {
            format!("{name} — {docstring}")
        } else {
            name.to_string()
        };

        // Prepend the camelCase/snake_case-split form of the name when enabled.
        // Static (lookup-table) embedders can't subword-recover `authenticateUser`
        // from one rare token; the split words ("authenticate user") are their
        // strongest signal, front-loaded so they survive truncation.
        let base = if split_identifiers && needs_word_split(name) {
            let words = split_identifier_words(name);
            if words.is_empty() || words == name.to_lowercase() {
                base
            } else {
                format!("{words} {base}")
            }
        } else {
            base
        };

        if !full_body {
            return base;
        }

        // Full-body: prefer body_prefix from graph (captured at parse time, no disk I/O)
        // Fall back to disk read only if body_prefix is not available
        let body_text = node
            .properties
            .get_string("body_prefix")
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .or_else(|| crate::domain::source_code::get_symbol_source(graph, node_id));

        match body_text {
            Some(body) if body.len() > base.len() + 10 => {
                // UTF-8-safe truncation: a raw `&body[..FULL_BODY_MAX_CHARS]`
                // panics when byte FULL_BODY_MAX_CHARS lands inside a multi-byte
                // char (CJK comment, accented identifier, emoji in a literal).
                // That panic was the `utf8_parse` crash seen in telemetry during
                // the post-ONNX embedding phase. The helper walks back to the
                // nearest char boundary.
                let truncated =
                    codegraph_parser_api::truncate_at_char_boundary(&body, FULL_BODY_MAX_CHARS);
                format!("{base}\n{truncated}")
            }
            _ => base, // fallback to signature mode if source unavailable
        }
    }

    /// Number of symbols that have been indexed (for logging).
    pub async fn symbol_count(&self) -> usize {
        let graph = self.graph.read().await;
        graph
            .iter_nodes()
            .filter(|(_, n)| {
                matches!(
                    n.node_type,
                    NodeType::Function
                        | NodeType::Class
                        | NodeType::Variable
                        | NodeType::Interface
                        | NodeType::Type
                )
            })
            .count()
    }

    /// Build indexes from the current graph state.
    /// Should be called after initial parsing or reindexing.
    pub async fn build_indexes(&self) {
        let graph = self.graph.read().await;

        // Build text index
        let mut text_builder = TextIndexBuilder::new();
        let mut import_map: HashMap<String, Vec<NodeId>> = HashMap::new();
        let mut caller_map: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        let mut callee_map: HashMap<NodeId, Vec<NodeId>> = HashMap::new();

        // Iterate over all nodes using iter_nodes()
        for (node_id, node) in graph.iter_nodes() {
            let name = node_props::name(node).to_string();
            let docstring = node.properties.get_string("doc").map(|s| s.to_string());

            // Add to text index
            text_builder.add_document(node_id, &name, docstring.as_deref(), &[]);

            // Build import index from Imports edges
            if let Ok(neighbors) = graph.get_neighbors(node_id, Direction::Outgoing) {
                for neighbor_id in neighbors {
                    if let Ok(edges) = graph.get_edges_between(node_id, neighbor_id) {
                        for edge_id in edges {
                            if let Ok(edge) = graph.get_edge(edge_id) {
                                match edge.edge_type {
                                    EdgeType::Imports => {
                                        // Get the imported module name
                                        if let Ok(target_node) = graph.get_node(neighbor_id) {
                                            let module_name =
                                                node_props::name(target_node).to_string();
                                            if !module_name.is_empty() {
                                                import_map
                                                    .entry(module_name)
                                                    .or_default()
                                                    .push(node_id);
                                            }
                                        }
                                    }
                                    EdgeType::Calls => {
                                        // Build caller/callee indexes
                                        callee_map.entry(node_id).or_default().push(neighbor_id);
                                        caller_map.entry(neighbor_id).or_default().push(node_id);
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
        }

        // Store built indexes
        *self.text_index.write().await = text_builder.build();
        *self.import_index.write().await = import_map;
        *self.caller_index.write().await = caller_map;
        *self.callee_index.write().await = callee_map;
    }

    /// Set the shared vector engine for semantic search.
    /// Called after MemoryManager initialization provides the engine.
    pub async fn set_vector_engine(&self, engine: Arc<VectorEngine>) {
        *self.vector_engine.write().await = Some(engine);
    }

    /// Build symbol embedding vectors for semantic search.
    /// Requires vector_engine to be set first. Embeds name + signature + docstring
    /// for each symbol in batch for efficiency.
    pub async fn build_symbol_vectors(&self) {
        self.build_symbol_vectors_checkpointed(None).await;
    }

    /// Like [`Self::build_symbol_vectors`], with crash resilience for marathon
    /// first-index runs (telemetry: linux OOM-kills ~25 min into embedding,
    /// losing the whole run because the only save happened at the very end):
    /// - with a `slug`, claims the project's vector set up front and then
    ///   persists accumulated vectors every [`EMBED_CHECKPOINT_SYMBOLS`]
    ///   symbols, so a kill loses minutes and the next start loads the partial
    ///   set and finishes it via [`Self::embed_missing_symbols`];
    /// - polls available RAM every [`EMBED_MEM_CHECK_CHUNKS`] chunks; under
    ///   [`EMBED_LOW_MEM_MB`] it halves the ONNX batch and forces a
    ///   checkpoint — degrade to slow instead of being OOM-killed.
    pub async fn build_symbol_vectors_checkpointed(&self, slug: Option<&str>) {
        let engine = match self.vector_engine.read().await.clone() {
            Some(e) => e,
            None => {
                tracing::warn!("[QueryEngine] No vector engine available, skipping symbol vectors");
                return;
            }
        };

        let start = Instant::now();
        // Read once: every vector this run writes, and the stamp its
        // checkpoints are matched against, must describe the same settings.
        let full_body = self.full_body_enabled();
        let split_identifiers = self.split_identifiers_enabled();
        let stamp = embed_text_id(engine.model_name(), full_body, split_identifiers);

        let graph = self.graph.read().await;

        // Collect symbol texts for embedding. Texts are stored ONCE here and
        // moved into symbol_texts at the end — the previous version kept a
        // second cloned copy alive for the whole run, doubling text RAM on
        // exactly the huge-corpus runs that already flirt with OOM.
        let mut node_ids = Vec::new();
        let mut texts = Vec::new();

        for (node_id, node) in graph.iter_nodes() {
            // Only embed meaningful symbol types
            if !matches!(
                node.node_type,
                NodeType::Function
                    | NodeType::Class
                    | NodeType::Variable
                    | NodeType::Interface
                    | NodeType::Type
            ) {
                continue;
            }

            let name = node_props::name(node);
            if name.is_empty() || name == "arrow_function" || name == "anonymous" {
                continue;
            }

            // Build embedding text
            let embed_text =
                Self::build_embed_text(node, node_id, name, full_body, split_identifiers, &graph);

            node_ids.push(node_id);
            texts.push(embed_text);
        }

        drop(graph); // Release graph lock before embedding

        if texts.is_empty() {
            return;
        }

        // Claiming replaces the stored set, so it happens only once this run
        // is known to have something to write. It used to run first, before a
        // single symbol had been collected: a rebuild that found nothing to
        // embed - an index whose paths were renamed away, say - cleared the
        // project's vectors and then returned having written none.
        let slug = match slug {
            Some(slug) => match Self::claim_vector_set(slug, &stamp) {
                Ok(true) => Some(slug),
                Ok(false) => {
                    tracing::info!(
                        "[QueryEngine] A watcher daemon owns '{slug}' - embedding in memory only, \
                         leaving its vectors alone."
                    );
                    None
                }
                Err(e) => {
                    tracing::warn!(
                        "[QueryEngine] Could not claim the vector set for '{slug}': {e}. \
                         Embedding in memory only; an interrupted run will restart."
                    );
                    None
                }
            },
            None => None,
        };

        let model_name = engine.model_name();
        tracing::info!(
            "[QueryEngine] Embedding {} symbols ({})...",
            texts.len(),
            model_name,
        );

        // Embed in small chunks to limit peak memory usage.
        // ONNX Runtime allocates intermediate tensors proportional to batch size × token count.
        // Signature mode (~20 tokens/item): batch 64 is fine.
        // Full-body mode (~500 tokens/item): reduce batch to 16 to stay within memory.
        let mut chunk_size: usize = if full_body { 16 } else { 64 };

        // Accumulated here and published in one swap at the end, never
        // incrementally: `symbol_vectors` is also the live search set, and a
        // partial one is worse than none. `compute_semantic_scores` succeeds as
        // soon as it is non-empty, so every symbol not embedded yet scores 0 on
        // the semantic half and is ranked below whatever the first chunks
        // happened to cover - for the whole run, on a big repo tens of minutes.
        // Leaving it empty keeps search on pure BM25 and keeps
        // `are_embeddings_ready` false, which is what tells the client
        // embeddings are still building.
        let mut symbol_vecs = HashMap::with_capacity(texts.len());

        let total = texts.len();
        let mut pos = 0usize;
        let mut chunks_done = 0usize;
        let mut since_checkpoint = 0usize;

        while pos < total {
            let end = (pos + chunk_size).min(total);
            let chunk_refs: Vec<&str> = texts[pos..end].iter().map(|s| s.as_str()).collect();

            match engine.embed_batch(&chunk_refs) {
                Ok(vectors) => {
                    for (i, vec) in vectors.into_iter().enumerate() {
                        symbol_vecs.insert(node_ids[pos + i], vec);
                    }
                }
                Err(e) => {
                    tracing::error!(
                        "[QueryEngine] Failed to embed chunk at {}/{}: {:?}",
                        pos,
                        total,
                        e
                    );
                }
            }

            since_checkpoint += end - pos;
            pos = end;
            chunks_done += 1;

            if chunks_done.is_multiple_of(10) && pos < total {
                tracing::info!("[QueryEngine] Embedded {}/{} symbols", pos, total);
            }

            // RAM backpressure: shed batch size under memory pressure.
            let pressured = chunks_done.is_multiple_of(EMBED_MEM_CHECK_CHUNKS)
                && embed_memory_pressured(available_memory_mb());
            if pressured && chunk_size > 4 {
                chunk_size = (chunk_size / 2).max(4);
                tracing::warn!(
                    "[QueryEngine] Low memory — reducing embed batch size to {}",
                    chunk_size
                );
            }

            // Crash-resilience checkpoint.
            if let Some(slug) = slug {
                if since_checkpoint >= EMBED_CHECKPOINT_SYMBOLS
                    || (pressured && since_checkpoint > 0)
                {
                    match Self::save_vectors_map(slug, &symbol_vecs, false, &stamp) {
                        Ok(true) => tracing::info!(
                            "[QueryEngine] Checkpointed {} vectors ({}/{} embedded)",
                            symbol_vecs.len(),
                            pos,
                            total
                        ),
                        Ok(false) => tracing::debug!(
                            "[QueryEngine] Checkpoint skipped: the stored set belongs to another \
                             embed text and only a complete save may replace it"
                        ),
                        Err(e) => tracing::warn!("[QueryEngine] Vector checkpoint failed: {e}"),
                    }
                    since_checkpoint = 0;
                }
            }
        }

        let count = symbol_vecs.len();
        *self.symbol_vectors.write().await = symbol_vecs;
        *self.symbol_texts.write().await = node_ids.into_iter().zip(texts).collect();
        tracing::info!(
            "[QueryEngine] Built {} symbol vectors in {:?}",
            count,
            start.elapsed()
        );
    }

    /// Embed only symbols that don't have vectors yet (after loading persisted vectors).
    /// Much faster than full rebuild when only a few files changed.
    pub async fn embed_missing_symbols(&self) {
        self.embed_missing_symbols_checkpointed(None).await;
    }

    /// [`Self::embed_missing_symbols`] with chunking, RAM backpressure and
    /// (with a `slug`) periodic vector checkpoints — see
    /// [`Self::build_symbol_vectors_checkpointed`]. This is also the resume
    /// path after a crash-interrupted embed run: persisted checkpoint vectors
    /// load on startup and this fills in only the remainder.
    pub async fn embed_missing_symbols_checkpointed(&self, slug: Option<&str>) {
        let engine = match self.vector_engine.read().await.clone() {
            Some(e) => e,
            None => return,
        };

        // Read once: every vector this run writes, and the stamp its
        // checkpoints are matched against, must describe the same settings.
        let full_body = self.full_body_enabled();
        let split_identifiers = self.split_identifiers_enabled();
        let stamp = embed_text_id(engine.model_name(), full_body, split_identifiers);
        let graph = self.graph.read().await;
        let existing_vecs = self.symbol_vectors.read().await;

        let mut node_ids = Vec::new();
        let mut texts = Vec::new();

        for (node_id, node) in graph.iter_nodes() {
            if !matches!(
                node.node_type,
                NodeType::Function
                    | NodeType::Class
                    | NodeType::Variable
                    | NodeType::Interface
                    | NodeType::Type
            ) {
                continue;
            }
            // Skip if already has a vector
            if existing_vecs.contains_key(&node_id) {
                continue;
            }
            let name = node_props::name(node);
            if name.is_empty() || name == "arrow_function" || name == "anonymous" {
                continue;
            }
            let embed_text =
                Self::build_embed_text(node, node_id, name, full_body, split_identifiers, &graph);
            node_ids.push(node_id);
            texts.push(embed_text);
        }

        drop(existing_vecs);
        drop(graph);

        if texts.is_empty() {
            tracing::info!("[QueryEngine] No new symbols to embed");
            return;
        }

        tracing::info!(
            "[QueryEngine] Embedding {} new/changed symbols",
            texts.len()
        );

        // Chunked like build_symbol_vectors. This used to push ALL missing
        // symbols through ONE embed_batch call — harmless for a few changed
        // files, but on the post-crash resume path "missing" can be most of
        // the corpus, making the single batch its own OOM. Same backpressure
        // and (with a slug) the same crash-resilience checkpoints.
        let mut chunk_size: usize = if full_body { 16 } else { 64 };
        let total = texts.len();
        let mut pos = 0usize;
        let mut chunks_done = 0usize;
        let mut since_checkpoint = 0usize;
        let mut embedded = 0usize;

        while pos < total {
            let end = (pos + chunk_size).min(total);
            let chunk_refs: Vec<&str> = texts[pos..end].iter().map(|s| s.as_str()).collect();

            match engine.embed_batch(&chunk_refs) {
                Ok(vectors) => {
                    let mut symbol_vecs = self.symbol_vectors.write().await;
                    for (i, vec) in vectors.into_iter().enumerate() {
                        symbol_vecs.insert(node_ids[pos + i], vec);
                    }
                    embedded += end - pos;
                }
                Err(e) => {
                    tracing::warn!(
                        "[QueryEngine] Failed to embed chunk at {}/{}: {}",
                        pos,
                        total,
                        e
                    );
                }
            }

            since_checkpoint += end - pos;
            pos = end;
            chunks_done += 1;

            let pressured = chunks_done.is_multiple_of(EMBED_MEM_CHECK_CHUNKS)
                && embed_memory_pressured(available_memory_mb());
            if pressured && chunk_size > 4 {
                chunk_size = (chunk_size / 2).max(4);
                tracing::warn!(
                    "[QueryEngine] Low memory — reducing embed batch size to {}",
                    chunk_size
                );
            }

            if let Some(slug) = slug {
                if since_checkpoint >= EMBED_CHECKPOINT_SYMBOLS
                    || (pressured && since_checkpoint > 0)
                {
                    let vecs = self.symbol_vectors.read().await;
                    match Self::save_vectors_map(slug, &vecs, false, &stamp) {
                        Ok(true) => tracing::info!(
                            "[QueryEngine] Checkpointed {} vectors ({}/{} resumed)",
                            vecs.len(),
                            pos,
                            total
                        ),
                        Ok(false) => tracing::debug!(
                            "[QueryEngine] Checkpoint skipped: the stored set belongs to another \
                             embed text and only a complete save may replace it"
                        ),
                        Err(e) => tracing::warn!("[QueryEngine] Vector checkpoint failed: {e}"),
                    }
                    since_checkpoint = 0;
                }
            }
        }

        tracing::info!(
            "[QueryEngine] Embedded {} new symbols (total: {})",
            embedded,
            self.symbol_vectors.read().await.len()
        );
    }

    /// Persist symbol vectors to RocksDB alongside the graph.
    ///
    /// Each vector is stored as key `vec:{node_id}` → binary `[f32]`
    /// (little-endian), stamped with the [`embed_text_id`] they were built
    /// from. Uses the namespaced backend so vectors are scoped per project.
    pub async fn save_symbol_vectors(&self, slug: &str) -> std::result::Result<(), String> {
        let Some(stamp) = self.embed_stamp().await else {
            return Ok(());
        };
        let vecs = self.symbol_vectors.read().await;
        if vecs.is_empty() {
            return Ok(());
        }

        if !Self::save_vectors_map(slug, &vecs, true, &stamp)? {
            tracing::debug!(
                "[QueryEngine] Not persisting symbol vectors for '{slug}': the project belongs to \
                 different embed text and only a rebuild may take it over"
            );
        }
        Ok(())
    }

    /// Take the project's vector set for `stamp`, so the checkpoints of the
    /// rebuild that follows are loadable if it is interrupted. Returns whether
    /// the project was taken - see [`claim_project`].
    fn claim_vector_set(slug: &str, stamp: &str) -> std::result::Result<bool, String> {
        use codegraph::RocksDBBackend;

        let db_path = crate::memory::shared_graph_db_path().map_err(|e| format!("{e}"))?;
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create ~/.codegraph: {e}"))?;
        }
        let rocks =
            RocksDBBackend::open(&db_path).map_err(|e| format!("Failed to open graph.db: {e}"))?;
        let mut namespaced = NamespacedBackend::new(Box::new(rocks), slug);

        claim_project(&mut namespaced, slug, stamp)
    }

    /// The embed-text id this engine's vectors carry, or `None` before a vector
    /// engine is attached - without a model there is nothing to compare.
    async fn embed_stamp(&self) -> Option<String> {
        let model = self
            .vector_engine
            .read()
            .await
            .as_ref()
            .map(|e| e.model_name().to_string())?;
        Some(embed_text_id(
            &model,
            self.full_body_enabled(),
            self.split_identifiers_enabled(),
        ))
    }

    /// Write a vector map to RocksDB under `slug`. `complete` only distinguishes
    /// a mid-run checkpoint from a final save in the log; both add to the
    /// project's set and neither removes anything from it.
    ///
    /// `stamp` is the [`embed_text_id`] these vectors were built from. Returns
    /// whether anything was written - see [`store_vectors`].
    fn save_vectors_map(
        slug: &str,
        vecs: &HashMap<NodeId, Vec<f32>>,
        complete: bool,
        stamp: &str,
    ) -> std::result::Result<bool, String> {
        use codegraph::RocksDBBackend;

        if vecs.is_empty() {
            return Ok(false);
        }

        let db_path = crate::memory::shared_graph_db_path().map_err(|e| format!("{e}"))?;
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create ~/.codegraph: {e}"))?;
        }

        let rocks =
            RocksDBBackend::open(&db_path).map_err(|e| format!("Failed to open graph.db: {e}"))?;
        let mut namespaced = NamespacedBackend::new(Box::new(rocks), slug);

        if !store_vectors(&mut namespaced, vecs, stamp)? {
            return Ok(false);
        }

        tracing::info!(
            "[QueryEngine] Saved {} symbol vectors to graph.db (namespace: {}, embed text: {}, {})",
            vecs.len(),
            slug,
            stamp,
            if complete { "complete" } else { "checkpoint" }
        );
        Ok(true)
    }

    /// Check if embeddings are ready (either loaded from persistence or built in background).
    pub fn are_embeddings_ready(&self) -> bool {
        !self
            .symbol_vectors
            .try_read()
            .map(|v| v.is_empty())
            .unwrap_or(true)
    }

    /// Load persisted symbol vectors from RocksDB.
    ///
    /// Loads nothing unless the stored set is stamped with the embed text this
    /// engine is configured to build; a set built from other text is left
    /// untouched for whoever wrote it. Returns the number of vectors loaded.
    pub async fn load_symbol_vectors(&self, slug: &str) -> VectorLoad {
        use codegraph::RocksDBBackend;

        let Some(stamp) = self.embed_stamp().await else {
            tracing::warn!("[QueryEngine] No vector engine attached - cannot load symbol vectors");
            return VectorLoad::Unreadable;
        };

        let db_path = match crate::memory::shared_graph_db_path() {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("[QueryEngine] No graph.db path for vectors: {}", e);
                return VectorLoad::Unreadable;
            }
        };

        if !db_path.exists() {
            return VectorLoad::Absent;
        }

        let rocks = match RocksDBBackend::open(&db_path) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("[QueryEngine] Failed to open graph.db for vectors: {}", e);
                return VectorLoad::Unreadable;
            }
        };
        let namespaced = NamespacedBackend::new(Box::new(rocks), slug);

        let entries = match read_vectors(&namespaced, &stamp) {
            Ok(entries) if entries.is_empty() => return VectorLoad::Absent,
            Ok(entries) => entries,
            Err(outcome) => {
                tracing::info!(
                    "[QueryEngine] No usable symbol vectors for '{}' ({:?}, wanted embed text: {})",
                    slug,
                    outcome,
                    stamp
                );
                return outcome;
            }
        };

        let loaded = entries.len();
        let mut symbol_vecs = self.symbol_vectors.write().await;
        symbol_vecs.extend(entries);

        tracing::info!(
            "[QueryEngine] Loaded {} symbol vectors from graph.db (namespace: {}, embed text: {})",
            loaded,
            slug,
            stamp
        );
        VectorLoad::Loaded(loaded)
    }

    /// Remove vectors for symbols from a deleted file.
    pub async fn remove_file_vectors(&self, file_path: &str) {
        let graph = self.graph.read().await;
        let mut symbol_vecs = self.symbol_vectors.write().await;
        let mut symbol_texts = self.symbol_texts.write().await;

        // Find node IDs that belonged to this file path and remove their vectors
        let mut removed = 0;
        let stale_ids: Vec<NodeId> = symbol_vecs
            .keys()
            .copied()
            .filter(|&node_id| {
                graph
                    .get_node(node_id)
                    .map(|n| {
                        n.properties
                            .get_string("path")
                            .map(|p| p == file_path)
                            .unwrap_or(false)
                    })
                    // Node was deleted from graph — its vector is stale
                    .unwrap_or(true)
            })
            .collect();

        // Only remove vectors for nodes that are actually gone from the graph
        for node_id in &stale_ids {
            if graph.get_node(*node_id).is_err() {
                symbol_vecs.remove(node_id);
                symbol_texts.remove(node_id);
                removed += 1;
            }
        }

        if removed > 0 {
            tracing::info!(
                "[QueryEngine] Removed {} stale vectors for {}",
                removed,
                file_path
            );
        }
    }

    /// Re-embed only symbols from a specific file path.
    /// Called on did_save to incrementally update embeddings without rebuilding all.
    pub async fn update_file_vectors(&self, file_path: &str) {
        self.update_files_vectors(&[file_path.to_string()]).await;
    }

    /// Re-embed the symbols of several files in one pass over the graph.
    ///
    /// Calling [`Self::update_file_vectors`] once per file walks every node once
    /// per file, so a burst of N changed files cost N full graph scans - one of
    /// the per-event costs behind issue #23. This walks the graph once and
    /// embeds everything that matched in one batch.
    pub async fn update_files_vectors(&self, file_paths: &[String]) {
        if file_paths.is_empty() {
            return;
        }
        let label = match file_paths {
            [only] => only.clone(),
            many => format!("{} files", many.len()),
        };
        let file_path = label.as_str();
        let engine = match self.vector_engine.read().await.clone() {
            Some(e) => e,
            None => {
                tracing::debug!(
                    "[QueryEngine] No vector engine, skipping file embedding for {}",
                    file_path
                );
                return;
            }
        };

        let graph = self.graph.read().await;

        // Collect symbols from this file only
        let mut node_ids = Vec::new();
        let mut texts = Vec::new();

        for (node_id, node) in graph.iter_nodes() {
            if !matches!(
                node.node_type,
                NodeType::Function
                    | NodeType::Class
                    | NodeType::Variable
                    | NodeType::Interface
                    | NodeType::Type
            ) {
                continue;
            }

            // Matched by suffix in either direction, as before: stored paths and
            // event paths are not guaranteed to agree on being absolute.
            let path = node_props::path(node);
            if !file_paths
                .iter()
                .any(|fp| path.ends_with(fp.as_str()) || fp.ends_with(path))
            {
                continue;
            }

            let name = node_props::name(node);
            if name.is_empty() || name == "arrow_function" || name == "anonymous" {
                continue;
            }

            let embed_text = Self::build_embed_text(
                node,
                node_id,
                name,
                self.full_body_embedding
                    .load(std::sync::atomic::Ordering::Relaxed),
                self.split_identifiers
                    .load(std::sync::atomic::Ordering::Relaxed),
                &graph,
            );

            node_ids.push(node_id);
            texts.push(embed_text);
        }

        drop(graph);

        if texts.is_empty() {
            return;
        }

        let text_refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
        match engine.embed_batch(&text_refs) {
            Ok(vectors) => {
                let mut symbol_vecs = self.symbol_vectors.write().await;
                let mut symbol_texts = self.symbol_texts.write().await;
                for (i, vec) in vectors.into_iter().enumerate() {
                    symbol_vecs.insert(node_ids[i], vec);
                    symbol_texts.insert(node_ids[i], texts[i].clone());
                }
                tracing::info!(
                    "[QueryEngine] Re-embedded {} symbols from {}",
                    node_ids.len(),
                    file_path
                );
            }
            Err(e) => {
                tracing::warn!(
                    "[QueryEngine] Failed to re-embed file {}: {:?}",
                    file_path,
                    e
                );
            }
        }
    }

    /// Drop vectors whose node no longer exists in the graph.
    ///
    /// Re-parsing a file deletes its old nodes and creates new ones with fresh
    /// IDs, leaving the old IDs' vectors orphaned. Left unchecked these accumulate
    /// across every edit. Semantic search already skips dead nodes, so this is
    /// hygiene (bounded memory) rather than a correctness fix.
    pub async fn prune_orphan_vectors(&self) {
        let live: std::collections::HashSet<NodeId> = {
            let graph = self.graph.read().await;
            graph.iter_nodes().map(|(id, _)| id).collect()
        };

        let mut vecs = self.symbol_vectors.write().await;
        let before = vecs.len();
        vecs.retain(|id, _| live.contains(id));
        let removed = before - vecs.len();
        drop(vecs);

        if removed > 0 {
            self.symbol_texts
                .write()
                .await
                .retain(|id, _| live.contains(id));
            tracing::debug!("[QueryEngine] pruned {} orphan vectors", removed);
        }
    }

    /// Search for symbols by name, docstring, or comments.
    /// Uses hybrid BM25 + semantic scoring when vector engine is available.
    pub async fn symbol_search(&self, query: &str, options: &SearchOptions) -> SymbolSearchResult {
        let start = Instant::now();

        let text_index = self.text_index.read().await;
        let graph = self.graph.read().await;

        // Fetch more candidates when type-filtering to avoid missing targets ranked lower in BM25
        let fetch_multiplier = if options.symbol_types.is_empty() {
            2
        } else {
            10
        };
        let text_results = text_index.search(query, options.limit * fetch_multiplier);

        // Compute semantic scores if vector engine is available
        let semantic_scores = self.compute_semantic_scores(query).await;
        let has_semantic = !semantic_scores.is_empty();

        // Find max BM25 score for normalization
        let max_bm25 = text_results
            .iter()
            .map(|r| r.score)
            .fold(0.0f32, f32::max)
            .max(0.001); // avoid division by zero

        // Merge BM25 candidates with semantic-only candidates
        let mut all_candidate_ids: HashSet<NodeId> =
            text_results.iter().map(|r| r.node_id).collect();
        if has_semantic {
            // Add top semantic candidates that BM25 missed (the key value of semantic search)
            let mut semantic_sorted: Vec<_> = semantic_scores.iter().collect();
            semantic_sorted
                .sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));
            for (node_id, _) in semantic_sorted
                .iter()
                .take(options.limit * fetch_multiplier)
            {
                all_candidate_ids.insert(**node_id);
            }
        }

        // Build BM25 score lookup
        let bm25_scores: HashMap<NodeId, f32> =
            text_results.iter().map(|r| (r.node_id, r.score)).collect();
        let bm25_reasons: HashMap<NodeId, &str> = text_results
            .iter()
            .map(|r| {
                (
                    r.node_id,
                    match r.match_reason {
                        super::text_index::MatchReason::SymbolName => "SymbolName",
                        super::text_index::MatchReason::Docstring => "Docstring",
                        super::text_index::MatchReason::Comment => "Comment",
                        super::text_index::MatchReason::Multiple => "Multiple",
                    },
                )
            })
            .collect();

        let total_matches = all_candidate_ids.len();

        // Score and filter all candidates
        let mut scored_results = Vec::new();
        for &node_id in &all_candidate_ids {
            if let Ok(node) = graph.get_node(node_id) {
                // Apply symbol type filter
                if !options.symbol_types.is_empty() {
                    let node_type_matches = options.symbol_types.iter().any(|st| {
                        matches!(
                            (st, &node.node_type),
                            (SymbolType::Function, NodeType::Function)
                                | (SymbolType::Class, NodeType::Class)
                                | (SymbolType::Variable, NodeType::Variable)
                                | (SymbolType::Module, NodeType::Module)
                                | (SymbolType::Interface, NodeType::Interface)
                                | (SymbolType::Type, NodeType::Type)
                        )
                    });
                    if !node_type_matches {
                        continue;
                    }
                }

                let symbol_info = self.node_to_symbol_info_opts(&graph, node_id, options.compact);
                if let Some(symbol) = symbol_info {
                    if !options.include_private && !symbol.is_public {
                        continue;
                    }

                    // Compute hybrid score
                    let bm25_norm = bm25_scores.get(&node_id).copied().unwrap_or(0.0) / max_bm25;
                    let semantic_sim = semantic_scores.get(&node_id).copied().unwrap_or(0.0);

                    let score = if has_semantic {
                        BM25_WEIGHT * bm25_norm + SEMANTIC_WEIGHT * semantic_sim
                    } else {
                        bm25_norm // pure BM25 fallback
                    };

                    let match_reason = if let Some(&reason) = bm25_reasons.get(&node_id) {
                        if has_semantic && semantic_sim > 0.3 && bm25_norm < 0.01 {
                            "Semantic".to_string()
                        } else {
                            reason.to_string()
                        }
                    } else {
                        "Semantic".to_string()
                    };

                    scored_results.push(SymbolMatch {
                        node_id,
                        symbol,
                        score,
                        match_reason,
                    });
                }
            }
        }

        // Sort by hybrid score descending
        scored_results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored_results.truncate(options.limit);

        let query_time_ms = start.elapsed().as_millis() as u64;

        let embedding_status = if self
            .daemon_vectors_mismatched
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            Some(DAEMON_VECTORS_MISMATCHED_STATUS.to_string())
        } else if !self.are_embeddings_ready() {
            Some("Embeddings are building in the background. Semantic matching is temporarily unavailable — results are from name/text search only.".to_string())
        } else {
            None
        };

        SymbolSearchResult {
            results: scored_results,
            total_matches,
            query_time_ms,
            embedding_status,
        }
    }

    /// Compute semantic similarity scores for all indexed symbols against a query.
    /// Returns empty map if vector engine or symbol vectors aren't available.
    async fn compute_semantic_scores(&self, query: &str) -> HashMap<NodeId, f32> {
        let engine = match self.vector_engine.read().await.clone() {
            Some(e) => e,
            None => return HashMap::new(),
        };

        let symbol_vecs = self.symbol_vectors.read().await;
        if symbol_vecs.is_empty() {
            return HashMap::new();
        }

        // Embed the query
        let query_vec = match engine.embed(query) {
            Ok(v) => v,
            Err(_) => return HashMap::new(),
        };

        // Brute-force cosine similarity against all symbol vectors
        let mut scores = HashMap::with_capacity(symbol_vecs.len());
        for (&node_id, symbol_vec) in symbol_vecs.iter() {
            let sim = cosine_similarity(&query_vec, symbol_vec);
            if sim > 0.1 {
                // Skip very low similarity to reduce noise
                scores.insert(node_id, sim);
            }
        }
        scores
    }

    /// Find code by imported libraries/modules.
    pub async fn find_by_imports(
        &self,
        library: &str,
        options: &ImportSearchOptions,
    ) -> Vec<SymbolMatch> {
        let import_index = self.import_index.read().await;
        let graph = self.graph.read().await;

        let mut matching_nodes = Vec::new();

        match options.match_mode {
            ImportMatchMode::Exact => {
                if let Some(nodes) = import_index.get(library) {
                    matching_nodes.extend(nodes.iter().copied());
                }
            }
            ImportMatchMode::Prefix => {
                for (module, nodes) in import_index.iter() {
                    if module.starts_with(library) {
                        matching_nodes.extend(nodes.iter().copied());
                    }
                }
            }
            ImportMatchMode::Fuzzy => {
                let library_lower = library.to_lowercase();
                for (module, nodes) in import_index.iter() {
                    if module.to_lowercase().contains(&library_lower) {
                        matching_nodes.extend(nodes.iter().copied());
                    }
                }
            }
        }

        // Convert to SymbolMatch
        matching_nodes
            .into_iter()
            .filter_map(|node_id| {
                self.node_to_symbol_info(&graph, node_id)
                    .map(|symbol| SymbolMatch {
                        node_id,
                        symbol,
                        score: 1.0, // No ranking for import-based search
                        match_reason: format!("imports {library}"),
                    })
            })
            .collect()
    }

    /// Get direct callers of a function.
    pub async fn get_callers(&self, node_id: NodeId, depth: u32) -> Vec<CallInfo> {
        let caller_index = self.caller_index.read().await;
        let graph = self.graph.read().await;

        self.get_call_chain(&graph, &caller_index, node_id, depth)
    }

    /// Get direct callees of a function.
    pub async fn get_callees(&self, node_id: NodeId, depth: u32) -> Vec<CallInfo> {
        let callee_index = self.callee_index.read().await;
        let graph = self.graph.read().await;

        self.get_call_chain(&graph, &callee_index, node_id, depth)
    }

    /// Traverse the graph from a starting node with filters.
    pub async fn traverse_graph(
        &self,
        start_node: NodeId,
        direction: TraversalDirection,
        max_depth: u32,
        filter: &TraversalFilter,
    ) -> Vec<TraversalNode> {
        let graph = self.graph.read().await;
        let mut results = Vec::new();
        let mut visited = HashSet::new();
        let mut queue: VecDeque<(NodeId, u32, Vec<NodeId>, String)> = VecDeque::new();

        queue.push_back((start_node, 0, vec![start_node], String::new()));
        visited.insert(start_node);

        let codegraph_direction = match direction {
            TraversalDirection::Outgoing => Direction::Outgoing,
            TraversalDirection::Incoming => Direction::Incoming,
            TraversalDirection::Both => Direction::Both,
        };

        while let Some((current, depth, path, incoming_edge_type)) = queue.pop_front() {
            if depth > max_depth {
                break;
            }

            // Phase 1: Add matching nodes to results (skip the start node)
            if depth > 0 && results.len() < filter.max_nodes {
                if let Ok(node) = graph.get_node(current) {
                    let type_matches = filter.symbol_types.is_empty()
                        || filter.symbol_types.iter().any(|st| {
                            matches!(
                                (st, &node.node_type),
                                (SymbolType::Function, NodeType::Function)
                                    | (SymbolType::Class, NodeType::Class)
                                    | (SymbolType::Variable, NodeType::Variable)
                                    | (SymbolType::Module, NodeType::Module)
                                    | (SymbolType::Interface, NodeType::Interface)
                                    | (SymbolType::Type, NodeType::Type)
                            )
                        });

                    if type_matches {
                        if let Some(symbol) = self.node_to_symbol_info(&graph, current) {
                            results.push(TraversalNode {
                                node_id: current,
                                depth,
                                path: path.clone(),
                                edge_type: incoming_edge_type.clone(),
                                symbol,
                            });
                        }
                    }
                }
            }

            // Phase 2: Always expand neighbors (edge type filter still applies)
            if let Ok(neighbors) = graph.get_neighbors(current, codegraph_direction) {
                for neighbor in neighbors {
                    if visited.contains(&neighbor) {
                        continue;
                    }

                    // Resolve all edge types between the two nodes
                    let edge_types_between = Self::resolve_edge_types(&graph, current, neighbor);

                    // Apply edge type filter
                    if !filter.edge_types.is_empty() {
                        let any_match = edge_types_between.iter().any(|et| {
                            filter
                                .edge_types
                                .iter()
                                .any(|ft| ft.eq_ignore_ascii_case(et))
                        });
                        if !any_match {
                            continue;
                        }
                    }

                    // Use first matching edge type for the record
                    let edge_type_str = if filter.edge_types.is_empty() {
                        edge_types_between.into_iter().next().unwrap_or_default()
                    } else {
                        edge_types_between
                            .into_iter()
                            .find(|et| {
                                filter
                                    .edge_types
                                    .iter()
                                    .any(|ft| ft.eq_ignore_ascii_case(et))
                            })
                            .unwrap_or_default()
                    };

                    visited.insert(neighbor);
                    let mut new_path = path.clone();
                    new_path.push(neighbor);
                    queue.push_back((neighbor, depth + 1, new_path, edge_type_str));
                }
            }
        }

        results
    }

    /// Resolve all edge types between two nodes, checking both directions.
    fn resolve_edge_types(
        graph: &CodeGraph,
        a: codegraph::NodeId,
        b: codegraph::NodeId,
    ) -> Vec<String> {
        let mut types = Vec::new();
        if let Ok(edges) = graph.get_edges_between(a, b) {
            for eid in edges {
                if let Ok(edge) = graph.get_edge(eid) {
                    types.push(edge.edge_type.to_string());
                }
            }
        }
        if types.is_empty() {
            if let Ok(edges) = graph.get_edges_between(b, a) {
                for eid in edges {
                    if let Ok(edge) = graph.get_edge(eid) {
                        types.push(edge.edge_type.to_string());
                    }
                }
            }
        }
        types
    }

    /// Get detailed information about a symbol.
    pub async fn get_symbol_info(&self, node_id: NodeId) -> Option<DetailedSymbolInfo> {
        let graph = self.graph.read().await;
        let caller_index = self.caller_index.read().await;
        let callee_index = self.callee_index.read().await;

        let node = graph.get_node(node_id).ok()?;
        let symbol = self.node_to_symbol_info(&graph, node_id)?;

        // Get callers and callees
        let callers = self.get_call_chain(&graph, &caller_index, node_id, 1);
        let callees = self.get_call_chain(&graph, &callee_index, node_id, 1);

        // Count references
        let reference_count = graph
            .get_neighbors(node_id, Direction::Incoming)
            .map(|n| n.len())
            .unwrap_or(0);

        // Get complexity if available
        let complexity = node.properties.get_int("complexity").map(|c| c as u32);

        // Get lines of code
        let lines_of_code = {
            let start_line = node_props::line_start(node) as i64;
            let end_line = node_props::line_end(node) as i64;
            (end_line - start_line + 1).max(1) as usize
        };

        // Check if public — fall back to visibility string when booleans are absent
        let is_public = node_props::is_public(node);

        // Check if deprecated
        let is_deprecated = node.properties.get_bool("deprecated").unwrap_or(false);

        // Collect dependencies (outgoing import edges)
        let mut dependencies = Vec::new();
        if let Ok(neighbors) = graph.get_neighbors(node_id, Direction::Outgoing) {
            for neighbor_id in neighbors {
                if let Ok(edges) = graph.get_edges_between(node_id, neighbor_id) {
                    let is_import = edges.iter().any(|eid| {
                        graph.get_edge(*eid).is_ok_and(|e| {
                            matches!(e.edge_type, EdgeType::Imports | EdgeType::ImportsFrom)
                        })
                    });
                    if is_import {
                        if let Ok(target) = graph.get_node(neighbor_id) {
                            let name = node_props::name(target).to_string();
                            if !name.is_empty() && !dependencies.contains(&name) {
                                dependencies.push(name);
                            }
                        }
                    }
                }
            }
        }

        // Collect dependents (incoming import edges)
        let mut dependents = Vec::new();
        if let Ok(neighbors) = graph.get_neighbors(node_id, Direction::Incoming) {
            for neighbor_id in neighbors {
                if let Ok(edges) = graph.get_edges_between(neighbor_id, node_id) {
                    let is_import = edges.iter().any(|eid| {
                        graph.get_edge(*eid).is_ok_and(|e| {
                            matches!(e.edge_type, EdgeType::Imports | EdgeType::ImportsFrom)
                        })
                    });
                    if is_import {
                        if let Ok(source) = graph.get_node(neighbor_id) {
                            let name = node_props::name(source).to_string();
                            if !name.is_empty() && !dependents.contains(&name) {
                                dependents.push(name);
                            }
                        }
                    }
                }
            }
        }

        // Detect test associations by checking if any caller is a test node
        let has_tests = callers.iter().any(|caller| {
            graph.get_node(caller.node_id).is_ok_and(|n| {
                let name = node_props::name(n);
                let path = node_props::path(n);
                name.starts_with("test_")
                    || name.ends_with("_test")
                    || name.contains("test ")
                    || path.contains("/test")
                    || path.contains("/tests")
            })
        });

        Some(DetailedSymbolInfo {
            symbol,
            callers,
            callees,
            dependencies,
            dependents,
            complexity,
            lines_of_code,
            has_tests,
            is_public,
            is_deprecated,
            reference_count,
        })
    }

    /// Find functions by signature patterns.
    pub async fn find_by_signature(
        &self,
        pattern: &SignaturePattern,
        limit: Option<usize>,
    ) -> Vec<SymbolMatch> {
        let graph = self.graph.read().await;
        let mut results = Vec::new();

        // Compile regex from name pattern, converting glob wildcards to regex
        let name_regex = pattern.name_pattern.as_ref().and_then(|p| {
            let regex_str = Self::glob_to_anchored_regex(p);
            regex::Regex::new(&regex_str).ok()
        });

        // Iterate over all function nodes using iter_nodes()
        for (node_id, node) in graph.iter_nodes() {
            // Only check functions
            if node.node_type != NodeType::Function {
                continue;
            }

            let name = node_props::name(node);

            // Check name pattern
            if let Some(ref regex) = name_regex {
                if !regex.is_match(name) {
                    continue;
                }
            }

            let signature = node.properties.get_string("signature").unwrap_or("");

            // Check return type
            if let Some(ref expected_return) = pattern.return_type {
                let actual_return = node.properties.get_string("return_type").unwrap_or("");
                // Fall back to extracting return type from signature
                let actual_return = if actual_return.is_empty() {
                    Self::extract_return_type_from_signature(signature)
                } else {
                    actual_return.to_string()
                };
                if !self.type_matches(&actual_return, expected_return) {
                    continue;
                }
            }

            // Check parameter count
            if let Some((min, max)) = pattern.param_count {
                // Try stored param_count first, fall back to parsing from signature
                let param_count = if let Some(count) = node.properties.get_int("param_count") {
                    count as usize
                } else {
                    Self::count_params_from_signature(signature)
                };
                if param_count < min || param_count > max {
                    continue;
                }
            }

            // Check modifiers
            if !pattern.modifiers.is_empty() {
                let visibility = node.properties.get_string("visibility").unwrap_or("");
                let mut all_modifiers_match = true;
                for modifier in &pattern.modifiers {
                    let has_modifier = match modifier.as_str() {
                        "async" => node.properties.get_bool("is_async").unwrap_or(false),
                        "public" | "pub" => {
                            node.properties
                                .get_bool("is_public")
                                .or_else(|| node.properties.get_bool("exported"))
                                .unwrap_or(false)
                                || visibility == "public"
                                || visibility == "pub"
                        }
                        "private" => {
                            visibility == "private"
                                || (!node
                                    .properties
                                    .get_bool("is_public")
                                    .or_else(|| node.properties.get_bool("exported"))
                                    .unwrap_or(false)
                                    && visibility != "public"
                                    && visibility != "pub"
                                    && !visibility.is_empty())
                        }
                        "protected" => visibility == "protected",
                        "static" => node.properties.get_bool("is_static").unwrap_or(false),
                        "const" => node.properties.get_bool("is_const").unwrap_or(false),
                        _ => false,
                    };
                    if !has_modifier {
                        all_modifiers_match = false;
                        break;
                    }
                }
                if !all_modifiers_match {
                    continue;
                }
            }

            // Build symbol info for matching function
            if let Some(symbol) = self.node_to_symbol_info(&graph, node_id) {
                let match_reason = self.build_signature_match_reason(pattern);
                results.push(SymbolMatch {
                    node_id,
                    symbol,
                    score: 1.0, // All matches are equally relevant for signature search
                    match_reason,
                });

                // Check limit and return early if reached
                if let Some(max) = limit {
                    if results.len() >= max {
                        return results;
                    }
                }
            }
        }

        results
    }

    /// Check if actual type matches expected type pattern.
    fn type_matches(&self, actual: &str, expected: &str) -> bool {
        // Handle exact match
        if actual == expected {
            return true;
        }

        // Handle primitive type aliases
        let actual_normalized = match actual.to_lowercase().as_str() {
            "boolean" => "bool",
            "integer" | "int" | "i32" | "i64" => "int",
            "string" | "str" | "&str" => "string",
            "void" | "()" | "none" | "null" => "void",
            _ => actual,
        };

        let expected_normalized = match expected.to_lowercase().as_str() {
            "boolean" => "bool",
            "integer" | "int" | "i32" | "i64" => "int",
            "string" | "str" | "&str" => "string",
            "void" | "()" | "none" | "null" => "void",
            _ => expected,
        };

        if actual_normalized == expected_normalized {
            return true;
        }

        // Handle wildcard patterns (e.g., "Result<*, *>")
        if expected.contains('*') {
            let pattern = expected.replace('*', ".*");
            if let Ok(regex) = regex::Regex::new(&format!("^{pattern}$")) {
                return regex.is_match(actual);
            }
        }

        // Handle generic type prefix matching: "Result" matches "Result<T, E>"
        if actual.starts_with(expected) && actual[expected.len()..].starts_with('<') {
            return true;
        }

        // Case-insensitive prefix matching for the base type
        let actual_base = actual.split('<').next().unwrap_or(actual).trim();
        let expected_base = expected.split('<').next().unwrap_or(expected).trim();
        if !actual_base.is_empty()
            && !expected_base.is_empty()
            && actual_base.eq_ignore_ascii_case(expected_base)
        {
            return true;
        }

        false
    }

    /// Convert a name pattern to an anchored regex.
    /// Detects whether the input is a glob pattern or regex:
    /// - Glob: standalone `*` (not preceded by `.`), `?` wildcards
    /// - Regex: `.*`, `\w`, `[`, `(`, `|`, `+`, `{`
    ///
    /// Both get anchored to match the full name.
    fn glob_to_anchored_regex(pattern: &str) -> String {
        // Detect if this is regex syntax (contains regex-specific constructs)
        let is_regex = pattern.contains(".*")
            || pattern.contains("\\w")
            || pattern.contains("\\d")
            || pattern.contains('(')
            || pattern.contains('|')
            || pattern.contains('[')
            || pattern.contains('+')
            || pattern.contains('{');

        if is_regex {
            // Already regex — just anchor it if not already anchored
            let anchored = if pattern.starts_with('^') && pattern.ends_with('$') {
                pattern.to_string()
            } else if pattern.starts_with('^') {
                format!("{pattern}$")
            } else if pattern.ends_with('$') {
                format!("^{pattern}")
            } else {
                format!("^(?:{pattern})$")
            };
            return anchored;
        }

        // Glob mode: convert glob wildcards to regex
        let mut regex = String::with_capacity(pattern.len() + 4);
        regex.push('^');
        for ch in pattern.chars() {
            match ch {
                '*' => regex.push_str(".*"),
                '?' => regex.push('.'),
                '.' | '^' | '$' | '\\' | '/' => {
                    regex.push('\\');
                    regex.push(ch);
                }
                _ => regex.push(ch),
            }
        }
        regex.push('$');
        regex
    }

    /// Count parameters from a function signature string.
    /// Handles `fn foo()` (0 params), `fn foo(a: i32)` (1 param),
    /// `fn foo(a: i32, b: String)` (2 params), etc.
    /// Also handles `self`/`&self`/`&mut self` — not counted as params.
    fn count_params_from_signature(signature: &str) -> usize {
        // Find the parameter list between first ( and matching )
        let paren_start = match signature.find('(') {
            Some(pos) => pos,
            None => return 0,
        };

        // Find matching closing paren, accounting for nested parens/generics
        let chars: Vec<char> = signature.chars().collect();
        let mut depth = 0;
        let mut paren_end = None;
        for (i, &ch) in chars.iter().enumerate().skip(paren_start) {
            match ch {
                '(' | '<' => depth += 1,
                ')' | '>' => {
                    depth -= 1;
                    if depth == 0 && ch == ')' {
                        paren_end = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }

        let paren_end = match paren_end {
            Some(pos) => pos,
            None => return 0,
        };

        let params_str: String = chars[paren_start + 1..paren_end].iter().collect();
        let params_str = params_str.trim();

        if params_str.is_empty() {
            return 0;
        }

        // Split by commas at top-level (not inside <> or ())
        let mut count: usize = 0;
        let mut depth = 0;
        let mut has_content = false;
        for ch in params_str.chars() {
            match ch {
                '<' | '(' | '[' => depth += 1,
                '>' | ')' | ']' => depth -= 1,
                ',' if depth == 0 => {
                    if has_content {
                        count += 1;
                    }
                    has_content = false;
                    continue;
                }
                _ if !ch.is_whitespace() => has_content = true,
                _ => {}
            }
        }
        if has_content {
            count += 1;
        }

        // Subtract self/&self/&mut self (Rust methods)
        let first_param = params_str.split(',').next().unwrap_or("").trim();
        if first_param == "self"
            || first_param == "&self"
            || first_param == "&mut self"
            || first_param.starts_with("self:")
        {
            count = count.saturating_sub(1);
        }

        count
    }

    /// Extract return type from a function signature string.
    fn extract_return_type_from_signature(signature: &str) -> String {
        // Rust: `fn foo(args) -> ReturnType`
        if let Some(pos) = signature.rfind("->") {
            let ret = signature[pos + 2..].trim();
            // Strip trailing braces/semicolons
            let ret = ret.trim_end_matches(|c: char| c == '{' || c == ';' || c.is_whitespace());
            if !ret.is_empty() {
                return ret.to_string();
            }
        }

        // TypeScript/Java: `function foo(args): ReturnType` or type annotation after `)`
        // Find closing paren, then look for `: Type`
        if let Some(paren_pos) = signature.rfind(')') {
            let after_paren = &signature[paren_pos + 1..];
            if let Some(colon_pos) = after_paren.find(':') {
                let ret = after_paren[colon_pos + 1..].trim();
                let ret = ret.trim_end_matches(|c: char| c == '{' || c == ';' || c.is_whitespace());
                if !ret.is_empty() {
                    return ret.to_string();
                }
            }
        }

        String::new()
    }

    /// Build a human-readable match reason for signature search.
    fn build_signature_match_reason(&self, pattern: &SignaturePattern) -> String {
        let mut parts = Vec::new();

        if let Some(ref name_pattern) = pattern.name_pattern {
            parts.push(format!("name matches /{name_pattern}/"));
        }
        if let Some(ref return_type) = pattern.return_type {
            parts.push(format!("returns {return_type}"));
        }
        if let Some((min, max)) = pattern.param_count {
            if min == max {
                parts.push(format!("{min} parameters"));
            } else {
                parts.push(format!("{min}-{max} parameters"));
            }
        }
        if !pattern.modifiers.is_empty() {
            let mods = pattern.modifiers.join(", ");
            parts.push(format!("modifiers: {mods}"));
        }

        if parts.is_empty() {
            "Signature match".to_string()
        } else {
            parts.join(", ")
        }
    }

    /// Find entry points in the codebase.
    pub async fn find_entry_points(&self, entry_types: &[EntryType]) -> Vec<EntryPoint> {
        self.find_entry_points_opts(entry_types, false, None).await
    }

    /// Find entry points with compact option and optional limit.
    pub async fn find_entry_points_opts(
        &self,
        entry_types: &[EntryType],
        compact: bool,
        limit: Option<usize>,
    ) -> Vec<EntryPoint> {
        let graph = self.graph.read().await;
        let mut results = Vec::new();

        // Iterate over all nodes using iter_nodes()
        for (node_id, node) in graph.iter_nodes() {
            // Only check functions
            if node.node_type != NodeType::Function {
                continue;
            }

            let name = node_props::name(node);

            // Detect entry type
            let entry_type = self.detect_entry_type(node, name);

            if let Some(et) = entry_type {
                // Filter by requested entry types
                if entry_types.is_empty() || entry_types.contains(&et) {
                    if let Some(symbol) = self.node_to_symbol_info_opts(&graph, node_id, compact) {
                        // In compact mode, also truncate description
                        let description = if compact {
                            None
                        } else {
                            node.properties
                                .get_string("doc")
                                .map(|s| truncate_string(s, MAX_SIGNATURE_LENGTH))
                        };

                        results.push(EntryPoint {
                            node_id,
                            entry_type: et,
                            route: node.properties.get_string("route").map(|s| s.to_string()),
                            method: node
                                .properties
                                .get_string("http_method")
                                .map(|s| s.to_string()),
                            description,
                            symbol,
                        });

                        // Check limit and return early if reached
                        if let Some(max) = limit {
                            if results.len() >= max {
                                return results;
                            }
                        }
                    }
                }
            }
        }

        results
    }

    // Helper methods

    /// Convert a node to SymbolInfo with default options (truncated signatures)
    fn node_to_symbol_info(&self, graph: &CodeGraph, node_id: NodeId) -> Option<SymbolInfo> {
        self.node_to_symbol_info_opts(graph, node_id, false)
    }

    /// Convert a node to SymbolInfo with options
    /// - compact: if true, omit signature and docstring entirely
    /// - if false, truncate signature to MAX_SIGNATURE_LENGTH
    fn node_to_symbol_info_opts(
        &self,
        graph: &CodeGraph,
        node_id: NodeId,
        compact: bool,
    ) -> Option<SymbolInfo> {
        let node = graph.get_node(node_id).ok()?;

        let name = node.properties.get_string("name")?.to_string();
        let kind = format!("{}", node.node_type);

        // Use canonical property accessors (with fallback for old-style keys)
        let line = node_props::line_start(node).max(1);
        let column = node_props::col_start_from_props(&node.properties);
        let end_line = {
            let e = node_props::line_end(node);
            if e == 0 {
                line
            } else {
                e
            }
        };
        let end_column = node_props::col_end_from_props(&node.properties);

        let file = node_props::path(node).to_string();

        let location = SymbolLocation {
            file,
            line,
            column,
            end_line,
            end_column,
        };

        // In compact mode, omit signature and docstring
        // Otherwise, truncate signature to prevent huge responses
        let (signature, docstring) = if compact {
            (None, None)
        } else {
            let sig = node
                .properties
                .get_string("signature")
                .map(|s| truncate_string(s, MAX_SIGNATURE_LENGTH));
            let doc = node
                .properties
                .get_string("doc")
                .map(|s| truncate_string(s, MAX_SIGNATURE_LENGTH));
            (sig, doc)
        };

        let is_public = node_props::is_public(node);

        let visibility = node
            .properties
            .get_string("visibility")
            .unwrap_or(if is_public { "public" } else { "private" })
            .to_string();

        Some(SymbolInfo {
            name,
            kind,
            location,
            signature,
            docstring,
            is_public,
            visibility,
        })
    }

    fn get_call_chain(
        &self,
        graph: &CodeGraph,
        index: &HashMap<NodeId, Vec<NodeId>>,
        start: NodeId,
        max_depth: u32,
    ) -> Vec<CallInfo> {
        let mut results = Vec::new();
        let mut visited = HashSet::new();
        let mut queue: VecDeque<(NodeId, u32)> = VecDeque::new();

        if let Some(direct) = index.get(&start) {
            for &node_id in direct {
                queue.push_back((node_id, 1));
            }
        }

        while let Some((current, depth)) = queue.pop_front() {
            if depth > max_depth || visited.contains(&current) {
                continue;
            }
            visited.insert(current);

            if let Some(symbol) = self.node_to_symbol_info(graph, current) {
                // Check edge properties for ops struct metadata
                let (via_ops_struct, ops_field) =
                    Self::get_ops_struct_from_edge(graph, current, start);

                results.push(CallInfo {
                    node_id: current,
                    symbol: symbol.clone(),
                    call_site: symbol.location.clone(),
                    depth,
                    via_ops_struct,
                    ops_field,
                });
            }

            // Continue to next depth if needed
            if depth < max_depth {
                if let Some(next_level) = index.get(&current) {
                    for &node_id in next_level {
                        if !visited.contains(&node_id) {
                            queue.push_back((node_id, depth + 1));
                        }
                    }
                }
            }
        }

        results
    }

    /// Look up ops struct metadata from the Calls edge between two nodes.
    fn get_ops_struct_from_edge(
        graph: &CodeGraph,
        source: NodeId,
        target: NodeId,
    ) -> (Option<String>, Option<String>) {
        // Check both directions — for callers, edge is source→target.
        // For callees (same function reused), edge is target→source.
        for (s, t) in [(source, target), (target, source)] {
            if let Ok(edge_ids) = graph.get_edges_between(s, t) {
                for edge_id in edge_ids {
                    if let Ok(edge) = graph.get_edge(edge_id) {
                        if edge.edge_type == codegraph::EdgeType::Calls {
                            let st = edge
                                .properties
                                .get_string("struct_type")
                                .map(|s| s.to_string());
                            let field = edge
                                .properties
                                .get_string("field_name")
                                .map(|s| s.to_string());
                            if st.is_some() || field.is_some() {
                                return (st, field);
                            }
                        }
                    }
                }
            }
        }
        (None, None)
    }

    /// Find all functions that implement a given ops struct field.
    ///
    /// Scans all Calls edges for `struct_type` and `field_name` properties.
    /// If `struct_type` is provided, filters by struct. If `field_name` is
    /// provided, filters by field. Returns matching functions with their
    /// struct/field context.
    pub async fn find_implementors(
        &self,
        struct_type: Option<&str>,
        field_name: Option<&str>,
    ) -> Vec<ImplementorInfo> {
        let graph = self.graph.read().await;
        let mut results = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for (_, edge) in graph.iter_edges() {
            if edge.edge_type != codegraph::EdgeType::Calls {
                continue;
            }

            let edge_struct = edge.properties.get_string("struct_type");
            let edge_field = edge.properties.get_string("field_name");

            // Skip edges without ops struct metadata
            if edge_struct.is_none() {
                continue;
            }

            // Filter by struct_type if provided
            if let Some(st) = struct_type {
                if edge_struct.map(|s| s != st).unwrap_or(true) {
                    continue;
                }
            }

            // Filter by field_name if provided
            if let Some(fn_name) = field_name {
                if edge_field.map(|f| f != fn_name).unwrap_or(true) {
                    continue;
                }
            }

            // Get the target function (the implementor)
            let target_id = edge.target_id;
            if !seen.insert((
                target_id,
                edge_struct.map(|s| s.to_string()),
                edge_field.map(|f| f.to_string()),
            )) {
                continue;
            }

            if let Some(symbol) = self.node_to_symbol_info(&graph, target_id) {
                results.push(ImplementorInfo {
                    node_id: target_id,
                    symbol,
                    struct_type: edge_struct.map(|s| s.to_string()).unwrap_or_default(),
                    field_name: edge_field.map(|f| f.to_string()).unwrap_or_default(),
                });
            }
        }

        // Sort by struct_type, then field_name for consistent output
        results.sort_by(|a, b| {
            a.struct_type
                .cmp(&b.struct_type)
                .then(a.field_name.cmp(&b.field_name))
                .then(a.symbol.name.cmp(&b.symbol.name))
        });

        results
    }

    fn detect_entry_type(&self, node: &codegraph::Node, name: &str) -> Option<EntryType> {
        let name_lower = name.to_lowercase();

        // Check for HTTP handlers
        if node.properties.get_string("route").is_some()
            || node.properties.get_string("http_method").is_some()
        {
            return Some(EntryType::HttpHandler);
        }

        // Check for main function
        if name == "main" || name == "__main__" {
            return Some(EntryType::Main);
        }

        // Check for test functions (by name, property, or file path)
        if name_lower.starts_with("test_")
            || name_lower.ends_with("_test")
            || name.starts_with("Test")
            || node.properties.get_bool("is_test").unwrap_or(false)
        {
            return Some(EntryType::TestEntry);
        }
        // Check if function lives in a test file (path-based detection)
        let path = node_props::path(node);
        if path.contains("/test/")
            || path.contains("/tests/")
            || path.contains("/__tests__/")
            || path.contains(".test.")
            || path.contains(".spec.")
            || path.contains("_test.")
        {
            return Some(EntryType::TestEntry);
        }

        // Check for CLI commands
        if name_lower.contains("command")
            || name_lower.contains("cli")
            || node.properties.get_bool("is_cli").unwrap_or(false)
        {
            return Some(EntryType::CliCommand);
        }

        // Check for event handlers
        if name_lower.starts_with("on_")
            || name_lower.starts_with("handle_")
            || name_lower.ends_with("_handler")
            || name_lower.ends_with("_callback")
        {
            return Some(EntryType::EventHandler);
        }

        // Check for public API (exported functions)
        if node_props::is_public(node) {
            return Some(EntryType::PublicApi);
        }

        None
    }

    /// Find duplicate/similar functions across the codebase.
    ///
    /// Compares all function embedding vectors pairwise and returns pairs
    /// above the similarity threshold. With Jina Code V2, a threshold of
    /// 0.70 reliably identifies clones while filtering noise.
    pub async fn find_duplicates(
        &self,
        threshold: f32,
        limit: usize,
        uri_filter: Option<&str>,
    ) -> DuplicateResult {
        let start = Instant::now();
        let symbol_vecs = self.symbol_vectors.read().await;
        let graph = self.graph.read().await;

        // Collect function vectors, optionally filtered by file
        let entries: Vec<(NodeId, &Vec<f32>)> = symbol_vecs
            .iter()
            .filter(|(node_id, _)| {
                let Ok(node) = graph.get_node(**node_id) else {
                    return false;
                };
                if node.node_type != NodeType::Function {
                    return false;
                }
                if let Some(filter) = uri_filter {
                    let path = node_props::path(node);
                    path.contains(filter)
                } else {
                    true
                }
            })
            .map(|(id, vec)| (*id, vec))
            .collect();

        let total_symbols = entries.len();
        let mut pairs: Vec<DuplicatePair> = Vec::new();

        // Pairwise comparison (O(n²) but n is typically <5000)
        for i in 0..entries.len() {
            for j in (i + 1)..entries.len() {
                let sim = cosine_similarity(entries[i].1, entries[j].1);
                if sim >= threshold {
                    let node_a = graph.get_node(entries[i].0);
                    let node_b = graph.get_node(entries[j].0);
                    if let (Ok(na), Ok(nb)) = (node_a, node_b) {
                        // Skip common trait impl names (default, new, from, etc.)
                        let name_a = node_props::name(na).to_lowercase();
                        let name_b = node_props::name(nb).to_lowercase();
                        if BOILERPLATE_NAMES.contains(&name_a.as_str())
                            && BOILERPLATE_NAMES.contains(&name_b.as_str())
                        {
                            continue;
                        }

                        // Skip pairs in the same file at similar lines (likely the same function)
                        let path_a = node_props::path(na);
                        let path_b = node_props::path(nb);
                        let line_a = node_props::line_start(na);
                        let line_b = node_props::line_start(nb);
                        if path_a == path_b && (line_a as i64 - line_b as i64).unsigned_abs() < 5 {
                            continue;
                        }

                        if let (Some(sym_a), Some(sym_b)) = (
                            self.node_to_symbol_info(&graph, entries[i].0),
                            self.node_to_symbol_info(&graph, entries[j].0),
                        ) {
                            // Canonicalise pair direction: symbol_a always
                            // has the lexicographically smaller name (ties
                            // broken by path + line). `entries` comes from
                            // a HashMap iterator with non-deterministic
                            // order, so without this, (a,b) and (b,a)
                            // appearances flip between runs and break
                            // regression tests.
                            let (sa, sb, ida, idb) = if (
                                sym_a.name.as_str(),
                                sym_a.location.file.as_str(),
                                sym_a.location.line,
                            ) <= (
                                sym_b.name.as_str(),
                                sym_b.location.file.as_str(),
                                sym_b.location.line,
                            ) {
                                (sym_a, sym_b, entries[i].0, entries[j].0)
                            } else {
                                (sym_b, sym_a, entries[j].0, entries[i].0)
                            };
                            pairs.push(DuplicatePair {
                                symbol_a: sa,
                                node_id_a: ida,
                                symbol_b: sb,
                                node_id_b: idb,
                                similarity: sim,
                            });
                        }
                    }
                }
            }
        }

        // Sort by similarity descending
        pairs.sort_by(|a, b| {
            b.similarity
                .partial_cmp(&a.similarity)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        pairs.truncate(limit);

        DuplicateResult {
            pairs,
            total_symbols_compared: total_symbols,
            threshold,
            query_time_ms: start.elapsed().as_millis() as u64,
        }
    }

    /// Find functions most similar to a given function.
    ///
    /// Returns the top N functions closest to the target in embedding space.
    /// Useful for "is there already a function that does this?" checks.
    pub async fn find_similar(&self, target_node_id: NodeId, limit: usize) -> DuplicateResult {
        let start = Instant::now();
        let symbol_vecs = self.symbol_vectors.read().await;
        let graph = self.graph.read().await;

        let target_vec = match symbol_vecs.get(&target_node_id) {
            Some(v) => v.clone(),
            None => {
                return DuplicateResult {
                    pairs: Vec::new(),
                    total_symbols_compared: 0,
                    threshold: 0.0,
                    query_time_ms: start.elapsed().as_millis() as u64,
                };
            }
        };

        let target_node = graph.get_node(target_node_id).ok();

        let mut scored: Vec<(NodeId, f32)> = symbol_vecs
            .iter()
            .filter(|(id, _)| **id != target_node_id)
            .filter(|(id, _)| {
                graph
                    .get_node(**id)
                    .map(|n| n.node_type == NodeType::Function)
                    .unwrap_or(false)
            })
            .map(|(id, vec)| (*id, cosine_similarity(&target_vec, vec)))
            .filter(|(_, sim)| *sim > 0.1)
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);

        let pairs: Vec<DuplicatePair> = scored
            .into_iter()
            .filter_map(|(node_id, sim)| {
                // Verify both nodes still exist
                graph.get_node(node_id).ok()?;
                target_node?;
                let sym_a = self.node_to_symbol_info(&graph, target_node_id)?;
                let sym_b = self.node_to_symbol_info(&graph, node_id)?;
                Some(DuplicatePair {
                    symbol_a: sym_a,
                    node_id_a: target_node_id,
                    symbol_b: sym_b,
                    node_id_b: node_id,
                    similarity: sim,
                })
            })
            .collect();

        let total = symbol_vecs.len();
        DuplicateResult {
            pairs,
            total_symbols_compared: total,
            threshold: 0.0,
            query_time_ms: start.elapsed().as_millis() as u64,
        }
    }

    /// Cluster functions into semantic groups using greedy clustering.
    ///
    /// Picks an unassigned function, finds all functions within `threshold`
    /// similarity, forms a cluster, repeats. Labels clusters by the most
    /// central member's name.
    pub async fn cluster_symbols(
        &self,
        threshold: f32,
        min_cluster_size: usize,
        limit: usize,
    ) -> ClusterResult {
        let start = Instant::now();
        let symbol_vecs = self.symbol_vectors.read().await;
        let graph = self.graph.read().await;

        // Collect function vectors, excluding boilerplate names
        let entries: Vec<(NodeId, &Vec<f32>)> = symbol_vecs
            .iter()
            .filter(|(node_id, _)| {
                let Ok(node) = graph.get_node(**node_id) else {
                    return false;
                };
                if node.node_type != NodeType::Function {
                    return false;
                }
                let name = node_props::name(node).to_lowercase();
                !BOILERPLATE_NAMES.contains(&name.as_str())
            })
            .map(|(id, vec)| (*id, vec))
            .collect();

        let total_symbols = entries.len();
        let mut assigned = vec![false; entries.len()];
        let mut clusters: Vec<SymbolCluster> = Vec::new();

        for i in 0..entries.len() {
            if assigned[i] {
                continue;
            }

            // Find all unassigned entries similar to entries[i]
            let mut members = vec![i];
            for j in (i + 1)..entries.len() {
                if assigned[j] {
                    continue;
                }
                let sim = cosine_similarity(entries[i].1, entries[j].1);
                if sim >= threshold {
                    members.push(j);
                }
            }

            if members.len() < min_cluster_size {
                continue;
            }

            // Mark as assigned
            for &idx in &members {
                assigned[idx] = true;
            }

            // Build cluster
            let cluster_members: Vec<ClusterMember> = members
                .iter()
                .filter_map(|&idx| {
                    let node = graph.get_node(entries[idx].0).ok()?;
                    let sim = cosine_similarity(entries[i].1, entries[idx].1);
                    Some(ClusterMember {
                        node_id: entries[idx].0,
                        name: node_props::name(node).to_string(),
                        file: node_props::path(node).to_string(),
                        line: node_props::line_start(node),
                        similarity_to_centroid: sim,
                    })
                })
                .collect();

            let label = cluster_members
                .first()
                .map(|m| m.name.clone())
                .unwrap_or_else(|| "unknown".to_string());

            let size = cluster_members.len();
            clusters.push(SymbolCluster {
                label,
                members: cluster_members,
                size,
            });
        }

        // Sort by size descending
        clusters.sort_by(|a, b| b.size.cmp(&a.size));
        clusters.truncate(limit);

        let unclustered = assigned.iter().filter(|&&a| !a).count();

        ClusterResult {
            clusters,
            total_symbols,
            unclustered,
            query_time_ms: start.elapsed().as_millis() as u64,
        }
    }

    /// Compare two functions structurally and semantically.
    pub async fn compare_symbols(
        &self,
        node_id_a: NodeId,
        node_id_b: NodeId,
    ) -> Option<SymbolComparison> {
        let symbol_vecs = self.symbol_vectors.read().await;
        let graph = self.graph.read().await;

        let node_a = graph.get_node(node_id_a).ok()?;
        let node_b = graph.get_node(node_id_b).ok()?;

        let sym_a = self.node_to_symbol_info(&graph, node_id_a)?;
        let sym_b = self.node_to_symbol_info(&graph, node_id_b)?;

        // Semantic similarity
        let similarity = match (symbol_vecs.get(&node_id_a), symbol_vecs.get(&node_id_b)) {
            (Some(va), Some(vb)) => cosine_similarity(va, vb),
            _ => 0.0,
        };

        // Structural comparison
        let path_a = node_props::path(node_a);
        let path_b = node_props::path(node_b);
        let lang_a = path_a.rsplit('.').next().unwrap_or("");
        let lang_b = path_b.rsplit('.').next().unwrap_or("");

        let complexity_a = node_a
            .properties
            .get_int("complexity_cyclomatic")
            .unwrap_or(0) as u32;
        let complexity_b = node_b
            .properties
            .get_int("complexity_cyclomatic")
            .unwrap_or(0) as u32;

        let structural = StructuralComparison {
            same_file: path_a == path_b,
            same_language: lang_a == lang_b,
            complexity_a,
            complexity_b,
            lines_a: node_props::line_end(node_a).saturating_sub(node_props::line_start(node_a))
                + 1,
            lines_b: node_props::line_end(node_b).saturating_sub(node_props::line_start(node_b))
                + 1,
            param_count_a: node_a
                .properties
                .get_string_list_compat("parameters")
                .map(|p| p.len())
                .unwrap_or(0),
            param_count_b: node_b
                .properties
                .get_string_list_compat("parameters")
                .map(|p| p.len())
                .unwrap_or(0),
        };

        // Find shared callers/callees by scanning all edges
        let mut callers_a = std::collections::HashSet::new();
        let mut callers_b = std::collections::HashSet::new();
        let mut callees_a = std::collections::HashSet::new();
        let mut callees_b = std::collections::HashSet::new();

        for (_eid, edge) in graph.iter_edges() {
            if edge.edge_type != EdgeType::Calls {
                continue;
            }
            if let Ok(src) = graph.get_node(edge.source_id) {
                let src_name = node_props::name(src).to_string();
                if edge.target_id == node_id_a {
                    callers_a.insert(src_name.clone());
                }
                if edge.target_id == node_id_b {
                    callers_b.insert(src_name);
                }
            }
            if let Ok(tgt) = graph.get_node(edge.target_id) {
                let tgt_name = node_props::name(tgt).to_string();
                if edge.source_id == node_id_a {
                    callees_a.insert(tgt_name.clone());
                }
                if edge.source_id == node_id_b {
                    callees_b.insert(tgt_name);
                }
            }
        }

        let shared_callers: Vec<String> = callers_a.intersection(&callers_b).cloned().collect();
        let shared_callees: Vec<String> = callees_a.intersection(&callees_b).cloned().collect();

        // Generate verdict
        let verdict = if similarity > 0.9 {
            "Near-identical: likely copy-paste clone. Consider extracting shared function."
                .to_string()
        } else if similarity > 0.7 {
            "Highly similar: same algorithm with different details. Review for consolidation."
                .to_string()
        } else if similarity > 0.5 {
            "Moderately similar: related functionality but distinct implementations.".to_string()
        } else if similarity > 0.3 {
            "Loosely related: some conceptual overlap but different purposes.".to_string()
        } else {
            "Unrelated: different functionality despite any naming similarity.".to_string()
        };

        Some(SymbolComparison {
            symbol_a: sym_a,
            symbol_b: sym_b,
            similarity,
            verdict,
            structural,
            shared_callers,
            shared_callees,
        })
    }
}

/// Common boilerplate function names across all supported languages.
/// These produce false positive duplicates/clusters because they share identical
/// signatures despite being unrelated implementations.
const BOILERPLATE_NAMES: &[&str] = &[
    // Rust traits
    "default",
    "new",
    "from",
    "fmt",
    "clone",
    "drop",
    "deref",
    "eq",
    "hash",
    "into",
    "try_from",
    "try_into",
    "serialize",
    "deserialize",
    // Java/Kotlin/C#
    "tostring",
    "hashcode",
    "equals",
    "compareto",
    "getclass",
    "finalize",
    "dispose",
    "close",
    "gettype",
    "gethashcode",
    // Python
    "__init__",
    "__str__",
    "__repr__",
    "__eq__",
    "__hash__",
    "__len__",
    "__iter__",
    "__next__",
    "__enter__",
    "__exit__",
    "__del__",
    // JavaScript/TypeScript
    "constructor",
    "tostring",
    "valueof",
    "tolocalestring",
    "tojson",
    // C/C++
    "main",
    "init",
    "destroy",
    "free",
    "malloc",
    "realloc",
    // Go
    "string",
    "error",
    "len",
    "close",
    // PHP
    "__construct",
    "__destruct",
    "__tostring",
    "__clone",
    "__get",
    "__set",
    // Ruby
    "initialize",
    "to_s",
    "to_str",
    "inspect",
    "hash",
    "eql?",
    // Swift
    "init",
    "deinit",
    "description",
    // Common getters/setters (all languages)
    "get",
    "set",
    "getvalue",
    "setvalue",
];

/// Cosine similarity between two vectors. Returns 0.0 for zero-length vectors.
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a * norm_b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph::PropertyMap;

    async fn create_test_engine() -> (QueryEngine, Arc<RwLock<CodeGraph>>) {
        let graph = Arc::new(RwLock::new(
            CodeGraph::in_memory().expect("Failed to create in-memory graph"),
        ));
        let engine = QueryEngine::new(Arc::clone(&graph));
        (engine, graph)
    }

    #[test]
    fn embed_memory_pressured_low_reading_is_pressure() {
        assert!(embed_memory_pressured(1));
        assert!(embed_memory_pressured(EMBED_LOW_MEM_MB - 1));
    }

    #[test]
    fn embed_memory_pressured_zero_reading_is_detection_failure_not_pressure() {
        assert!(!embed_memory_pressured(0));
    }

    #[test]
    fn embed_memory_pressured_at_or_above_floor_is_not_pressure() {
        assert!(!embed_memory_pressured(EMBED_LOW_MEM_MB));
        assert!(!embed_memory_pressured(EMBED_LOW_MEM_MB + 1));
        assert!(!embed_memory_pressured(64 * 1024));
    }

    #[tokio::test]
    async fn test_engine_creation() {
        let (engine, _) = create_test_engine().await;
        // Engine should be created successfully
        let text_index = engine.text_index.read().await;
        assert_eq!(text_index.document_count(), 0);
    }

    #[tokio::test]
    async fn test_symbol_search_empty() {
        let (engine, _) = create_test_engine().await;

        let results = engine.symbol_search("test", &SearchOptions::new()).await;

        assert_eq!(results.results.len(), 0);
        assert_eq!(results.total_matches, 0);
    }

    #[tokio::test]
    async fn test_symbol_search_with_data() {
        let (engine, graph) = create_test_engine().await;

        // Add a function node to the graph
        {
            let mut g = graph.write().await;
            let mut props = PropertyMap::new();
            props.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("validateEmail".to_string()),
            );
            props.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/test.rs".to_string()),
            );
            props.insert("line_start".to_string(), codegraph::PropertyValue::Int(10));
            props.insert("line_end".to_string(), codegraph::PropertyValue::Int(20));

            let node_id = g
                .add_node(NodeType::Function, props)
                .expect("Failed to add node");

            // Node ID is always valid (can be 0 for first node)
            let _ = node_id;
        }

        // Build indexes
        engine.build_indexes().await;

        // Search should find the function
        let results = engine
            .symbol_search("validate", &SearchOptions::new())
            .await;

        assert_eq!(results.results.len(), 1);
        assert_eq!(results.results[0].symbol.name, "validateEmail");
    }

    #[tokio::test]
    async fn symbol_search_status_names_mismatched_daemon_vectors_not_a_build() {
        let (engine, _) = create_test_engine().await;
        let building = engine
            .symbol_search("test", &SearchOptions::new())
            .await
            .embedding_status
            .expect("no vectors yet");
        assert!(building.contains("building"));

        engine.set_daemon_vectors_mismatched();
        let status = engine
            .symbol_search("test", &SearchOptions::new())
            .await
            .embedding_status
            .expect("mismatched daemon vectors leave semantic search unavailable");
        assert!(!status.contains("building"), "{status}");
        assert!(status.contains("different embedding settings"), "{status}");
        assert!(status.contains("Restart the daemon"), "{status}");
    }

    #[tokio::test]
    async fn test_symbol_search_with_type_filter() {
        let (engine, graph) = create_test_engine().await;

        // Add a function and a class
        {
            let mut g = graph.write().await;

            let mut func_props = PropertyMap::new();
            func_props.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("processData".to_string()),
            );
            func_props.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/test.rs".to_string()),
            );
            func_props.insert("line_start".to_string(), codegraph::PropertyValue::Int(1));
            g.add_node(NodeType::Function, func_props)
                .expect("Failed to add function");

            let mut class_props = PropertyMap::new();
            class_props.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("DataProcessor".to_string()),
            );
            class_props.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/test.rs".to_string()),
            );
            class_props.insert("line_start".to_string(), codegraph::PropertyValue::Int(10));
            g.add_node(NodeType::Class, class_props)
                .expect("Failed to add class");
        }

        engine.build_indexes().await;

        // Search with function type filter
        let options = SearchOptions::new().with_symbol_types(vec![SymbolType::Function]);
        let results = engine.symbol_search("data", &options).await;

        assert_eq!(results.results.len(), 1);
        assert_eq!(results.results[0].symbol.kind, "Function");
    }

    #[tokio::test]
    async fn test_traverse_graph() {
        let (engine, graph) = create_test_engine().await;

        // Create a simple call chain: A -> B -> C
        let (a, b, c);
        {
            let mut g = graph.write().await;

            let mut props_a = PropertyMap::new();
            props_a.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("functionA".to_string()),
            );
            props_a.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/test.rs".to_string()),
            );
            props_a.insert("line_start".to_string(), codegraph::PropertyValue::Int(1));
            a = g
                .add_node(NodeType::Function, props_a)
                .expect("Failed to add node");

            let mut props_b = PropertyMap::new();
            props_b.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("functionB".to_string()),
            );
            props_b.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/test.rs".to_string()),
            );
            props_b.insert("line_start".to_string(), codegraph::PropertyValue::Int(10));
            b = g
                .add_node(NodeType::Function, props_b)
                .expect("Failed to add node");

            let mut props_c = PropertyMap::new();
            props_c.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("functionC".to_string()),
            );
            props_c.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/test.rs".to_string()),
            );
            props_c.insert("line_start".to_string(), codegraph::PropertyValue::Int(20));
            c = g
                .add_node(NodeType::Function, props_c)
                .expect("Failed to add node");

            // A calls B, B calls C
            g.add_edge(a, b, EdgeType::Calls, PropertyMap::new())
                .expect("Failed to add edge");
            g.add_edge(b, c, EdgeType::Calls, PropertyMap::new())
                .expect("Failed to add edge");
        }

        engine.build_indexes().await;

        // Traverse from A with depth 2
        let filter = TraversalFilter::new().with_max_nodes(100);
        let results = engine
            .traverse_graph(a, TraversalDirection::Outgoing, 2, &filter)
            .await;

        // Should find B and C
        assert_eq!(results.len(), 2);
        let names: Vec<&str> = results.iter().map(|r| r.symbol.name.as_str()).collect();
        assert!(names.contains(&"functionB"));
        assert!(names.contains(&"functionC"));
    }

    #[tokio::test]
    async fn test_traverse_graph_node_type_filter() {
        let (engine, graph) = create_test_engine().await;

        // Graph: functionA -> ClassB -> functionC
        // With nodeTypes: [Function], ClassB should be traversed through but not in results
        let (a, _b, c);
        {
            let mut g = graph.write().await;

            let mut props_a = PropertyMap::new();
            props_a.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("functionA".to_string()),
            );
            props_a.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/test.rs".to_string()),
            );
            props_a.insert("line_start".to_string(), codegraph::PropertyValue::Int(1));
            a = g.add_node(NodeType::Function, props_a).expect("add node");

            let mut props_b = PropertyMap::new();
            props_b.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("ClassB".to_string()),
            );
            props_b.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/test.rs".to_string()),
            );
            props_b.insert("line_start".to_string(), codegraph::PropertyValue::Int(10));
            _b = g.add_node(NodeType::Class, props_b).expect("add node");

            let mut props_c = PropertyMap::new();
            props_c.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("functionC".to_string()),
            );
            props_c.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/test.rs".to_string()),
            );
            props_c.insert("line_start".to_string(), codegraph::PropertyValue::Int(20));
            c = g.add_node(NodeType::Function, props_c).expect("add node");

            g.add_edge(a, _b, EdgeType::Calls, PropertyMap::new())
                .expect("add edge");
            g.add_edge(_b, c, EdgeType::Calls, PropertyMap::new())
                .expect("add edge");
        }

        engine.build_indexes().await;

        // Filter for only Function nodes — should still reach functionC through ClassB
        let filter = TraversalFilter::new()
            .with_max_nodes(100)
            .with_symbol_types(vec![SymbolType::Function]);
        let results = engine
            .traverse_graph(a, TraversalDirection::Outgoing, 3, &filter)
            .await;

        let names: Vec<&str> = results.iter().map(|r| r.symbol.name.as_str()).collect();
        assert!(
            names.contains(&"functionC"),
            "functionC should be reachable through filtered ClassB"
        );
        assert!(
            !names.contains(&"ClassB"),
            "ClassB should be filtered from results"
        );
        assert_eq!(results.len(), 1, "only functionC should be in results");
    }

    #[tokio::test]
    async fn test_traverse_graph_edge_type_filter() {
        let (engine, graph) = create_test_engine().await;

        // Graph: A -calls-> B -imports-> C
        // With edgeTypes: ["Calls"], should only reach B (not C via Imports)
        let (a, _b, _c);
        {
            let mut g = graph.write().await;

            let mut props_a = PropertyMap::new();
            props_a.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("funcA".to_string()),
            );
            props_a.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/test.rs".to_string()),
            );
            props_a.insert("line_start".to_string(), codegraph::PropertyValue::Int(1));
            a = g.add_node(NodeType::Function, props_a).expect("add node");

            let mut props_b = PropertyMap::new();
            props_b.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("funcB".to_string()),
            );
            props_b.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/test.rs".to_string()),
            );
            props_b.insert("line_start".to_string(), codegraph::PropertyValue::Int(10));
            _b = g.add_node(NodeType::Function, props_b).expect("add node");

            let mut props_c = PropertyMap::new();
            props_c.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("funcC".to_string()),
            );
            props_c.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/test.rs".to_string()),
            );
            props_c.insert("line_start".to_string(), codegraph::PropertyValue::Int(20));
            _c = g.add_node(NodeType::Function, props_c).expect("add node");

            g.add_edge(a, _b, EdgeType::Calls, PropertyMap::new())
                .expect("add edge");
            g.add_edge(_b, _c, EdgeType::Imports, PropertyMap::new())
                .expect("add edge");
        }

        engine.build_indexes().await;

        // Only follow Calls edges
        let filter = TraversalFilter::new()
            .with_max_nodes(100)
            .with_edge_types(vec!["Calls".to_string()]);
        let results = engine
            .traverse_graph(a, TraversalDirection::Outgoing, 3, &filter)
            .await;

        let names: Vec<&str> = results.iter().map(|r| r.symbol.name.as_str()).collect();
        assert!(names.contains(&"funcB"), "funcB reachable via Calls edge");
        assert!(
            !names.contains(&"funcC"),
            "funcC should not be reachable via Imports edge when filtering for Calls"
        );
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn test_get_callers() {
        let (engine, graph) = create_test_engine().await;

        let (a, b, c);
        {
            let mut g = graph.write().await;

            let mut props_a = PropertyMap::new();
            props_a.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("caller1".to_string()),
            );
            props_a.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/test.rs".to_string()),
            );
            props_a.insert("line_start".to_string(), codegraph::PropertyValue::Int(1));
            a = g
                .add_node(NodeType::Function, props_a)
                .expect("Failed to add node");

            let mut props_b = PropertyMap::new();
            props_b.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("caller2".to_string()),
            );
            props_b.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/test.rs".to_string()),
            );
            props_b.insert("line_start".to_string(), codegraph::PropertyValue::Int(10));
            b = g
                .add_node(NodeType::Function, props_b)
                .expect("Failed to add node");

            let mut props_c = PropertyMap::new();
            props_c.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("target".to_string()),
            );
            props_c.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/test.rs".to_string()),
            );
            props_c.insert("line_start".to_string(), codegraph::PropertyValue::Int(20));
            c = g
                .add_node(NodeType::Function, props_c)
                .expect("Failed to add node");

            // A and B both call C
            g.add_edge(a, c, EdgeType::Calls, PropertyMap::new())
                .expect("Failed to add edge");
            g.add_edge(b, c, EdgeType::Calls, PropertyMap::new())
                .expect("Failed to add edge");
        }

        engine.build_indexes().await;

        // Get callers of C
        let callers = engine.get_callers(c, 1).await;

        assert_eq!(callers.len(), 2);
        let caller_names: Vec<&str> = callers.iter().map(|c| c.symbol.name.as_str()).collect();
        assert!(caller_names.contains(&"caller1"));
        assert!(caller_names.contains(&"caller2"));
    }

    #[tokio::test]
    async fn test_find_entry_points() {
        let (engine, graph) = create_test_engine().await;

        {
            let mut g = graph.write().await;

            // Main function
            let mut main_props = PropertyMap::new();
            main_props.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("main".to_string()),
            );
            main_props.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/main.rs".to_string()),
            );
            main_props.insert("line_start".to_string(), codegraph::PropertyValue::Int(1));
            g.add_node(NodeType::Function, main_props)
                .expect("Failed to add main");

            // Test function
            let mut test_props = PropertyMap::new();
            test_props.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("test_something".to_string()),
            );
            test_props.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/test.rs".to_string()),
            );
            test_props.insert("line_start".to_string(), codegraph::PropertyValue::Int(10));
            g.add_node(NodeType::Function, test_props)
                .expect("Failed to add test");

            // Regular function (not an entry point)
            let mut helper_props = PropertyMap::new();
            helper_props.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("helper".to_string()),
            );
            helper_props.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/lib.rs".to_string()),
            );
            helper_props.insert("line_start".to_string(), codegraph::PropertyValue::Int(20));
            g.add_node(NodeType::Function, helper_props)
                .expect("Failed to add helper");
        }

        engine.build_indexes().await;

        // Find main entry points
        let mains = engine.find_entry_points(&[EntryType::Main]).await;
        assert_eq!(mains.len(), 1);
        assert_eq!(mains[0].symbol.name, "main");

        // Find test entry points
        let tests = engine.find_entry_points(&[EntryType::TestEntry]).await;
        assert_eq!(tests.len(), 1);
        assert_eq!(tests[0].symbol.name, "test_something");
    }

    #[tokio::test]
    async fn test_get_symbol_info() {
        let (engine, graph) = create_test_engine().await;

        let node_id;
        {
            let mut g = graph.write().await;
            let mut props = PropertyMap::new();
            props.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("processData".to_string()),
            );
            props.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/lib.rs".to_string()),
            );
            props.insert("line_start".to_string(), codegraph::PropertyValue::Int(10));
            props.insert("line_end".to_string(), codegraph::PropertyValue::Int(25));
            props.insert(
                "doc".to_string(),
                codegraph::PropertyValue::String("Processes input data".to_string()),
            );
            props.insert(
                "is_public".to_string(),
                codegraph::PropertyValue::Bool(true),
            );

            node_id = g
                .add_node(NodeType::Function, props)
                .expect("Failed to add node");
        }

        engine.build_indexes().await;

        let info = engine.get_symbol_info(node_id).await;

        assert!(info.is_some());
        let info = info.unwrap();
        assert_eq!(info.symbol.name, "processData");
        assert_eq!(info.lines_of_code, 16); // 25 - 10 + 1
        assert!(info.is_public);
    }

    #[tokio::test]
    async fn test_query_performance() {
        let (engine, graph) = create_test_engine().await;

        // Add 1000 nodes using camelCase so tokens are properly split
        {
            let mut g = graph.write().await;
            for i in 0..1000 {
                let mut props = PropertyMap::new();
                // Use functionXXX format like "functionProcess0" so "function" is a separate token
                props.insert(
                    "name".to_string(),
                    codegraph::PropertyValue::String(format!("functionProcess{i}")),
                );
                props.insert(
                    "path".to_string(),
                    codegraph::PropertyValue::String("/src/test.rs".to_string()),
                );
                props.insert(
                    "line_start".to_string(),
                    codegraph::PropertyValue::Int(i as i64),
                );

                g.add_node(NodeType::Function, props)
                    .expect("Failed to add node");
            }
        }

        engine.build_indexes().await;

        // Search should complete quickly (< 10ms)
        let start = Instant::now();
        let results = engine
            .symbol_search("function", &SearchOptions::new())
            .await;
        let duration = start.elapsed();

        assert!(
            duration.as_millis() < 10,
            "Search took too long: {duration:?}"
        );
        assert!(!results.results.is_empty());
    }

    // ==========================================
    // find_by_signature tests
    // ==========================================

    #[tokio::test]
    async fn test_find_by_signature_name_pattern() {
        let (engine, graph) = create_test_engine().await;

        {
            let mut g = graph.write().await;

            // Add functions with different names
            for (name, is_async) in [
                ("getUserById", false),
                ("getOrderById", false),
                ("createUser", true),
                ("deleteUser", false),
                ("processData", false),
            ] {
                let mut props = PropertyMap::new();
                props.insert(
                    "name".to_string(),
                    codegraph::PropertyValue::String(name.to_string()),
                );
                props.insert(
                    "path".to_string(),
                    codegraph::PropertyValue::String("/src/api.rs".to_string()),
                );
                props.insert("line_start".to_string(), codegraph::PropertyValue::Int(1));
                props.insert(
                    "is_async".to_string(),
                    codegraph::PropertyValue::Bool(is_async),
                );
                g.add_node(NodeType::Function, props)
                    .expect("Failed to add node");
            }
        }

        engine.build_indexes().await;

        // Search for functions matching "get.*ById" pattern
        let pattern = SignaturePattern {
            name_pattern: Some("get.*ById".to_string()),
            return_type: None,
            param_count: None,
            modifiers: vec![],
        };

        let results = engine.find_by_signature(&pattern, None).await;

        assert_eq!(results.len(), 2);
        let names: Vec<&str> = results.iter().map(|r| r.symbol.name.as_str()).collect();
        assert!(names.contains(&"getUserById"));
        assert!(names.contains(&"getOrderById"));
    }

    #[tokio::test]
    async fn test_find_by_signature_return_type() {
        let (engine, graph) = create_test_engine().await;

        {
            let mut g = graph.write().await;

            // Add functions with different return types
            for (name, return_type) in [
                ("getString", "String"),
                ("getInt", "i32"),
                ("getBool", "bool"),
                ("getResult", "Result<String, Error>"),
            ] {
                let mut props = PropertyMap::new();
                props.insert(
                    "name".to_string(),
                    codegraph::PropertyValue::String(name.to_string()),
                );
                props.insert(
                    "path".to_string(),
                    codegraph::PropertyValue::String("/src/lib.rs".to_string()),
                );
                props.insert("line_start".to_string(), codegraph::PropertyValue::Int(1));
                props.insert(
                    "return_type".to_string(),
                    codegraph::PropertyValue::String(return_type.to_string()),
                );
                g.add_node(NodeType::Function, props)
                    .expect("Failed to add node");
            }
        }

        engine.build_indexes().await;

        // Search for functions returning String
        let pattern = SignaturePattern {
            name_pattern: None,
            return_type: Some("String".to_string()),
            param_count: None,
            modifiers: vec![],
        };

        let results = engine.find_by_signature(&pattern, None).await;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].symbol.name, "getString");
    }

    #[tokio::test]
    async fn test_find_by_signature_return_type_normalized() {
        let (engine, graph) = create_test_engine().await;

        {
            let mut g = graph.write().await;

            // Add functions with equivalent return types
            for (name, return_type) in [
                ("fn1", "boolean"),
                ("fn2", "bool"),
                ("fn3", "void"),
                ("fn4", "()"),
            ] {
                let mut props = PropertyMap::new();
                props.insert(
                    "name".to_string(),
                    codegraph::PropertyValue::String(name.to_string()),
                );
                props.insert(
                    "path".to_string(),
                    codegraph::PropertyValue::String("/src/lib.rs".to_string()),
                );
                props.insert("line_start".to_string(), codegraph::PropertyValue::Int(1));
                props.insert(
                    "return_type".to_string(),
                    codegraph::PropertyValue::String(return_type.to_string()),
                );
                g.add_node(NodeType::Function, props)
                    .expect("Failed to add node");
            }
        }

        engine.build_indexes().await;

        // Search for boolean (should match both "boolean" and "bool")
        let pattern = SignaturePattern {
            name_pattern: None,
            return_type: Some("bool".to_string()),
            param_count: None,
            modifiers: vec![],
        };

        let results = engine.find_by_signature(&pattern, None).await;

        assert_eq!(results.len(), 2);
        let names: Vec<&str> = results.iter().map(|r| r.symbol.name.as_str()).collect();
        assert!(names.contains(&"fn1"));
        assert!(names.contains(&"fn2"));

        // Search for void (should match "void" and "()")
        let pattern = SignaturePattern {
            name_pattern: None,
            return_type: Some("void".to_string()),
            param_count: None,
            modifiers: vec![],
        };

        let results = engine.find_by_signature(&pattern, None).await;

        assert_eq!(results.len(), 2);
        let names: Vec<&str> = results.iter().map(|r| r.symbol.name.as_str()).collect();
        assert!(names.contains(&"fn3"));
        assert!(names.contains(&"fn4"));
    }

    #[tokio::test]
    async fn test_find_by_signature_param_count() {
        let (engine, graph) = create_test_engine().await;

        {
            let mut g = graph.write().await;

            // Add functions with different param counts
            for (name, param_count) in [
                ("noParams", 0),
                ("oneParam", 1),
                ("twoParams", 2),
                ("threeParams", 3),
                ("manyParams", 5),
            ] {
                let mut props = PropertyMap::new();
                props.insert(
                    "name".to_string(),
                    codegraph::PropertyValue::String(name.to_string()),
                );
                props.insert(
                    "path".to_string(),
                    codegraph::PropertyValue::String("/src/lib.rs".to_string()),
                );
                props.insert("line_start".to_string(), codegraph::PropertyValue::Int(1));
                props.insert(
                    "param_count".to_string(),
                    codegraph::PropertyValue::Int(param_count),
                );
                g.add_node(NodeType::Function, props)
                    .expect("Failed to add node");
            }
        }

        engine.build_indexes().await;

        // Search for functions with 1-2 parameters
        let pattern = SignaturePattern {
            name_pattern: None,
            return_type: None,
            param_count: Some((1, 2)),
            modifiers: vec![],
        };

        let results = engine.find_by_signature(&pattern, None).await;

        assert_eq!(results.len(), 2);
        let names: Vec<&str> = results.iter().map(|r| r.symbol.name.as_str()).collect();
        assert!(names.contains(&"oneParam"));
        assert!(names.contains(&"twoParams"));

        // Search for functions with exactly 0 parameters
        let pattern = SignaturePattern {
            name_pattern: None,
            return_type: None,
            param_count: Some((0, 0)),
            modifiers: vec![],
        };

        let results = engine.find_by_signature(&pattern, None).await;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].symbol.name, "noParams");
    }

    #[tokio::test]
    async fn test_find_by_signature_modifiers() {
        let (engine, graph) = create_test_engine().await;

        {
            let mut g = graph.write().await;

            // Add functions with different modifiers
            let configs = [
                ("syncPublic", false, true, false),
                ("asyncPublic", true, true, false),
                ("syncPrivate", false, false, false),
                ("asyncPrivate", true, false, false),
                ("staticFunc", false, true, true),
            ];

            for (name, is_async, is_public, is_static) in configs {
                let mut props = PropertyMap::new();
                props.insert(
                    "name".to_string(),
                    codegraph::PropertyValue::String(name.to_string()),
                );
                props.insert(
                    "path".to_string(),
                    codegraph::PropertyValue::String("/src/lib.rs".to_string()),
                );
                props.insert("line_start".to_string(), codegraph::PropertyValue::Int(1));
                props.insert(
                    "is_async".to_string(),
                    codegraph::PropertyValue::Bool(is_async),
                );
                props.insert(
                    "is_public".to_string(),
                    codegraph::PropertyValue::Bool(is_public),
                );
                props.insert(
                    "is_static".to_string(),
                    codegraph::PropertyValue::Bool(is_static),
                );
                g.add_node(NodeType::Function, props)
                    .expect("Failed to add node");
            }
        }

        engine.build_indexes().await;

        // Search for async functions
        let pattern = SignaturePattern {
            name_pattern: None,
            return_type: None,
            param_count: None,
            modifiers: vec!["async".to_string()],
        };

        let results = engine.find_by_signature(&pattern, None).await;

        assert_eq!(results.len(), 2);
        let names: Vec<&str> = results.iter().map(|r| r.symbol.name.as_str()).collect();
        assert!(names.contains(&"asyncPublic"));
        assert!(names.contains(&"asyncPrivate"));

        // Search for public async functions
        let pattern = SignaturePattern {
            name_pattern: None,
            return_type: None,
            param_count: None,
            modifiers: vec!["async".to_string(), "public".to_string()],
        };

        let results = engine.find_by_signature(&pattern, None).await;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].symbol.name, "asyncPublic");

        // Search for static functions
        let pattern = SignaturePattern {
            name_pattern: None,
            return_type: None,
            param_count: None,
            modifiers: vec!["static".to_string()],
        };

        let results = engine.find_by_signature(&pattern, None).await;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].symbol.name, "staticFunc");
    }

    #[tokio::test]
    async fn test_find_by_signature_combined_filters() {
        let (engine, graph) = create_test_engine().await;

        {
            let mut g = graph.write().await;

            // Add various functions
            let configs = [
                ("getUserById", "User", 1, true, true),
                ("getOrderById", "Order", 1, true, false),
                ("fetchUserData", "User", 2, true, true),
                ("createUser", "User", 3, false, true),
                ("processRequest", "Response", 1, true, true),
            ];

            for (name, return_type, param_count, is_async, is_public) in configs {
                let mut props = PropertyMap::new();
                props.insert(
                    "name".to_string(),
                    codegraph::PropertyValue::String(name.to_string()),
                );
                props.insert(
                    "path".to_string(),
                    codegraph::PropertyValue::String("/src/api.rs".to_string()),
                );
                props.insert("line_start".to_string(), codegraph::PropertyValue::Int(1));
                props.insert(
                    "return_type".to_string(),
                    codegraph::PropertyValue::String(return_type.to_string()),
                );
                props.insert(
                    "param_count".to_string(),
                    codegraph::PropertyValue::Int(param_count),
                );
                props.insert(
                    "is_async".to_string(),
                    codegraph::PropertyValue::Bool(is_async),
                );
                props.insert(
                    "is_public".to_string(),
                    codegraph::PropertyValue::Bool(is_public),
                );
                g.add_node(NodeType::Function, props)
                    .expect("Failed to add node");
            }
        }

        engine.build_indexes().await;

        // Search for async public functions returning User with 1 param
        let pattern = SignaturePattern {
            name_pattern: None,
            return_type: Some("User".to_string()),
            param_count: Some((1, 1)),
            modifiers: vec!["async".to_string(), "public".to_string()],
        };

        let results = engine.find_by_signature(&pattern, None).await;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].symbol.name, "getUserById");
    }

    #[tokio::test]
    async fn test_find_by_signature_no_matches() {
        let (engine, graph) = create_test_engine().await;

        {
            let mut g = graph.write().await;
            let mut props = PropertyMap::new();
            props.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("someFunction".to_string()),
            );
            props.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/lib.rs".to_string()),
            );
            props.insert("line_start".to_string(), codegraph::PropertyValue::Int(1));
            g.add_node(NodeType::Function, props)
                .expect("Failed to add node");
        }

        engine.build_indexes().await;

        // Search for a pattern that won't match
        let pattern = SignaturePattern {
            name_pattern: Some("nonexistent.*".to_string()),
            return_type: None,
            param_count: None,
            modifiers: vec![],
        };

        let results = engine.find_by_signature(&pattern, None).await;

        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_find_by_signature_only_matches_functions() {
        let (engine, graph) = create_test_engine().await;

        {
            let mut g = graph.write().await;

            // Add a function
            let mut func_props = PropertyMap::new();
            func_props.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("myFunction".to_string()),
            );
            func_props.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/lib.rs".to_string()),
            );
            func_props.insert("line_start".to_string(), codegraph::PropertyValue::Int(1));
            g.add_node(NodeType::Function, func_props)
                .expect("Failed to add function");

            // Add a class with similar name
            let mut class_props = PropertyMap::new();
            class_props.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("myClass".to_string()),
            );
            class_props.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/lib.rs".to_string()),
            );
            class_props.insert("line_start".to_string(), codegraph::PropertyValue::Int(10));
            g.add_node(NodeType::Class, class_props)
                .expect("Failed to add class");

            // Add a variable with similar name
            let mut var_props = PropertyMap::new();
            var_props.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("myVariable".to_string()),
            );
            var_props.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/lib.rs".to_string()),
            );
            var_props.insert("line_start".to_string(), codegraph::PropertyValue::Int(20));
            g.add_node(NodeType::Variable, var_props)
                .expect("Failed to add variable");
        }

        engine.build_indexes().await;

        // Search with pattern matching all "my*"
        let pattern = SignaturePattern {
            name_pattern: Some("my.*".to_string()),
            return_type: None,
            param_count: None,
            modifiers: vec![],
        };

        let results = engine.find_by_signature(&pattern, None).await;

        // Should only match the function
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].symbol.name, "myFunction");
        assert_eq!(results[0].symbol.kind, "Function");
    }

    #[tokio::test]
    async fn test_find_by_signature_match_reason() {
        let (engine, graph) = create_test_engine().await;

        {
            let mut g = graph.write().await;
            let mut props = PropertyMap::new();
            props.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("testFunc".to_string()),
            );
            props.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/lib.rs".to_string()),
            );
            props.insert("line_start".to_string(), codegraph::PropertyValue::Int(1));
            props.insert(
                "return_type".to_string(),
                codegraph::PropertyValue::String("bool".to_string()),
            );
            props.insert("param_count".to_string(), codegraph::PropertyValue::Int(2));
            props.insert("is_async".to_string(), codegraph::PropertyValue::Bool(true));
            g.add_node(NodeType::Function, props)
                .expect("Failed to add node");
        }

        engine.build_indexes().await;

        let pattern = SignaturePattern {
            name_pattern: Some("test.*".to_string()),
            return_type: Some("bool".to_string()),
            param_count: Some((2, 2)),
            modifiers: vec!["async".to_string()],
        };

        let results = engine.find_by_signature(&pattern, None).await;

        assert_eq!(results.len(), 1);
        let match_reason = &results[0].match_reason;
        assert!(match_reason.contains("name matches /test.*/"));
        assert!(match_reason.contains("returns bool"));
        assert!(match_reason.contains("2 parameters"));
        assert!(match_reason.contains("modifiers: async"));
    }

    #[tokio::test]
    async fn test_type_matches_wildcard() {
        let (engine, graph) = create_test_engine().await;

        {
            let mut g = graph.write().await;

            for (name, return_type) in [
                ("fn1", "Result<String, Error>"),
                ("fn2", "Result<i32, Error>"),
                ("fn3", "Option<String>"),
            ] {
                let mut props = PropertyMap::new();
                props.insert(
                    "name".to_string(),
                    codegraph::PropertyValue::String(name.to_string()),
                );
                props.insert(
                    "path".to_string(),
                    codegraph::PropertyValue::String("/src/lib.rs".to_string()),
                );
                props.insert("line_start".to_string(), codegraph::PropertyValue::Int(1));
                props.insert(
                    "return_type".to_string(),
                    codegraph::PropertyValue::String(return_type.to_string()),
                );
                g.add_node(NodeType::Function, props)
                    .expect("Failed to add node");
            }
        }

        engine.build_indexes().await;

        // Search for Result<*, Error> pattern
        let pattern = SignaturePattern {
            name_pattern: None,
            return_type: Some("Result<*, Error>".to_string()),
            param_count: None,
            modifiers: vec![],
        };

        let results = engine.find_by_signature(&pattern, None).await;

        assert_eq!(results.len(), 2);
        let names: Vec<&str> = results.iter().map(|r| r.symbol.name.as_str()).collect();
        assert!(names.contains(&"fn1"));
        assert!(names.contains(&"fn2"));
    }

    // ==========================================
    // detect_entry_type tests
    // ==========================================

    #[tokio::test]
    async fn test_detect_entry_type_http_handler() {
        let (engine, graph) = create_test_engine().await;

        {
            let mut g = graph.write().await;
            let mut props = PropertyMap::new();
            props.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("getUsers".to_string()),
            );
            props.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/handlers.rs".to_string()),
            );
            props.insert("line_start".to_string(), codegraph::PropertyValue::Int(1));
            props.insert(
                "route".to_string(),
                codegraph::PropertyValue::String("/api/users".to_string()),
            );
            props.insert(
                "http_method".to_string(),
                codegraph::PropertyValue::String("GET".to_string()),
            );
            g.add_node(NodeType::Function, props)
                .expect("Failed to add node");
        }

        engine.build_indexes().await;

        let results = engine.find_entry_points(&[EntryType::HttpHandler]).await;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].symbol.name, "getUsers");
        assert!(matches!(results[0].entry_type, EntryType::HttpHandler));
        assert_eq!(results[0].route, Some("/api/users".to_string()));
        assert_eq!(results[0].method, Some("GET".to_string()));
    }

    #[tokio::test]
    async fn test_detect_entry_type_cli_command() {
        let (engine, graph) = create_test_engine().await;

        {
            let mut g = graph.write().await;

            // CLI command by name
            let mut props = PropertyMap::new();
            props.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("runCommand".to_string()),
            );
            props.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/cli.rs".to_string()),
            );
            props.insert("line_start".to_string(), codegraph::PropertyValue::Int(1));
            g.add_node(NodeType::Function, props)
                .expect("Failed to add node");

            // CLI command by property
            let mut props2 = PropertyMap::new();
            props2.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("execute".to_string()),
            );
            props2.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/cli.rs".to_string()),
            );
            props2.insert("line_start".to_string(), codegraph::PropertyValue::Int(10));
            props2.insert("is_cli".to_string(), codegraph::PropertyValue::Bool(true));
            g.add_node(NodeType::Function, props2)
                .expect("Failed to add node");
        }

        engine.build_indexes().await;

        let results = engine.find_entry_points(&[EntryType::CliCommand]).await;

        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_detect_entry_type_event_handler() {
        let (engine, graph) = create_test_engine().await;

        {
            let mut g = graph.write().await;

            // Use names that match the actual detection patterns:
            // - starts_with("on_")
            // - starts_with("handle_")
            // - ends_with("_handler")
            // - ends_with("_callback")
            // Note: Avoid "cli" in names as it triggers CliCommand detection first
            for name in [
                "on_submit",
                "handle_submit",
                "button_handler",
                "data_callback",
            ] {
                let mut props = PropertyMap::new();
                props.insert(
                    "name".to_string(),
                    codegraph::PropertyValue::String(name.to_string()),
                );
                props.insert(
                    "path".to_string(),
                    codegraph::PropertyValue::String("/src/events.rs".to_string()),
                );
                props.insert("line_start".to_string(), codegraph::PropertyValue::Int(1));
                g.add_node(NodeType::Function, props)
                    .expect("Failed to add node");
            }
        }

        engine.build_indexes().await;

        let results = engine.find_entry_points(&[EntryType::EventHandler]).await;

        assert_eq!(results.len(), 4);

        // Verify all expected patterns are detected
        let names: Vec<&str> = results.iter().map(|r| r.symbol.name.as_str()).collect();
        assert!(
            names.contains(&"on_submit"),
            "on_submit should be detected (starts with on_)"
        );
        assert!(
            names.contains(&"handle_submit"),
            "handle_submit should be detected (starts with handle_)"
        );
        assert!(
            names.contains(&"button_handler"),
            "button_handler should be detected (ends with _handler)"
        );
        assert!(
            names.contains(&"data_callback"),
            "data_callback should be detected (ends with _callback)"
        );
    }

    #[tokio::test]
    async fn test_find_entry_points_all_types() {
        let (engine, graph) = create_test_engine().await;

        {
            let mut g = graph.write().await;

            // Main
            let mut main_props = PropertyMap::new();
            main_props.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("main".to_string()),
            );
            main_props.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/main.rs".to_string()),
            );
            main_props.insert("line_start".to_string(), codegraph::PropertyValue::Int(1));
            g.add_node(NodeType::Function, main_props)
                .expect("Failed to add main");

            // Test
            let mut test_props = PropertyMap::new();
            test_props.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("test_something".to_string()),
            );
            test_props.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/tests.rs".to_string()),
            );
            test_props.insert("line_start".to_string(), codegraph::PropertyValue::Int(10));
            g.add_node(NodeType::Function, test_props)
                .expect("Failed to add test");

            // Public API
            let mut api_props = PropertyMap::new();
            api_props.insert(
                "name".to_string(),
                codegraph::PropertyValue::String("someApi".to_string()),
            );
            api_props.insert(
                "path".to_string(),
                codegraph::PropertyValue::String("/src/lib.rs".to_string()),
            );
            api_props.insert("line_start".to_string(), codegraph::PropertyValue::Int(20));
            api_props.insert("exported".to_string(), codegraph::PropertyValue::Bool(true));
            g.add_node(NodeType::Function, api_props)
                .expect("Failed to add api");
        }

        engine.build_indexes().await;

        // Find all entry points (empty filter)
        let results = engine.find_entry_points(&[]).await;

        // Should find main, test, and public API
        assert!(results.len() >= 3);
    }

    #[test]
    fn split_identifier_words_handles_camel_and_snake() {
        assert_eq!(
            split_identifier_words("authenticate_user"),
            "authenticate user"
        );
        assert_eq!(split_identifier_words("getUserById"), "get user by id");
        // The existing tokenizer keeps acronym+word runs joined (HTML|Parser is
        // NOT split) and drops 1-char tokens — known limitations worth revisiting
        // for static-embedding quality (Phase 3 input refinement).
        assert_eq!(split_identifier_words("HTMLParser"), "htmlparser");
        // A single lowercase word splits to itself.
        assert_eq!(split_identifier_words("foo"), "foo");
    }

    /// An empty project namespace backed by an in-memory store.
    fn namespace() -> NamespacedBackend {
        use codegraph::MemoryBackend;
        NamespacedBackend::new(Box::new(MemoryBackend::new()), "proj")
    }

    fn vectors(ids: &[NodeId]) -> HashMap<NodeId, Vec<f32>> {
        ids.iter().map(|&id| (id, vec![id as f32, 0.5])).collect()
    }

    fn sorted(mut got: Vec<(NodeId, Vec<f32>)>) -> Vec<(NodeId, Vec<f32>)> {
        got.sort_by_key(|(id, _)| *id);
        got
    }

    fn stored_ids(ns: &NamespacedBackend) -> Vec<NodeId> {
        use codegraph::StorageBackend;
        let mut ids: Vec<NodeId> = ns
            .scan_prefix_keys(VECTOR_KEY_PREFIX.as_bytes())
            .unwrap()
            .iter()
            .filter_map(|k| vector_key_node(k))
            .collect();
        ids.sort_unstable();
        ids
    }

    /// A project slug no watcher daemon holds a heartbeat for, so claiming is
    /// permitted. `live_daemon_for` reads `~/.codegraph/daemons/<slug>.json`.
    const UNWATCHED: &str = "no-daemon-owns-this-slug-c0ffee";

    /// bge-small and jina-code-v2 differ in dimension, which is the sharpest
    /// reason two vector sets cannot be ranked against each other.
    const MODEL_A: &str = "bge-small";
    const MODEL_B: &str = "jina-code-v2";

    /// A namespace a rebuild has already taken for `stamp`, which is the only
    /// state in which anything may be stored.
    fn claimed(stamp: &str) -> NamespacedBackend {
        let mut ns = namespace();
        assert!(claim_project(&mut ns, UNWATCHED, stamp).unwrap());
        ns
    }

    #[test]
    fn vectors_round_trip_under_the_configuration_that_stored_them() {
        let stamp = embed_text_id(MODEL_A, true, true);
        let mut ns = claimed(&stamp);
        let written = vectors(&[1, 2, 3]);

        assert!(store_vectors(&mut ns, &written, &stamp).unwrap());

        assert_eq!(
            sorted(read_vectors(&ns, &stamp).unwrap()),
            sorted(written.into_iter().collect())
        );
    }

    #[test]
    fn checkpoints_survive_a_crash_on_a_first_index() {
        // The whole point of checkpointing: an OOM-kill partway through a
        // marathon first index must leave loadable work behind. The rebuild
        // claims the project up front, so its checkpoints have a set to land
        // in and the next start finds them.
        let mut ns = namespace();
        let stamp = embed_text_id(MODEL_A, true, true);

        assert!(claim_project(&mut ns, UNWATCHED, &stamp).unwrap());
        assert!(store_vectors(&mut ns, &vectors(&[1, 2]), &stamp).unwrap());
        assert!(store_vectors(&mut ns, &vectors(&[1, 2, 3, 4]), &stamp).unwrap());
        // ... killed here, before any complete save.

        assert_eq!(sorted(read_vectors(&ns, &stamp).unwrap()).len(), 4);
        assert_eq!(stored_ids(&ns), vec![1, 2, 3, 4]);
    }

    #[test]
    fn checkpoints_survive_a_crash_while_migrating_an_older_index() {
        use codegraph::StorageBackend;

        // Same guarantee when the store already holds something incompatible:
        // 0.20.1 raw-name vectors under the same prefix with no stamp.
        let mut ns = namespace();
        ns.put(b"vec:77", &[0u8; 8]).unwrap();
        let stamp = embed_text_id(MODEL_A, true, true);

        assert!(claim_project(&mut ns, UNWATCHED, &stamp).unwrap());
        assert!(store_vectors(&mut ns, &vectors(&[1, 2]), &stamp).unwrap());

        assert_eq!(stored_ids(&ns), vec![1, 2], "the claim drops the old set");
        assert_eq!(read_vectors(&ns, &stamp).unwrap().len(), 2);
    }

    #[test]
    fn switching_the_embedding_model_invalidates_the_stored_set() {
        // Regression: before the model was part of the stamp, a 384d set stayed
        // loaded after a switch to a 768d model, and every symbol already had a
        // vector so nothing re-embedded. Queries then scored 768d against 384d.
        let bge = embed_text_id(MODEL_A, true, true);
        let jina = embed_text_id(MODEL_B, true, true);
        let mut ns = claimed(&bge);

        store_vectors(&mut ns, &vectors(&[1, 2, 3]), &bge).unwrap();

        assert_eq!(read_vectors(&ns, &jina), Err(VectorLoad::Mismatched));
        assert_eq!(read_vectors(&ns, &bge).unwrap().len(), 3);
    }

    #[test]
    fn an_empty_store_is_distinguishable_from_a_foreign_one() {
        // The caller reacts to these differently: a daemon that has not
        // persisted yet must be waited for, a foreign set must be re-embedded.
        let ns = namespace();
        assert_eq!(
            read_vectors(&ns, &embed_text_id(MODEL_A, true, true)),
            Err(VectorLoad::Absent)
        );

        let mut ns = claimed(&embed_text_id(MODEL_A, true, true));
        store_vectors(&mut ns, &vectors(&[1]), &embed_text_id(MODEL_A, true, true)).unwrap();
        assert_eq!(
            read_vectors(&ns, &embed_text_id(MODEL_A, false, true)),
            Err(VectorLoad::Mismatched)
        );
    }

    #[test]
    fn unstamped_0_20_1_vectors_are_a_mismatch_not_an_empty_store() {
        use codegraph::StorageBackend;

        let mut ns = namespace();
        ns.put(b"vec:1", &[0u8; 8]).unwrap();
        ns.put(b"vec:2", &[0u8; 8]).unwrap();

        assert_eq!(
            read_vectors(&ns, &embed_text_id(MODEL_A, true, true)),
            Err(VectorLoad::Mismatched)
        );
    }

    #[test]
    fn no_save_may_replace_a_set_another_embed_text_owns() {
        // Enforced here rather than trusted to callers: a session attached to a
        // watcher daemon embeds its own in-memory copy, and its reindex tool
        // reaches the same complete-save path the daemon's periodic persist
        // does. Taking a project over is a rebuild's job, via claim_project.
        use codegraph::StorageBackend;

        let theirs = embed_text_id(MODEL_A, true, true);
        let ours = embed_text_id(MODEL_A, true, false);
        let mut ns = claimed(&theirs);

        store_vectors(&mut ns, &vectors(&[1, 2, 3]), &theirs).unwrap();

        assert!(!store_vectors(&mut ns, &vectors(&[9]), &ours).unwrap());

        assert_eq!(stored_ids(&ns), vec![1, 2, 3]);
        assert_eq!(read_vectors(&ns, &theirs).unwrap().len(), 3);
        assert_eq!(ns.get(EMBED_STAMP_KEY).unwrap(), Some(theirs.into_bytes()));
    }

    #[test]
    fn claiming_is_how_a_rebuild_takes_a_project_over() {
        let theirs = embed_text_id(MODEL_A, true, true);
        let ours = embed_text_id(MODEL_B, false, false);
        let mut ns = claimed(&theirs);

        store_vectors(&mut ns, &vectors(&[1, 2, 3]), &theirs).unwrap();
        assert!(claim_project(&mut ns, UNWATCHED, &ours).unwrap());

        assert!(
            stored_ids(&ns).is_empty(),
            "storage stays bounded at one set"
        );
        assert_eq!(read_vectors(&ns, &theirs), Err(VectorLoad::Absent));
        assert_eq!(read_vectors(&ns, &ours).unwrap(), vec![]);

        assert!(store_vectors(&mut ns, &vectors(&[9]), &ours).unwrap());
        assert_eq!(read_vectors(&ns, &ours).unwrap().len(), 1);
    }

    #[test]
    fn no_save_writes_into_a_project_nobody_has_claimed() {
        // Stamping is a rebuild's act, not a side effect of saving. Otherwise
        // the first complete save to come along adopts whatever unstamped set
        // is lying there - everything a pre-0.21 binary wrote - and serves it
        // as if it matched.
        let stamp = embed_text_id(MODEL_A, true, true);

        let mut ns = namespace();
        assert!(!store_vectors(&mut ns, &vectors(&[1]), &stamp).unwrap());
        assert!(stored_ids(&ns).is_empty());
        assert_eq!(read_vectors(&ns, &stamp), Err(VectorLoad::Absent));
    }

    #[test]
    fn a_pre_0_21_set_is_never_adopted_by_a_save() {
        use codegraph::StorageBackend;

        // A pre-0.21 `--watch` daemon left running across an upgrade keeps
        // writing unstamped vectors. If a save could stamp them, later sessions
        // would rank split-identifier queries against raw-name vectors and
        // report semantic search ready.
        let mut ns = namespace();
        ns.put(b"vec:1", &[0u8; 8]).unwrap();
        ns.put(b"vec:2", &[0u8; 8]).unwrap();
        let stamp = embed_text_id(MODEL_A, true, true);

        assert!(!store_vectors(&mut ns, &vectors(&[3]), &stamp).unwrap());

        assert_eq!(stored_ids(&ns), vec![1, 2]);
        assert_eq!(ns.get(EMBED_STAMP_KEY).unwrap(), None);
        assert_eq!(read_vectors(&ns, &stamp), Err(VectorLoad::Mismatched));
    }

    #[test]
    fn a_save_never_deletes_work_another_writer_stored() {
        // Saves only add. A rebuild's checkpoints, a watcher daemon's periodic
        // persist and a session indexing a narrower set of paths all reach this
        // path with maps covering different parts of the project; any of them
        // deleting what its own map does not carry would throw away the others'
        // work. Clearing the set is the claim's job.
        let stamp = embed_text_id(MODEL_A, true, true);
        let mut ns = claimed(&stamp);

        store_vectors(&mut ns, &vectors(&[1, 2, 3]), &stamp).unwrap();
        store_vectors(&mut ns, &vectors(&[1]), &stamp).unwrap();
        store_vectors(&mut ns, &vectors(&[9]), &stamp).unwrap();

        assert_eq!(stored_ids(&ns), vec![1, 2, 3, 9]);
    }

    #[test]
    fn a_set_larger_than_one_write_batch_round_trips() {
        // Written in chunks so peak memory does not scale with the set: the
        // checkpoint forced under memory pressure must not triple the footprint
        // at the moment memory is already short.
        let stamp = embed_text_id(MODEL_A, true, true);
        let mut ns = claimed(&stamp);
        let ids: Vec<NodeId> = (1..=(STORE_BATCH_VECTORS as NodeId + 17)).collect();

        assert!(store_vectors(&mut ns, &vectors(&ids), &stamp).unwrap());

        assert_eq!(stored_ids(&ns), ids);
        assert_eq!(read_vectors(&ns, &stamp).unwrap().len(), ids.len());
    }

    #[test]
    fn a_live_daemon_s_project_may_not_be_taken_by_another_process() {
        // The reindex tool inside a daemon-attached session reaches the same
        // rebuild path a standalone session does, so refusing at the call sites
        // that remember to check is not enough - the delete itself refuses.
        let mut daemon = crate::daemon::DaemonHeartbeat::new(
            std::path::PathBuf::from("/tmp/some-project"),
            "some-project".to_string(),
        );

        daemon.pid = u32::MAX;
        assert!(owned_by_another_process(Some(&daemon)));

        // The daemon indexes its own workspace, so it must not lock itself out.
        daemon.pid = std::process::id();
        assert!(!owned_by_another_process(Some(&daemon)));

        // No live daemon - a stale heartbeat is discarded before this point -
        // leaves the project free to claim.
        assert!(!owned_by_another_process(None));
    }

    #[test]
    fn an_unwatched_project_can_be_claimed_and_rebuilt() {
        let mut ns = namespace();
        let stamp = embed_text_id(MODEL_A, true, true);

        assert!(claim_project(&mut ns, UNWATCHED, &stamp).unwrap());
        assert!(store_vectors(&mut ns, &vectors(&[1]), &stamp).unwrap());
        assert_eq!(read_vectors(&ns, &stamp).unwrap().len(), 1);
    }

    #[test]
    fn needs_word_split_only_when_words_are_not_already_separated() {
        // Delimited names already tokenize into words, so prepending the split
        // form repeats what the embedder sees. Measured as a wash on Rust
        // (+1.3% R@1) and a small loss on SystemVerilog (-2.5%).
        assert!(!needs_word_split("authenticate_user"));
        assert!(!needs_word_split("axi_lite_slave"));
        assert!(!needs_word_split("kebab-case-name"));

        // These arrive as one rare token and are where splitting paid: +79%
        // R@1 on camelCase, +23% on PascalCase.
        assert!(needs_word_split("getUserById"));
        assert!(needs_word_split("ParseRequestBody"));

        // A leading or trailing delimiter separates nothing, so these reach the
        // embedder as the same single rare token their undecorated siblings do.
        assert!(needs_word_split("_handleClick"));
        assert!(needs_word_split("__privateField"));
        assert!(needs_word_split("parseType_"));
        assert!(needs_word_split("-kebabLeading"));

        // A single lowercase word has no words to separate but splits to itself;
        // build_embed_text's own equality guard drops it, so this predicate
        // does not need to.
        assert!(needs_word_split("foo"));
    }

    #[test]
    fn build_embed_text_splits_camel_case_but_leaves_snake_case_alone() {
        let graph = CodeGraph::in_memory().unwrap();
        let node = codegraph::Node::new(
            0,
            codegraph::NodeType::Function,
            PropertyMap::new().with("signature", "fn get(id: u64) -> User"),
        );

        // camelCase arrives as one rare token; the split words are front-loaded.
        let camel = QueryEngine::build_embed_text(&node, 0, "getUserById", false, true, &graph);
        assert!(
            camel.starts_with("get user by id"),
            "camelCase should be split, got: {camel}"
        );

        // snake_case already tokenizes into the same words, so prepending them
        // only repeats the name - measured as a wash to a small loss.
        let snake = QueryEngine::build_embed_text(&node, 0, "get_user_by_id", false, true, &graph);
        assert!(
            snake.starts_with("get_user_by_id"),
            "snake_case must be left as-is, got: {snake}"
        );
        assert!(!snake.starts_with("get user by id"));

        // A leading underscore is not a word boundary: `_handleClick` is one
        // rare token exactly as `handleClick` is, so it gets the same split.
        let prefixed = QueryEngine::build_embed_text(&node, 0, "_handleClick", false, true, &graph);
        assert!(
            prefixed.starts_with("handle click"),
            "prefixed camelCase should be split, got: {prefixed}"
        );
    }

    #[test]
    fn build_embed_text_prepends_split_name_only_when_enabled() {
        let graph = CodeGraph::in_memory().unwrap();
        let node = codegraph::Node::new(
            0,
            codegraph::NodeType::Function,
            PropertyMap::new().with("signature", "fn getUserById(id: u64) -> User"),
        );

        // Enabled: split name words are front-loaded for the static embedder.
        let with_split =
            QueryEngine::build_embed_text(&node, 0, "getUserById", false, true, &graph);
        assert!(
            with_split.starts_with("get user by id"),
            "got: {with_split}"
        );

        // Explicitly disabled: the original raw-name text, as shipped
        // through 0.20.1.
        let without = QueryEngine::build_embed_text(&node, 0, "getUserById", false, false, &graph);
        assert!(without.starts_with("getUserById"), "got: {without}");
        assert!(!without.contains("get user by id"));
    }
}
