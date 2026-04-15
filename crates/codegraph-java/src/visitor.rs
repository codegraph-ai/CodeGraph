// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting Java entities

use codegraph_parser_api::{
    CallRelation, ClassEntity, ComplexityBuilder, ComplexityMetrics, FunctionEntity,
    ImplementationRelation, ImportRelation, InheritanceRelation, Parameter, TraitEntity,
    BODY_PREFIX_MAX_CHARS,
    truncate_body_prefix,
};
use tree_sitter::Node;

pub struct JavaVisitor<'a> {
    pub source: &'a [u8],
    pub functions: Vec<FunctionEntity>,
    pub classes: Vec<ClassEntity>,
    pub traits: Vec<TraitEntity>,
    pub imports: Vec<ImportRelation>,
    pub calls: Vec<CallRelation>,
    pub inheritance: Vec<InheritanceRelation>,
    pub implementations: Vec<ImplementationRelation>,
    current_package: Option<String>,
    current_class: Option<String>,
    current_function: Option<String>,
}

impl<'a> JavaVisitor<'a> {
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
            current_package: None,
            current_class: None,
            current_function: None,
        }
    }

    fn node_text(&self, node: Node) -> String {
        node.utf8_text(self.source).unwrap_or("").to_string()
    }

    pub fn visit_node(&mut self, node: Node) {
        let should_recurse = match node.kind() {
            "package_declaration" => {
                self.visit_package(node);
                false
            }
            "import_declaration" => {
                self.visit_import(node);
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
            "enum_declaration" => {
                self.visit_enum(node);
                false // visit_enum handles body itself
            }
            "record_declaration" => {
                self.visit_record(node);
                false // visit_record handles body itself
            }
            "method_declaration" | "constructor_declaration" => {
                // Only visit if not in a class context (would be double-counted)
                if self.current_class.is_none() {
                    self.visit_method(node);
                }
                false
            }
            // Call expressions - extract call relationships
            "method_invocation" | "object_creation_expression" => {
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

    fn visit_package(&mut self, node: Node) {
        // package com.example.app;
        // The package name is in a child node
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "scoped_identifier" || child.kind() == "identifier" {
                self.current_package = Some(self.node_text(child));
                break;
            }
        }
    }

    fn visit_import(&mut self, node: Node) {
        // import java.util.List;
        // import java.util.*;
        let mut cursor = node.walk();
        let mut imported = String::new();
        let mut is_wildcard = false;

        for child in node.children(&mut cursor) {
            match child.kind() {
                "scoped_identifier" | "identifier" => {
                    imported = self.node_text(child);
                }
                "asterisk" => {
                    is_wildcard = true;
                }
                _ => {}
            }
        }

        if !imported.is_empty() {
            let import = ImportRelation {
                importer: self
                    .current_package
                    .clone()
                    .unwrap_or_else(|| "default".to_string()),
                imported,
                symbols: Vec::new(),
                is_wildcard,
                alias: None,
            };
            self.imports.push(import);
        }
    }

    fn visit_class(&mut self, node: Node) {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_else(|| "Class".to_string());

        let qualified_name = self.qualify_name(&name);
        let modifiers = self.extract_modifiers(node);
        let is_abstract = modifiers.contains(&"abstract".to_string());
        let visibility = self.extract_visibility(&modifiers);
        let doc_comment = self.extract_doc_comment(node);

        // Extract superclass (extends)
        let mut base_classes = Vec::new();
        if let Some(superclass) = node.child_by_field_name("superclass") {
            let parent_name = self.extract_type_name(superclass);
            if !parent_name.is_empty() {
                base_classes.push(parent_name.clone());
                self.inheritance.push(InheritanceRelation {
                    child: qualified_name.clone(),
                    parent: parent_name,
                    order: 0,
                });
            }
        }

        // Extract implemented interfaces
        let mut implemented_traits = Vec::new();
        if let Some(interfaces) = node.child_by_field_name("interfaces") {
            self.extract_implemented_interfaces(
                interfaces,
                &qualified_name,
                &mut implemented_traits,
            );
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
            visibility,
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
            type_parameters: self.extract_type_parameters(node),
            body_prefix,
        };
        self.classes.push(class_entity);

        // Set current class context for method extraction
        let previous_class = self.current_class.take();
        self.current_class = Some(qualified_name);

        // Visit class body to extract methods
        if let Some(body) = node.child_by_field_name("body") {
            self.visit_class_body(body);
        }

        self.current_class = previous_class;
    }

    fn visit_class_body(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "method_declaration" | "constructor_declaration" => self.visit_method(child),
                "class_declaration" => self.visit_class(child), // Inner class
                "interface_declaration" => self.visit_interface(child), // Inner interface
                "enum_declaration" => self.visit_enum(child),   // Inner enum
                _ => {}
            }
        }
    }

    fn visit_interface(&mut self, node: Node) {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_else(|| "Interface".to_string());

        let qualified_name = self.qualify_name(&name);
        let modifiers = self.extract_modifiers(node);
        let visibility = self.extract_visibility(&modifiers);
        let doc_comment = self.extract_doc_comment(node);

        // Extract parent interfaces (extends)
        let mut parent_traits = Vec::new();
        if let Some(extends_interfaces) = node.child_by_field_name("extends_interfaces") {
            let mut cursor = extends_interfaces.walk();
            for child in extends_interfaces.children(&mut cursor) {
                if child.kind() == "type_identifier" || child.kind() == "scoped_type_identifier" {
                    parent_traits.push(self.node_text(child));
                }
            }
        }

        // Extract required methods
        let required_methods = self.extract_interface_methods(node);

        let interface_entity = TraitEntity {
            name: qualified_name.clone(),
            visibility,
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

    fn visit_enum(&mut self, node: Node) {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_else(|| "Enum".to_string());

        let qualified_name = self.qualify_name(&name);
        let modifiers = self.extract_modifiers(node);
        let visibility = self.extract_visibility(&modifiers);
        let doc_comment = self.extract_doc_comment(node);

        // Enums can implement interfaces
        let mut implemented_traits = Vec::new();
        if let Some(interfaces) = node.child_by_field_name("interfaces") {
            self.extract_implemented_interfaces(
                interfaces,
                &qualified_name,
                &mut implemented_traits,
            );
        }

        let body_prefix = node
            .child_by_field_name("body")
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| {
                truncate_body_prefix(t)
            })
            .map(|t| t.to_string());

        let enum_entity = ClassEntity {
            name: qualified_name.clone(),
            visibility,
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_abstract: false,
            is_interface: false,
            base_classes: Vec::new(),
            implemented_traits,
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
                if child.kind() == "method_declaration" || child.kind() == "constructor_declaration"
                {
                    self.visit_method(child);
                }
            }
        }

        self.current_class = previous_class;
    }

    fn visit_record(&mut self, node: Node) {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_else(|| "Record".to_string());

        let qualified_name = self.qualify_name(&name);
        let modifiers = self.extract_modifiers(node);
        let visibility = self.extract_visibility(&modifiers);
        let doc_comment = self.extract_doc_comment(node);

        // Records can implement interfaces
        let mut implemented_traits = Vec::new();
        if let Some(interfaces) = node.child_by_field_name("interfaces") {
            self.extract_implemented_interfaces(
                interfaces,
                &qualified_name,
                &mut implemented_traits,
            );
        }

        let body_prefix = node
            .child_by_field_name("body")
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| {
                truncate_body_prefix(t)
            })
            .map(|t| t.to_string());

        let record_entity = ClassEntity {
            name: qualified_name.clone(),
            visibility,
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_abstract: false,
            is_interface: false,
            base_classes: Vec::new(),
            implemented_traits,
            methods: Vec::new(),
            fields: Vec::new(),
            doc_comment,
            attributes: vec!["record".to_string()],
            type_parameters: self.extract_type_parameters(node),
            body_prefix,
        };
        self.classes.push(record_entity);

        // Set current class context for method extraction
        let previous_class = self.current_class.take();
        self.current_class = Some(qualified_name);

        // Visit record body to extract methods
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

    fn visit_method(&mut self, node: Node) {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_else(|| {
                // For constructors, the name is the same as the class name
                if node.kind() == "constructor_declaration" {
                    self.current_class
                        .clone()
                        .unwrap_or_else(|| "constructor".to_string())
                        .split('.')
                        .next_back()
                        .unwrap_or("constructor")
                        .to_string()
                } else {
                    "method".to_string()
                }
            });

        let modifiers = self.extract_modifiers(node);
        let visibility = self.extract_visibility(&modifiers);
        let is_static = modifiers.contains(&"static".to_string());
        let is_abstract = modifiers.contains(&"abstract".to_string());
        let return_type = self.extract_return_type(node);
        let parameters = self.extract_parameters(node);
        let doc_comment = self.extract_doc_comment(node);

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
            signature: self.extract_method_signature(node),
            visibility,
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_async: false,
            is_test: self.has_test_annotation(node),
            is_static,
            is_abstract,
            parameters,
            return_type,
            doc_comment,
            attributes: self.extract_annotations(node),
            parent_class: self.current_class.clone(),
            complexity,
            body_prefix,
        };
        self.functions.push(func);

        // Set current function context and visit body to extract calls
        let previous_function = self.current_function.take();
        self.current_function = Some(name);

        if let Some(body) = node.child_by_field_name("body") {
            self.visit_method_body(body);
        }

        self.current_function = previous_function;
    }

    /// Visit a call expression and extract the call relationship
    fn visit_call_expression(&mut self, node: Node) {
        // Only record calls if we're inside a function/method
        let caller = match &self.current_function {
            Some(name) => name.clone(),
            None => return,
        };

        let callee = self.extract_callee_name(node);

        // Skip empty callees
        if callee.is_empty() {
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
            // Method invocation: obj.method(), method(), Class.staticMethod()
            "method_invocation" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let method_name = self.node_text(name_node);

                    // Check if there's an object/class prefix
                    if let Some(object_node) = node.child_by_field_name("object") {
                        let object_text = self.node_text(object_node);
                        // Skip "this" and "super" prefixes, just return method name
                        if object_text == "this" || object_text == "super" {
                            return method_name;
                        }
                        // For other objects, include the object/class name
                        return format!("{}.{}", object_text, method_name);
                    }

                    method_name
                } else {
                    String::new()
                }
            }
            // Object creation: new ClassName()
            "object_creation_expression" => {
                if let Some(type_node) = node.child_by_field_name("type") {
                    format!("new {}", self.node_text(type_node))
                } else {
                    String::new()
                }
            }
            _ => String::new(),
        }
    }

    /// Visit method body to extract calls
    fn visit_method_body(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "method_invocation" | "object_creation_expression" => {
                    self.visit_call_expression(child);
                    // Also recurse into arguments for nested calls
                    self.visit_method_body(child);
                }
                _ => {
                    self.visit_method_body(child);
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
            if child.kind() == "type_identifier"
                || child.kind() == "scoped_type_identifier"
                || child.kind() == "generic_type"
            {
                let interface_name = self.extract_type_name(child);
                if !interface_name.is_empty() {
                    implemented_traits.push(interface_name.clone());
                    self.implementations.push(ImplementationRelation {
                        implementor: class_name.to_string(),
                        trait_name: interface_name,
                    });
                }
            }
            // Also handle type_list which contains multiple interfaces
            if child.kind() == "type_list" {
                self.extract_implemented_interfaces(child, class_name, implemented_traits);
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

                    let modifiers = self.extract_modifiers(child);
                    let visibility = self.extract_visibility(&modifiers);
                    let is_static = modifiers.contains(&"static".to_string());
                    let return_type = self.extract_return_type(child);
                    let parameters = self.extract_parameters(child);

                    let func = FunctionEntity {
                        name,
                        signature: self.extract_method_signature(child),
                        visibility,
                        line_start: child.start_position().row + 1,
                        line_end: child.end_position().row + 1,
                        is_async: false,
                        is_test: false,
                        is_static,
                        is_abstract: true, // Interface methods are abstract
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

    fn extract_modifiers(&self, node: Node) -> Vec<String> {
        let mut modifiers = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "modifiers" {
                let mut mod_cursor = child.walk();
                for modifier in child.children(&mut mod_cursor) {
                    let mod_text = self.node_text(modifier);
                    if !mod_text.is_empty()
                        && !mod_text.starts_with('@')
                        && !mod_text.starts_with('(')
                    {
                        modifiers.push(mod_text);
                    }
                }
            }
        }
        modifiers
    }

    fn extract_visibility(&self, modifiers: &[String]) -> String {
        for modifier in modifiers {
            match modifier.as_str() {
                "public" | "private" | "protected" => return modifier.clone(),
                _ => {}
            }
        }
        "package".to_string() // Default Java visibility
    }

    fn extract_return_type(&self, node: Node) -> Option<String> {
        node.child_by_field_name("type")
            .map(|n| self.node_text(n))
            .filter(|t| t != "void")
    }

    fn extract_parameters(&self, node: Node) -> Vec<Parameter> {
        let mut params = Vec::new();
        if let Some(params_node) = node.child_by_field_name("parameters") {
            let mut cursor = params_node.walk();
            for child in params_node.children(&mut cursor) {
                if child.kind() == "formal_parameter" || child.kind() == "spread_parameter" {
                    let is_variadic = child.kind() == "spread_parameter";

                    // Extract parameter name
                    let name = child
                        .child_by_field_name("name")
                        .map(|n| self.node_text(n))
                        .unwrap_or_else(|| "param".to_string());

                    // Extract type
                    let type_annotation = child.child_by_field_name("type").map(|n| {
                        let mut type_text = self.node_text(n);
                        // Handle array types and varargs
                        if let Some(dims) = child.child_by_field_name("dimensions") {
                            type_text.push_str(&self.node_text(dims));
                        }
                        type_text
                    });

                    let mut param = Parameter::new(name);
                    if let Some(t) = type_annotation {
                        param = param.with_type(t);
                    }
                    if is_variadic {
                        param = param.variadic();
                    }
                    params.push(param);
                }
            }
        }
        params
    }

    fn extract_method_signature(&self, node: Node) -> String {
        self.node_text(node)
            .lines()
            .next()
            .unwrap_or("")
            .to_string()
    }

    fn extract_type_name(&self, node: Node) -> String {
        match node.kind() {
            "type_identifier" | "scoped_type_identifier" => self.node_text(node),
            "generic_type" => {
                // For generic types like List<String>, just get the base type
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "type_identifier" || child.kind() == "scoped_type_identifier"
                    {
                        return self.node_text(child);
                    }
                }
                self.node_text(node)
            }
            "superclass" => {
                // Superclass node contains the actual type
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "type_identifier"
                        || child.kind() == "scoped_type_identifier"
                        || child.kind() == "generic_type"
                    {
                        return self.extract_type_name(child);
                    }
                }
                String::new()
            }
            _ => self.node_text(node),
        }
    }

    fn extract_type_parameters(&self, node: Node) -> Vec<String> {
        let mut type_params = Vec::new();
        if let Some(params_node) = node.child_by_field_name("type_parameters") {
            let mut cursor = params_node.walk();
            for child in params_node.children(&mut cursor) {
                if child.kind() == "type_parameter" {
                    type_params.push(self.node_text(child));
                }
            }
        }
        type_params
    }

    fn extract_doc_comment(&self, node: Node) -> Option<String> {
        // Look for preceding block comment (Javadoc)
        if let Some(prev) = node.prev_sibling() {
            if prev.kind() == "block_comment" {
                let comment = self.node_text(prev);
                if comment.starts_with("/**") {
                    return Some(comment);
                }
            }
        }
        None
    }

    fn extract_annotations(&self, node: Node) -> Vec<String> {
        let mut annotations = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "modifiers" {
                let mut mod_cursor = child.walk();
                for modifier in child.children(&mut mod_cursor) {
                    if modifier.kind() == "marker_annotation" || modifier.kind() == "annotation" {
                        annotations.push(self.node_text(modifier));
                    }
                }
            }
        }
        annotations
    }

    fn has_test_annotation(&self, node: Node) -> bool {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "modifiers" {
                let mut mod_cursor = child.walk();
                for modifier in child.children(&mut mod_cursor) {
                    if modifier.kind() == "marker_annotation" || modifier.kind() == "annotation" {
                        let text = self.node_text(modifier);
                        if text.contains("@Test")
                            || text.contains("@org.junit")
                            || text.contains("@ParameterizedTest")
                        {
                            return true;
                        }
                    }
                }
            }
        }
        false
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
                // Count the else branch: the Java grammar emits a bare `else`
                // keyword token as a direct child of if_statement (no else_clause
                // wrapper node), so we look for it here.
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "else" {
                        builder.add_branch();
                        break;
                    }
                }
            }
            "for_statement" => {
                builder.add_loop();
                builder.enter_scope();
            }
            "enhanced_for_statement" => {
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
            "switch_expression" => {
                builder.enter_scope();
            }
            "switch_label" => {
                // Covers both `case X:` and `default:` labels
                builder.add_branch();
            }
            "ternary_expression" => {
                builder.add_branch();
            }
            "catch_clause" => {
                builder.add_exception_handler();
            }
            "finally_clause" => {
                builder.add_exception_handler();
            }
            "binary_expression" => {
                if let Some(op) = node.child_by_field_name("operator") {
                    let op_text = self.node_text(op);
                    if op_text == "&&" || op_text == "||" {
                        builder.add_logical_operator();
                    }
                }
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_for_complexity(child, builder);
        }

        // Exit scope for constructs that entered one
        match node.kind() {
            "if_statement"
            | "for_statement"
            | "enhanced_for_statement"
            | "while_statement"
            | "do_statement"
            | "switch_expression" => {
                builder.exit_scope();
            }
            _ => {}
        }
    }

    fn qualify_name(&self, name: &str) -> String {
        if let Some(ref pkg) = self.current_package {
            format!("{}.{}", pkg, name)
        } else {
            name.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_visit(source: &[u8]) -> JavaVisitor<'_> {
        use tree_sitter::Parser;

        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_java::LANGUAGE.into()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = JavaVisitor::new(source);
        visitor.visit_node(tree.root_node());
        visitor
    }

    #[test]
    fn test_visitor_basics() {
        let visitor = JavaVisitor::new(b"");
        assert_eq!(visitor.functions.len(), 0);
        assert_eq!(visitor.classes.len(), 0);
        assert_eq!(visitor.traits.len(), 0);
    }

    #[test]
    fn test_visitor_class_extraction() {
        let source = b"public class Person { public String name; }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "Person");
    }

    #[test]
    fn test_visitor_interface_extraction() {
        let source = b"public interface Reader { String read(); }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.traits.len(), 1);
        assert_eq!(visitor.traits[0].name, "Reader");
    }

    #[test]
    fn test_visitor_method_extraction() {
        let source = b"public class Calculator { public int add(int a, int b) { return a + b; } }";
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
    fn test_visitor_import_extraction() {
        let source = b"import java.util.List;\nimport java.util.ArrayList;";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 2);
        assert_eq!(visitor.imports[0].imported, "java.util.List");
        assert_eq!(visitor.imports[1].imported, "java.util.ArrayList");
    }

    #[test]
    fn test_visitor_wildcard_import() {
        let source = b"import java.util.*;";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "java.util");
        assert!(visitor.imports[0].is_wildcard);
    }

    #[test]
    fn test_visitor_inheritance() {
        let source = b"class Animal {}\nclass Dog extends Animal {}";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 2);
        assert_eq!(visitor.inheritance.len(), 1);
        assert_eq!(visitor.inheritance[0].child, "Dog");
        assert_eq!(visitor.inheritance[0].parent, "Animal");
    }

    #[test]
    fn test_visitor_implements() {
        let source = b"interface Shape { double area(); }\nclass Circle implements Shape { public double area() { return 0.0; } }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.traits.len(), 1);
        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.implementations.len(), 1);
        assert_eq!(visitor.implementations[0].implementor, "Circle");
        assert_eq!(visitor.implementations[0].trait_name, "Shape");
    }

    #[test]
    fn test_visitor_enum() {
        let source = b"public enum Status { PENDING, ACTIVE, COMPLETED }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "Status");
        assert!(visitor.classes[0].attributes.contains(&"enum".to_string()));
    }

    #[test]
    fn test_visitor_record() {
        let source = b"public record Person(String name, int age) {}";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "Person");
        assert!(visitor.classes[0]
            .attributes
            .contains(&"record".to_string()));
    }

    #[test]
    fn test_visitor_package() {
        let source = b"package com.example.app;\npublic class App {}";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "com.example.app.App");
    }

    #[test]
    fn test_visitor_abstract_class() {
        let source = b"public abstract class BaseController { public abstract void handle(); }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert!(visitor.classes[0].is_abstract);
    }

    #[test]
    fn test_visitor_static_method() {
        let source = b"public class Helper { public static String format(String s) { return s; } }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert!(visitor.functions[0].is_static);
    }

    #[test]
    fn test_visitor_visibility_modifiers() {
        let source = b"public class Foo { private void bar() {} protected void baz() {} public void qux() {} }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 3);
        assert_eq!(visitor.functions[0].visibility, "private");
        assert_eq!(visitor.functions[1].visibility, "protected");
        assert_eq!(visitor.functions[2].visibility, "public");
    }

    #[test]
    fn test_visitor_method_call_extraction() {
        let source = b"
public class MyClass {
    public void caller() {
        helper();
        process();
    }

    public void helper() {}
    public void process() {}
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
    fn test_visitor_this_method_call() {
        let source = b"
public class MyClass {
    public void caller() {
        this.helper();
    }

    public void helper() {}
}
";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.calls.len(), 1);
        assert_eq!(visitor.calls[0].caller, "caller");
        assert_eq!(visitor.calls[0].callee, "helper");
    }

    #[test]
    fn test_visitor_static_call_extraction() {
        let source = b"
public class Calculator {
    public void calculate() {
        Math.abs(-1);
        Helper.format();
    }
}
";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.calls.len(), 2);
        assert!(visitor
            .calls
            .iter()
            .any(|c| c.caller == "calculate" && c.callee == "Math.abs"));
        assert!(visitor
            .calls
            .iter()
            .any(|c| c.caller == "calculate" && c.callee == "Helper.format"));
    }

    #[test]
    fn test_visitor_constructor_call() {
        let source = b"
public class Factory {
    public void create() {
        new ArrayList();
        new HashMap();
    }
}
";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.calls.len(), 2);
        assert!(visitor
            .calls
            .iter()
            .any(|c| c.caller == "create" && c.callee == "new ArrayList"));
        assert!(visitor
            .calls
            .iter()
            .any(|c| c.caller == "create" && c.callee == "new HashMap"));
    }

    #[test]
    fn test_visitor_test_annotation() {
        let source = b"
import org.junit.Test;
public class MyTest {
    @Test
    public void testSomething() {}
}
";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert!(visitor.functions[0].is_test);
    }

    #[test]
    fn test_visitor_generic_class() {
        let source = b"public class Container<T> { private T value; }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert!(!visitor.classes[0].type_parameters.is_empty());
    }

    #[test]
    fn test_visitor_call_line_numbers() {
        let source = b"
public class Test {
    void caller() {
        helper();
    }
    void helper() {}
}
";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.calls.len(), 1);
        assert_eq!(visitor.calls[0].caller, "caller");
        assert_eq!(visitor.calls[0].callee, "helper");
        assert_eq!(visitor.calls[0].call_site_line, 4);
        assert!(visitor.calls[0].is_direct);
    }

    #[test]
    fn test_visitor_complexity_simple_method() {
        // A method with no branches has CC=1
        let source = b"
public class Calc {
    public int add(int a, int b) { return a + b; }
}
";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert_eq!(complexity.cyclomatic_complexity, 1);
        assert_eq!(complexity.branches, 0);
        assert_eq!(complexity.loops, 0);
    }

    #[test]
    fn test_visitor_complexity_if_else_and_loop() {
        // if + else = 2 branches, for = 1 loop  =>  CC = 1 + 2 + 1 = 4
        let source = b"
public class Checker {
    public String classify(int[] nums) {
        String result = \"\";
        for (int n : nums) {
            if (n > 0) {
                result = \"positive\";
            } else {
                result = \"non-positive\";
            }
        }
        return result;
    }
}
";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(
            complexity.branches >= 2,
            "expected >= 2 branches, got {}",
            complexity.branches
        );
        assert!(
            complexity.loops >= 1,
            "expected >= 1 loop, got {}",
            complexity.loops
        );
        assert!(complexity.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_visitor_complexity_try_catch_finally() {
        // catch + finally each add an exception handler  =>  CC = 1 + 2 = 3
        let source = b"
public class Resource {
    public void load(String path) {
        try {
            System.out.println(path);
        } catch (Exception e) {
            e.printStackTrace();
        } finally {
            System.out.println(\"done\");
        }
    }
}
";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(
            complexity.exception_handlers >= 2,
            "expected >= 2 exception handlers (catch + finally), got {}",
            complexity.exception_handlers
        );
        assert!(complexity.cyclomatic_complexity >= 3);
    }
}
