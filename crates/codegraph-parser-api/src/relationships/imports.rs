// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

/// Represents an import/dependency relationship
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ImportRelation {
    /// Importing module
    pub importer: String,

    /// Imported module
    pub imported: String,

    /// Specific symbols imported (empty = whole module)
    pub symbols: Vec<String>,

    /// Is this a wildcard import?
    pub is_wildcard: bool,

    /// Import alias (if any)
    pub alias: Option<String>,
}

impl ImportRelation {
    pub fn new(importer: impl Into<String>, imported: impl Into<String>) -> Self {
        Self {
            importer: importer.into(),
            imported: imported.into(),
            symbols: Vec::new(),
            is_wildcard: false,
            alias: None,
        }
    }

    pub fn with_symbols(mut self, symbols: Vec<String>) -> Self {
        self.symbols = symbols;
        self
    }

    pub fn wildcard(mut self) -> Self {
        self.is_wildcard = true;
        self
    }

    pub fn with_alias(mut self, alias: impl Into<String>) -> Self {
        self.alias = Some(alias.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_defaults() {
        let i = ImportRelation::new("app", "std::io");
        assert_eq!(i.importer, "app");
        assert_eq!(i.imported, "std::io");
        assert!(i.symbols.is_empty());
        assert!(!i.is_wildcard);
        assert_eq!(i.alias, None);
    }

    #[test]
    fn wildcard_sets_flag() {
        assert!(ImportRelation::new("a", "b").wildcard().is_wildcard);
    }

    #[test]
    fn builder_symbols_and_alias() {
        let i = ImportRelation::new("app", "collections")
            .with_symbols(vec!["Map".to_string(), "Set".to_string()])
            .with_alias("c");
        assert_eq!(i.symbols, vec!["Map".to_string(), "Set".to_string()]);
        assert_eq!(i.alias, Some("c".to_string()));
    }

    #[test]
    fn accepts_string_and_str_inputs() {
        let i = ImportRelation::new(String::from("app"), "std::io").with_alias(String::from("io"));
        assert_eq!(i.importer, "app");
        assert_eq!(i.alias, Some("io".to_string()));
    }

    #[test]
    fn with_symbols_replaces_rather_than_appends() {
        let i = ImportRelation::new("a", "b")
            .with_symbols(vec!["X".to_string()])
            .with_symbols(vec!["Y".to_string(), "Z".to_string()]);
        assert_eq!(i.symbols, vec!["Y".to_string(), "Z".to_string()]);
    }

    #[test]
    fn equality_considers_all_fields() {
        let base = ImportRelation::new("a", "b");
        assert_eq!(base, ImportRelation::new("a", "b"));
        assert_ne!(base, ImportRelation::new("a", "b").wildcard());
        assert_ne!(base, ImportRelation::new("a", "b").with_alias("c"));
        assert_ne!(
            base,
            ImportRelation::new("a", "b").with_symbols(vec!["S".to_string()])
        );
    }

    #[test]
    fn round_trips_through_json() {
        let i = ImportRelation::new("app", "collections")
            .with_symbols(vec!["Map".to_string()])
            .wildcard()
            .with_alias("c");
        let json = serde_json::to_string(&i).unwrap();
        let back: ImportRelation = serde_json::from_str(&json).unwrap();
        assert_eq!(i, back);
    }

    #[test]
    fn serializes_to_exact_json_default() {
        // Pins the default wire contract: multi-word is_wildcard stays snake_case,
        // symbols is an empty array, and the None alias serializes as explicit null
        // (no skip_serializing_if), so downstream consumers can rely on the key existing.
        let i = ImportRelation::new("app", "std::io");
        let json = serde_json::to_string(&i).unwrap();
        assert_eq!(
            json,
            r#"{"importer":"app","imported":"std::io","symbols":[],"is_wildcard":false,"alias":null}"#
        );
    }

    #[test]
    fn serializes_to_exact_json_populated() {
        // Pins the populated arm: is_wildcard:true and the Some alias so an accidental
        // rename_all or field rename of the multi-word is_wildcard field would be caught.
        let i = ImportRelation::new("app", "collections")
            .with_symbols(vec!["Map".to_string(), "Set".to_string()])
            .wildcard()
            .with_alias("c");
        let json = serde_json::to_string(&i).unwrap();
        assert_eq!(
            json,
            r#"{"importer":"app","imported":"collections","symbols":["Map","Set"],"is_wildcard":true,"alias":"c"}"#
        );
    }

    #[test]
    fn eq_and_hash_dedup_in_hashset() {
        use std::collections::HashSet;
        // ImportRelation derives Eq + Hash; exercise both by using it as a set key so
        // structurally-equal relations collapse while any field difference stays distinct.
        let mut set = HashSet::new();
        set.insert(ImportRelation::new("a", "b").with_alias("x"));
        set.insert(ImportRelation::new("a", "b").with_alias("x"));
        assert_eq!(set.len(), 1);
        set.insert(ImportRelation::new("a", "b").wildcard());
        set.insert(ImportRelation::new("a", "b").with_symbols(vec!["S".to_string()]));
        assert_eq!(set.len(), 3);
    }
}
