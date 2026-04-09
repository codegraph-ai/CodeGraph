// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting TypeScript/JavaScript entities

use codegraph_parser_api::{
    CallRelation, ClassEntity, ComplexityBuilder, ComplexityMetrics, Field, FunctionEntity,
    ImplementationRelation, ImportRelation, InheritanceRelation, Parameter, TraitEntity,
    TypeReference, BODY_PREFIX_MAX_CHARS,
    truncate_body_prefix,
};
use tree_sitter::Node;

/// Visitor that extracts entities and relationships from TypeScript/JavaScript AST
pub struct TypeScriptVisitor<'a> {
    pub source: &'a [u8],
    pub functions: Vec<FunctionEntity>,
    pub classes: Vec<ClassEntity>,
    pub interfaces: Vec<TraitEntity>,
    pub imports: Vec<ImportRelation>,
    pub calls: Vec<CallRelation>,
    pub implementations: Vec<ImplementationRelation>,
    pub inheritance: Vec<InheritanceRelation>,
    pub type_references: Vec<TypeReference>,
    current_class: Option<String>,
    current_function: Option<String>,
}

impl<'a> TypeScriptVisitor<'a> {
    pub fn new(source: &'a [u8]) -> Self {
        Self {
            source,
            functions: Vec::new(),
            classes: Vec::new(),
            interfaces: Vec::new(),
            imports: Vec::new(),
            calls: Vec::new(),
            implementations: Vec::new(),
            inheritance: Vec::new(),
            type_references: Vec::new(),
            current_class: None,
            current_function: None,
        }
    }

    /// Get text for a node
    fn node_text(&self, node: Node) -> String {
        node.utf8_text(self.source).unwrap_or("").to_string()
    }

    /// Extract decorators from a node. Checks:
    /// 1. `decorator` child nodes (TypeScript class methods)
    /// 2. Previous siblings that are `decorator` nodes
    fn extract_decorators(&self, node: Node) -> Vec<String> {
        let mut decorators = Vec::new();

        // Check child nodes (decorators inside the method definition node)
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "decorator" {
                decorators.push(self.node_text(child).trim_start_matches('@').to_string());
            }
        }

        // Check previous siblings (decorators before the method in class body)
        let mut prev = node.prev_sibling();
        while let Some(sibling) = prev {
            if sibling.kind() == "decorator" {
                decorators.push(self.node_text(sibling).trim_start_matches('@').to_string());
            } else if sibling.kind() != "comment" {
                break; // Stop at non-decorator, non-comment nodes
            }
            prev = sibling.prev_sibling();
        }

        decorators
    }

    /// Visit a tree-sitter node
    pub fn visit_node(&mut self, node: Node) {
        match node.kind() {
            // Only match declaration nodes to avoid duplicates
            "function_declaration" => {
                self.visit_function(node);
            }
            "arrow_function" => {
                self.visit_arrow_function(node);
            }
            "method_definition" => {
                self.visit_method(node);
            }
            "class_declaration" => {
                self.visit_class(node);
            }
            "interface_declaration" => {
                self.visit_interface(node);
            }
            "enum_declaration" => {
                self.visit_enum(node);
            }
            "import_statement" => {
                self.visit_import(node);
            }
            "comment" => {
                self.visit_comment(node);
            }
            "call_expression" => {
                self.visit_call_expression(node);
                // Also recurse into call expression children (e.g., nested calls)
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.visit_node(child);
                }
            }
            "new_expression" => {
                self.visit_new_expression(node);
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.visit_node(child);
                }
            }
            "as_expression" | "satisfies_expression" => {
                self.visit_type_assertion(node);
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.visit_node(child);
                }
            }
            "variable_declarator" => {
                // const x: MyType = ... — extract type annotation
                self.visit_variable_type_annotation(node);
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.visit_node(child);
                }
            }
            _ => {
                // Recursively visit children for unhandled node types
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.visit_node(child);
                }
            }
        }
    }

    fn visit_function(&mut self, node: Node) {
        // Extract function name
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_else(|| "anonymous".to_string());

        // Extract parameters
        let parameters = if let Some(params_node) = node.child_by_field_name("parameters") {
            self.extract_parameters(params_node)
        } else {
            Vec::new()
        };

        // Check if async
        let is_async = self.node_text(node).starts_with("async");

        // Calculate complexity from the function body
        let complexity = node
            .child_by_field_name("body")
            .map(|body| self.calculate_complexity(body));

        let body_prefix = node
            .child_by_field_name("body")
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| {
                truncate_body_prefix(t)
            })
            .map(|t| t.to_string());

        let attributes = self.extract_decorators(node);

        let func = FunctionEntity {
            name: name.clone(),
            signature: self
                .node_text(node)
                .lines()
                .next()
                .unwrap_or("")
                .to_string(),
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_async,
            is_test: false,
            is_static: false,
            is_abstract: false,
            parameters,
            return_type: None,
            doc_comment: None,
            attributes,
            parent_class: self.current_class.clone(),
            complexity,
            body_prefix,
        };
        self.functions.push(func);

        // Extract type references from parameter and return type annotations
        self.extract_type_refs_from_function(&name, node);

        // Set current function context and visit body to extract calls
        let previous_function = self.current_function.clone();
        self.current_function = Some(name);

        // Visit function body to extract call expressions
        if let Some(body) = node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for child in body.children(&mut cursor) {
                self.visit_node(child);
            }
        }

        self.current_function = previous_function;
    }

    fn visit_arrow_function(&mut self, node: Node) {
        // When inside a named function/method, don't create a separate entity —
        // just recurse into the body so calls are attributed to the enclosing function.
        // This ensures `this.formatCallGraph()` inside an arrow callback within
        // `registerTools()` creates a Calls edge from registerTools → formatCallGraph.
        if self.current_function.is_some() {
            if let Some(body) = node.child_by_field_name("body") {
                let mut cursor = body.walk();
                for child in body.children(&mut cursor) {
                    self.visit_node(child);
                }
            }
            return;
        }

        // Top-level arrow function (e.g., `const func = () => {...}`)
        let complexity = node
            .child_by_field_name("body")
            .map(|body| self.calculate_complexity(body));

        // Check if the arrow function or its parent variable declaration starts with "async"
        let is_async = {
            let text = self.node_text(node);
            text.starts_with("async")
                || node
                    .parent()
                    .map(|p| self.node_text(p).starts_with("async"))
                    .unwrap_or(false)
        };

        let body_prefix = node
            .child_by_field_name("body")
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| {
                truncate_body_prefix(t)
            })
            .map(|t| t.to_string());

        let func = FunctionEntity {
            name: "arrow_function".to_string(),
            signature: "() => {}".to_string(),
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_async,
            is_test: false,
            is_static: false,
            is_abstract: false,
            parameters: Vec::new(),
            return_type: None,
            doc_comment: None,
            attributes: Vec::new(),
            parent_class: None,
            complexity,
            body_prefix,
        };
        self.functions.push(func);

        // Visit body for calls (current_function remains None, so calls won't be recorded
        // unless we set it — for top-level arrows, calls are truly unresolvable)
    }

    fn visit_method(&mut self, node: Node) {
        // Extract method name from property_identifier or identifier
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_else(|| "method".to_string());

        // Extract parameters if available
        let parameters = if let Some(params_node) = node.child_by_field_name("parameters") {
            self.extract_parameters(params_node)
        } else {
            Vec::new()
        };

        // Check modifiers from the method signature (first line only to avoid body matches)
        let node_text = self.node_text(node);
        let first_line = node_text.lines().next().unwrap_or("");
        let is_static = first_line.contains("static ");
        let is_async = first_line.contains("async ");

        // Determine visibility from TS keywords or JS # prefix
        let visibility = if name.starts_with('#')
            || first_line.starts_with("private ")
            || first_line.contains(" private ")
        {
            "private".to_string()
        } else if first_line.starts_with("protected ") || first_line.contains(" protected ") {
            "protected".to_string()
        } else {
            "public".to_string()
        };

        // Calculate complexity from the method body
        let complexity = node
            .child_by_field_name("body")
            .map(|body| self.calculate_complexity(body));

        let body_prefix = node
            .child_by_field_name("body")
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| {
                truncate_body_prefix(t)
            })
            .map(|t| t.to_string());

        let attributes = self.extract_decorators(node);

        let func = FunctionEntity {
            name: name.clone(),
            signature: node_text.lines().next().unwrap_or("").to_string(),
            visibility,
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_async,
            is_test: false,
            is_static,
            is_abstract: false,
            parameters,
            return_type: None,
            doc_comment: None,
            attributes,
            parent_class: self.current_class.clone(),
            complexity,
            body_prefix,
        };
        self.functions.push(func);

        // Extract type references from parameter and return type annotations
        self.extract_type_refs_from_function(&name, node);

        // Set current function context and visit body to extract calls
        let previous_function = self.current_function.clone();
        self.current_function = Some(name);

        // Visit method body to extract call expressions
        if let Some(body) = node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for child in body.children(&mut cursor) {
                self.visit_node(child);
            }
        }

        self.current_function = previous_function;
    }

    fn visit_class(&mut self, node: Node) {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_else(|| "AnonymousClass".to_string());

        // Set current class context
        let previous_class = self.current_class.clone();
        self.current_class = Some(name.clone());

        let body_prefix = node
            .child_by_field_name("body")
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| {
                truncate_body_prefix(t)
            })
            .map(|t| t.to_string());

        let class = ClassEntity {
            name: name.clone(),
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_abstract: false,
            is_interface: false,
            base_classes: Vec::new(),
            implemented_traits: Vec::new(),
            methods: Vec::new(),
            fields: Vec::new(),
            doc_comment: None,
            attributes: Vec::new(),
            type_parameters: Vec::new(),
            body_prefix,
        };
        self.classes.push(class);

        // Visit children (methods, properties)
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "class_body" {
                let mut body_cursor = child.walk();
                for member in child.children(&mut body_cursor) {
                    self.visit_node(member);
                }
            }
        }

        // Restore previous class context
        self.current_class = previous_class;
    }

    fn visit_interface(&mut self, node: Node) {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_else(|| "AnonymousInterface".to_string());

        let line = node.start_position().row + 1;

        // Extract type references from interface body (property types, method signatures)
        if let Some(body) = node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for child in body.children(&mut cursor) {
                match child.kind() {
                    "property_signature" | "public_field_definition" => {
                        if let Some(type_node) = child.child_by_field_name("type") {
                            for type_name in self.extract_type_names(type_node) {
                                self.type_references.push(TypeReference::new(
                                    name.clone(),
                                    type_name,
                                    line,
                                ));
                            }
                        }
                    }
                    "method_signature" => {
                        // Extract param and return type refs from method signature
                        if let Some(params) = child.child_by_field_name("parameters") {
                            let mut pcursor = params.walk();
                            for param in params.children(&mut pcursor) {
                                if param.kind() == "required_parameter"
                                    || param.kind() == "optional_parameter"
                                {
                                    if let Some(type_node) = param.child_by_field_name("type") {
                                        for type_name in self.extract_type_names(type_node) {
                                            self.type_references.push(TypeReference::new(
                                                name.clone(),
                                                type_name,
                                                line,
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                        if let Some(return_type) = child.child_by_field_name("return_type") {
                            for type_name in self.extract_type_names(return_type) {
                                self.type_references.push(TypeReference::new(
                                    name.clone(),
                                    type_name,
                                    line,
                                ));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // Extract extends clause for interface inheritance
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "extends_type_clause" {
                let mut ext_cursor = child.walk();
                for ext_child in child.children(&mut ext_cursor) {
                    for type_name in self.extract_type_names(ext_child) {
                        self.type_references.push(TypeReference::new(
                            name.clone(),
                            type_name,
                            line,
                        ));
                    }
                }
            }
        }

        let interface = TraitEntity {
            name,
            visibility: "public".to_string(),
            line_start: line,
            line_end: node.end_position().row + 1,
            required_methods: Vec::new(),
            parent_traits: Vec::new(),
            doc_comment: None,
            attributes: Vec::new(),
        };

        self.interfaces.push(interface);
    }

    fn visit_enum(&mut self, node: Node) {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_else(|| "AnonymousEnum".to_string());

        // Extract enum members as fields
        let mut fields = Vec::new();
        if let Some(body) = node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for child in body.children(&mut cursor) {
                if child.kind() == "enum_assignment" || child.kind() == "property_identifier" {
                    let member_name = if child.kind() == "enum_assignment" {
                        child
                            .child_by_field_name("name")
                            .map(|n| self.node_text(n))
                            .unwrap_or_default()
                    } else {
                        self.node_text(child)
                    };

                    if !member_name.is_empty() {
                        let default_value = if child.kind() == "enum_assignment" {
                            child
                                .child_by_field_name("value")
                                .map(|n| self.node_text(n))
                        } else {
                            None
                        };

                        fields.push(Field {
                            name: member_name,
                            type_annotation: None,
                            visibility: "public".to_string(),
                            is_static: true,
                            is_constant: true,
                            default_value,
                        });
                    }
                }
            }
        }

        let body_prefix = node
            .child_by_field_name("body")
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| {
                truncate_body_prefix(t)
            })
            .map(|t| t.to_string());

        let class = ClassEntity {
            name,
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_abstract: false,
            is_interface: false,
            base_classes: Vec::new(),
            implemented_traits: Vec::new(),
            methods: Vec::new(),
            fields,
            doc_comment: None,
            attributes: vec!["enum".to_string()],
            type_parameters: Vec::new(),
            body_prefix,
        };
        self.classes.push(class);
    }

    fn visit_import(&mut self, node: Node) {
        // Extract the source (from 'react', './utils', etc.)
        let source = node
            .child_by_field_name("source")
            .map(|n| {
                let text = self.node_text(n);
                // Remove quotes from source
                text.trim_matches(|c| c == '"' || c == '\'').to_string()
            })
            .unwrap_or_default();

        let mut symbols = Vec::new();
        let mut alias = None;
        let mut is_wildcard = false;

        // Parse import_clause to extract specifiers
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "import_clause" {
                // Parse the import_clause
                let mut clause_cursor = child.walk();
                for clause_child in child.children(&mut clause_cursor) {
                    match clause_child.kind() {
                        // Default import: import React from 'react'
                        "identifier" => {
                            symbols.push(self.node_text(clause_child));
                        }
                        // Named imports: { useState, useEffect }
                        "named_imports" => {
                            symbols.extend(self.extract_named_imports(clause_child));
                        }
                        // Namespace import: * as Utils
                        "namespace_import" => {
                            is_wildcard = true;
                            // Extract the identifier after 'as'
                            let mut ns_cursor = clause_child.walk();
                            for ns_child in clause_child.children(&mut ns_cursor) {
                                if ns_child.kind() == "identifier" {
                                    alias = Some(self.node_text(ns_child));
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        let import = ImportRelation {
            importer: "current_module".to_string(),
            imported: source,
            symbols,
            is_wildcard,
            alias,
        };

        self.imports.push(import);
    }

    fn visit_comment(&mut self, node: Node) {
        let text = self.node_text(node);

        // Only process triple-slash directives
        if !text.starts_with("///") {
            return;
        }

        // Extract path from: /// <reference path="./types.d.ts" />
        // Skip types references (external type packages)
        if let Some(path) = Self::extract_reference_path(&text) {
            let import = ImportRelation {
                importer: "current_module".to_string(),
                imported: path,
                symbols: Vec::new(),
                is_wildcard: false,
                alias: Some("reference".to_string()),
            };
            self.imports.push(import);
        }
    }

    fn extract_reference_path(comment: &str) -> Option<String> {
        // Match: /// <reference path="..." />
        let path_marker = "path=\"";
        if let Some(start) = comment.find(path_marker) {
            let rest = &comment[start + path_marker.len()..];
            if let Some(end) = rest.find('"') {
                let path = &rest[..end];
                if !path.is_empty() {
                    return Some(path.to_string());
                }
            }
        }
        None
    }

    fn extract_named_imports(&self, node: Node) -> Vec<String> {
        let mut imports = Vec::new();
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            if child.kind() == "import_specifier" {
                // Handle both "name" and "name as alias" forms
                let mut spec_cursor = child.walk();
                for spec_child in child.children(&mut spec_cursor) {
                    if spec_child.kind() == "identifier" {
                        imports.push(self.node_text(spec_child));
                        break; // Only take the first identifier (the imported name, not the alias)
                    }
                }
            }
        }

        imports
    }

    fn extract_parameters(&self, params_node: Node) -> Vec<Parameter> {
        let mut parameters = Vec::new();
        let mut cursor = params_node.walk();

        for child in params_node.children(&mut cursor) {
            if child.kind() == "required_parameter" || child.kind() == "optional_parameter" {
                let name = child
                    .child_by_field_name("pattern")
                    .map(|n| self.node_text(n))
                    .unwrap_or_else(|| "param".to_string());

                let type_annotation = child.child_by_field_name("type").map(|n| self.node_text(n));

                parameters.push(Parameter {
                    name,
                    type_annotation,
                    default_value: None,
                    is_variadic: false,
                });
            }
        }

        parameters
    }

    /// Extract user-defined type names from a type annotation node.
    /// Skips built-in types (string, number, boolean, etc.) and returns
    /// only identifiers that could be user-defined types/interfaces/classes.
    fn extract_type_names(&self, type_node: Node) -> Vec<String> {
        let mut names = Vec::new();
        match type_node.kind() {
            "type_identifier" => {
                let name = self.node_text(type_node);
                if !Self::is_builtin_type(&name) {
                    names.push(name);
                }
            }
            "generic_type" => {
                // e.g., Promise<T>, Array<string> — extract the base type and type args
                let mut cursor = type_node.walk();
                for child in type_node.children(&mut cursor) {
                    names.extend(self.extract_type_names(child));
                }
            }
            "union_type" | "intersection_type" => {
                let mut cursor = type_node.walk();
                for child in type_node.children(&mut cursor) {
                    names.extend(self.extract_type_names(child));
                }
            }
            "array_type" => {
                // e.g., MyType[] — extract element type
                let mut cursor = type_node.walk();
                for child in type_node.children(&mut cursor) {
                    names.extend(self.extract_type_names(child));
                }
            }
            "type_arguments" => {
                // <T, U> — extract each type argument
                let mut cursor = type_node.walk();
                for child in type_node.children(&mut cursor) {
                    names.extend(self.extract_type_names(child));
                }
            }
            "type_annotation" => {
                // type_annotation wraps `: TypeName` — recurse into children
                let mut cursor = type_node.walk();
                for child in type_node.children(&mut cursor) {
                    names.extend(self.extract_type_names(child));
                }
            }
            _ => {}
        }
        names
    }

    fn is_builtin_type(name: &str) -> bool {
        matches!(
            name,
            "string"
                | "number"
                | "boolean"
                | "void"
                | "null"
                | "undefined"
                | "never"
                | "any"
                | "unknown"
                | "object"
                | "symbol"
                | "bigint"
                | "Promise"
                | "Array"
                | "Map"
                | "Set"
                | "Record"
                | "Partial"
                | "Required"
                | "Readonly"
                | "Pick"
                | "Omit"
                | "Exclude"
                | "Extract"
                | "NonNullable"
                | "ReturnType"
                | "Parameters"
                | "InstanceType"
                | "Awaited"
        )
    }

    /// Extract type references from a function/method node's parameters and return type.
    fn extract_type_refs_from_function(&mut self, name: &str, node: Node) {
        let line = node.start_position().row + 1;

        // Extract from parameter type annotations
        if let Some(params_node) = node.child_by_field_name("parameters") {
            let mut cursor = params_node.walk();
            for child in params_node.children(&mut cursor) {
                if child.kind() == "required_parameter" || child.kind() == "optional_parameter" {
                    if let Some(type_node) = child.child_by_field_name("type") {
                        for type_name in self.extract_type_names(type_node) {
                            self.type_references.push(TypeReference::new(
                                name.to_string(),
                                type_name,
                                line,
                            ));
                        }
                    }
                }
            }
        }

        // Extract from return type annotation
        if let Some(return_type_node) = node.child_by_field_name("return_type") {
            for type_name in self.extract_type_names(return_type_node) {
                self.type_references
                    .push(TypeReference::new(name.to_string(), type_name, line));
            }
        }
    }

    /// Visit a call expression and extract the call relationship
    fn visit_call_expression(&mut self, node: Node) {
        // Only record calls if we're inside a function/method
        let caller = match &self.current_function {
            Some(name) => name.clone(),
            None => return,
        };

        // Extract the callee name from the call expression
        // call_expression has a "function" field that is the thing being called
        if let Some(function_node) = node.child_by_field_name("function") {
            let callee = self.extract_callee_name(function_node);

            // Skip empty or "this" only callees
            if callee.is_empty() || callee == "this" {
                return;
            }

            let call_site_line = node.start_position().row + 1;

            let call = CallRelation {
                caller: caller.clone(),
                callee,
                call_site_line,
                is_direct: true,
                struct_type: None,
                field_name: None,
            };

            self.calls.push(call);
        }
    }

    /// Visit a `new ClassName()` expression — record as a call to the class
    /// and extract generic type arguments as type references.
    fn visit_new_expression(&mut self, node: Node) {
        let caller = match &self.current_function {
            Some(name) => name.clone(),
            None => return,
        };

        // tree-sitter: new_expression → "new" keyword + constructor + type_arguments? + arguments
        // Constructor is typically child index 1 (after "new" keyword)
        if let Some(constructor) = node.child(1) {
            let class_name = match constructor.kind() {
                "identifier" => self.node_text(constructor),
                "member_expression" => {
                    // e.g., new vscode.TreeItem() → extract "TreeItem"
                    constructor
                        .child_by_field_name("property")
                        .map(|p| self.node_text(p))
                        .unwrap_or_default()
                }
                _ => return,
            };

            // Only treat PascalCase names as class instantiations
            if !class_name.is_empty() && class_name.chars().next().is_some_and(|c| c.is_uppercase())
            {
                self.calls.push(CallRelation {
                    caller: caller.clone(),
                    callee: class_name,
                    call_site_line: node.start_position().row + 1,
                    is_direct: true,
                    struct_type: None,
                    field_name: None,
                });
            }
        }

        // Extract generic type arguments: new RequestType<ParamType>()
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "type_arguments" {
                for type_name in self.extract_type_names(child) {
                    self.type_references.push(TypeReference::new(
                        caller.clone(),
                        type_name,
                        node.start_position().row + 1,
                    ));
                }
            }
        }
    }

    /// Visit variable declarations with type annotations: const x: MyType = ...
    fn visit_variable_type_annotation(&mut self, node: Node) {
        let func_name = match &self.current_function {
            Some(name) => name.clone(),
            None => return,
        };

        // variable_declarator has a "type" field for the type annotation
        if let Some(type_node) = node.child_by_field_name("type") {
            for type_name in self.extract_type_names(type_node) {
                self.type_references.push(TypeReference::new(
                    func_name.clone(),
                    type_name,
                    node.start_position().row + 1,
                ));
            }
        }
    }

    /// Visit `as` casts and `satisfies` expressions — extract the type as a reference.
    fn visit_type_assertion(&mut self, node: Node) {
        let func_name = match &self.current_function {
            Some(name) => name.clone(),
            None => return,
        };

        // tree-sitter: as_expression → expression "as" type
        // satisfies_expression → expression "satisfies" type
        // The type is typically the last meaningful child
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            for type_name in self.extract_type_names(child) {
                self.type_references.push(TypeReference::new(
                    func_name.clone(),
                    type_name,
                    node.start_position().row + 1,
                ));
            }
        }
    }

    /// Extract the callee name from a function node in a call expression
    fn extract_callee_name(&self, node: Node) -> String {
        match node.kind() {
            // Simple identifier: foo()
            "identifier" => self.node_text(node),

            // Member expression: this.foo(), obj.method()
            "member_expression" => {
                // Get the property (method name) from member expression
                if let Some(property) = node.child_by_field_name("property") {
                    self.node_text(property)
                } else {
                    self.node_text(node)
                }
            }

            // Optional chaining: this?.foo()
            "call_expression" => {
                // Nested call, e.g., getProvider()()
                if let Some(func) = node.child_by_field_name("function") {
                    self.extract_callee_name(func)
                } else {
                    String::new()
                }
            }

            // Await expression: await this.foo()
            "await_expression" => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() != "await" {
                        return self.extract_callee_name(child);
                    }
                }
                String::new()
            }

            _ => self.node_text(node),
        }
    }

    /// Calculate cyclomatic complexity for a function/method body
    fn calculate_complexity(&self, body: Node) -> ComplexityMetrics {
        let mut builder = ComplexityBuilder::new();
        self.calculate_complexity_recursive(body, &mut builder);
        builder.build()
    }

    /// Recursively calculate complexity from a tree-sitter node
    fn calculate_complexity_recursive(&self, node: Node, builder: &mut ComplexityBuilder) {
        match node.kind() {
            // Control flow - branches
            "if_statement" => {
                builder.add_branch();
                builder.enter_scope();
                // Process children
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.calculate_complexity_recursive(child, builder);
                }
                builder.exit_scope();
            }
            "else_clause" => {
                builder.add_branch();
                builder.enter_scope();
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.calculate_complexity_recursive(child, builder);
                }
                builder.exit_scope();
            }
            "switch_statement" => {
                builder.enter_scope();
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.calculate_complexity_recursive(child, builder);
                }
                builder.exit_scope();
            }
            "switch_case" | "switch_default" => {
                builder.add_branch();
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.calculate_complexity_recursive(child, builder);
                }
            }
            "ternary_expression" => {
                builder.add_branch();
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.calculate_complexity_recursive(child, builder);
                }
            }

            // Loops
            "for_statement" | "for_in_statement" | "for_of_statement" | "while_statement"
            | "do_statement" => {
                builder.add_loop();
                builder.enter_scope();
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.calculate_complexity_recursive(child, builder);
                }
                builder.exit_scope();
            }

            // Exception handling
            "try_statement" => {
                builder.enter_scope();
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.calculate_complexity_recursive(child, builder);
                }
                builder.exit_scope();
            }
            "catch_clause" => {
                builder.add_exception_handler();
                builder.enter_scope();
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.calculate_complexity_recursive(child, builder);
                }
                builder.exit_scope();
            }

            // Logical operators
            "binary_expression" => {
                // Check for && or ||
                if let Some(operator) = node.child_by_field_name("operator") {
                    let op_text = self.node_text(operator);
                    if op_text == "&&" || op_text == "||" {
                        builder.add_logical_operator();
                    }
                }
                // Process children
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.calculate_complexity_recursive(child, builder);
                }
            }

            // Optional chaining adds a path but doesn't add complexity per se
            "optional_chain_expression" => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.calculate_complexity_recursive(child, builder);
                }
            }

            // Nullish coalescing adds a branch-like path
            // Usually captured as binary_expression with ?? operator

            // Don't recurse into nested functions/arrows - they have their own complexity
            "function_declaration"
            | "function_expression"
            | "arrow_function"
            | "method_definition" => {
                // Skip nested functions - they are analyzed separately
            }

            // All other nodes - recurse into children
            _ => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.calculate_complexity_recursive(child, builder);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_visitor_basics() {
        let visitor = TypeScriptVisitor::new(b"test");
        assert_eq!(visitor.functions.len(), 0);
        assert_eq!(visitor.classes.len(), 0);
    }

    #[test]
    fn test_visitor_function_parameters() {
        use tree_sitter::Parser;

        let source = b"function greet(name: string, age: number): void {}";
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::language_typescript())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = TypeScriptVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "greet");
        assert_eq!(visitor.functions[0].parameters.len(), 2);
        assert_eq!(visitor.functions[0].parameters[0].name, "name");
        assert_eq!(visitor.functions[0].parameters[1].name, "age");
    }

    #[test]
    fn test_visitor_async_function_detection() {
        use tree_sitter::Parser;

        let source = b"async function loadData() { await fetch(); }";
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::language_typescript())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = TypeScriptVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.functions.len(), 1);
        assert!(visitor.functions[0].is_async);
    }

    #[test]
    fn test_visitor_class_context() {
        use tree_sitter::Parser;

        let source = b"class MyClass { myMethod() {} }";
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::language_typescript())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = TypeScriptVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "MyClass");
        // Note: Method extraction not yet implemented
        // Visitor would need to match "method_definition" node type
    }

    #[test]
    fn test_visitor_interface_extraction() {
        use tree_sitter::Parser;

        let source = b"interface IPerson { name: string; age: number; }";
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::language_typescript())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = TypeScriptVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.interfaces.len(), 1);
        assert_eq!(visitor.interfaces[0].name, "IPerson");
    }

    #[test]
    fn test_visitor_import_extraction() {
        use tree_sitter::Parser;

        let source = b"import { useState } from 'react';";
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::language_typescript())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = TypeScriptVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.imports.len(), 1);
    }

    #[test]
    fn test_visitor_named_imports() {
        use tree_sitter::Parser;

        let source = b"import { useState, useEffect } from 'react';";
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::language_typescript())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = TypeScriptVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "react");
        assert_eq!(visitor.imports[0].symbols.len(), 2);
        assert_eq!(visitor.imports[0].symbols[0], "useState");
        assert_eq!(visitor.imports[0].symbols[1], "useEffect");
        assert!(!visitor.imports[0].is_wildcard);
    }

    #[test]
    fn test_visitor_default_import() {
        use tree_sitter::Parser;

        let source = b"import React from 'react';";
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::language_typescript())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = TypeScriptVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "react");
        assert_eq!(visitor.imports[0].symbols.len(), 1);
        assert_eq!(visitor.imports[0].symbols[0], "React");
    }

    #[test]
    fn test_visitor_namespace_import() {
        use tree_sitter::Parser;

        let source = b"import * as Utils from './utils';";
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::language_typescript())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = TypeScriptVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "./utils");
        assert!(visitor.imports[0].is_wildcard);
        assert_eq!(visitor.imports[0].alias, Some("Utils".to_string()));
    }

    #[test]
    fn test_visitor_side_effect_import() {
        use tree_sitter::Parser;

        let source = b"import './styles.css';";
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::language_typescript())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = TypeScriptVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "./styles.css");
        assert_eq!(visitor.imports[0].symbols.len(), 0);
    }

    #[test]
    fn test_visitor_mixed_default_and_named_imports() {
        use tree_sitter::Parser;

        let source = b"import React, { useState, useEffect } from 'react';";
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::language_typescript())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = TypeScriptVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "react");
        assert_eq!(visitor.imports[0].symbols.len(), 3);
        assert_eq!(visitor.imports[0].symbols[0], "React");
        assert_eq!(visitor.imports[0].symbols[1], "useState");
        assert_eq!(visitor.imports[0].symbols[2], "useEffect");
    }

    #[test]
    fn test_visitor_arrow_function_extraction() {
        use tree_sitter::Parser;

        let source = b"const func = () => { return 42; };";
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::language_typescript())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = TypeScriptVisitor::new(source);
        visitor.visit_node(tree.root_node());

        // Arrow functions should be extracted
        assert!(!visitor.functions.is_empty());
    }

    #[test]
    fn test_visitor_method_extraction() {
        use tree_sitter::Parser;

        let source = b"class Calculator { add(a: number, b: number): number { return a + b; } }";
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::language_typescript())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = TypeScriptVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "Calculator");
        // Should extract method as a function with parent_class
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "add");
        assert_eq!(
            visitor.functions[0].parent_class,
            Some("Calculator".to_string())
        );
    }

    #[test]
    fn test_visitor_multiple_methods() {
        use tree_sitter::Parser;

        let source = b"class Math { add(a, b) { return a + b; } subtract(a, b) { return a - b; } multiply(a, b) { return a * b; } }";
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::language_typescript())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = TypeScriptVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.classes.len(), 1);
        // Should extract all 3 methods
        assert_eq!(visitor.functions.len(), 3);
        assert_eq!(visitor.functions[0].name, "add");
        assert_eq!(visitor.functions[1].name, "subtract");
        assert_eq!(visitor.functions[2].name, "multiply");
        // All methods should have parent_class set
        assert!(visitor
            .functions
            .iter()
            .all(|f| f.parent_class == Some("Math".to_string())));
    }

    #[test]
    fn test_visitor_constructor_extraction() {
        use tree_sitter::Parser;

        let source = b"class Person { constructor(name: string) { this.name = name; } }";
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::language_typescript())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = TypeScriptVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.classes.len(), 1);
        // Should extract constructor
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "constructor");
        assert_eq!(
            visitor.functions[0].parent_class,
            Some("Person".to_string())
        );
    }

    #[test]
    fn test_visitor_static_method() {
        use tree_sitter::Parser;

        let source = b"class Utils { static format(value: string): string { return value; } }";
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::language_typescript())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = TypeScriptVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.classes.len(), 1);
        // Should extract static method
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "format");
        assert!(visitor.functions[0].is_static);
        assert_eq!(visitor.functions[0].parent_class, Some("Utils".to_string()));
    }

    #[test]
    fn test_visitor_call_extraction() {
        use tree_sitter::Parser;

        let source = b"function caller() { callee(); helper(); }";
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::language_typescript())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = TypeScriptVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "caller");

        // Should extract 2 call relationships
        assert_eq!(visitor.calls.len(), 2);
        assert_eq!(visitor.calls[0].caller, "caller");
        assert_eq!(visitor.calls[0].callee, "callee");
        assert_eq!(visitor.calls[1].caller, "caller");
        assert_eq!(visitor.calls[1].callee, "helper");
    }

    #[test]
    fn test_visitor_method_call_extraction() {
        use tree_sitter::Parser;

        let source = b"class MyClass { myMethod() { this.helper(); this.anotherMethod(); } }";
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::language_typescript())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = TypeScriptVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "myMethod");

        // Should extract 2 call relationships (this.helper and this.anotherMethod)
        assert_eq!(visitor.calls.len(), 2);
        assert_eq!(visitor.calls[0].caller, "myMethod");
        assert_eq!(visitor.calls[0].callee, "helper");
        assert_eq!(visitor.calls[1].caller, "myMethod");
        assert_eq!(visitor.calls[1].callee, "anotherMethod");
    }

    #[test]
    fn test_visitor_async_call_extraction() {
        use tree_sitter::Parser;

        let source = b"async function fetchData() { await this.initialize(); const result = await this.getData(); }";
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::language_typescript())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = TypeScriptVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.functions.len(), 1);
        assert!(visitor.functions[0].is_async);

        // Should extract calls from await expressions
        assert!(visitor.calls.len() >= 2);
        let callee_names: Vec<&str> = visitor.calls.iter().map(|c| c.callee.as_str()).collect();
        assert!(callee_names.contains(&"initialize"));
        assert!(callee_names.contains(&"getData"));
    }

    // ==========================================
    // Complexity Tests
    // ==========================================

    #[test]
    fn test_complexity_simple_function() {
        use tree_sitter::Parser;

        let source = b"function simple() { return 1; }";
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::language_typescript())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = TypeScriptVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert_eq!(complexity.cyclomatic_complexity, 1); // Base complexity
        assert_eq!(complexity.branches, 0);
        assert_eq!(complexity.loops, 0);
    }

    #[test]
    fn test_complexity_with_if_else() {
        use tree_sitter::Parser;

        let source = b"function check(x: number) { if (x > 0) { return 1; } else { return 0; } }";
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::language_typescript())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = TypeScriptVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert_eq!(complexity.branches, 2); // if + else
        assert!(complexity.cyclomatic_complexity >= 2);
    }

    #[test]
    fn test_complexity_with_loops() {
        use tree_sitter::Parser;

        let source = b"function loop() { for (let i = 0; i < 10; i++) { console.log(i); } while (true) { break; } }";
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::language_typescript())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = TypeScriptVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert_eq!(complexity.loops, 2); // for + while
        assert!(complexity.cyclomatic_complexity >= 3); // 1 + 2 loops
    }

    #[test]
    fn test_complexity_with_logical_operators() {
        use tree_sitter::Parser;

        let source = b"function check(a: boolean, b: boolean, c: boolean) { if (a && b || c) { return true; } return false; }";
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::language_typescript())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = TypeScriptVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert_eq!(complexity.logical_operators, 2); // && and ||
        assert!(complexity.cyclomatic_complexity >= 4); // 1 + 1 branch + 2 logical ops
    }

    #[test]
    fn test_complexity_with_try_catch() {
        use tree_sitter::Parser;

        let source = b"function safe() { try { doSomething(); } catch (e) { console.error(e); } }";
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::language_typescript())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = TypeScriptVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert_eq!(complexity.exception_handlers, 1); // catch
        assert!(complexity.cyclomatic_complexity >= 2);
    }

    #[test]
    fn test_complexity_with_switch() {
        use tree_sitter::Parser;

        let source = b"function grade(score: number) { switch (score) { case 90: return 'A'; case 80: return 'B'; default: return 'C'; } }";
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::language_typescript())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = TypeScriptVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert_eq!(complexity.branches, 3); // case 90, case 80, default
        assert!(complexity.cyclomatic_complexity >= 4);
    }

    #[test]
    fn test_complexity_with_ternary() {
        use tree_sitter::Parser;

        let source = b"function abs(x: number) { return x >= 0 ? x : -x; }";
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::language_typescript())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = TypeScriptVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert_eq!(complexity.branches, 1); // ternary
        assert!(complexity.cyclomatic_complexity >= 2);
    }

    #[test]
    fn test_complexity_nesting_depth() {
        use tree_sitter::Parser;

        let source = b"function nested(x: number) { if (x > 0) { if (x > 10) { if (x > 100) { return 3; } return 2; } return 1; } return 0; }";
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::language_typescript())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = TypeScriptVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert_eq!(complexity.max_nesting_depth, 3); // 3 levels of if
        assert_eq!(complexity.branches, 3); // 3 if statements
    }

    #[test]
    fn test_complexity_grade() {
        use tree_sitter::Parser;

        // Simple function should get grade A
        let source = b"function simple() { return 1; }";
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::language_typescript())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = TypeScriptVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert_eq!(complexity.grade(), 'A');
    }
}
