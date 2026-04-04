// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Bindings to the vendored tree-sitter-tcl grammar

extern "C" {
    fn tree_sitter_tcl() -> *const std::ffi::c_void;
}

/// Get the tree-sitter Language for Tcl
pub fn language() -> tree_sitter::Language {
    unsafe { tree_sitter::Language::from_raw(tree_sitter_tcl() as *const _) }
}
