// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

/// Represents a type reference — an entity using a type in an annotation.
/// E.g., function `buildGraph(params: DependencyGraphParams)` references `DependencyGraphParams`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TypeReference {
    /// The entity that references the type (function, class, or interface name)
    pub referrer: String,

    /// The referenced type name
    pub type_name: String,

    /// Line number where the type reference occurs
    pub line_number: usize,
}

impl TypeReference {
    pub fn new(referrer: impl Into<String>, type_name: impl Into<String>, line: usize) -> Self {
        Self {
            referrer: referrer.into(),
            type_name: type_name.into(),
            line_number: line,
        }
    }
}
