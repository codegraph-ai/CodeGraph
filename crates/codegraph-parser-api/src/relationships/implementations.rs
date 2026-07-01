// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

/// Represents trait/interface implementation
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ImplementationRelation {
    /// Implementing class
    pub implementor: String,

    /// Trait/interface being implemented
    pub trait_name: String,
}

impl ImplementationRelation {
    pub fn new(implementor: impl Into<String>, trait_name: impl Into<String>) -> Self {
        Self {
            implementor: implementor.into(),
            trait_name: trait_name.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_sets_both_fields() {
        let r = ImplementationRelation::new("MyStruct", "Display");
        assert_eq!(r.implementor, "MyStruct");
        assert_eq!(r.trait_name, "Display");
    }

    #[test]
    fn accepts_string_and_str_inputs() {
        let r = ImplementationRelation::new(String::from("S"), "T");
        assert_eq!(r.implementor, "S");
        assert_eq!(r.trait_name, "T");
    }

    #[test]
    fn equality_and_hash_distinguish_pairs() {
        use std::collections::HashSet;
        let a = ImplementationRelation::new("S", "T");
        let b = ImplementationRelation::new("S", "T");
        let c = ImplementationRelation::new("S", "U");
        assert_eq!(a, b);
        assert_ne!(a, c);
        let mut set = HashSet::new();
        set.insert(a);
        assert!(!set.insert(b));
        assert!(set.insert(c));
    }

    #[test]
    fn round_trips_through_json() {
        let r = ImplementationRelation::new("S", "T");
        let json = serde_json::to_string(&r).unwrap();
        let back: ImplementationRelation = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
    }
}
