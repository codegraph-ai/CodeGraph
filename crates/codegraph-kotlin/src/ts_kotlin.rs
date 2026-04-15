// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Bindings to the vendored tree-sitter-kotlin grammar

extern "C" {
    fn tree_sitter_kotlin() -> *const ();
}

/// The tree-sitter [`LanguageFn`] for Kotlin
pub const LANGUAGE: tree_sitter_language::LanguageFn =
    unsafe { tree_sitter_language::LanguageFn::from_raw(tree_sitter_kotlin) };

/// Get the tree-sitter Language for Kotlin
pub fn language() -> tree_sitter::Language {
    LANGUAGE.into()
}
