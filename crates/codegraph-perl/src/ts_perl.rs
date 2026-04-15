// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Bindings to the vendored tree-sitter-perl grammar

extern "C" {
    fn tree_sitter_perl() -> *const ();
}

/// The tree-sitter [`LanguageFn`] for Perl
pub const LANGUAGE: tree_sitter_language::LanguageFn =
    unsafe { tree_sitter_language::LanguageFn::from_raw(tree_sitter_perl) };

/// Get the tree-sitter Language for Perl
pub fn language() -> tree_sitter::Language {
    LANGUAGE.into()
}
