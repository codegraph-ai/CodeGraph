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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_new_defaults() {
        let m = ModuleEntity::new("lib", "/src/lib.rs", "rust");
        assert_eq!(m.name, "lib");
        assert_eq!(m.path, "/src/lib.rs");
        assert_eq!(m.language, "rust");
        assert_eq!(m.line_count, 0);
        assert_eq!(m.doc_comment, None);
        assert!(m.attributes.is_empty());
    }

    #[test]
    fn module_builder_covers_all_setters() {
        let m = ModuleEntity::new("app", "/src/app.py", "python")
            .with_line_count(250)
            .with_doc("app module")
            .with_attributes(vec!["# type: ignore".to_string()]);
        assert_eq!(m.line_count, 250);
        assert_eq!(m.doc_comment, Some("app module".to_string()));
        assert_eq!(m.attributes, vec!["# type: ignore".to_string()]);
    }

    #[test]
    fn module_serde_round_trip() {
        let m = ModuleEntity::new("rt", "/rt.rs", "rust").with_line_count(3);
        let json = serde_json::to_string(&m).unwrap();
        let back: ModuleEntity = serde_json::from_str(&json).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn accepts_string_and_str_inputs() {
        let m = ModuleEntity::new(String::from("m"), "/m.rs", String::from("rust"))
            .with_doc(String::from("d"));
        assert_eq!(m.name, "m");
        assert_eq!(m.language, "rust");
        assert_eq!(m.doc_comment, Some("d".to_string()));
    }

    #[test]
    fn with_attributes_replaces_rather_than_appends() {
        let m = ModuleEntity::new("m", "/m.rs", "rust")
            .with_attributes(vec!["a".to_string()])
            .with_attributes(vec!["b".to_string(), "c".to_string()]);
        assert_eq!(m.attributes, vec!["b".to_string(), "c".to_string()]);
    }

    #[test]
    fn equality_considers_all_fields() {
        let base = ModuleEntity::new("m", "/m.rs", "rust");
        assert_eq!(base, ModuleEntity::new("m", "/m.rs", "rust"));
        assert_ne!(
            base,
            ModuleEntity::new("m", "/m.rs", "rust").with_line_count(1)
        );
        assert_ne!(base, ModuleEntity::new("m", "/m.rs", "rust").with_doc("d"));
        assert_ne!(base, ModuleEntity::new("m", "/other.rs", "rust"));
    }
}
