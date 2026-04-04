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
