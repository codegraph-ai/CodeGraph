// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Tests for parser API types and utilities

use crate::*;
use std::path::PathBuf;

#[test]
fn test_code_ir_new() {
    let path = PathBuf::from("test.rs");
    let ir = CodeIR::new(path.clone());

    assert_eq!(ir.file_path, path);
    assert!(ir.module.is_none());
    assert_eq!(ir.functions.len(), 0);
    assert_eq!(ir.classes.len(), 0);
    assert_eq!(ir.traits.len(), 0);
}

#[test]
fn test_code_ir_entity_count() {
    let mut ir = CodeIR::new(PathBuf::from("test.rs"));

    // Initially empty
    assert_eq!(ir.entity_count(), 0);

    // Add module
    ir.set_module(ModuleEntity::new("test", "test.rs", "rust"));
    assert_eq!(ir.entity_count(), 1);

    // Add function
    ir.add_function(FunctionEntity::new("foo", 1, 5));
    assert_eq!(ir.entity_count(), 2);

    // Add class
    ir.add_class(ClassEntity::new("Bar", 10, 20));
    assert_eq!(ir.entity_count(), 3);

    // Add trait
    ir.add_trait(TraitEntity::new("Baz", 30, 35));
    assert_eq!(ir.entity_count(), 4);
}

#[test]
fn test_code_ir_relationship_count() {
    let mut ir = CodeIR::new(PathBuf::from("test.rs"));

    // Initially empty
    assert_eq!(ir.relationship_count(), 0);

    // Add call
    ir.add_call(CallRelation::new("foo", "bar", 5));
    assert_eq!(ir.relationship_count(), 1);

    // Add import
    ir.add_import(ImportRelation::new("test", "std::io"));
    assert_eq!(ir.relationship_count(), 2);

    // Add inheritance
    ir.add_inheritance(InheritanceRelation::new("Child", "Parent"));
    assert_eq!(ir.relationship_count(), 3);

    // Add implementation
    ir.add_implementation(ImplementationRelation::new("Struct", "Trait"));
    assert_eq!(ir.relationship_count(), 4);
}

#[test]
fn test_function_entity_builder() {
    let func = FunctionEntity::new("test_func", 10, 20)
        .with_signature("fn test_func() -> String")
        .with_visibility("public")
        .async_fn()
        .test_fn();

    assert_eq!(func.name, "test_func");
    assert_eq!(func.line_start, 10);
    assert_eq!(func.line_end, 20);
    assert_eq!(func.signature, "fn test_func() -> String");
    assert_eq!(func.visibility, "public");
    assert!(func.is_async);
    assert!(func.is_test);
}

#[test]
fn test_class_entity_builder() {
    let class = ClassEntity::new("MyClass", 5, 50)
        .with_visibility("public")
        .with_doc("A test class");

    assert_eq!(class.name, "MyClass");
    assert_eq!(class.line_start, 5);
    assert_eq!(class.line_end, 50);
    assert_eq!(class.visibility, "public");
    assert_eq!(class.doc_comment, Some("A test class".to_string()));
}

#[test]
fn test_module_entity_builder() {
    let module = ModuleEntity::new("my_module", "/path/to/file.rs", "rust")
        .with_line_count(100)
        .with_doc("Module documentation");

    assert_eq!(module.name, "my_module");
    assert_eq!(module.path, "/path/to/file.rs");
    assert_eq!(module.language, "rust");
    assert_eq!(module.line_count, 100);
    assert_eq!(module.doc_comment, Some("Module documentation".to_string()));
}

#[test]
fn test_trait_entity_builder() {
    let trait_entity = TraitEntity::new("MyTrait", 1, 10)
        .with_visibility("public")
        .with_doc("Trait docs");

    assert_eq!(trait_entity.name, "MyTrait");
    assert_eq!(trait_entity.line_start, 1);
    assert_eq!(trait_entity.line_end, 10);
    assert_eq!(trait_entity.visibility, "public");
    assert_eq!(trait_entity.doc_comment, Some("Trait docs".to_string()));
}

#[test]
fn test_call_relation_builder() {
    let call = CallRelation::new("caller", "callee", 42);

    assert_eq!(call.caller, "caller");
    assert_eq!(call.callee, "callee");
    assert_eq!(call.call_site_line, 42);
    assert!(call.is_direct);

    // Test indirect call
    let indirect_call = CallRelation::new("foo", "bar", 10).indirect();
    assert!(!indirect_call.is_direct);
}

#[test]
fn test_import_relation_builder() {
    let import = ImportRelation::new("my_module", "std::collections::HashMap")
        .with_alias("HMap")
        .with_symbols(vec!["HashMap".to_string()]);

    assert_eq!(import.importer, "my_module");
    assert_eq!(import.imported, "std::collections::HashMap");
    assert_eq!(import.alias, Some("HMap".to_string()));
    assert_eq!(import.symbols.len(), 1);
    assert_eq!(import.symbols[0], "HashMap");
}

#[test]
fn test_inheritance_relation_builder() {
    let inheritance = InheritanceRelation::new("Dog", "Animal").with_order(0);

    assert_eq!(inheritance.child, "Dog");
    assert_eq!(inheritance.parent, "Animal");
    assert_eq!(inheritance.order, 0);
}

#[test]
fn test_parser_config_defaults() {
    let config = ParserConfig::default();

    assert!(!config.skip_tests);
    assert!(!config.skip_private);
    assert_eq!(config.max_file_size, 10 * 1024 * 1024); // 10MB
    assert!(!config.parallel);
    assert_eq!(config.parallel_workers, None);
    assert!(config.include_docs);
    assert!(config.extract_types);
}

#[test]
fn test_parser_metrics() {
    let mut metrics = ParserMetrics::default();

    assert_eq!(metrics.files_attempted, 0);
    assert_eq!(metrics.files_succeeded, 0);
    assert_eq!(metrics.files_failed, 0);

    metrics.files_attempted = 10;
    metrics.files_succeeded = 8;
    metrics.files_failed = 2;

    assert_eq!(metrics.success_rate(), 0.8);
}
