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

    #[test]
    fn accepts_string_and_str_inputs() {
        let t = TypeReference::new(String::from("f"), "T", 1);
        assert_eq!(t.referrer, "f");
        assert_eq!(t.type_name, "T");
    }

    #[test]
    fn line_zero_is_allowed() {
        let t = TypeReference::new("f", "T", 0);
        assert_eq!(t.line_number, 0);
    }

    #[test]
    fn equality_considers_all_fields() {
        let a = TypeReference::new("f", "T", 5);
        let b = TypeReference::new("f", "T", 5);
        let diff_line = TypeReference::new("f", "T", 6);
        let diff_type = TypeReference::new("f", "U", 5);
        let diff_referrer = TypeReference::new("g", "T", 5);
        assert_eq!(a, b);
        assert_ne!(a, diff_line);
        assert_ne!(a, diff_type);
        assert_ne!(a, diff_referrer);
    }

    #[test]
    fn hashes_by_value_in_set() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(TypeReference::new("f", "T", 5));
        assert!(!set.insert(TypeReference::new("f", "T", 5)));
        assert!(set.insert(TypeReference::new("f", "T", 6)));
    }

    #[test]
    fn clone_produces_equal_value() {
        let t = TypeReference::new("buildGraph", "DependencyGraphParams", 12);
        assert_eq!(t.clone(), t);
    }
}
