// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

fn main() {
    // tree-sitter-perl 1.1.x requires tree-sitter ^0.26 which conflicts with our 0.25.
    // We vendor parser.c and scanner.c directly.
    let src_dir = std::path::Path::new("tree-sitter-perl-src");

    let mut build = cc::Build::new();
    build
        .include(src_dir)
        .file(src_dir.join("parser.c"))
        .file(src_dir.join("scanner.c"))
        .warnings(false);

    let ts_include = src_dir.join("tree_sitter");
    if ts_include.exists() {
        build.include(ts_include.parent().unwrap());
    }

    if let Ok(ts_include_dir) = std::env::var("DEP_TREE_SITTER_INCLUDE") {
        if !ts_include_dir.is_empty() {
            build.include(&ts_include_dir);
        }
    }

    build.compile("tree_sitter_perl");

    println!("cargo:rerun-if-changed=build.rs");
    println!(
        "cargo:rerun-if-changed={}",
        src_dir.join("parser.c").display()
    );
}
