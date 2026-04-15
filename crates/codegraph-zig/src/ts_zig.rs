// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Bindings to the vendored tree-sitter-zig grammar

extern "C" {
    fn tree_sitter_zig() -> *const ();
}

/// The tree-sitter LanguageFn for Zig
pub const LANGUAGE: tree_sitter_language::LanguageFn =
    unsafe { tree_sitter_language::LanguageFn::from_raw(tree_sitter_zig) };

/// Get the tree-sitter Language for Zig
pub(crate) fn language() -> tree_sitter::Language {
    // SAFETY: tree_sitter_zig() returns a valid TSLanguage pointer.
    LANGUAGE.into()
}
