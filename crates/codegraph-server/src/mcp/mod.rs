// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! MCP (Model Context Protocol) Server Module
//!
//! This module implements an MCP server for CodeGraph, allowing AI clients
//! like Claude Desktop, Cursor, and Cline to interact with the code graph.
//!
//! ## Usage
//!
//! ```bash
//! codegraph-server --mcp --workspace /path/to/project
//! ```
//!
//! The MCP server communicates via stdio using JSON-RPC 2.0.

pub mod file_watcher;
pub mod pro_hooks;
pub mod protocol;
pub mod resources;
pub mod server;
pub mod tools;
pub mod transport;

pub use protocol::*;
pub use server::McpServer;
