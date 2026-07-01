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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trait_new_defaults() {
        let t = TraitEntity::new("Drawable", 5, 15);
        assert_eq!(t.name, "Drawable");
        assert_eq!(t.visibility, "public");
        assert_eq!(t.line_start, 5);
        assert_eq!(t.line_end, 15);
        assert!(t.required_methods.is_empty());
        assert!(t.parent_traits.is_empty());
        assert_eq!(t.doc_comment, None);
        assert!(t.attributes.is_empty());
    }

    #[test]
    fn trait_builder_covers_all_setters() {
        let methods = vec![FunctionEntity::new("draw", 1, 2)];
        let t = TraitEntity::new("Widget", 1, 20)
            .with_visibility("private")
            .with_methods(methods.clone())
            .with_parent_traits(vec!["Base".to_string()])
            .with_doc("widget trait")
            .with_attributes(vec!["#[async_trait]".to_string()]);
        assert_eq!(t.visibility, "private");
        assert_eq!(t.required_methods, methods);
        assert_eq!(t.parent_traits, vec!["Base".to_string()]);
        assert_eq!(t.doc_comment, Some("widget trait".to_string()));
        assert_eq!(t.attributes, vec!["#[async_trait]".to_string()]);
    }

    #[test]
    fn trait_serde_round_trip() {
        let t = TraitEntity::new("Rt", 1, 2).with_parent_traits(vec!["P".to_string()]);
        let json = serde_json::to_string(&t).unwrap();
        let back: TraitEntity = serde_json::from_str(&json).unwrap();
        assert_eq!(t, back);
    }
}
