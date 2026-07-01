// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

/// Represents class inheritance
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InheritanceRelation {
    /// Child class
    pub child: String,

    /// Parent class
    pub parent: String,

    /// Inheritance order (for multiple inheritance)
    pub order: usize,
}

impl InheritanceRelation {
    pub fn new(child: impl Into<String>, parent: impl Into<String>) -> Self {
        Self {
            child: child.into(),
            parent: parent.into(),
            order: 0,
        }
    }

    pub fn with_order(mut self, order: usize) -> Self {
        self.order = order;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_defaults_order_to_zero() {
        let r = InheritanceRelation::new("Child", "Parent");
        assert_eq!(r.child, "Child");
        assert_eq!(r.parent, "Parent");
        assert_eq!(r.order, 0);
    }

    #[test]
    fn with_order_sets_order() {
        let r = InheritanceRelation::new("Child", "Parent").with_order(3);
        assert_eq!(r.order, 3);
    }

    #[test]
    fn accepts_string_and_str_inputs() {
        let r = InheritanceRelation::new(String::from("C"), "P");
        assert_eq!(r.child, "C");
        assert_eq!(r.parent, "P");
    }

    #[test]
    fn equality_and_hash_consider_order() {
        use std::collections::HashSet;
        let a = InheritanceRelation::new("C", "P").with_order(1);
        let b = InheritanceRelation::new("C", "P").with_order(1);
        let c = InheritanceRelation::new("C", "P").with_order(2);
        assert_eq!(a, b);
        assert_ne!(a, c);
        let mut set = HashSet::new();
        set.insert(a);
        assert!(!set.insert(b));
        assert!(set.insert(c));
    }

    #[test]
    fn round_trips_through_json() {
        let r = InheritanceRelation::new("C", "P").with_order(2);
        let json = serde_json::to_string(&r).unwrap();
        let back: InheritanceRelation = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn serializes_exact_wire_format() {
        let r = InheritanceRelation::new("Dog", "Animal").with_order(1);
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(json, r#"{"child":"Dog","parent":"Animal","order":1}"#);
    }
}
