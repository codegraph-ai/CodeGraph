// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

fn main() {
    // tree-sitter-toml-ng 0.7 ships parser.c in its src/ directory.
    // We compile it directly from the cargo registry source so we can link
    // it against tree-sitter 0.25 (the LanguageFn / tree-sitter-language ABI).

    let cargo_home = std::env::var("CARGO_HOME").unwrap_or_else(|_| {
        format!(
            "{}/.cargo",
            std::env::var("HOME").unwrap_or_else(|_| "/root".to_string())
        )
    });

    let registry_src = std::path::PathBuf::from(&cargo_home).join("registry/src");

    let toml_src = find_grammar_src(&registry_src, "tree-sitter-toml-ng-").expect(
        "Could not find tree-sitter-toml-ng source in Cargo registry. \
         Ensure tree-sitter-toml-ng is listed in [build-dependencies].",
    );

    let parser_c = toml_src.join("parser.c");
    assert!(
        parser_c.exists(),
        "tree-sitter-toml-ng parser.c not found at: {:?}",
        parser_c
    );

    let mut build = cc::Build::new();
    build.file(&parser_c).include(&toml_src).warnings(false);

    // scanner.c if present
    let scanner_c = toml_src.join("scanner.c");
    if scanner_c.exists() {
        build.file(&scanner_c);
    }

    // scanner.cc (C++ scanner) if present
    let scanner_cc = toml_src.join("scanner.cc");
    if scanner_cc.exists() {
        build.file(&scanner_cc);
    }

    let ts_include = toml_src.join("tree_sitter");
    if ts_include.exists() {
        build.include(&ts_include);
    }

    if let Ok(ts_include_dir) = std::env::var("DEP_TREE_SITTER_INCLUDE") {
        if !ts_include_dir.is_empty() {
            build.include(&ts_include_dir);
        }
    }

    build.compile("tree_sitter_toml");

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
