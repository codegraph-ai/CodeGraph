// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Git history mining module for extracting memories from commit history.
//!
//! This module provides functionality to:
//! - Parse git commit history
//! - Extract debug contexts, architectural decisions, and known issues
//! - Link mined memories to code graph nodes

mod error;
mod executor;
mod miner;
mod parser;

pub use error::GitMiningError;
pub use executor::GitExecutor;
pub use miner::{GitMiner, MiningConfig, MiningResult};
pub use parser::{CommitInfo, CommitPattern, ParsedCommit};
