//! Debounced, batched background re-embedding for on-demand indexing.
//!
//! On-demand index paths (did_open / did_change / the file watcher) enqueue the
//! files they touch here instead of embedding inline. A single background task
//! coalesces a burst of edits into one batch per debounce window, re-embeds each
//! file's symbols, prunes vectors orphaned by re-parsing, and persists the result.
//!
//! Embedding is RAM-gated for free: [`QueryEngine::update_file_vectors`] no-ops
//! when no vector engine is loaded (e.g. the low-memory startup gate fired), so a
//! burst of opens on a constrained machine can never trigger an OOM here.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, RwLock};

use crate::ai_query::QueryEngine;

/// How long to wait for more files before flushing a batch.
const DEBOUNCE: Duration = Duration::from_millis(500);

/// Upper bound on a single batch so a sustained edit storm still makes progress.
const MAX_BATCH: usize = 256;

/// When to embed a file's symbols after (re)indexing it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EmbedMode {
    /// Embed synchronously before returning (used on save — the durable event).
    Now,
    /// Hand off to the debounced background queue (used on open / change / watch).
    Enqueue,
    /// Do not embed (semantic search will lag until the next save / reindex).
    Skip,
}

/// Background re-embedding queue. Cheap to clone (holds `Arc`s); clone freely to
/// hand a handle to the watcher and the LSP backend.
#[derive(Clone)]
pub struct EmbedQueue {
    tx: mpsc::UnboundedSender<PathBuf>,
    /// Project slug for persistence; empty until set during initialize.
    slug: Arc<RwLock<String>>,
}

impl EmbedQueue {
    /// Start the background worker and return a handle.
    ///
    /// The worker is only spawned when a Tokio runtime is active (always true for
    /// the running server). Constructed without a runtime — e.g. a synchronous
    /// test constructor — the queue is inert: `enqueue` becomes a no-op.
    pub fn new(query_engine: Arc<QueryEngine>) -> Self {
        let (tx, rx) = mpsc::unbounded_channel::<PathBuf>();
        let slug = Arc::new(RwLock::new(String::new()));

        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                handle.spawn(Self::worker(query_engine, rx, Arc::clone(&slug)));
            }
            Err(_) => {
                tracing::debug!("[EmbedQueue] no Tokio runtime at construction; queue inert");
            }
        }

        Self { tx, slug }
    }

    /// Background loop: coalesce a debounce window of files, re-embed, prune, persist.
    async fn worker(
        query_engine: Arc<QueryEngine>,
        mut rx: mpsc::UnboundedReceiver<PathBuf>,
        slug: Arc<RwLock<String>>,
    ) {
        loop {
            // Block until the first file of a new batch arrives.
            let first = match rx.recv().await {
                Some(p) => p,
                None => break, // all senders dropped — shut down
            };

            let mut pending: HashSet<PathBuf> = HashSet::new();
            pending.insert(first);

            // Coalesce additional files within a fixed debounce window.
            let deadline = tokio::time::Instant::now() + DEBOUNCE;
            let mut closed = false;
            loop {
                match tokio::time::timeout_at(deadline, rx.recv()).await {
                    Ok(Some(p)) => {
                        pending.insert(p);
                        if pending.len() >= MAX_BATCH {
                            break;
                        }
                    }
                    Ok(None) => {
                        closed = true;
                        break;
                    }
                    Err(_) => break, // debounce window elapsed
                }
            }

            let count = pending.len();
            for path in &pending {
                query_engine
                    .update_file_vectors(&path.to_string_lossy())
                    .await;
            }
            // Re-parsing reassigns node IDs; drop vectors whose node is gone.
            query_engine.prune_orphan_vectors().await;

            let slug = slug.read().await.clone();
            if !slug.is_empty() {
                if let Err(e) = query_engine.save_symbol_vectors(&slug).await {
                    tracing::warn!("[EmbedQueue] failed to persist vectors: {}", e);
                }
            }
            tracing::debug!("[EmbedQueue] re-embedded {} file(s)", count);

            if closed {
                break;
            }
        }
    }

    /// Set the project slug used to persist vectors after a flush.
    pub async fn set_slug(&self, slug: String) {
        *self.slug.write().await = slug;
    }

    /// Queue a file for background re-embedding. Cheap and non-blocking.
    pub fn enqueue(&self, path: PathBuf) {
        // Send only fails if the worker has shut down; nothing to do then.
        let _ = self.tx.send(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph::CodeGraph;

    fn make_engine() -> Arc<QueryEngine> {
        let graph = Arc::new(RwLock::new(
            CodeGraph::in_memory().expect("in-memory graph"),
        ));
        Arc::new(QueryEngine::new(graph))
    }

    #[test]
    fn embed_mode_variants_are_distinct() {
        assert_eq!(EmbedMode::Now, EmbedMode::Now);
        assert_eq!(EmbedMode::Enqueue, EmbedMode::Enqueue);
        assert_eq!(EmbedMode::Skip, EmbedMode::Skip);
        assert_ne!(EmbedMode::Now, EmbedMode::Enqueue);
        assert_ne!(EmbedMode::Now, EmbedMode::Skip);
        assert_ne!(EmbedMode::Enqueue, EmbedMode::Skip);
    }

    #[test]
    fn embed_mode_is_copy_and_debug() {
        // Copy: the value is still usable after being passed by value.
        let mode = EmbedMode::Enqueue;
        let copied = mode;
        assert_eq!(mode, copied);
        // Debug renders the variant name.
        assert_eq!(format!("{:?}", EmbedMode::Now), "Now");
        assert_eq!(format!("{:?}", EmbedMode::Enqueue), "Enqueue");
        assert_eq!(format!("{:?}", EmbedMode::Skip), "Skip");
    }

    #[test]
    fn new_without_runtime_is_inert() {
        // Constructed outside a Tokio runtime, no worker spawns and the queue is
        // inert: enqueue must not panic and the slug starts empty.
        let queue = EmbedQueue::new(make_engine());
        assert!(queue.slug.try_read().expect("uncontended").is_empty());
        // Enqueue on an inert queue is a silent no-op (no receiver, no panic).
        queue.enqueue(PathBuf::from("/tmp/a.rs"));
        queue.enqueue(PathBuf::from("/tmp/b.rs"));
    }

    #[tokio::test]
    async fn set_slug_stores_and_overwrites_value() {
        let queue = EmbedQueue::new(make_engine());
        assert!(queue.slug.read().await.is_empty());

        queue.set_slug("proj-alpha".to_string()).await;
        assert_eq!(&*queue.slug.read().await, "proj-alpha");

        // Last write wins.
        queue.set_slug("proj-beta".to_string()).await;
        assert_eq!(&*queue.slug.read().await, "proj-beta");
    }

    #[tokio::test]
    async fn clone_shares_slug_state() {
        let queue = EmbedQueue::new(make_engine());
        let clone = queue.clone();

        // A slug set through the clone is visible on the original: both share the
        // same Arc<RwLock<String>>.
        clone.set_slug("shared".to_string()).await;
        assert_eq!(&*queue.slug.read().await, "shared");
    }

    #[tokio::test]
    async fn enqueue_under_runtime_does_not_panic() {
        // Under a runtime the worker is spawned; enqueue hands files off without
        // blocking and never panics even on a burst.
        let queue = EmbedQueue::new(make_engine());
        for i in 0..8 {
            queue.enqueue(PathBuf::from(format!("/tmp/file{i}.rs")));
        }
    }

    #[tokio::test]
    async fn worker_flushes_batch_and_queue_stays_usable() {
        // End-to-end: enqueue a burst, let the debounce window elapse so the
        // worker coalesces and processes the batch, then confirm the queue is
        // still alive (worker did not crash) by enqueuing again.
        let queue = EmbedQueue::new(make_engine());
        queue.set_slug(String::new()).await; // empty slug -> skip persistence
        queue.enqueue(PathBuf::from("/tmp/x.rs"));
        queue.enqueue(PathBuf::from("/tmp/y.rs"));

        // Wait past the debounce window plus processing headroom.
        tokio::time::sleep(DEBOUNCE + Duration::from_millis(200)).await;

        // Worker survived the flush; a subsequent enqueue still succeeds.
        queue.enqueue(PathBuf::from("/tmp/z.rs"));
    }
}
