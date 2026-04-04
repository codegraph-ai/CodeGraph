// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AI Agent Query Engine
//!
//! This module implements fast, composable query primitives for AI agents
//! to explore codebases. The design optimizes for:
//!
//! - **Speed**: Sub-10ms queries for simple operations, sub-20ms for graph traversals
//! - **Composition**: AI agents chain 5-10 queries per user question
//! - **Rich metadata**: Structural information over similarity scores
//! - **Explainability**: Clear reasons for why results match

mod engine;
mod primitives;
mod text_index;

pub use engine::QueryEngine;
pub use primitives::*;
pub use text_index::{Posting, TextIndex, TextIndexBuilder};
