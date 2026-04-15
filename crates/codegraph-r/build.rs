// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

fn main() {
    let src_dir = std::path::Path::new("tree-sitter-r-src");

    let mut build = cc::Build::new();
    build
        .include(src_dir)
        .file(src_dir.join("parser.c"))
        .file(src_dir.join("scanner.c"))
        .warnings(false);

    if let Ok(ts_include_dir) = std::env::var("DEP_TREE_SITTER_INCLUDE") {
        if !ts_include_dir.is_empty() {
            build.include(&ts_include_dir);
        }
    }

    build.compile("tree_sitter_r");

    println!("cargo:rerun-if-changed=build.rs");
    println!(
        "cargo:rerun-if-changed={}",
        src_dir.join("parser.c").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        src_dir.join("scanner.c").display()
    );
}
