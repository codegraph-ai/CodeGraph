// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Bindings to the vendored tree-sitter-tcl grammar

extern "C" {
    fn tree_sitter_tcl() -> *const ();
}

/// The tree-sitter [`LanguageFn`] for Tcl
pub const LANGUAGE: tree_sitter_language::LanguageFn =
    unsafe { tree_sitter_language::LanguageFn::from_raw(tree_sitter_tcl) };

/// Get the tree-sitter Language for Tcl
pub fn language() -> tree_sitter::Language {
    LANGUAGE.into()
}
