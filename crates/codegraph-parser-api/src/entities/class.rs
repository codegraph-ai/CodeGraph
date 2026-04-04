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
