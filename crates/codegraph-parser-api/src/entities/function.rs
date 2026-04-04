// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use crate::complexity::ComplexityMetrics;
use serde::{Deserialize, Serialize};

/// Maximum characters to capture for function body prefix embeddings.
pub const BODY_PREFIX_MAX_CHARS: usize = 1024;

/// Represents a function parameter
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Parameter {
    /// Parameter name
    pub name: String,

    /// Type annotation (if available)
    pub type_annotation: Option<String>,

    /// Default value (if any)
    pub default_value: Option<String>,

    /// Is this a variadic parameter? (e.g., *args, **kwargs)
    pub is_variadic: bool,
}

impl Parameter {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            type_annotation: None,
            default_value: None,
            is_variadic: false,
        }
    }

    pub fn with_type(mut self, type_ann: impl Into<String>) -> Self {
        self.type_annotation = Some(type_ann.into());
        self
    }

    pub fn with_default(mut self, default: impl Into<String>) -> Self {
        self.default_value = Some(default.into());
        self
    }

    pub fn variadic(mut self) -> Self {
        self.is_variadic = true;
        self
    }
}

/// Represents a function/method in any language
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionEntity {
    /// Function name
    pub name: String,

    /// Full signature (including parameters and return type)
    pub signature: String,

    /// Visibility: "public", "private", "protected", "internal"
    pub visibility: String,

    /// Starting line number (1-indexed)
    pub line_start: usize,

    /// Ending line number (1-indexed)
    pub line_end: usize,

    /// Is this an async/coroutine function?
    pub is_async: bool,

    /// Is this a test function?
    pub is_test: bool,

    /// Is this a static method?
    pub is_static: bool,

    /// Is this an abstract method?
    pub is_abstract: bool,

    /// Function parameters
    pub parameters: Vec<Parameter>,

    /// Return type annotation (if available)
    pub return_type: Option<String>,

    /// Documentation/docstring
    pub doc_comment: Option<String>,

    /// Decorators/attributes (e.g., `@property`, `@deprecated`)
    pub attributes: Vec<String>,

    /// Parent class (if this is a method)
    pub parent_class: Option<String>,

    /// Complexity metrics for this function
    pub complexity: Option<ComplexityMetrics>,

    /// First ~1024 chars of the function body, captured at parse time.
    /// Used for full-body embeddings without disk I/O.
    pub body_prefix: Option<String>,
}

impl FunctionEntity {
    pub fn new(name: impl Into<String>, line_start: usize, line_end: usize) -> Self {
        let name = name.into();
        Self {
            signature: name.clone(),
            name,
            visibility: "public".to_string(),
            line_start,
            line_end,
            is_async: false,
            is_test: false,
            is_static: false,
            is_abstract: false,
            parameters: Vec::new(),
            return_type: None,
            doc_comment: None,
            attributes: Vec::new(),
            parent_class: None,
            complexity: None,
            body_prefix: None,
        }
    }

    // Builder methods
    pub fn with_signature(mut self, sig: impl Into<String>) -> Self {
        self.signature = sig.into();
        self
    }

    pub fn with_visibility(mut self, vis: impl Into<String>) -> Self {
        self.visibility = vis.into();
        self
    }

    pub fn async_fn(mut self) -> Self {
        self.is_async = true;
        self
    }

    pub fn test_fn(mut self) -> Self {
        self.is_test = true;
        self
    }

    pub fn static_fn(mut self) -> Self {
        self.is_static = true;
        self
    }

    pub fn abstract_fn(mut self) -> Self {
        self.is_abstract = true;
        self
    }

    pub fn with_parameters(mut self, params: Vec<Parameter>) -> Self {
        self.parameters = params;
        self
    }

    pub fn with_return_type(mut self, ret: impl Into<String>) -> Self {
        self.return_type = Some(ret.into());
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

    pub fn with_parent_class(mut self, parent: impl Into<String>) -> Self {
        self.parent_class = Some(parent.into());
        self
    }

    pub fn with_complexity(mut self, metrics: ComplexityMetrics) -> Self {
        self.complexity = Some(metrics);
        self
    }

    pub fn with_body_prefix(mut self, body: impl Into<String>) -> Self {
        self.body_prefix = Some(body.into());
        self
    }

    /// Get the cyclomatic complexity, returning 1 if not calculated
    pub fn cyclomatic_complexity(&self) -> u32 {
        self.complexity
            .as_ref()
            .map(|c| c.cyclomatic_complexity)
            .unwrap_or(1)
    }

    /// Get the complexity grade (A-F), returning 'A' if not calculated
    pub fn complexity_grade(&self) -> char {
        self.complexity.as_ref().map(|c| c.grade()).unwrap_or('A')
    }
}
