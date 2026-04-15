// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting PHP entities

use codegraph_parser_api::{
    CallRelation, ClassEntity, ComplexityBuilder, ComplexityMetrics, FunctionEntity,
    ImplementationRelation, ImportRelation, InheritanceRelation, Parameter, TraitEntity,
    BODY_PREFIX_MAX_CHARS,
    truncate_body_prefix,
};
use tree_sitter::Node;

pub struct PhpVisitor<'a> {
    pub source: &'a [u8],
    pub functions: Vec<FunctionEntity>,
    pub classes: Vec<ClassEntity>,
    pub traits: Vec<TraitEntity>,
    pub imports: Vec<ImportRelation>,
    pub calls: Vec<CallRelation>,
    pub inheritance: Vec<InheritanceRelation>,
    pub implementations: Vec<ImplementationRelation>,
    current_namespace: Option<String>,
    current_class: Option<String>,
    current_function: Option<String>,
}

impl<'a> PhpVisitor<'a> {
    pub fn new(source: &'a [u8]) -> Self {
        Self {
            source,
            functions: Vec::new(),
            classes: Vec::new(),
            traits: Vec::new(),
            imports: Vec::new(),
            calls: Vec::new(),
            inheritance: Vec::new(),
            implementations: Vec::new(),
            current_namespace: None,
            current_class: None,
            current_function: None,
        }
    }

    fn node_text(&self, node: Node) -> String {
        node.utf8_text(self.source).unwrap_or("").to_string()
    }

    pub fn visit_node(&mut self, node: Node) {
        // Track whether we should recurse into children
        let should_recurse = match node.kind() {
            "function_definition" => {
                self.visit_function(node);
                false // Don't recurse into function body
            }
            "method_declaration" => {
                // Only visit if not in a class context (would be double-counted)
                if self.current_class.is_none() {
                    self.visit_method(node);
                }
                false
            }
            "class_declaration" => {
                self.visit_class(node);
                false // visit_class handles body itself
            }
            "interface_declaration" => {
                self.visit_interface(node);
                false // visit_interface handles body itself
            }
            "trait_declaration" => {
                self.visit_trait(node);
                false // visit_trait handles body itself
            }
            "enum_declaration" => {
                self.visit_enum(node);
                false // visit_enum handles body itself
            }
            "namespace_definition" => {
                self.visit_namespace(node);
                true // Recurse to find declarations inside namespace
            }
            "namespace_use_declaration" => {
                self.visit_use(node);
                false
            }
            "include_expression"
            | "include_once_expression"
            | "require_expression"
            | "require_once_expression" => {
                self.visit_include_require(node);
                false
            }
            "anonymous_function_creation_expression" | "arrow_function" => {
                false // Skip anonymous functions
            }
            // Call expressions - extract call relationships
            "function_call_expression" | "member_call_expression" | "scoped_call_expression" => {
                self.visit_call_expression(node);
                true // Recurse to find nested calls
            }
            _ => true, // Recurse into other nodes
        };

        if should_recurse {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                self.visit_node(child);
            }
        }
    }

    fn visit_function(&mut self, node: Node) {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_else(|| "anonymous".to_string());

        let visibility = "public".to_string(); // Top-level functions are public

        let return_type = self.extract_return_type(node);
        let parameters = self.extract_parameters(node);
        let doc_comment = self.extract_doc_comment(node);

        let qualified_name = self.qualify_name(&name);

        // Calculate complexity from function body
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

        let func = FunctionEntity {
            name: qualified_name.clone(),
            signature: self.extract_function_signature(node),
            visibility,
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_async: false,
            is_test: false,
            is_static: false,
            is_abstract: false,
            parameters,
            return_type,
            doc_comment,
            attributes: Vec::new(),
            parent_class: None,
            complexity,
            body_prefix,
        };

        self.functions.push(func);

        // Set current function context and visit body to extract calls
        let previous_function = self.current_function.take();
        self.current_function = Some(qualified_name);

        if let Some(body) = node.child_by_field_name("body") {
            self.visit_function_body(body);
        }

        self.current_function = previous_function;
    }

    fn visit_method(&mut self, node: Node) {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_else(|| "method".to_string());

        let (visibility, is_static, is_abstract) = self.extract_method_modifiers(node);
        let return_type = self.extract_return_type(node);
        let parameters = self.extract_parameters(node);
        let doc_comment = self.extract_doc_comment(node);

        // Calculate complexity from method body
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

        let func = FunctionEntity {
            name: name.clone(),
            signature: self.extract_function_signature(node),
            visibility,
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_async: false,
            is_test: false,
            is_static,
            is_abstract,
            parameters,
            return_type,
            doc_comment,
            attributes: Vec::new(),
            parent_class: self.current_class.clone(),
            complexity,
            body_prefix,
        };

        self.functions.push(func);

        // Set current function context and visit body to extract calls
        let previous_function = self.current_function.take();
        self.current_function = Some(name);

        if let Some(body) = node.child_by_field_name("body") {
            self.visit_function_body(body);
        }

        self.current_function = previous_function;
    }

    fn visit_class(&mut self, node: Node) {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_else(|| "Class".to_string());

        let qualified_name = self.qualify_name(&name);
        let is_abstract = self.has_abstract_modifier(node);
        let doc_comment = self.extract_doc_comment(node);

        // Extract base class (extends) and interfaces (implements)
        // These are child nodes, not fields in tree-sitter-php
        let mut base_classes = Vec::new();
        let mut implemented_traits = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "base_clause" {
                // Extract parent class from extends clause
                let mut bc_cursor = child.walk();
                for bc_child in child.named_children(&mut bc_cursor) {
                    if bc_child.kind() == "name" || bc_child.kind() == "qualified_name" {
                        let base_name = self.node_text(bc_child);
                        base_classes.push(base_name.clone());
                        self.inheritance.push(InheritanceRelation {
                            child: qualified_name.clone(),
                            parent: base_name,
                            order: 0,
                        });
                    }
                }
            } else if child.kind() == "class_interface_clause" {
                // Extract implemented interfaces
                self.extract_implemented_interfaces(
                    child,
                    &qualified_name,
                    &mut implemented_traits,
                );
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

        let class_entity = ClassEntity {
            name: qualified_name.clone(),
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_abstract,
            is_interface: false,
            base_classes,
            implemented_traits,
            methods: Vec::new(),
            fields: Vec::new(),
            doc_comment,
            attributes: Vec::new(),
            type_parameters: Vec::new(),
            body_prefix,
        };

        self.classes.push(class_entity);

        // Set current class context for method extraction
        let previous_class = self.current_class.take();
        self.current_class = Some(qualified_name);

        // Visit class body to extract methods and trait uses
        if let Some(body) = node.child_by_field_name("body") {
            self.visit_class_body(body);
        }

        self.current_class = previous_class;
    }

    fn visit_class_body(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "method_declaration" => self.visit_method(child),
                "use_declaration" => self.visit_trait_use(child),
                _ => {}
            }
        }
    }

    fn visit_trait_use(&mut self, node: Node) {
        if let Some(class_name) = &self.current_class.clone() {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "name" || child.kind() == "qualified_name" {
                    let trait_name = self.node_text(child);
                    self.implementations.push(ImplementationRelation {
                        implementor: class_name.clone(),
                        trait_name,
                    });
                }
            }
        }
    }

    fn visit_interface(&mut self, node: Node) {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_else(|| "Interface".to_string());

        let qualified_name = self.qualify_name(&name);
        let doc_comment = self.extract_doc_comment(node);

        // Extract parent interfaces (extends)
        let mut parent_traits = Vec::new();
        if let Some(base_clause) = node.child_by_field_name("base_clause") {
            for child in base_clause.children(&mut base_clause.walk()) {
                if child.kind() == "name" || child.kind() == "qualified_name" {
                    parent_traits.push(self.node_text(child));
                }
            }
        }

        // Extract required methods
        let required_methods = self.extract_interface_methods(node);

        let interface_entity = TraitEntity {
            name: qualified_name.clone(),
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            required_methods,
            parent_traits,
            doc_comment,
            attributes: Vec::new(),
        };

        self.traits.push(interface_entity);

        // Set current class context for method extraction in interface
        let previous_class = self.current_class.take();
        self.current_class = Some(qualified_name);

        // Visit interface body
        if let Some(body) = node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for child in body.children(&mut cursor) {
                if child.kind() == "method_declaration" {
                    self.visit_method(child);
                }
            }
        }

        self.current_class = previous_class;
    }

    fn visit_trait(&mut self, node: Node) {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_else(|| "Trait".to_string());

        let qualified_name = self.qualify_name(&name);
        let doc_comment = self.extract_doc_comment(node);

        let trait_entity = TraitEntity {
            name: qualified_name.clone(),
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            required_methods: Vec::new(),
            parent_traits: Vec::new(),
            doc_comment,
            attributes: Vec::new(),
        };

        self.traits.push(trait_entity);

        // Set current class context for method extraction
        let previous_class = self.current_class.take();
        self.current_class = Some(qualified_name);

        // Visit trait body to extract methods
        if let Some(body) = node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for child in body.children(&mut cursor) {
                if child.kind() == "method_declaration" {
                    self.visit_method(child);
                }
            }
        }

        self.current_class = previous_class;
    }

    fn visit_enum(&mut self, node: Node) {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_else(|| "Enum".to_string());

        let qualified_name = self.qualify_name(&name);
        let doc_comment = self.extract_doc_comment(node);

        let body_prefix = node
            .child_by_field_name("body")
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| {
                truncate_body_prefix(t)
            })
            .map(|t| t.to_string());

        // PHP 8.1 enums are treated as classes
        let enum_entity = ClassEntity {
            name: qualified_name.clone(),
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_abstract: false,
            is_interface: false,
            base_classes: Vec::new(),
            implemented_traits: Vec::new(),
            methods: Vec::new(),
            fields: Vec::new(),
            doc_comment,
            attributes: vec!["enum".to_string()],
            type_parameters: Vec::new(),
            body_prefix,
        };

        self.classes.push(enum_entity);

        // Set current class context for method extraction
        let previous_class = self.current_class.take();
        self.current_class = Some(qualified_name);

        // Visit enum body to extract methods
        if let Some(body) = node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for child in body.children(&mut cursor) {
                if child.kind() == "method_declaration" {
                    self.visit_method(child);
                }
            }
        }

        self.current_class = previous_class;
    }

    fn visit_namespace(&mut self, node: Node) {
        let name = node.child_by_field_name("name").map(|n| self.node_text(n));

        self.current_namespace = name;
    }

    fn visit_use(&mut self, node: Node) {
        // Extract use statements (imports)
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "namespace_use_clause" {
                self.extract_use_clause(child);
            }
        }
    }

    fn visit_include_require(&mut self, node: Node) {
        // Extract the file path from include/require expressions.
        // Handles: string literals, parenthesized expressions, and binary_expression
        // concatenations (e.g., ABSPATH . WPINC . '/version.php').
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "string" | "encapsed_string" => {
                    let path = self.extract_string_content(child);
                    if !path.is_empty() {
                        self.push_include_import(path, false, node.kind());
                    }
                    return;
                }
                "binary_expression" => {
                    self.handle_concat_include(child, node.kind());
                    return;
                }
                "parenthesized_expression" => {
                    let mut inner_cursor = child.walk();
                    for inner in child.children(&mut inner_cursor) {
                        match inner.kind() {
                            "string" | "encapsed_string" => {
                                let path = self.extract_string_content(inner);
                                if !path.is_empty() {
                                    self.push_include_import(path, false, node.kind());
                                }
                                return;
                            }
                            "binary_expression" => {
                                self.handle_concat_include(inner, node.kind());
                                return;
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Build the imported path from a concatenation expression and push an import.
    fn handle_concat_include(&mut self, concat_node: Node, include_kind: &str) {
        let mut string_parts: Vec<String> = Vec::new();
        let mut has_dir_marker = false;
        let mut has_dynamic_parts = false;
        self.collect_concat_parts(
            concat_node,
            &mut string_parts,
            &mut has_dir_marker,
            &mut has_dynamic_parts,
        );

        if string_parts.is_empty() {
            return;
        }

        let joined = string_parts.join("");

        if has_dir_marker && !has_dynamic_parts {
            // __DIR__ . '/config.php' or dirname(__FILE__) . '/helpers.php'
            let path = if joined.starts_with('/') {
                format!(".{}", joined)
            } else {
                format!("./{}", joined)
            };
            self.push_include_import(path, false, include_kind);
        } else if !joined.is_empty() {
            // Dynamic parts present — emit as suffix match
            self.push_include_import(joined, true, include_kind);
        }
    }

    /// Recursively walk a binary_expression tree collecting string literals
    /// and detecting __DIR__/dirname(__FILE__) markers.
    fn collect_concat_parts(
        &self,
        node: Node,
        string_parts: &mut Vec<String>,
        has_dir_marker: &mut bool,
        has_dynamic_parts: &mut bool,
    ) {
        match node.kind() {
            "binary_expression" => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() != "." {
                        self.collect_concat_parts(
                            child,
                            string_parts,
                            has_dir_marker,
                            has_dynamic_parts,
                        );
                    }
                }
            }
            "string" | "encapsed_string" => {
                let content = self.extract_string_content(node);
                if !content.is_empty() {
                    string_parts.push(content);
                }
            }
            "name" => {
                let text = self.node_text(node);
                if text == "__DIR__" {
                    *has_dir_marker = true;
                } else {
                    *has_dynamic_parts = true;
                }
            }
            "variable_name" => {
                *has_dynamic_parts = true;
            }
            "function_call_expression" => {
                if self.is_dirname_file_call(node) {
                    *has_dir_marker = true;
                } else {
                    *has_dynamic_parts = true;
                }
            }
            "parenthesized_expression" => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() != "(" && child.kind() != ")" {
                        self.collect_concat_parts(
                            child,
                            string_parts,
                            has_dir_marker,
                            has_dynamic_parts,
                        );
                    }
                }
            }
            _ => {
                *has_dynamic_parts = true;
            }
        }
    }

    /// Check if a function_call_expression is `dirname(__FILE__)`.
    fn is_dirname_file_call(&self, node: Node) -> bool {
        let mut cursor = node.walk();
        let mut is_dirname = false;
        let mut has_file_arg = false;
        for child in node.children(&mut cursor) {
            match child.kind() {
                "name" => {
                    if self.node_text(child) == "dirname" {
                        is_dirname = true;
                    }
                }
                "arguments" => {
                    let mut arg_cursor = child.walk();
                    for arg in child.children(&mut arg_cursor) {
                        if arg.kind() == "argument" || arg.kind() == "name" {
                            let text = self.node_text(arg);
                            if text == "__FILE__" || text.contains("__FILE__") {
                                has_file_arg = true;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        is_dirname && has_file_arg
    }

    /// Helper to push an include/require import.
    fn push_include_import(&mut self, path: String, is_suffix: bool, include_kind: &str) {
        let import = ImportRelation {
            importer: "include_require".to_string(),
            imported: path,
            symbols: Vec::new(),
            is_wildcard: is_suffix,
            alias: Some(include_kind.to_string()),
        };
        self.imports.push(import);
    }

    fn extract_string_content(&self, node: Node) -> String {
        let text = self.node_text(node);
        // Remove quotes: 'file.php' or "file.php"
        text.trim_matches(|c| c == '\'' || c == '"').to_string()
    }

    fn extract_use_clause(&mut self, node: Node) {
        let mut cursor = node.walk();
        let mut imported = String::new();
        let mut alias = None;

        for child in node.children(&mut cursor) {
            match child.kind() {
                "qualified_name" | "name" => {
                    imported = self.node_text(child);
                }
                "namespace_aliasing_clause" => {
                    // Extract alias from "as Alias" clause
                    let mut alias_cursor = child.walk();
                    for alias_child in child.children(&mut alias_cursor) {
                        if alias_child.kind() == "name" {
                            alias = Some(self.node_text(alias_child));
                        }
                    }
                }
                _ => {}
            }
        }

        if !imported.is_empty() {
            let import = ImportRelation {
                importer: self
                    .current_namespace
                    .clone()
                    .unwrap_or_else(|| "global".to_string()),
                imported,
                symbols: Vec::new(),
                is_wildcard: false,
                alias,
            };
            self.imports.push(import);
        }
    }

    /// Visit a call expression and extract the call relationship
    fn visit_call_expression(&mut self, node: Node) {
        // Only record calls if we're inside a function/method
        let caller = match &self.current_function {
            Some(name) => name.clone(),
            None => return,
        };

        let callee = self.extract_callee_name(node);

        // Skip empty callees or $this only
        if callee.is_empty() || callee == "$this" || callee == "this" {
            return;
        }

        let call_site_line = node.start_position().row + 1;

        let call = CallRelation {
            caller,
            callee,
            call_site_line,
            is_direct: true,
            struct_type: None,
            field_name: None,
        };

        self.calls.push(call);
    }

    /// Extract the callee name from a call expression node
    fn extract_callee_name(&self, node: Node) -> String {
        match node.kind() {
            // Regular function call: foo(), bar()
            "function_call_expression" => {
                if let Some(function_node) = node.child_by_field_name("function") {
                    match function_node.kind() {
                        "name" => self.node_text(function_node),
                        "qualified_name" => {
                            // Fully qualified name like \Namespace\func()
                            self.node_text(function_node)
                        }
                        "variable_name" => {
                            // Dynamic call like $callback()
                            self.node_text(function_node)
                        }
                        _ => self.node_text(function_node),
                    }
                } else {
                    String::new()
                }
            }
            // Method call: $obj->method(), $this->method()
            "member_call_expression" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    self.node_text(name_node)
                } else {
                    String::new()
                }
            }
            // Static method call: Class::method(), self::method()
            "scoped_call_expression" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    // Get the scope (class name) for context
                    let scope = node
                        .child_by_field_name("scope")
                        .map(|n| self.node_text(n))
                        .unwrap_or_default();
                    let method = self.node_text(name_node);

                    // Return as Class::method for static calls
                    if scope.is_empty() || scope == "self" || scope == "static" {
                        method
                    } else {
                        format!("{}::{}", scope, method)
                    }
                } else {
                    String::new()
                }
            }
            _ => String::new(),
        }
    }

    /// Visit function body to extract calls
    fn visit_function_body(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "function_call_expression"
                | "member_call_expression"
                | "scoped_call_expression" => {
                    self.visit_call_expression(child);
                    // Also recurse into arguments for nested calls
                    self.visit_function_body(child);
                }
                _ => {
                    self.visit_function_body(child);
                }
            }
        }
    }

    fn extract_implemented_interfaces(
        &mut self,
        node: Node,
        class_name: &str,
        implemented_traits: &mut Vec<String>,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "name" || child.kind() == "qualified_name" {
                let interface_name = self.node_text(child);
                implemented_traits.push(interface_name.clone());
                self.implementations.push(ImplementationRelation {
                    implementor: class_name.to_string(),
                    trait_name: interface_name,
                });
            }
        }
    }

    fn extract_interface_methods(&self, node: Node) -> Vec<FunctionEntity> {
        let mut methods = Vec::new();
        if let Some(body) = node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for child in body.children(&mut cursor) {
                if child.kind() == "method_declaration" {
                    let name = child
                        .child_by_field_name("name")
                        .map(|n| self.node_text(n))
                        .unwrap_or_else(|| "method".to_string());

                    let (visibility, is_static, _is_abstract) =
                        self.extract_method_modifiers(child);
                    let return_type = self.extract_return_type(child);
                    let parameters = self.extract_parameters(child);

                    let func = FunctionEntity {
                        name,
                        signature: self.extract_function_signature(child),
                        visibility,
                        line_start: child.start_position().row + 1,
                        line_end: child.end_position().row + 1,
                        is_async: false,
                        is_test: false,
                        is_static,
                        is_abstract: true, // Interface methods are always abstract
                        parameters,
                        return_type,
                        doc_comment: None,
                        attributes: Vec::new(),
                        parent_class: None,
                        complexity: None,
                        body_prefix: None,
                    };
                    methods.push(func);
                }
            }
        }
        methods
    }

    fn extract_return_type(&self, node: Node) -> Option<String> {
        node.child_by_field_name("return_type")
            .map(|n| self.node_text(n))
    }

    fn extract_parameters(&self, node: Node) -> Vec<Parameter> {
        let mut params = Vec::new();
        if let Some(params_node) = node.child_by_field_name("parameters") {
            let mut cursor = params_node.walk();
            for child in params_node.children(&mut cursor) {
                if child.kind() == "simple_parameter" || child.kind() == "variadic_parameter" {
                    if let Some(name_node) = child.child_by_field_name("name") {
                        let name = self.node_text(name_node);
                        let is_variadic = child.kind() == "variadic_parameter";

                        // Extract type annotation
                        let type_annotation =
                            child.child_by_field_name("type").map(|n| self.node_text(n));

                        // Extract default value
                        let default_value = child
                            .child_by_field_name("default_value")
                            .map(|n| self.node_text(n));

                        let mut param = Parameter::new(name);
                        if let Some(t) = type_annotation {
                            param = param.with_type(t);
                        }
                        if let Some(d) = default_value {
                            param = param.with_default(d);
                        }
                        if is_variadic {
                            param = param.variadic();
                        }
                        params.push(param);
                    }
                }
            }
        }
        params
    }

    fn extract_function_signature(&self, node: Node) -> String {
        self.node_text(node)
            .lines()
            .next()
            .unwrap_or("")
            .to_string()
    }

    fn extract_method_modifiers(&self, node: Node) -> (String, bool, bool) {
        let mut visibility = "public".to_string();
        let mut is_static = false;
        let mut is_abstract = false;

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "visibility_modifier" => {
                    visibility = self.node_text(child);
                }
                "static_modifier" => {
                    is_static = true;
                }
                "abstract_modifier" => {
                    is_abstract = true;
                }
                _ => {}
            }
        }

        (visibility, is_static, is_abstract)
    }

    fn has_abstract_modifier(&self, node: Node) -> bool {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "abstract_modifier" {
                return true;
            }
        }
        false
    }

    fn extract_doc_comment(&self, node: Node) -> Option<String> {
        // Look for preceding comment node
        if let Some(prev) = node.prev_sibling() {
            if prev.kind() == "comment" {
                let comment = self.node_text(prev);
                if comment.starts_with("/**") {
                    return Some(comment);
                }
            }
        }
        None
    }

    fn calculate_complexity(&self, body: Node) -> ComplexityMetrics {
        let mut builder = ComplexityBuilder::new();
        self.visit_for_complexity(body, &mut builder);
        builder.build()
    }

    fn visit_for_complexity(&self, node: Node, builder: &mut ComplexityBuilder) {
        match node.kind() {
            "if_statement" => {
                builder.add_branch();
                builder.enter_scope();
            }
            "else_if_clause" => {
                builder.add_branch();
                builder.enter_scope();
            }
            "else_clause" => {
                builder.add_branch();
            }
            "switch_statement" => {
                builder.enter_scope();
            }
            "switch_case" | "case_statement" => {
                builder.add_branch();
            }
            "switch_default_case" | "default_statement" => {
                builder.add_branch();
            }
            "conditional_expression" => {
                // Ternary operator ?:
                builder.add_branch();
            }
            "match_conditional_expression" => {
                // PHP 8.0 match expression arm
                builder.add_branch();
            }
            "for_statement" => {
                builder.add_loop();
                builder.enter_scope();
            }
            "while_statement" => {
                builder.add_loop();
                builder.enter_scope();
            }
            "do_statement" => {
                builder.add_loop();
                builder.enter_scope();
            }
            "foreach_statement" => {
                builder.add_loop();
                builder.enter_scope();
            }
            "binary_expression" => {
                // Check for &&, ||, and, or operators
                if let Some(op) = node.child_by_field_name("operator") {
                    let op_text = self.node_text(op);
                    if op_text == "&&" || op_text == "||" || op_text == "and" || op_text == "or" {
                        builder.add_logical_operator();
                    }
                }
            }
            "catch_clause" => {
                builder.add_exception_handler();
                builder.enter_scope();
            }
            "finally_clause" => {
                builder.add_exception_handler();
                builder.enter_scope();
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_for_complexity(child, builder);
        }

        // Exit scope for control structures
        match node.kind() {
            "if_statement" | "else_if_clause" | "for_statement" | "while_statement"
            | "do_statement" | "foreach_statement" | "switch_statement" | "catch_clause"
            | "finally_clause" => {
                builder.exit_scope();
            }
            _ => {}
        }
    }

    fn qualify_name(&self, name: &str) -> String {
        if let Some(ref ns) = self.current_namespace {
            format!("{}\\{}", ns, name)
        } else {
            name.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_visit(source: &[u8]) -> PhpVisitor<'_> {
        use tree_sitter::Parser;

        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_php::LANGUAGE_PHP.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = PhpVisitor::new(source);
        visitor.visit_node(tree.root_node());
        visitor
    }

    #[test]
    fn test_visitor_basics() {
        let visitor = PhpVisitor::new(b"<?php");
        assert_eq!(visitor.functions.len(), 0);
        assert_eq!(visitor.classes.len(), 0);
        assert_eq!(visitor.traits.len(), 0);
    }

    #[test]
    fn test_visitor_function_extraction() {
        let source = b"<?php\nfunction greet(string $name): string { return \"Hello\"; }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "greet");
    }

    #[test]
    fn test_visitor_class_extraction() {
        let source = b"<?php\nclass Person { public string $name; }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "Person");
    }

    #[test]
    fn test_visitor_interface_extraction() {
        let source = b"<?php\ninterface Reader { public function read(): string; }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.traits.len(), 1);
        assert_eq!(visitor.traits[0].name, "Reader");
    }

    #[test]
    fn test_visitor_trait_extraction() {
        let source = b"<?php\ntrait Loggable { public function log(string $msg): void {} }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.traits.len(), 1);
        assert_eq!(visitor.traits[0].name, "Loggable");
    }

    #[test]
    fn test_visitor_method_extraction() {
        let source = b"<?php\nclass Calculator { public function add(int $a, int $b): int { return $a + $b; } }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "add");
        assert_eq!(
            visitor.functions[0].parent_class,
            Some("Calculator".to_string())
        );
    }

    #[test]
    fn test_visitor_use_extraction() {
        let source = b"<?php\nuse App\\Models\\User;\nuse App\\Services\\AuthService;";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 2);
        assert_eq!(visitor.imports[0].imported, "App\\Models\\User");
        assert_eq!(visitor.imports[1].imported, "App\\Services\\AuthService");
    }

    #[test]
    fn test_visitor_use_with_alias() {
        let source = b"<?php\nuse App\\Models\\User as UserModel;";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "App\\Models\\User");
        assert_eq!(visitor.imports[0].alias, Some("UserModel".to_string()));
    }

    #[test]
    fn test_visitor_inheritance() {
        let source = b"<?php\nclass Animal {}\nclass Dog extends Animal {}";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 2);
        assert_eq!(visitor.inheritance.len(), 1);
        assert_eq!(visitor.inheritance[0].child, "Dog");
        assert_eq!(visitor.inheritance[0].parent, "Animal");
    }

    #[test]
    fn test_visitor_implements() {
        let source =
            b"<?php\ninterface Shape { public function area(): float; }\nclass Circle implements Shape { public function area(): float { return 0.0; } }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.traits.len(), 1);
        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.implementations.len(), 1);
        assert_eq!(visitor.implementations[0].implementor, "Circle");
        assert_eq!(visitor.implementations[0].trait_name, "Shape");
    }

    #[test]
    fn test_visitor_enum() {
        let source = b"<?php\nenum Status: string { case Pending = 'pending'; }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "Status");
        assert!(visitor.classes[0].attributes.contains(&"enum".to_string()));
    }

    #[test]
    fn test_visitor_namespace() {
        let source = b"<?php\nnamespace App\\Controllers;\nclass HomeController {}";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "App\\Controllers\\HomeController");
    }

    #[test]
    fn test_visitor_abstract_class() {
        let source =
            b"<?php\nabstract class BaseController { abstract public function handle(): void; }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert!(visitor.classes[0].is_abstract);
    }

    #[test]
    fn test_visitor_static_method() {
        let source =
            b"<?php\nclass Helper { public static function format(string $s): string { return $s; } }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert!(visitor.functions[0].is_static);
    }

    #[test]
    fn test_visitor_visibility_modifiers() {
        let source = b"<?php\nclass Foo { private function bar(): void {} protected function baz(): void {} public function qux(): void {} }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 3);
        assert_eq!(visitor.functions[0].visibility, "private");
        assert_eq!(visitor.functions[1].visibility, "protected");
        assert_eq!(visitor.functions[2].visibility, "public");
    }

    #[test]
    fn test_visitor_trait_use() {
        let source =
            b"<?php\ntrait Loggable {}\nclass Logger { use Loggable; public function log(): void {} }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.traits.len(), 1);
        assert_eq!(visitor.classes.len(), 1);
        assert!(visitor
            .implementations
            .iter()
            .any(|i| i.implementor == "Logger" && i.trait_name == "Loggable"));
    }

    #[test]
    fn test_visitor_function_call_extraction() {
        let source = b"<?php
function caller() {
    helper();
    another_func();
}
function helper() {}
function another_func() {}
";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 3);
        assert_eq!(visitor.calls.len(), 2);

        // Check the calls are from caller to helper and another_func
        assert!(visitor
            .calls
            .iter()
            .any(|c| c.caller == "caller" && c.callee == "helper"));
        assert!(visitor
            .calls
            .iter()
            .any(|c| c.caller == "caller" && c.callee == "another_func"));
    }

    #[test]
    fn test_visitor_method_call_extraction() {
        let source = b"<?php
class MyClass {
    public function caller() {
        $this->helper();
        $this->process();
    }

    public function helper() {}
    public function process() {}
}
";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.functions.len(), 3);
        assert_eq!(visitor.calls.len(), 2);

        // Check calls: caller -> helper, caller -> process
        assert!(visitor
            .calls
            .iter()
            .any(|c| c.caller == "caller" && c.callee == "helper"));
        assert!(visitor
            .calls
            .iter()
            .any(|c| c.caller == "caller" && c.callee == "process"));
    }

    #[test]
    fn test_visitor_static_call_extraction() {
        let source = b"<?php
class Calculator {
    public function calculate() {
        self::helper();
        Helper::format();
    }

    public static function helper() {}
}
";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.calls.len(), 2);

        // Check calls: calculate -> helper (self::), calculate -> Helper::format
        assert!(visitor
            .calls
            .iter()
            .any(|c| c.caller == "calculate" && c.callee == "helper"));
        assert!(visitor
            .calls
            .iter()
            .any(|c| c.caller == "calculate" && c.callee == "Helper::format"));
    }

    #[test]
    fn test_visitor_nested_calls() {
        let source = b"<?php
function outer() {
    process(helper());
}
function helper() {}
function process($x) {}
";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.calls.len(), 2);

        // Both calls should be from outer
        assert!(visitor
            .calls
            .iter()
            .any(|c| c.caller == "outer" && c.callee == "process"));
        assert!(visitor
            .calls
            .iter()
            .any(|c| c.caller == "outer" && c.callee == "helper"));
    }

    #[test]
    fn test_visitor_call_line_numbers() {
        let source = b"<?php
function caller() {
    helper();
}
function helper() {}
";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.calls.len(), 1);
        assert_eq!(visitor.calls[0].caller, "caller");
        assert_eq!(visitor.calls[0].callee, "helper");
        assert_eq!(visitor.calls[0].call_site_line, 3);
        assert!(visitor.calls[0].is_direct);
    }

    #[test]
    fn test_visitor_include_require() {
        let source = b"<?php
include 'helpers.php';
require 'config.php';
include_once 'db.php';
require_once 'auth.php';
";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 4);
        assert_eq!(visitor.imports[0].imported, "helpers.php");
        assert_eq!(visitor.imports[0].importer, "include_require");
        assert_eq!(
            visitor.imports[0].alias,
            Some("include_expression".to_string())
        );

        assert_eq!(visitor.imports[1].imported, "config.php");
        assert_eq!(
            visitor.imports[1].alias,
            Some("require_expression".to_string())
        );

        assert_eq!(visitor.imports[2].imported, "db.php");
        assert_eq!(
            visitor.imports[2].alias,
            Some("include_once_expression".to_string())
        );

        assert_eq!(visitor.imports[3].imported, "auth.php");
        assert_eq!(
            visitor.imports[3].alias,
            Some("require_once_expression".to_string())
        );
    }

    #[test]
    fn test_visitor_include_with_parens() {
        let source = b"<?php
require('lib/helper.php');
include_once('utils/format.php');
";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 2);
        assert_eq!(visitor.imports[0].imported, "lib/helper.php");
        assert_eq!(visitor.imports[1].imported, "utils/format.php");
    }

    #[test]
    fn test_visitor_include_with_relative_path() {
        let source = b"<?php
require '../config.php';
include './helpers/utils.php';
";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 2);
        assert_eq!(visitor.imports[0].imported, "../config.php");
        assert_eq!(visitor.imports[1].imported, "./helpers/utils.php");
    }

    #[test]
    fn test_visitor_include_and_use_combined() {
        let source = b"<?php
use App\\Models\\User;
require_once 'vendor/autoload.php';
include 'helpers.php';
";
        let visitor = parse_and_visit(source);

        // 1 namespace use + 2 include/require
        assert_eq!(visitor.imports.len(), 3);
        // Namespace use comes first in source order
        assert_eq!(visitor.imports[0].imported, "App\\Models\\User");
        assert_eq!(visitor.imports[0].importer, "global"); // namespace use
        assert_eq!(visitor.imports[1].imported, "vendor/autoload.php");
        assert_eq!(visitor.imports[1].importer, "include_require");
        assert_eq!(visitor.imports[2].imported, "helpers.php");
        assert_eq!(visitor.imports[2].importer, "include_require");
    }

    // --- Dynamic concatenation tests ---

    #[test]
    fn test_visitor_include_concat_dir_magic() {
        let source = b"<?php\nrequire __DIR__ . '/config.php';\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "./config.php");
        assert_eq!(visitor.imports[0].importer, "include_require");
        assert!(!visitor.imports[0].is_wildcard);
    }

    #[test]
    fn test_visitor_include_concat_dirname_file() {
        let source = b"<?php\ninclude dirname(__FILE__) . '/helpers.php';\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "./helpers.php");
        assert!(!visitor.imports[0].is_wildcard);
    }

    #[test]
    fn test_visitor_include_concat_constants() {
        let source = b"<?php\nrequire ABSPATH . WPINC . '/version.php';\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "/version.php");
        assert!(visitor.imports[0].is_wildcard);
    }

    #[test]
    fn test_visitor_include_concat_constant_with_path() {
        let source = b"<?php\nrequire ABSPATH . 'wp-admin/includes/file.php';\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "wp-admin/includes/file.php");
        assert!(visitor.imports[0].is_wildcard);
    }

    #[test]
    fn test_visitor_include_concat_variable() {
        let source = b"<?php\nrequire $basePath . '/config.php';\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "/config.php");
        assert!(visitor.imports[0].is_wildcard);
    }

    #[test]
    fn test_visitor_include_concat_multiple_strings() {
        let source = b"<?php\nrequire ABSPATH . 'wp-admin' . '/' . 'file.php';\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "wp-admin/file.php");
        assert!(visitor.imports[0].is_wildcard);
    }

    #[test]
    fn test_visitor_include_fully_dynamic() {
        let source = b"<?php\nrequire $dynamic_path;\n";
        let visitor = parse_and_visit(source);
        // Fully dynamic — no string parts, should be skipped
        assert_eq!(visitor.imports.len(), 0);
    }

    #[test]
    fn test_visitor_include_concat_with_parens() {
        let source = b"<?php\nrequire(ABSPATH . '/wp-settings.php');\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "/wp-settings.php");
        assert!(visitor.imports[0].is_wildcard);
    }

    #[test]
    fn test_visitor_include_concat_dir_no_slash() {
        let source = b"<?php\nrequire __DIR__ . 'config.php';\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "./config.php");
        assert!(!visitor.imports[0].is_wildcard);
    }

    // --- Complexity tests ---

    #[test]
    fn test_visitor_complexity_simple_function() {
        // A simple function with no branches: CC = 1
        let source = b"<?php\nfunction hello(): string { return 'hello'; }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert_eq!(complexity.cyclomatic_complexity, 1);
        assert_eq!(complexity.branches, 0);
        assert_eq!(complexity.loops, 0);
        assert_eq!(complexity.logical_operators, 0);
    }

    #[test]
    fn test_visitor_complexity_if_elseif_else_foreach() {
        // Function with if/elseif/else and foreach: CC > 1
        let source = b"<?php
function categorize(array $items): string {
    $result = '';
    if (count($items) === 0) {
        $result = 'empty';
    } elseif (count($items) < 5) {
        foreach ($items as $item) {
            $result .= $item;
        }
    } else {
        $result = 'large';
    }
    return $result;
}
";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        // if + elseif + else = 3 branches, foreach = 1 loop => CC = 1 + 3 + 1 = 5
        assert!(
            complexity.branches >= 3,
            "Expected at least 3 branches, got {}",
            complexity.branches
        );
        assert!(
            complexity.loops >= 1,
            "Expected at least 1 loop, got {}",
            complexity.loops
        );
        assert!(complexity.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_visitor_complexity_try_catch() {
        // Function with try/catch: exception_handlers should be counted
        let source = b"<?php
function loadFile(string $path): string {
    try {
        $content = file_get_contents($path);
        if ($content === false) {
            throw new RuntimeException('Cannot read file');
        }
        return $content;
    } catch (RuntimeException $e) {
        return '';
    }
}
";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        // catch_clause = 1 exception_handler, if_statement = 1 branch => CC = 1 + 1 + 1 = 3
        assert!(
            complexity.exception_handlers >= 1,
            "Expected at least 1 exception handler, got {}",
            complexity.exception_handlers
        );
        assert!(complexity.cyclomatic_complexity > 1);
    }
}
