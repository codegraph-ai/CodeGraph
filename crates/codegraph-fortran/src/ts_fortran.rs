// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Bridge to the tree-sitter-fortran grammar via extern C binding.
//!
//! tree-sitter-fortran v0.5.1 uses the newer tree-sitter-language API which is
//! incompatible with tree-sitter 0.22's Rust bindings. We call the underlying
//! C symbol directly and wrap it with Language::from_raw (ABI-compatible).

extern "C" {
    fn tree_sitter_fortran() -> *const ();
}

/// The tree-sitter LanguageFn for Fortran
pub const LANGUAGE: tree_sitter_language::LanguageFn =
    unsafe { tree_sitter_language::LanguageFn::from_raw(tree_sitter_fortran) };

/// Get the tree-sitter Language for Fortran
pub fn language() -> tree_sitter::Language {
    // SAFETY: tree_sitter_fortran() returns a valid TSLanguage pointer
    LANGUAGE.into()
}
