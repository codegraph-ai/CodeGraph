// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Domain layer — transport-agnostic business logic.
//!
//! This module contains the core domain operations shared by both the LSP
//! and MCP transports. Functions here know nothing about JSON-RPC, tower-lsp,
//! or MCP protocol types.

pub(crate) mod ai_context;
pub(crate) mod call_graph;
pub(crate) mod callers;
pub(crate) mod complexity;
pub(crate) mod curated_context;
pub(crate) mod dependency_graph;
pub(crate) mod edit_context;
pub(crate) mod impact;
pub(crate) mod node_props;
pub(crate) mod node_resolution;
pub(crate) mod related_tests;
pub(crate) mod source_code;
pub(crate) mod symbol_info;
pub(crate) mod unused_code;
