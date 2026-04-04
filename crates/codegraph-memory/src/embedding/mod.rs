// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Embedding module for semantic search
//!
//! Supports configurable embedding models: Jina Code V2 (768d, code-aware) or BGE-Small (384d, fast).

mod engine;
mod fastembed_embed;

pub use engine::VectorEngine;
pub use fastembed_embed::CodeGraphEmbeddingModel;
