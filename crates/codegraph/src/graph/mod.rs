// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Core graph types and operations.
//!
//! This module defines the fundamental building blocks:
//! - [`Node`]: Graph nodes representing code entities
//! - [`Edge`]: Directed relationships between nodes
//! - [`CodeGraph`]: The main graph database interface

pub mod algorithms;
mod codegraph;
mod property;
mod types;

pub use codegraph::CodeGraph;
pub use property::{PropertyMap, PropertyValue};
pub use types::{Direction, Edge, EdgeId, EdgeType, Node, NodeId, NodeType};
