// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

// tree-sitter-dockerfile-updated requires tree-sitter ^0.20 which conflicts
// with our 0.25. We compile parser.c / scanner.c directly using cc, taking
// the source from one of two locations:
//
//  1. A vendored copy at `tree-sitter-dockerfile-src/` (preferred — fully
//     reproducible, no cargo registry path discovery needed).
//  2. The cargo registry source for `tree-sitter-dockerfile-updated` declared
//     as a build-dependency (fallback — used when a vendored copy is absent).
//
// The first time this crate builds, the cargo registry path acts as the source
// of truth; you may then copy those files into `tree-sitter-dockerfile-src/`
// to vendor them permanently and remove the build-dependency.

use std::path::{Path, PathBuf};

fn vendored_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tree-sitter-dockerfile-src")
}

/// Locate the tree-sitter-dockerfile-updated source in the cargo registry.
///
/// We look in `$CARGO_HOME/registry/src/<index>/tree-sitter-dockerfile-updated-*`,
/// preferring the highest version present.
fn locate_registry_source() -> Option<PathBuf> {
    let cargo_home = std::env::var("CARGO_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::var("HOME")
                .map(|h| PathBuf::from(h).join(".cargo"))
                .unwrap_or_default()
        });

    let registry_src = cargo_home.join("registry").join("src");
    let entries = std::fs::read_dir(&registry_src).ok()?;

    let mut best: Option<(String, PathBuf)> = None;
    for entry in entries.flatten() {
        let index_dir = entry.path();
        if !index_dir.is_dir() {
            continue;
        }
        let inner = match std::fs::read_dir(&index_dir) {
            Ok(it) => it,
            Err(_) => continue,
        };
        for sub in inner.flatten() {
            let name = sub.file_name().to_string_lossy().into_owned();
            if let Some(version) = name.strip_prefix("tree-sitter-dockerfile-updated-") {
                let candidate = sub.path().join("src");
                if candidate.join("parser.c").exists() {
                    let v = version.to_string();
                    let take = match &best {
                        None => true,
                        Some((current, _)) => v.as_str() > current.as_str(),
                    };
                    if take {
                        best = Some((v, candidate));
                    }
                }
            }
        }
    }
    best.map(|(_, p)| p)
}

fn pick_source_dir() -> PathBuf {
    let vendored = vendored_dir();
    if vendored.join("parser.c").exists() {
        return vendored;
    }
    locate_registry_source().expect(
        "Could not locate tree-sitter-dockerfile grammar source. \
         Run `cargo fetch` or vendor parser.c/scanner.c into \
         crates/codegraph-dockerfile/tree-sitter-dockerfile-src/",
    )
}

fn main() {
    let src_dir = pick_source_dir();
    eprintln!(
        "codegraph-dockerfile: using grammar source from {}",
        src_dir.display()
    );

    let mut build = cc::Build::new();
    build
        .include(&src_dir)
        .file(src_dir.join("parser.c"))
        .warnings(false);

    let scanner_c = src_dir.join("scanner.c");
    if scanner_c.exists() {
        build.file(&scanner_c);
    }

    let scanner_cc = src_dir.join("scanner.cc");
    if scanner_cc.exists() {
        build.file(&scanner_cc).cpp(true);
    }

    let ts_include = src_dir.join("tree_sitter");
    if ts_include.exists() {
        build.include(ts_include.parent().unwrap());
    }

    if let Ok(ts_include_dir) = std::env::var("DEP_TREE_SITTER_INCLUDE") {
        if !ts_include_dir.is_empty() {
            build.include(&ts_include_dir);
        }
    }

    build.compile("tree_sitter_dockerfile");

    println!("cargo:rerun-if-changed=build.rs");
    rerun_if_present(&src_dir.join("parser.c"));
    rerun_if_present(&scanner_c);
    rerun_if_present(&scanner_cc);
}

fn rerun_if_present(p: &Path) {
    if p.exists() {
        println!("cargo:rerun-if-changed={}", p.display());
    }
}
