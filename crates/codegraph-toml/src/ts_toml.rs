// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Bindings to tree-sitter-toml-ng
//!
//! tree-sitter-toml-ng 0.7 depends on `tree-sitter-language` (not `tree-sitter` ≤ 0.22),
//! so it is directly compatible with our tree-sitter 0.25 runtime without vendoring.

/// Get the tree-sitter Language for TOML
pub fn language() -> tree_sitter::Language {
    tree_sitter_toml_ng::LANGUAGE.into()
}
