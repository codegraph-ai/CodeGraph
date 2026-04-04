// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

/// Represents a file/module
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModuleEntity {
    /// Module name (usually filename without extension)
    pub name: String,

    /// Full path to the file
    pub path: String,

    /// Language identifier
    pub language: String,

    /// Number of lines
    pub line_count: usize,

    /// Documentation/module docstring
    pub doc_comment: Option<String>,

    /// Module-level attributes/pragmas
    pub attributes: Vec<String>,
}

impl ModuleEntity {
    pub fn new(
        name: impl Into<String>,
        path: impl Into<String>,
        language: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
            language: language.into(),
            line_count: 0,
            doc_comment: None,
            attributes: Vec::new(),
        }
    }

    pub fn with_line_count(mut self, count: usize) -> Self {
        self.line_count = count;
        self
    }

    pub fn with_doc(mut self, doc: impl Into<String>) -> Self {
        self.doc_comment = Some(doc.into());
        self
    }

    pub fn with_attributes(mut self, attrs: Vec<String>) -> Self {
        self.attributes = attrs;
        self
    }
}
