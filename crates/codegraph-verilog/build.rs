// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

fn main() {
    // tree-sitter-verilog v1.0 uses the newer tree-sitter-language API which is
    // incompatible with tree-sitter 0.22. We use the C symbol directly via extern "C".
    // The tree-sitter-verilog crate compiles parser.c into a static lib, but its
    // cargo:rustc-link-lib directive may not propagate reliably without a `links` key.
    // We recompile the source ourselves to guarantee the symbol is available.
    //
    // Note: despite its name, tree-sitter-verilog IS the SystemVerilog grammar (ABI 14).

    let cargo_home = std::env::var("CARGO_HOME").unwrap_or_else(|_| {
        format!(
            "{}/.cargo",
            std::env::var("HOME").unwrap_or_else(|_| "/root".to_string())
        )
    });

    let registry_src = std::path::PathBuf::from(&cargo_home).join("registry/src");

    // Find the tree-sitter-verilog source directory
    let verilog_src = find_verilog_src(&registry_src).expect(
        "Could not find tree-sitter-verilog source in Cargo registry. \
         Ensure tree-sitter-verilog is listed in [dependencies].",
    );

    let parser_c = verilog_src.join("parser.c");
    assert!(
        parser_c.exists(),
        "tree-sitter-verilog parser.c not found at: {:?}",
        parser_c
    );

    let ts_include = verilog_src.join("tree_sitter");

    let mut build = cc::Build::new();
    build.file(&parser_c).include(&verilog_src).warnings(false);

    if ts_include.exists() {
        build.include(&ts_include);
    }

    // Also include tree-sitter headers if available
    if let Ok(ts_include_dir) = std::env::var("DEP_TREE_SITTER_INCLUDE") {
        if !ts_include_dir.is_empty() {
            build.include(&ts_include_dir);
        }
    }

    build.compile("tree_sitter_verilog");

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={}", parser_c.display());
}

fn find_verilog_src(registry_src: &std::path::Path) -> Option<std::path::PathBuf> {
    // Walk registry src dirs to find tree-sitter-verilog
    let Ok(entries) = std::fs::read_dir(registry_src) else {
        return None;
    };

    for index_entry in entries.flatten() {
        let index_path = index_entry.path();
        if !index_path.is_dir() {
            continue;
        }
        // Each subdir is an index mirror like "index.crates.io-..."
        if let Ok(pkg_entries) = std::fs::read_dir(&index_path) {
            for pkg_entry in pkg_entries.flatten() {
                let pkg_path = pkg_entry.path();
                let pkg_name = pkg_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if pkg_name.starts_with("tree-sitter-verilog-") && pkg_path.is_dir() {
                    let src = pkg_path.join("src");
                    if src.join("parser.c").exists() {
                        return Some(src);
                    }
                }
            }
        }
    }
    None
}
