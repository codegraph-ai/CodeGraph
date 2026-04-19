// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Bindings to the vendored tree-sitter-dockerfile grammar

extern "C" {
    fn tree_sitter_dockerfile() -> *const ();
}

/// The tree-sitter [`LanguageFn`] for Dockerfile
pub const LANGUAGE: tree_sitter_language::LanguageFn =
    unsafe { tree_sitter_language::LanguageFn::from_raw(tree_sitter_dockerfile) };

/// Get the tree-sitter Language for Dockerfile
pub fn language() -> tree_sitter::Language {
    LANGUAGE.into()
}
