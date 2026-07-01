// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use super::function::FunctionEntity;
use serde::{Deserialize, Serialize};

/// Represents a class field/attribute
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Field {
    /// Field name
    pub name: String,

    /// Type annotation (if available)
    pub type_annotation: Option<String>,

    /// Visibility: "public", "private", "protected"
    pub visibility: String,

    /// Is this a static/class field?
    pub is_static: bool,

    /// Is this a constant?
    pub is_constant: bool,

    /// Default value
    pub default_value: Option<String>,
}

impl Field {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            type_annotation: None,
            visibility: "public".to_string(),
            is_static: false,
            is_constant: false,
            default_value: None,
        }
    }

    pub fn with_type(mut self, type_ann: impl Into<String>) -> Self {
        self.type_annotation = Some(type_ann.into());
        self
    }

    pub fn with_visibility(mut self, vis: impl Into<String>) -> Self {
        self.visibility = vis.into();
        self
    }

    pub fn static_field(mut self) -> Self {
        self.is_static = true;
        self
    }

    pub fn constant(mut self) -> Self {
        self.is_constant = true;
        self
    }

    pub fn with_default(mut self, default: impl Into<String>) -> Self {
        self.default_value = Some(default.into());
        self
    }
}

/// Represents a class/struct in any language
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClassEntity {
    /// Class name
    pub name: String,

    /// Visibility: "public", "private", "internal"
    pub visibility: String,

    /// Starting line number (1-indexed)
    pub line_start: usize,

    /// Ending line number (1-indexed)
    pub line_end: usize,

    /// Is this an abstract class?
    pub is_abstract: bool,

    /// Is this an interface/trait definition?
    pub is_interface: bool,

    /// Base classes (inheritance)
    pub base_classes: Vec<String>,

    /// Interfaces/traits implemented
    pub implemented_traits: Vec<String>,

    /// Methods in this class
    pub methods: Vec<FunctionEntity>,

    /// Fields/attributes
    pub fields: Vec<Field>,

    /// Documentation/docstring
    pub doc_comment: Option<String>,

    /// Decorators/attributes
    pub attributes: Vec<String>,

    /// Generic type parameters (if any)
    pub type_parameters: Vec<String>,

    /// First ~1024 chars of the class body, captured at parse time.
    pub body_prefix: Option<String>,
}

impl ClassEntity {
    pub fn new(name: impl Into<String>, line_start: usize, line_end: usize) -> Self {
        Self {
            name: name.into(),
            visibility: "public".to_string(),
            line_start,
            line_end,
            is_abstract: false,
            is_interface: false,
            base_classes: Vec::new(),
            implemented_traits: Vec::new(),
            methods: Vec::new(),
            fields: Vec::new(),
            doc_comment: None,
            attributes: Vec::new(),
            type_parameters: Vec::new(),
            body_prefix: None,
        }
    }

    pub fn with_visibility(mut self, vis: impl Into<String>) -> Self {
        self.visibility = vis.into();
        self
    }

    pub fn abstract_class(mut self) -> Self {
        self.is_abstract = true;
        self
    }

    pub fn interface(mut self) -> Self {
        self.is_interface = true;
        self
    }

    pub fn with_bases(mut self, bases: Vec<String>) -> Self {
        self.base_classes = bases;
        self
    }

    pub fn with_traits(mut self, traits: Vec<String>) -> Self {
        self.implemented_traits = traits;
        self
    }

    pub fn with_methods(mut self, methods: Vec<FunctionEntity>) -> Self {
        self.methods = methods;
        self
    }

    pub fn with_fields(mut self, fields: Vec<Field>) -> Self {
        self.fields = fields;
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

    pub fn with_type_parameters(mut self, type_params: Vec<String>) -> Self {
        self.type_parameters = type_params;
        self
    }

    pub fn with_body_prefix(mut self, body: impl Into<String>) -> Self {
        self.body_prefix = Some(body.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_new_defaults() {
        let f = Field::new("count");
        assert_eq!(f.name, "count");
        assert_eq!(f.type_annotation, None);
        assert_eq!(f.visibility, "public");
        assert!(!f.is_static);
        assert!(!f.is_constant);
        assert_eq!(f.default_value, None);
    }

    #[test]
    fn field_builder_chain() {
        let f = Field::new("MAX")
            .with_type("u32")
            .with_visibility("private")
            .static_field()
            .constant()
            .with_default("100");
        assert_eq!(f.type_annotation, Some("u32".to_string()));
        assert_eq!(f.visibility, "private");
        assert!(f.is_static);
        assert!(f.is_constant);
        assert_eq!(f.default_value, Some("100".to_string()));
    }

    #[test]
    fn class_new_defaults() {
        let c = ClassEntity::new("Widget", 1, 20);
        assert_eq!(c.name, "Widget");
        assert_eq!(c.visibility, "public");
        assert_eq!(c.line_start, 1);
        assert_eq!(c.line_end, 20);
        assert!(!c.is_abstract);
        assert!(!c.is_interface);
        assert!(c.base_classes.is_empty());
        assert!(c.implemented_traits.is_empty());
        assert!(c.methods.is_empty());
        assert!(c.fields.is_empty());
        assert_eq!(c.doc_comment, None);
        assert!(c.attributes.is_empty());
        assert!(c.type_parameters.is_empty());
        assert_eq!(c.body_prefix, None);
    }

    #[test]
    fn class_builder_covers_all_setters() {
        let methods = vec![FunctionEntity::new("render", 2, 4)];
        let fields = vec![Field::new("id")];
        let c = ClassEntity::new("View", 1, 30)
            .with_visibility("internal")
            .abstract_class()
            .interface()
            .with_bases(vec!["Base".to_string()])
            .with_traits(vec!["Drawable".to_string()])
            .with_methods(methods.clone())
            .with_fields(fields.clone())
            .with_doc("a view")
            .with_attributes(vec!["@component".to_string()])
            .with_type_parameters(vec!["T".to_string()])
            .with_body_prefix("class View {}");
        assert_eq!(c.visibility, "internal");
        assert!(c.is_abstract);
        assert!(c.is_interface);
        assert_eq!(c.base_classes, vec!["Base".to_string()]);
        assert_eq!(c.implemented_traits, vec!["Drawable".to_string()]);
        assert_eq!(c.methods, methods);
        assert_eq!(c.fields, fields);
        assert_eq!(c.doc_comment, Some("a view".to_string()));
        assert_eq!(c.attributes, vec!["@component".to_string()]);
        assert_eq!(c.type_parameters, vec!["T".to_string()]);
        assert_eq!(c.body_prefix, Some("class View {}".to_string()));
    }

    #[test]
    fn class_serde_round_trip() {
        let c = ClassEntity::new("Rt", 1, 2).with_fields(vec![Field::new("x")]);
        let json = serde_json::to_string(&c).unwrap();
        let back: ClassEntity = serde_json::from_str(&json).unwrap();
        assert_eq!(c, back);
    }

    #[test]
    fn field_serde_round_trip_all_fields() {
        // Field's derived Serialize/Deserialize is only ever exercised nested
        // inside ClassEntity; pin the standalone round-trip with every field
        // populated so the Option arms and exact snake_case wire names are covered.
        let f = Field::new("MAX")
            .with_type("u32")
            .with_visibility("private")
            .static_field()
            .constant()
            .with_default("100");
        let json = serde_json::to_string(&f).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["name"], "MAX");
        assert_eq!(v["type_annotation"], "u32");
        assert_eq!(v["visibility"], "private");
        assert_eq!(v["is_static"], true);
        assert_eq!(v["is_constant"], true);
        assert_eq!(v["default_value"], "100");
        let back: Field = serde_json::from_str(&json).unwrap();
        assert_eq!(f, back);
    }

    #[test]
    fn field_eq_and_hash_dedup_in_set() {
        // Field uniquely derives Eq + Hash (ClassEntity only has PartialEq);
        // exercise those derives by using Field as a HashSet key.
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(Field::new("x").with_type("i32"));
        set.insert(Field::new("x").with_type("i32")); // equal -> collapses
        set.insert(Field::new("x").with_type("i64")); // differs by type -> distinct
        set.insert(Field::new("y").with_type("i32")); // differs by name -> distinct
        assert_eq!(set.len(), 3);
        assert!(set.contains(&Field::new("x").with_type("i32")));
    }
}
