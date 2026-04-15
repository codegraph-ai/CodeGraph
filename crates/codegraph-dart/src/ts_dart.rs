// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Bindings to the vendored tree-sitter-dart grammar

extern "C" {
    fn tree_sitter_dart() -> *const ();
}

/// The tree-sitter LanguageFn for Dart
pub const LANGUAGE: tree_sitter_language::LanguageFn =
    unsafe { tree_sitter_language::LanguageFn::from_raw(tree_sitter_dart) };

/// Get the tree-sitter Language for Dart
pub(crate) fn language() -> tree_sitter::Language {
    // SAFETY: tree_sitter_dart() returns a valid TSLanguage pointer.
    LANGUAGE.into()
}
