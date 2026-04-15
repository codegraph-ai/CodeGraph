// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

fn main() {
    // tree-sitter-kotlin 0.3.x uses the old tree-sitter <0.23 API.
    // We compile parser.c and scanner.c directly from the cargo registry source.

    let cargo_home = std::env::var("CARGO_HOME").unwrap_or_else(|_| {
        format!(
            "{}/.cargo",
            std::env::var("HOME").unwrap_or_else(|_| "/root".to_string())
        )
    });

    let registry_src = std::path::PathBuf::from(&cargo_home).join("registry/src");

    let kotlin_src = find_grammar_src(&registry_src, "tree-sitter-kotlin-").expect(
        "Could not find tree-sitter-kotlin source in Cargo registry. \
         Ensure tree-sitter-kotlin is listed in [build-dependencies].",
    );

    let parser_c = kotlin_src.join("parser.c");
    assert!(
        parser_c.exists(),
        "tree-sitter-kotlin parser.c not found at: {:?}",
        parser_c
    );

    let mut build = cc::Build::new();
    build.file(&parser_c).include(&kotlin_src).warnings(false);

    // scanner.c if present
    let scanner_c = kotlin_src.join("scanner.c");
    if scanner_c.exists() {
        build.file(&scanner_c);
    }

    let ts_include = kotlin_src.join("tree_sitter");
    if ts_include.exists() {
        build.include(&ts_include);
    }

    if let Ok(ts_include_dir) = std::env::var("DEP_TREE_SITTER_INCLUDE") {
        if !ts_include_dir.is_empty() {
            build.include(&ts_include_dir);
        }
    }

    build.compile("tree_sitter_kotlin");

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={}", parser_c.display());
}

fn find_grammar_src(
    registry_src: &std::path::Path,
    prefix: &str,
) -> Option<std::path::PathBuf> {
    let Ok(entries) = std::fs::read_dir(registry_src) else {
        return None;
    };

    for index_entry in entries.flatten() {
        let index_path = index_entry.path();
        if !index_path.is_dir() {
            continue;
        }
        if let Ok(pkg_entries) = std::fs::read_dir(&index_path) {
            for pkg_entry in pkg_entries.flatten() {
                let pkg_path = pkg_entry.path();
                let pkg_name = pkg_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if pkg_name.starts_with(prefix) && pkg_path.is_dir() {
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
