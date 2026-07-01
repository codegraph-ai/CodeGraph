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
}
