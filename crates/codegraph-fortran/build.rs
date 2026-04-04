// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

fn main() {
    // tree-sitter-fortran v0.5.1 uses the newer tree-sitter-language API which is
    // incompatible with tree-sitter 0.22. We use the C symbol directly via extern "C".
    // The grammar has both parser.c and scanner.c (external scanner).
    // We recompile the source ourselves to guarantee the symbols are available.

    let cargo_home = std::env::var("CARGO_HOME").unwrap_or_else(|_| {
        format!(
            "{}/.cargo",
            std::env::var("HOME").unwrap_or_else(|_| "/root".to_string())
        )
    });

    let registry_src = std::path::PathBuf::from(&cargo_home).join("registry/src");

    let fortran_src = find_fortran_src(&registry_src).expect(
        "Could not find tree-sitter-fortran source in Cargo registry. \
         Ensure tree-sitter-fortran is listed in [build-dependencies].",
    );

    let parser_c = fortran_src.join("parser.c");
    assert!(
        parser_c.exists(),
        "tree-sitter-fortran parser.c not found at: {:?}",
        parser_c
    );

    let scanner_c = fortran_src.join("scanner.c");

    let mut build = cc::Build::new();
    build.file(&parser_c).include(&fortran_src).warnings(false);

    if scanner_c.exists() {
        build.file(&scanner_c);
    }

    // Include tree_sitter subdir if present
    let ts_include = fortran_src.join("tree_sitter");
    if ts_include.exists() {
        build.include(&ts_include);
    }

    if let Ok(ts_include_dir) = std::env::var("DEP_TREE_SITTER_INCLUDE") {
        if !ts_include_dir.is_empty() {
            build.include(&ts_include_dir);
        }
    }

    build.compile("tree_sitter_fortran");

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={}", parser_c.display());
    if scanner_c.exists() {
        println!("cargo:rerun-if-changed={}", scanner_c.display());
    }
}

fn find_fortran_src(registry_src: &std::path::Path) -> Option<std::path::PathBuf> {
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
                if pkg_name.starts_with("tree-sitter-fortran-") && pkg_path.is_dir() {
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
