// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Bindings to the vendored tree-sitter-r grammar

extern "C" {
    fn tree_sitter_r() -> *const ();
}

/// The tree-sitter LanguageFn for R
pub const LANGUAGE: tree_sitter_language::LanguageFn =
    unsafe { tree_sitter_language::LanguageFn::from_raw(tree_sitter_r) };

/// Get the tree-sitter Language for R
pub(crate) fn language() -> tree_sitter::Language {
    // SAFETY: tree_sitter_r() returns a valid TSLanguage pointer.
    LANGUAGE.into()
}
