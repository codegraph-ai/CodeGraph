// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST extraction for Python source code using tree-sitter
//!
//! This module parses Python source code and extracts entities and relationships
//! into a CodeIR representation.

use crate::config::ParserConfig;
use crate::visitor::{extract_decorators, extract_docstring};
use codegraph_parser_api::{
    truncate_body_prefix, CallRelation, ClassEntity, CodeIR, ComplexityBuilder, ComplexityMetrics,
    FunctionEntity, ImportRelation, InheritanceRelation, ModuleEntity, Parameter, TraitEntity,
};
use std::path::Path;
use tree_sitter::{Node, Parser};

/// Apply a class extraction result to the IR
fn apply_class_extraction(ir: &mut CodeIR, extraction: Option<ClassExtraction>) {
    if let Some(ext) = extraction {
        for method in ext.methods {
            ir.add_function(method);
        }
        for call in ext.calls {
            ir.add_call(call);
        }
        for inh in ext.inheritance {
            ir.add_inheritance(inh);
        }
        if let Some(class) = ext.class {
            ir.add_class(class);
        }
        if let Some(trait_entity) = ext.trait_entity {
            ir.add_trait(trait_entity);
        }
    }
}

/// Extract all entities and relationships from Python source code
pub fn extract(source: &str, file_path: &Path, config: &ParserConfig) -> Result<CodeIR, String> {
    // Initialize tree-sitter parser
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .map_err(|e| format!("Failed to set language: {e}"))?;

    // Parse the source code
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| "Failed to parse".to_string())?;

    let root_node = tree.root_node();

    // Check for syntax errors
    if root_node.has_error() {
        // Find the first error node for better error reporting
        let mut cursor = root_node.walk();
        for child in root_node.children(&mut cursor) {
            if child.is_error() || child.has_error() {
                return Err(format!(
                    "Syntax error at line {}, column {}: {}",
                    child.start_position().row + 1,
                    child.start_position().column,
                    file_path.display()
                ));
            }
        }
        return Err(format!("Syntax error in {}", file_path.display()));
    }

    let source_bytes = source.as_bytes();

    let mut ir = CodeIR::new(file_path.to_path_buf());

    // Extract module entity
    let module_name = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("module")
        .to_string();

    let line_count = source.lines().count();
    let module = ModuleEntity::new(
        module_name.clone(),
        file_path.display().to_string(),
        "python",
    )
    .with_line_count(line_count);
    ir.set_module(module);

    // Walk through top-level statements
    let mut cursor = root_node.walk();
    for child in root_node.children(&mut cursor) {
        match child.kind() {
            "function_definition" => {
                if let Some(func) = extract_function(source_bytes, child, config, None) {
                    // Extract calls from function body
                    let calls =
                        extract_calls_from_node(source_bytes, child, &func.name, func.line_start);
                    for call in calls {
                        ir.add_call(call);
                    }
                    ir.add_function(func);
                }
            }
            "decorated_definition" => {
                // Handle decorated functions/classes
                if let Some(definition) = find_definition_in_decorated(child) {
                    match definition.kind() {
                        "function_definition" => {
                            if let Some(func) =
                                extract_function(source_bytes, definition, config, None)
                            {
                                let calls = extract_calls_from_node(
                                    source_bytes,
                                    definition,
                                    &func.name,
                                    func.line_start,
                                );
                                for call in calls {
                                    ir.add_call(call);
                                }
                                ir.add_function(func);
                            }
                        }
                        "class_definition" => {
                            apply_class_extraction(
                                &mut ir,
                                extract_class(source_bytes, definition, config),
                            );
                        }
                        _ => {}
                    }
                }
            }
            "class_definition" => {
                apply_class_extraction(&mut ir, extract_class(source_bytes, child, config));
            }
            "import_statement" => {
                let imports = extract_import(source_bytes, child, &module_name);
                for import in imports {
                    ir.add_import(import);
                }
            }
            "import_from_statement" => {
                let imports = extract_import_from(source_bytes, child, &module_name);
                for import in imports {
                    ir.add_import(import);
                }
            }
            _ => {}
        }
    }

    Ok(ir)
}

/// Find the actual function/class definition inside a decorated_definition
fn find_definition_in_decorated(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "function_definition" | "class_definition" => return Some(child),
            _ => {}
        }
    }
    None
}

/// Extract a function entity from a function_definition node
fn extract_function(
    source: &[u8],
    node: Node,
    config: &ParserConfig,
    parent_class: Option<&str>,
) -> Option<FunctionEntity> {
    let name = node
        .child_by_field_name("name")
        .map(|n| n.utf8_text(source).unwrap_or("unknown").to_string())?;

    // Skip private functions if configured
    // In Python: _private is private, __mangled is name-mangled private
    // But __init__, __str__ (dunder methods) should be kept as they're special methods
    let is_dunder = name.starts_with("__") && name.ends_with("__") && name.len() > 4;
    if !config.include_private && name.starts_with('_') && !is_dunder {
        return None;
    }

    // Skip test functions if configured
    if !config.include_tests && (name.starts_with("test_") || name.starts_with("Test")) {
        return None;
    }

    let line_start = node.start_position().row + 1;
    let line_end = node.end_position().row + 1;

    // Check for async
    let is_async = node
        .parent()
        .map(|p| p.kind() == "decorated_definition")
        .unwrap_or(false)
        || has_async_keyword(source, node);

    // Extract parameters
    let parameters = extract_parameters(source, node);

    // Extract return type
    let return_type = node
        .child_by_field_name("return_type")
        .map(|n| n.utf8_text(source).unwrap_or("").to_string());

    // Extract docstring from body
    let doc_comment = node
        .child_by_field_name("body")
        .and_then(|body| extract_docstring(source, body));

    // Check decorators for staticmethod/classmethod
    let decorators = if let Some(parent) = node.parent() {
        if parent.kind() == "decorated_definition" {
            extract_decorators(source, parent)
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    let is_static = decorators.iter().any(|d| d.contains("staticmethod"));
    let is_test = decorators
        .iter()
        .any(|d| d.contains("test") || d.contains("pytest"));

    // Calculate complexity
    let complexity = node
        .child_by_field_name("body")
        .map(|body| calculate_complexity_from_node(source, body));

    let mut func = FunctionEntity::new(&name, line_start, line_end);
    func.visibility = python_visibility(&name);
    func.parameters = parameters;
    func.return_type = return_type;
    func.doc_comment = doc_comment;
    func.is_async = is_async;
    func.is_static = is_static;
    func.is_test = is_test;
    func.attributes = decorators;
    func.complexity = complexity;
    func.body_prefix = node
        .child_by_field_name("body")
        .and_then(|b| b.utf8_text(source).ok())
        .filter(|t| !t.is_empty())
        .map(truncate_body_prefix)
        .map(|t| t.to_string());

    if let Some(class_name) = parent_class {
        func.parent_class = Some(class_name.to_string());
    }

    Some(func)
}

/// Check if a function has async keyword
fn has_async_keyword(source: &[u8], node: Node) -> bool {
    // The function might be inside an async function definition
    if let Some(first_child) = node.child(0) {
        let text = first_child.utf8_text(source).unwrap_or("");
        return text == "async";
    }
    false
}

/// Extract parameters from a function's parameter list
fn extract_parameters(source: &[u8], node: Node) -> Vec<Parameter> {
    let mut params = Vec::new();

    if let Some(params_node) = node.child_by_field_name("parameters") {
        let mut cursor = params_node.walk();
        for child in params_node.children(&mut cursor) {
            match child.kind() {
                "identifier" => {
                    // Simple parameter: x
                    let name = child.utf8_text(source).unwrap_or("unknown").to_string();
                    params.push(Parameter {
                        name,
                        type_annotation: None,
                        default_value: None,
                        is_variadic: false,
                    });
                }
                "typed_parameter" => {
                    // Parameter with type: x: int
                    let name = child
                        .child_by_field_name("name")
                        .or_else(|| child.child(0))
                        .map(|n| n.utf8_text(source).unwrap_or("unknown").to_string())
                        .unwrap_or_else(|| "unknown".to_string());

                    let type_annotation = child
                        .child_by_field_name("type")
                        .map(|n| n.utf8_text(source).unwrap_or("").to_string());

                    params.push(Parameter {
                        name,
                        type_annotation,
                        default_value: None,
                        is_variadic: false,
                    });
                }
                "default_parameter" => {
                    // Parameter with default: x=1 or x: int = 1
                    let name = child
                        .child_by_field_name("name")
                        .or_else(|| child.child(0))
                        .map(|n| n.utf8_text(source).unwrap_or("unknown").to_string())
                        .unwrap_or_else(|| "unknown".to_string());

                    let type_annotation = child
                        .child_by_field_name("type")
                        .map(|n| n.utf8_text(source).unwrap_or("").to_string());

                    let default_value = child
                        .child_by_field_name("value")
                        .map(|n| n.utf8_text(source).unwrap_or("").to_string());

                    params.push(Parameter {
                        name,
                        type_annotation,
                        default_value,
                        is_variadic: false,
                    });
                }
                "typed_default_parameter" => {
                    // Parameter with type and default: x: int = 1
                    let name = child
                        .child_by_field_name("name")
                        .or_else(|| child.child(0))
                        .map(|n| n.utf8_text(source).unwrap_or("unknown").to_string())
                        .unwrap_or_else(|| "unknown".to_string());

                    let type_annotation = child
                        .child_by_field_name("type")
                        .map(|n| n.utf8_text(source).unwrap_or("").to_string());

                    let default_value = child
                        .child_by_field_name("value")
                        .map(|n| n.utf8_text(source).unwrap_or("").to_string());

                    params.push(Parameter {
                        name,
                        type_annotation,
                        default_value,
                        is_variadic: false,
                    });
                }
                "list_splat_pattern" | "dictionary_splat_pattern" => {
                    // *args or **kwargs
                    let name = child
                        .child(1)
                        .map(|n| n.utf8_text(source).unwrap_or("unknown").to_string())
                        .unwrap_or_else(|| "args".to_string());

                    params.push(Parameter {
                        name,
                        type_annotation: None,
                        default_value: None,
                        is_variadic: true,
                    });
                }
                _ => {}
            }
        }
    }

    params
}

/// Result of extracting a class definition — may produce a class or trait entity
struct ClassExtraction {
    class: Option<ClassEntity>,
    trait_entity: Option<TraitEntity>,
    methods: Vec<FunctionEntity>,
    calls: Vec<CallRelation>,
    inheritance: Vec<InheritanceRelation>,
}

/// Known Python enum base classes
const ENUM_BASES: &[&str] = &["Enum", "IntEnum", "Flag", "IntFlag", "StrEnum", "auto"];

/// Known Python abstract base classes / protocols
const ABC_BASES: &[&str] = &["ABC", "ABCMeta", "Protocol"];

/// Extract a class entity with its methods
fn extract_class(source: &[u8], node: Node, config: &ParserConfig) -> Option<ClassExtraction> {
    let name = node
        .child_by_field_name("name")
        .map(|n| n.utf8_text(source).unwrap_or("Class").to_string())?;

    let line_start = node.start_position().row + 1;
    let line_end = node.end_position().row + 1;

    // Extract base classes
    let mut inheritance = Vec::new();
    let mut base_class_names = Vec::new();
    if let Some(bases) = node.child_by_field_name("superclasses") {
        let mut cursor = bases.walk();
        for child in bases.children(&mut cursor) {
            if let Some(base_name) = extract_base_class_name(source, child) {
                base_class_names.push(base_name.clone());
                inheritance.push(InheritanceRelation::new(&name, base_name));
            }
        }
    }

    // Detect enum and ABC/Protocol patterns from base classes
    let is_enum = base_class_names
        .iter()
        .any(|b| ENUM_BASES.iter().any(|e| b.ends_with(e)));
    let is_abc = base_class_names
        .iter()
        .any(|b| ABC_BASES.iter().any(|a| b.ends_with(a)));

    // Extract docstring
    let doc_comment = node
        .child_by_field_name("body")
        .and_then(|body| extract_docstring(source, body));

    // Determine visibility from name prefix
    let visibility = python_visibility(&name);

    // Extract methods and calls
    let mut methods = Vec::new();
    let mut calls = Vec::new();

    if let Some(body) = node.child_by_field_name("body") {
        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            match child.kind() {
                "function_definition" => {
                    if let Some(method) = extract_function(source, child, config, Some(&name)) {
                        let method_qualified_name = format!("{}.{}", name, method.name);
                        let method_calls = extract_calls_from_node(
                            source,
                            child,
                            &method_qualified_name,
                            method.line_start,
                        );
                        calls.extend(method_calls);
                        methods.push(method);
                    }
                }
                "decorated_definition" => {
                    if let Some(definition) = find_definition_in_decorated(child) {
                        if definition.kind() == "function_definition" {
                            if let Some(method) =
                                extract_function(source, definition, config, Some(&name))
                            {
                                let method_qualified_name = format!("{}.{}", name, method.name);
                                let method_calls = extract_calls_from_node(
                                    source,
                                    definition,
                                    &method_qualified_name,
                                    method.line_start,
                                );
                                calls.extend(method_calls);
                                methods.push(method);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // If this is an ABC or Protocol, create a TraitEntity instead
    if is_abc {
        let required_methods: Vec<FunctionEntity> = methods
            .iter()
            .filter(|m| m.attributes.iter().any(|a| a.contains("abstractmethod")))
            .cloned()
            .map(|mut m| {
                m.is_abstract = true;
                m
            })
            .collect();

        let parent_traits: Vec<String> = base_class_names
            .into_iter()
            .filter(|b| !ABC_BASES.iter().any(|a| b.ends_with(a)))
            .collect();

        let trait_entity = TraitEntity {
            name,
            visibility,
            line_start,
            line_end,
            required_methods,
            parent_traits,
            doc_comment,
            attributes: Vec::new(),
        };

        return Some(ClassExtraction {
            class: None,
            trait_entity: Some(trait_entity),
            methods,
            calls,
            inheritance,
        });
    }

    let mut class = ClassEntity::new(&name, line_start, line_end);
    class.doc_comment = doc_comment;
    class.methods = methods.clone();
    class.visibility = visibility;
    class.body_prefix = node
        .child_by_field_name("body")
        .and_then(|b| b.utf8_text(source).ok())
        .filter(|t| !t.is_empty())
        .map(truncate_body_prefix)
        .map(|t| t.to_string());

    if is_enum {
        class.attributes = vec!["enum".to_string()];
    }

    Some(ClassExtraction {
        class: Some(class),
        trait_entity: None,
        methods,
        calls,
        inheritance,
    })
}

/// Determine Python visibility from name prefix conventions
fn python_visibility(name: &str) -> String {
    let is_dunder = name.starts_with("__") && name.ends_with("__") && name.len() > 4;
    if is_dunder {
        // Dunder methods (__init__, __str__, etc.) are public API
        "public".to_string()
    } else if name.starts_with("__") {
        // Name-mangled: __private
        "private".to_string()
    } else if name.starts_with('_') {
        // Convention: _protected
        "protected".to_string()
    } else {
        "public".to_string()
    }
}

/// Extract base class name from an argument node
fn extract_base_class_name(source: &[u8], node: Node) -> Option<String> {
    match node.kind() {
        "identifier" => Some(node.utf8_text(source).unwrap_or("").to_string()),
        "attribute" => {
            // Handle module.ClassName
            Some(node.utf8_text(source).unwrap_or("").to_string())
        }
        _ => None,
    }
}

/// Extract calls from a node (function body, class body, etc.)
fn extract_calls_from_node(
    source: &[u8],
    node: Node,
    caller_name: &str,
    line_offset: usize,
) -> Vec<CallRelation> {
    let mut calls = Vec::new();
    extract_calls_recursive(source, node, caller_name, line_offset, &mut calls);
    calls
}

fn extract_calls_recursive(
    source: &[u8],
    node: Node,
    caller_name: &str,
    _line_offset: usize,
    calls: &mut Vec<CallRelation>,
) {
    if node.kind() == "call" {
        if let Some(func_node) = node.child_by_field_name("function") {
            let callee_name = extract_callee_name(source, func_node);
            if !callee_name.is_empty() {
                let call_line = node.start_position().row + 1;
                calls.push(CallRelation::new(caller_name, &callee_name, call_line));
            }
        }
    }

    // Recurse into children
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        extract_calls_recursive(source, child, caller_name, _line_offset, calls);
    }
}

/// Extract the callee name from a call's function node
fn extract_callee_name(source: &[u8], node: Node) -> String {
    match node.kind() {
        "identifier" => node.utf8_text(source).unwrap_or("").to_string(),
        "attribute" => {
            // Handle obj.method() or self.method()
            node.utf8_text(source).unwrap_or("").to_string()
        }
        _ => String::new(),
    }
}

/// Extract import statement
fn extract_import(source: &[u8], node: Node, importer: &str) -> Vec<ImportRelation> {
    let mut imports = Vec::new();
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        if child.kind() == "dotted_name" || child.kind() == "aliased_import" {
            let module_name = if child.kind() == "aliased_import" {
                child
                    .child_by_field_name("name")
                    .map(|n| n.utf8_text(source).unwrap_or("").to_string())
            } else {
                Some(child.utf8_text(source).unwrap_or("").to_string())
            };

            let alias = if child.kind() == "aliased_import" {
                child
                    .child_by_field_name("alias")
                    .map(|n| n.utf8_text(source).unwrap_or("").to_string())
            } else {
                None
            };

            if let Some(module) = module_name {
                let mut import_rel = ImportRelation::new(importer, &module);
                if let Some(a) = alias {
                    import_rel = import_rel.with_alias(&a);
                }
                imports.push(import_rel);
            }
        }
    }

    imports
}

/// Extract from import statement
fn extract_import_from(source: &[u8], node: Node, importer: &str) -> Vec<ImportRelation> {
    let from_module = node
        .child_by_field_name("module_name")
        .map(|n| n.utf8_text(source).unwrap_or(".").to_string())
        .unwrap_or_else(|| ".".to_string());

    let mut symbols = Vec::new();
    let mut is_wildcard = false;

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "wildcard_import" => {
                is_wildcard = true;
            }
            "dotted_name" | "identifier" => {
                // Skip the module name part
                if child.start_byte()
                    > node
                        .child_by_field_name("module_name")
                        .map_or(0, |n| n.end_byte())
                {
                    symbols.push(child.utf8_text(source).unwrap_or("").to_string());
                }
            }
            "aliased_import" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    symbols.push(name_node.utf8_text(source).unwrap_or("").to_string());
                }
            }
            _ => {}
        }
    }

    if is_wildcard {
        vec![ImportRelation::new(importer, &from_module).wildcard()]
    } else if !symbols.is_empty() {
        vec![ImportRelation::new(importer, &from_module).with_symbols(symbols)]
    } else {
        vec![ImportRelation::new(importer, &from_module)]
    }
}

/// Calculate complexity metrics from a function body node
fn calculate_complexity_from_node(source: &[u8], node: Node) -> ComplexityMetrics {
    let mut builder = ComplexityBuilder::new();
    calculate_complexity_recursive(source, node, &mut builder);
    builder.build()
}

fn calculate_complexity_recursive(source: &[u8], node: Node, builder: &mut ComplexityBuilder) {
    match node.kind() {
        "if_statement" => {
            builder.add_branch();
            builder.enter_scope();

            // Process if body
            if let Some(body) = node.child_by_field_name("consequence") {
                calculate_complexity_recursive(source, body, builder);
            }

            builder.exit_scope();

            // Process elif/else
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                match child.kind() {
                    "elif_clause" => {
                        builder.add_branch();
                        builder.enter_scope();
                        if let Some(body) = child.child_by_field_name("consequence") {
                            calculate_complexity_recursive(source, body, builder);
                        }
                        builder.exit_scope();
                    }
                    "else_clause" => {
                        builder.add_branch();
                        builder.enter_scope();
                        if let Some(body) = child.child_by_field_name("body") {
                            calculate_complexity_recursive(source, body, builder);
                        }
                        builder.exit_scope();
                    }
                    _ => {}
                }
            }

            // Check for logical operators in condition
            if let Some(condition) = node.child_by_field_name("condition") {
                count_logical_operators(source, condition, builder);
            }
        }
        "while_statement" => {
            builder.add_loop();
            builder.enter_scope();

            if let Some(body) = node.child_by_field_name("body") {
                calculate_complexity_recursive(source, body, builder);
            }

            builder.exit_scope();

            // Check condition for logical operators
            if let Some(condition) = node.child_by_field_name("condition") {
                count_logical_operators(source, condition, builder);
            }
        }
        "for_statement" => {
            builder.add_loop();
            builder.enter_scope();

            if let Some(body) = node.child_by_field_name("body") {
                calculate_complexity_recursive(source, body, builder);
            }

            builder.exit_scope();
        }
        "with_statement" => {
            builder.enter_scope();

            if let Some(body) = node.child_by_field_name("body") {
                calculate_complexity_recursive(source, body, builder);
            }

            builder.exit_scope();
        }
        "try_statement" => {
            builder.enter_scope();

            if let Some(body) = node.child_by_field_name("body") {
                calculate_complexity_recursive(source, body, builder);
            }

            builder.exit_scope();

            // Count exception handlers
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "except_clause" {
                    builder.add_exception_handler();
                    builder.enter_scope();
                    let mut except_cursor = child.walk();
                    for except_child in child.children(&mut except_cursor) {
                        if except_child.kind() == "block" {
                            calculate_complexity_recursive(source, except_child, builder);
                        }
                    }
                    builder.exit_scope();
                } else if child.kind() == "finally_clause" {
                    builder.enter_scope();
                    let mut finally_cursor = child.walk();
                    for finally_child in child.children(&mut finally_cursor) {
                        if finally_child.kind() == "block" {
                            calculate_complexity_recursive(source, finally_child, builder);
                        }
                    }
                    builder.exit_scope();
                }
            }
        }
        "match_statement" => {
            // Each match case adds a branch. tree-sitter nests the case_clause
            // nodes inside the match_statement's `body` block rather than making
            // them direct children, so walk descendants to find them.
            let mut cursor = node.walk();
            let case_clauses: Vec<Node> = node
                .child_by_field_name("body")
                .map(|body| {
                    body.children(&mut cursor)
                        .filter(|c| c.kind() == "case_clause")
                        .collect()
                })
                .unwrap_or_default();
            for case_clause in case_clauses {
                builder.add_branch();
                builder.enter_scope();
                // The case body is a `block` child of the case_clause; the
                // grammar exposes no `consequence` field name for it.
                let mut case_cursor = case_clause.walk();
                for case_child in case_clause.children(&mut case_cursor) {
                    if case_child.kind() == "block" {
                        calculate_complexity_recursive(source, case_child, builder);
                    }
                }
                builder.exit_scope();
            }
        }
        "boolean_operator" => {
            // 'and' or 'or' operators
            builder.add_logical_operator();
        }
        "conditional_expression" => {
            // Ternary: a if condition else b
            builder.add_branch();
        }
        "list_comprehension"
        | "set_comprehension"
        | "dictionary_comprehension"
        | "generator_expression" => {
            // Comprehensions with conditions
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "for_in_clause" {
                    builder.add_loop();
                }
                if child.kind() == "if_clause" {
                    builder.add_branch();
                }
            }
        }
        _ => {}
    }

    // Recurse into children (except for already handled cases)
    if !matches!(
        node.kind(),
        "if_statement" | "while_statement" | "for_statement" | "try_statement" | "match_statement"
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            calculate_complexity_recursive(source, child, builder);
        }
    }
}

fn count_logical_operators(_source: &[u8], node: Node, builder: &mut ComplexityBuilder) {
    if node.kind() == "boolean_operator" {
        builder.add_logical_operator();
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        count_logical_operators(_source, child, builder);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_code_ir_new() {
        let path = Path::new("test.py");
        let ir = CodeIR::new(path.to_path_buf());
        assert_eq!(ir.entity_count(), 0);
        assert_eq!(ir.relationship_count(), 0);
    }

    #[test]
    fn test_extract_simple_function() {
        let source = r#"
def greet(name):
    print(f"Hello, {name}")
    return name.upper()
"#;
        let path = Path::new("test.py");
        let config = ParserConfig::default();
        let ir = extract(source, path, &config).unwrap();

        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].name, "greet");
        assert_eq!(ir.functions[0].line_start, 2);
    }

    #[test]
    fn test_extract_class_with_methods() {
        let source = r#"
class Calculator:
    def add(self, a, b):
        return a + b

    def multiply(self, a, b):
        return a * b
"#;
        let path = Path::new("test.py");
        let config = ParserConfig::default();
        let ir = extract(source, path, &config).unwrap();

        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "Calculator");
        assert_eq!(ir.classes[0].line_start, 2);
    }

    #[test]
    fn test_extract_calls() {
        let source = r#"
def main():
    greet("World")
    result = greet("Alice")

def greet(name):
    print(f"Hello, {name}")
"#;
        let path = Path::new("test.py");
        let config = ParserConfig::default();
        let ir = extract(source, path, &config).unwrap();

        assert_eq!(ir.functions.len(), 2);
        assert!(ir.calls.len() >= 2, "Should find at least 2 calls");
    }

    #[test]
    fn test_extract_imports() {
        let source = r#"
import os
import sys
from pathlib import Path
from typing import List, Dict
from collections import *

def main():
    pass
"#;
        let path = Path::new("test.py");
        let config = ParserConfig::default();
        let ir = extract(source, path, &config).unwrap();

        assert!(
            ir.imports.len() >= 4,
            "Should find at least 4 import statements"
        );
    }

    #[test]
    fn test_extract_inheritance() {
        let source = r#"
class Animal:
    def move(self):
        pass

class Dog(Animal):
    def bark(self):
        pass
"#;
        let path = Path::new("test.py");
        let config = ParserConfig::default();
        let ir = extract(source, path, &config).unwrap();

        assert_eq!(ir.classes.len(), 2);
        assert_eq!(ir.inheritance.len(), 1);
        assert_eq!(ir.inheritance[0].child, "Dog");
        assert_eq!(ir.inheritance[0].parent, "Animal");
    }

    #[test]
    fn test_complexity_simple_function() {
        let source = r#"
def simple():
    return 1
"#;
        let path = Path::new("test.py");
        let config = ParserConfig::default();
        let ir = extract(source, path, &config).unwrap();

        assert_eq!(ir.functions.len(), 1);
        let func = &ir.functions[0];
        assert!(func.complexity.is_some());
        let complexity = func.complexity.as_ref().unwrap();
        assert_eq!(complexity.cyclomatic_complexity, 1);
    }

    #[test]
    fn test_complexity_with_branches() {
        let source = r#"
def branching(x):
    if x > 0:
        return 1
    elif x < 0:
        return -1
    else:
        return 0
"#;
        let path = Path::new("test.py");
        let config = ParserConfig::default();
        let ir = extract(source, path, &config).unwrap();

        assert_eq!(ir.functions.len(), 1);
        let func = &ir.functions[0];
        let complexity = func.complexity.as_ref().unwrap();
        assert!(complexity.branches >= 3);
    }

    #[test]
    fn test_complexity_with_loops() {
        let source = r#"
def loopy(items):
    total = 0
    for item in items:
        while item > 0:
            total += 1
            item -= 1
    return total
"#;
        let path = Path::new("test.py");
        let config = ParserConfig::default();
        let ir = extract(source, path, &config).unwrap();

        assert_eq!(ir.functions.len(), 1);
        let func = &ir.functions[0];
        let complexity = func.complexity.as_ref().unwrap();
        assert_eq!(complexity.loops, 2);
    }

    #[test]
    fn test_complexity_with_logical_operators() {
        let source = r#"
def complex_condition(a, b, c):
    if a > 0 and b > 0 or c > 0:
        return True
    return False
"#;
        let path = Path::new("test.py");
        let config = ParserConfig::default();
        let ir = extract(source, path, &config).unwrap();

        assert_eq!(ir.functions.len(), 1);
        let func = &ir.functions[0];
        let complexity = func.complexity.as_ref().unwrap();
        assert!(complexity.logical_operators >= 2);
    }

    #[test]
    fn test_complexity_with_try_except() {
        let source = r#"
def risky():
    try:
        result = dangerous_operation()
    except ValueError:
        result = 0
    except TypeError:
        result = -1
    return result
"#;
        let path = Path::new("test.py");
        let config = ParserConfig::default();
        let ir = extract(source, path, &config).unwrap();

        assert_eq!(ir.functions.len(), 1);
        let func = &ir.functions[0];
        let complexity = func.complexity.as_ref().unwrap();
        assert_eq!(complexity.exception_handlers, 2);
    }

    #[test]
    fn test_accurate_line_numbers() {
        let source = "def first():\n    pass\n\ndef second():\n    pass";
        let path = Path::new("test.py");
        let config = ParserConfig::default();
        let ir = extract(source, path, &config).unwrap();

        assert_eq!(ir.functions.len(), 2);
        assert_eq!(ir.functions[0].name, "first");
        assert_eq!(ir.functions[0].line_start, 1);
        assert_eq!(ir.functions[1].name, "second");
        assert_eq!(ir.functions[1].line_start, 4);
    }

    #[test]
    fn test_enum_detection() {
        let source = r#"
from enum import Enum, IntEnum

class Color(Enum):
    RED = 1
    GREEN = 2
    BLUE = 3

class Status(IntEnum):
    PENDING = 0
    ACTIVE = 1
"#;
        let path = Path::new("test.py");
        let config = ParserConfig::default();
        let ir = extract(source, path, &config).unwrap();

        assert_eq!(ir.classes.len(), 2);
        assert!(
            ir.classes[0].attributes.contains(&"enum".to_string()),
            "Color should have enum attribute"
        );
        assert!(
            ir.classes[1].attributes.contains(&"enum".to_string()),
            "Status should have enum attribute"
        );
    }

    #[test]
    fn test_abc_to_trait() {
        let source = r#"
from abc import ABC, abstractmethod

class Animal(ABC):
    @abstractmethod
    def make_sound(self) -> str:
        pass

    def breathe(self):
        pass
"#;
        let path = Path::new("test.py");
        let config = ParserConfig::default();
        let ir = extract(source, path, &config).unwrap();

        assert_eq!(ir.classes.len(), 0, "ABC should not produce a ClassEntity");
        assert_eq!(ir.traits.len(), 1, "ABC should produce a TraitEntity");
        assert_eq!(ir.traits[0].name, "Animal");

        // Only @abstractmethod methods should be in required_methods
        assert_eq!(ir.traits[0].required_methods.len(), 1);
        assert_eq!(ir.traits[0].required_methods[0].name, "make_sound");
        assert!(ir.traits[0].required_methods[0].is_abstract);
    }

    #[test]
    fn test_visibility_from_name() {
        let source = r#"
def public_func():
    pass

def _protected_func():
    pass

def __private_func():
    pass

def __dunder__():
    pass
"#;
        let path = Path::new("test.py");
        let config = ParserConfig {
            include_private: true,
            ..Default::default()
        };
        let ir = extract(source, path, &config).unwrap();

        assert_eq!(ir.functions.len(), 4);
        assert_eq!(ir.functions[0].visibility, "public");
        assert_eq!(ir.functions[1].visibility, "protected");
        assert_eq!(ir.functions[2].visibility, "private");
        assert_eq!(ir.functions[3].visibility, "public"); // dunder methods are public
    }

    #[test]
    fn test_async_function() {
        let source = r#"
async def fetch_data():
    return "data"
"#;
        let path = Path::new("test.py");
        let config = ParserConfig::default();
        let ir = extract(source, path, &config).unwrap();

        assert_eq!(ir.functions.len(), 1);
        // Note: async detection depends on tree-sitter grammar details
    }

    // --- python_visibility unit coverage ---------------------------------

    #[test]
    fn test_python_visibility_all_branches() {
        assert_eq!(python_visibility("foo"), "public");
        assert_eq!(python_visibility("_protected"), "protected");
        assert_eq!(python_visibility("__private"), "private");
        assert_eq!(python_visibility("__init__"), "public");
        // Bare "__" is not a dunder (len <= 4), so it falls through to private.
        assert_eq!(python_visibility("__"), "private");
    }

    // --- parameter extraction --------------------------------------------

    #[test]
    fn test_parameter_kinds_extracted() {
        let source = r#"
def f(a, b: int, c=1, d: str = "x", *args, **kwargs):
    pass
"#;
        let path = Path::new("test.py");
        let config = ParserConfig::default();
        let ir = extract(source, path, &config).unwrap();

        let params = &ir.functions[0].parameters;
        assert_eq!(params.len(), 6);

        // Plain identifier: no type, no default, not variadic.
        assert_eq!(params[0].name, "a");
        assert_eq!(params[0].type_annotation, None);
        assert_eq!(params[0].default_value, None);
        assert!(!params[0].is_variadic);

        // typed_parameter: type only.
        assert_eq!(params[1].name, "b");
        assert_eq!(params[1].type_annotation.as_deref(), Some("int"));
        assert_eq!(params[1].default_value, None);

        // default_parameter: default only.
        assert_eq!(params[2].name, "c");
        assert_eq!(params[2].type_annotation, None);
        assert_eq!(params[2].default_value.as_deref(), Some("1"));

        // typed_default_parameter: type and default.
        assert_eq!(params[3].name, "d");
        assert_eq!(params[3].type_annotation.as_deref(), Some("str"));
        assert_eq!(params[3].default_value.as_deref(), Some("\"x\""));

        // *args and **kwargs are variadic.
        assert_eq!(params[4].name, "args");
        assert!(params[4].is_variadic);
        assert_eq!(params[5].name, "kwargs");
        assert!(params[5].is_variadic);
    }

    // --- return type ------------------------------------------------------

    #[test]
    fn test_return_type_extracted() {
        let source = r#"
def total() -> int:
    return 0
"#;
        let path = Path::new("test.py");
        let config = ParserConfig::default();
        let ir = extract(source, path, &config).unwrap();

        assert_eq!(ir.functions[0].return_type.as_deref(), Some("int"));
    }

    #[test]
    fn test_return_type_absent_when_unannotated() {
        let source = r#"
def total():
    return 0
"#;
        let path = Path::new("test.py");
        let config = ParserConfig::default();
        let ir = extract(source, path, &config).unwrap();

        assert_eq!(ir.functions[0].return_type, None);
    }

    // --- docstrings / body_prefix ----------------------------------------

    #[test]
    fn test_function_docstring_extracted() {
        let source = "def f():\n    \"\"\"Adds things.\"\"\"\n    return 1\n";
        let path = Path::new("test.py");
        let config = ParserConfig::default();
        let ir = extract(source, path, &config).unwrap();

        assert!(ir.functions[0]
            .doc_comment
            .as_deref()
            .unwrap()
            .contains("Adds things"));
    }

    #[test]
    fn test_function_body_prefix_present() {
        let source = r#"
def f():
    return 1
"#;
        let path = Path::new("test.py");
        let config = ParserConfig::default();
        let ir = extract(source, path, &config).unwrap();

        assert!(ir.functions[0].body_prefix.is_some());
    }

    // --- decorators -------------------------------------------------------

    #[test]
    fn test_staticmethod_decorator_sets_is_static() {
        let source = r#"
class C:
    @staticmethod
    def util():
        return 1
"#;
        let path = Path::new("test.py");
        let config = ParserConfig::default();
        let ir = extract(source, path, &config).unwrap();

        let m = ir.functions.iter().find(|f| f.name == "util").unwrap();
        assert!(m.is_static);
        assert!(m.attributes.iter().any(|a| a.contains("staticmethod")));
    }

    #[test]
    fn test_classmethod_decorator_is_not_static() {
        let source = r#"
class C:
    @classmethod
    def make(cls):
        return cls()
"#;
        let path = Path::new("test.py");
        let config = ParserConfig::default();
        let ir = extract(source, path, &config).unwrap();

        let m = ir.functions.iter().find(|f| f.name == "make").unwrap();
        assert!(!m.is_static);
        assert!(m.attributes.iter().any(|a| a.contains("classmethod")));
    }

    #[test]
    fn test_decorated_function_is_flagged_async() {
        // Quirk: any decorated function is flagged is_async because the
        // parent node kind is decorated_definition.
        let source = r#"
class C:
    @property
    def value(self):
        return 1
"#;
        let path = Path::new("test.py");
        let config = ParserConfig::default();
        let ir = extract(source, path, &config).unwrap();

        let m = ir.functions.iter().find(|f| f.name == "value").unwrap();
        assert!(m.is_async);
    }

    // --- config filters ---------------------------------------------------

    #[test]
    fn test_include_private_false_skips_private_keeps_dunder() {
        let source = r#"
def public_func():
    pass

def _helper():
    pass

def __init__():
    pass
"#;
        let path = Path::new("test.py");
        let config = ParserConfig {
            include_private: false,
            ..Default::default()
        };
        let ir = extract(source, path, &config).unwrap();

        let names: Vec<&str> = ir.functions.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"public_func"));
        assert!(names.contains(&"__init__"));
        assert!(!names.contains(&"_helper"));
    }

    #[test]
    fn test_include_tests_false_skips_test_functions() {
        let source = r#"
def regular():
    pass

def test_regular():
    pass
"#;
        let path = Path::new("test.py");
        let config = ParserConfig {
            include_tests: false,
            ..Default::default()
        };
        let ir = extract(source, path, &config).unwrap();

        let names: Vec<&str> = ir.functions.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"regular"));
        assert!(!names.contains(&"test_regular"));
    }

    // --- methods / parent_class ------------------------------------------

    #[test]
    fn test_method_has_parent_class() {
        let source = r#"
class Calc:
    def add(self, a, b):
        return a + b
"#;
        let path = Path::new("test.py");
        let config = ParserConfig::default();
        let ir = extract(source, path, &config).unwrap();

        let m = ir.functions.iter().find(|f| f.name == "add").unwrap();
        assert_eq!(m.parent_class.as_deref(), Some("Calc"));
    }

    // --- imports ----------------------------------------------------------

    #[test]
    fn test_import_with_alias() {
        let source = "import numpy as np\n";
        let path = Path::new("test.py");
        let config = ParserConfig::default();
        let ir = extract(source, path, &config).unwrap();

        assert_eq!(ir.imports.len(), 1);
        assert_eq!(ir.imports[0].imported, "numpy");
        assert_eq!(ir.imports[0].alias.as_deref(), Some("np"));
    }

    #[test]
    fn test_from_import_records_symbols() {
        let source = "from typing import List, Dict\n";
        let path = Path::new("test.py");
        let config = ParserConfig::default();
        let ir = extract(source, path, &config).unwrap();

        assert_eq!(ir.imports.len(), 1);
        assert_eq!(ir.imports[0].imported, "typing");
        assert!(ir.imports[0].symbols.contains(&"List".to_string()));
        assert!(ir.imports[0].symbols.contains(&"Dict".to_string()));
        assert!(!ir.imports[0].is_wildcard);
    }

    #[test]
    fn test_from_import_wildcard() {
        let source = "from collections import *\n";
        let path = Path::new("test.py");
        let config = ParserConfig::default();
        let ir = extract(source, path, &config).unwrap();

        assert_eq!(ir.imports.len(), 1);
        assert_eq!(ir.imports[0].imported, "collections");
        assert!(ir.imports[0].is_wildcard);
    }

    // --- calls ------------------------------------------------------------

    #[test]
    fn test_call_with_attribute_callee_uses_full_path() {
        let source = r#"
class C:
    def run(self):
        self.helper()

    def helper(self):
        pass
"#;
        let path = Path::new("test.py");
        let config = ParserConfig::default();
        let ir = extract(source, path, &config).unwrap();

        assert!(ir
            .calls
            .iter()
            .any(|c| c.caller == "C.run" && c.callee == "self.helper"));
    }

    // --- inheritance / class detection -----------------------------------

    #[test]
    fn test_inheritance_from_qualified_base() {
        let source = r#"
class Child(mod.Base):
    pass
"#;
        let path = Path::new("test.py");
        let config = ParserConfig::default();
        let ir = extract(source, path, &config).unwrap();

        assert_eq!(ir.inheritance.len(), 1);
        assert_eq!(ir.inheritance[0].child, "Child");
        assert_eq!(ir.inheritance[0].parent, "mod.Base");
    }

    #[test]
    fn test_qualified_enum_base_detected() {
        let source = r#"
import enum

class Color(enum.Enum):
    RED = 1
"#;
        let path = Path::new("test.py");
        let config = ParserConfig::default();
        let ir = extract(source, path, &config).unwrap();

        assert_eq!(ir.classes.len(), 1);
        assert!(ir.classes[0].attributes.contains(&"enum".to_string()));
    }

    #[test]
    fn test_protocol_maps_to_trait() {
        let source = r#"
from typing import Protocol

class Drawable(Protocol):
    def draw(self) -> None:
        ...
"#;
        let path = Path::new("test.py");
        let config = ParserConfig::default();
        let ir = extract(source, path, &config).unwrap();

        assert_eq!(ir.classes.len(), 0);
        assert_eq!(ir.traits.len(), 1);
        assert_eq!(ir.traits[0].name, "Drawable");
    }

    // --- complexity edge cases -------------------------------------------

    #[test]
    fn test_ternary_adds_branch() {
        let source = r#"
def choose(x):
    return 1 if x else 0
"#;
        let path = Path::new("test.py");
        let config = ParserConfig::default();
        let ir = extract(source, path, &config).unwrap();

        let c = ir.functions[0].complexity.as_ref().unwrap();
        assert!(c.branches >= 1);
    }

    #[test]
    fn test_match_statement_adds_branches() {
        let source = r#"
def dispatch(cmd):
    match cmd:
        case "a":
            return 1
        case "b":
            return 2
"#;
        let path = Path::new("test.py");
        let config = ParserConfig::default();
        let ir = extract(source, path, &config).unwrap();

        let c = ir.functions[0].complexity.as_ref().unwrap();
        assert!(c.branches >= 2);
    }

    #[test]
    fn test_comprehension_with_condition_adds_loop_and_branch() {
        let source = r#"
def evens(items):
    return [x for x in items if x % 2 == 0]
"#;
        let path = Path::new("test.py");
        let config = ParserConfig::default();
        let ir = extract(source, path, &config).unwrap();

        let c = ir.functions[0].complexity.as_ref().unwrap();
        assert!(c.loops >= 1);
        assert!(c.branches >= 1);
    }

    #[test]
    fn test_relative_from_import_default_module() {
        let source = "from . import sibling\n";
        let path = Path::new("test.py");
        let config = ParserConfig::default();
        let ir = extract(source, path, &config).unwrap();

        assert_eq!(ir.imports.len(), 1);
        assert_eq!(ir.imports[0].imported, ".");
        assert!(ir.imports[0].symbols.contains(&"sibling".to_string()));
    }
}
