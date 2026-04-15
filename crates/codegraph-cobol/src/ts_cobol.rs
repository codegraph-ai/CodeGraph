// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Bindings to the vendored tree-sitter-cobol grammar.
//!
//! The grammar function is exported as `tree_sitter_COBOL` (uppercase) by the
//! vendored parser.c. We call it via extern "C" and wrap with Language::from_raw
//! since the grammar may use a different ABI version than the tree-sitter crate.

extern "C" {
    fn tree_sitter_COBOL() -> *const ();
}

/// The tree-sitter LanguageFn for COBOL
pub const LANGUAGE: tree_sitter_language::LanguageFn =
    unsafe { tree_sitter_language::LanguageFn::from_raw(tree_sitter_COBOL) };

/// Get the tree-sitter Language for COBOL
pub fn language() -> tree_sitter::Language {
    // SAFETY: tree_sitter_COBOL() returns a valid TSLanguage pointer.
    LANGUAGE.into()
}
