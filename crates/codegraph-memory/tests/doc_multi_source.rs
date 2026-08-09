// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! End-to-end regression test for issue #16.
//!
//! The unit tests in `docs.rs` prove that chunk ids differ between sources.
//! This one proves the thing the user actually reported: indexing a second
//! markdown file used to delete most of the first file's chunks from RocksDB,
//! so `list_doc_sources` and `search_docs` stopped returning them - while
//! indexing still reported success.
//!
//! It drives the real `DocStore`: real RocksDB keys, real embeddings, real
//! HNSW search, and a reopen to confirm what survived on disk.
//!
//! Needs a local model2vec model directory, which is also what the
//! `--embedding-model static` server path uses. Point `CODEGRAPH_STATIC_MODEL`
//! at one, or have the default `~/.codegraph/static_models/jina-code-static-256`
//! in place. The test skips (with a message) when no model is available rather
//! than failing, since the model is not vendored in the repo.

use codegraph_memory::{DocStore, VectorEngine};
use std::path::PathBuf;
use std::sync::Arc;

fn static_model_dir() -> Option<PathBuf> {
    let dir = match std::env::var("CODEGRAPH_STATIC_MODEL") {
        Ok(v) => PathBuf::from(v),
        Err(_) => dirs_home()?
            .join(".codegraph")
            .join("static_models")
            .join("jina-code-static-256"),
    };
    dir.join("model.safetensors").exists().then_some(dir)
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Ten sections, so the document is comfortably larger than the second one.
fn architecture_md() -> String {
    let mut md = String::from("# Architecture Guide\n\n");
    for i in 1..=10 {
        md.push_str(&format!(
            "## Subsystem {i}\n\nThe subsystem-{i} component owns marker-architecture-{i} and \
             is responsible for coordinating work across the graph engine. It keeps its own \
             state and reports progress to the supervisor.\n\n"
        ));
    }
    md
}

/// Three sections - smaller than the guide above, which is what made the
/// original data loss look intermittent.
fn onboarding_md() -> String {
    let mut md = String::from("# Onboarding Guide\n\n");
    for i in 1..=3 {
        md.push_str(&format!(
            "## Step {i}\n\nFollow step-{i} to set up your workstation; marker-onboarding-{i} \
             covers the tools you need before your first change lands.\n\n"
        ));
    }
    md
}

#[test]
fn indexing_a_second_source_does_not_evict_the_first() {
    let Some(model_dir) = static_model_dir() else {
        eprintln!("skipping: no static embedding model available (set CODEGRAPH_STATIC_MODEL)");
        return;
    };

    let tmp = tempfile::tempdir().expect("temp dir");
    let arch_path = tmp.path().join("architecture.md");
    let onboard_path = tmp.path().join("onboarding.md");
    std::fs::write(&arch_path, architecture_md()).expect("write architecture.md");
    std::fs::write(&onboard_path, onboarding_md()).expect("write onboarding.md");

    let engine = Arc::new(VectorEngine::with_static_model(&model_dir).expect("static engine"));
    let db_path = tmp.path().join("docs.db");

    let arch_indexed;
    let onboard_indexed;
    {
        let store = DocStore::new(&db_path, Arc::clone(&engine)).expect("open store");
        arch_indexed = store
            .index_file(&arch_path, 500)
            .expect("index architecture.md")
            .len();
        onboard_indexed = store
            .index_file(&onboard_path, 500)
            .expect("index onboarding.md")
            .len();

        assert!(arch_indexed > onboard_indexed, "sanity: sizes differ");
        println!("indexed architecture.md -> {arch_indexed} chunks");
        println!("indexed onboarding.md   -> {onboard_indexed} chunks");

        let sources = store.list_sources();
        println!("list_doc_sources        -> {} source(s)", sources.len());
        assert_eq!(sources.len(), 2, "both sources must be listed: {sources:?}");

        // The first file must still have every chunk it was indexed with.
        let arch_source = arch_path.to_string_lossy().to_string();
        let arch_stored = store.get_chunks_by_source(&arch_source).len();
        println!("chunks still stored for architecture.md -> {arch_stored}");
        assert_eq!(
            arch_stored, arch_indexed,
            "indexing the second file must not drop chunks from the first"
        );

        // And it must still be findable, which is the user-visible symptom.
        let hits = store.search("marker-architecture-7 subsystem", 3).expect("search");
        for hit in &hits {
            let file = std::path::Path::new(&hit.chunk.source_file);
            println!(
                "search_docs hit         -> {} § {} ({:.2})",
                file.file_name().unwrap_or_default().to_string_lossy(),
                hit.chunk.title,
                hit.score
            );
        }
        assert!(
            hits.iter().any(|h| h.chunk.source_file == arch_source),
            "search must still reach the first document"
        );
    }

    // Reopen: chunk ids are RocksDB keys, so a collision would show up as
    // missing rows after a restart too.
    let store = DocStore::new(&db_path, engine).expect("reopen store");
    println!(
        "after reopen            -> {} source(s), architecture.md {} chunks, onboarding.md {} chunks",
        store.list_sources().len(),
        store.get_chunks_by_source(&arch_path.to_string_lossy()).len(),
        store
            .get_chunks_by_source(&onboard_path.to_string_lossy())
            .len(),
    );
    assert_eq!(store.list_sources().len(), 2, "both sources survive a reopen");
    assert_eq!(
        store
            .get_chunks_by_source(&arch_path.to_string_lossy())
            .len(),
        arch_indexed,
        "first document survives a reopen intact"
    );
    assert_eq!(
        store
            .get_chunks_by_source(&onboard_path.to_string_lossy())
            .len(),
        onboard_indexed,
        "second document survives a reopen intact"
    );
}
