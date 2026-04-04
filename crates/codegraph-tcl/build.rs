// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

fn main() {
    let src_dir = std::path::Path::new("tree-sitter-tcl-src");

    let mut build = cc::Build::new();
    build
        .include(src_dir)
        .file(src_dir.join("parser.c"))
        .file(src_dir.join("scanner.c"))
        .warnings(false);

    // For tree-sitter headers
    let ts_header_dir = std::env::var("DEP_TREE_SITTER_INCLUDE").unwrap_or_default();
    if !ts_header_dir.is_empty() {
        build.include(&ts_header_dir);
    }

    build.compile("tree_sitter_tcl");
}
