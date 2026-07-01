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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_sets_all_fields() {
        let t = TypeReference::new("buildGraph", "DependencyGraphParams", 12);
        assert_eq!(t.referrer, "buildGraph");
        assert_eq!(t.type_name, "DependencyGraphParams");
        assert_eq!(t.line_number, 12);
    }

    #[test]
    fn serde_round_trip() {
        let t = TypeReference::new("f", "T", 3);
        let json = serde_json::to_string(&t).unwrap();
        let back: TypeReference = serde_json::from_str(&json).unwrap();
        assert_eq!(t, back);
    }
}
