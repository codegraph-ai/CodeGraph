// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use super::function::FunctionEntity;
use serde::{Deserialize, Serialize};

/// Represents a trait/protocol/interface definition
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraitEntity {
    /// Trait name
    pub name: String,

    /// Visibility
    pub visibility: String,

    /// Starting line number
    pub line_start: usize,

    /// Ending line number
    pub line_end: usize,

    /// Required methods
    pub required_methods: Vec<FunctionEntity>,

    /// Parent traits (trait inheritance)
    pub parent_traits: Vec<String>,

    /// Documentation
    pub doc_comment: Option<String>,

    /// Attributes/decorators
    pub attributes: Vec<String>,
}

impl TraitEntity {
    pub fn new(name: impl Into<String>, line_start: usize, line_end: usize) -> Self {
        Self {
            name: name.into(),
            visibility: "public".to_string(),
            line_start,
            line_end,
            required_methods: Vec::new(),
            parent_traits: Vec::new(),
            doc_comment: None,
            attributes: Vec::new(),
        }
    }

    pub fn with_visibility(mut self, vis: impl Into<String>) -> Self {
        self.visibility = vis.into();
        self
    }

    pub fn with_methods(mut self, methods: Vec<FunctionEntity>) -> Self {
        self.required_methods = methods;
        self
    }

    pub fn with_parent_traits(mut self, parents: Vec<String>) -> Self {
        self.parent_traits = parents;
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
