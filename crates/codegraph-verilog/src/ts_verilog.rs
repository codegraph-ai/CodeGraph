// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Bridge to the tree-sitter-verilog grammar via extern C binding.
//!
//! Despite its name, tree-sitter-verilog v1.0 IS the SystemVerilog grammar
//! (see its README: "SystemVerilog grammar for tree-sitter"). It uses ABI 14,
//! compatible with tree-sitter 0.22. We call the underlying C symbol directly
//! and wrap it with Language::from_raw since the 1.0 crate uses the newer
//! tree-sitter-language API incompatible with tree-sitter 0.22's Rust bindings.

extern "C" {
    fn tree_sitter_verilog() -> *const std::ffi::c_void;
}

/// Get the tree-sitter Language for SystemVerilog/Verilog
pub fn language() -> tree_sitter::Language {
    // SAFETY: tree_sitter_verilog() returns a valid TSLanguage pointer (ABI 14)
    unsafe { tree_sitter::Language::from_raw(tree_sitter_verilog() as *const _) }
}
