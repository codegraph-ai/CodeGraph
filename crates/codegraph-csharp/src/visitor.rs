// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting C# entities

use codegraph_parser_api::{
    CallRelation, ClassEntity, ComplexityBuilder, ComplexityMetrics, FunctionEntity,
    ImplementationRelation, ImportRelation, InheritanceRelation, Parameter, TraitEntity,
    BODY_PREFIX_MAX_CHARS,
    truncate_body_prefix,
};
use tree_sitter::Node;

pub struct CSharpVisitor<'a> {
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

impl<'a> CSharpVisitor<'a> {
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
        let should_recurse = match node.kind() {
            "using_directive" => {
                self.visit_using(node);
                false
            }
            "namespace_declaration" => {
                self.visit_namespace(node);
                false // visit_namespace handles body itself
            }
            "file_scoped_namespace_declaration" => {
                self.visit_file_scoped_namespace(node);
                true // Continue to visit declarations after namespace
            }
            "class_declaration" => {
                self.visit_class(node);
                false // visit_class handles body itself
            }
            "interface_declaration" => {
                self.visit_interface(node);
                false // visit_interface handles body itself
            }
            "struct_declaration" => {
                self.visit_struct(node);
                false // visit_struct handles body itself
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
            "invocation_expression" | "object_creation_expression" => {
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

    fn visit_using(&mut self, node: Node) {
        // using System;
        // using System.Collections.Generic;
        let mut cursor = node.walk();
        let mut imported = String::new();

        for child in node.children(&mut cursor) {
            match child.kind() {
                "qualified_name" | "identifier" => {
                    imported = self.node_text(child);
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
                alias: None,
            };
            self.imports.push(import);
        }
    }

    fn visit_namespace(&mut self, node: Node) {
        // namespace MyApp.Models { ... }
        if let Some(name_node) = node.child_by_field_name("name") {
            self.current_namespace = Some(self.node_text(name_node));
        }

        // Visit namespace body
        if let Some(body) = node.child_by_field_name("body") {
            self.visit_namespace_body(body);
        }
    }

    fn visit_file_scoped_namespace(&mut self, node: Node) {
        // namespace MyApp.Models;
        if let Some(name_node) = node.child_by_field_name("name") {
            self.current_namespace = Some(self.node_text(name_node));
        }
        // File-scoped namespaces continue with rest of file, handled by visit_node recursion
    }

    fn visit_namespace_body(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "class_declaration" => self.visit_class(child),
                "interface_declaration" => self.visit_interface(child),
                "struct_declaration" => self.visit_struct(child),
                "enum_declaration" => self.visit_enum(child),
                "record_declaration" => self.visit_record(child),
                "namespace_declaration" => self.visit_namespace(child), // Nested namespace
                _ => {}
            }
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

        // Extract base classes and implemented interfaces
        let mut base_classes = Vec::new();
        let mut implemented_traits = Vec::new();
        self.extract_base_list(
            node,
            &qualified_name,
            &mut base_classes,
            &mut implemented_traits,
        );

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
            attributes: self.extract_attributes(node),
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
                "property_declaration" => self.visit_property(child),
                "class_declaration" => self.visit_class(child), // Nested class
                "interface_declaration" => self.visit_interface(child), // Nested interface
                "struct_declaration" => self.visit_struct(child), // Nested struct
                "enum_declaration" => self.visit_enum(child),   // Nested enum
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

        // Extract parent interfaces
        let mut parent_traits = Vec::new();
        let mut dummy_base_classes = Vec::new();
        self.extract_base_list(
            node,
            &qualified_name,
            &mut dummy_base_classes,
            &mut parent_traits,
        );

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
            attributes: self.extract_attributes(node),
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

    fn visit_struct(&mut self, node: Node) {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_else(|| "Struct".to_string());

        let qualified_name = self.qualify_name(&name);
        let modifiers = self.extract_modifiers(node);
        let visibility = self.extract_visibility(&modifiers);
        let doc_comment = self.extract_doc_comment(node);

        // Structs can implement interfaces
        let mut implemented_traits = Vec::new();
        let mut dummy_base = Vec::new();
        self.extract_base_list(
            node,
            &qualified_name,
            &mut dummy_base,
            &mut implemented_traits,
        );

        let body_prefix = node
            .child_by_field_name("body")
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| {
                truncate_body_prefix(t)
            })
            .map(|t| t.to_string());

        let struct_entity = ClassEntity {
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
            attributes: vec!["struct".to_string()],
            type_parameters: self.extract_type_parameters(node),
            body_prefix,
        };

        self.classes.push(struct_entity);

        // Set current class context for method extraction
        let previous_class = self.current_class.take();
        self.current_class = Some(qualified_name);

        // Visit struct body to extract methods
        if let Some(body) = node.child_by_field_name("body") {
            self.visit_class_body(body);
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

        let body_prefix = node
            .child_by_field_name("body")
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| {
                truncate_body_prefix(t)
            })
            .map(|t| t.to_string());

        let enum_entity = ClassEntity {
            name: qualified_name,
            visibility,
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

        // Records can implement interfaces and inherit from other records
        let mut base_classes = Vec::new();
        let mut implemented_traits = Vec::new();
        self.extract_base_list(
            node,
            &qualified_name,
            &mut base_classes,
            &mut implemented_traits,
        );

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
            base_classes,
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

        // Visit record body to extract methods if present
        if let Some(body) = node.child_by_field_name("body") {
            self.visit_class_body(body);
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
        let is_async = modifiers.contains(&"async".to_string());
        let return_type = self.extract_return_type(node);
        let parameters = self.extract_parameters(node);
        let doc_comment = self.extract_doc_comment(node);

        // Calculate complexity from method body (abstract/interface methods have no body)
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
            is_async,
            is_test: self.has_test_attribute(node),
            is_static,
            is_abstract,
            parameters,
            return_type,
            doc_comment,
            attributes: self.extract_attributes(node),
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

    fn visit_property(&mut self, node: Node) {
        // Properties with accessors can have code in getters/setters
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "accessor_list" {
                self.visit_accessor_list(child);
            }
        }
    }

    fn visit_accessor_list(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "accessor_declaration" {
                if let Some(body) = child.child_by_field_name("body") {
                    self.visit_method_body(body);
                }
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
            // Method invocation: obj.Method(), Method(), Class.StaticMethod()
            "invocation_expression" => {
                if let Some(func_node) = node.child_by_field_name("function") {
                    let text = self.node_text(func_node);
                    // Handle this. and base. prefixes
                    if let Some(stripped) = text.strip_prefix("this.") {
                        return stripped.to_string();
                    }
                    if let Some(stripped) = text.strip_prefix("base.") {
                        return stripped.to_string();
                    }
                    text
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
                "invocation_expression" | "object_creation_expression" => {
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

    fn extract_base_list(
        &mut self,
        node: Node,
        class_name: &str,
        base_classes: &mut Vec<String>,
        implemented_traits: &mut Vec<String>,
    ) {
        // In C#, base_list contains both base class and interfaces
        // class Dog : Animal, IWalkable { }
        // The first can be a class, the rest are usually interfaces

        // Find base_list child node
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "base_list" {
                let mut base_cursor = child.walk();
                let mut first = true;

                for base_child in child.children(&mut base_cursor) {
                    // Skip syntax tokens
                    if base_child.kind() == ":" || base_child.kind() == "," {
                        continue;
                    }

                    let type_name = self.extract_type_name(base_child);
                    if type_name.is_empty() {
                        continue;
                    }

                    // Check if it starts with 'I' (convention for interfaces in C#)
                    let is_interface = type_name.starts_with('I')
                        && type_name.len() > 1
                        && type_name.chars().nth(1).is_some_and(|c| c.is_uppercase());

                    if is_interface {
                        implemented_traits.push(type_name.clone());
                        self.implementations.push(ImplementationRelation {
                            implementor: class_name.to_string(),
                            trait_name: type_name,
                        });
                    } else if first {
                        // First non-interface is the base class
                        base_classes.push(type_name.clone());
                        self.inheritance.push(InheritanceRelation {
                            child: class_name.to_string(),
                            parent: type_name,
                            order: 0,
                        });
                        first = false;
                    } else {
                        // Subsequent non-interface names are still interfaces
                        implemented_traits.push(type_name.clone());
                        self.implementations.push(ImplementationRelation {
                            implementor: class_name.to_string(),
                            trait_name: type_name,
                        });
                    }
                }
                break;
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
            if child.kind() == "modifier" {
                modifiers.push(self.node_text(child));
            }
        }
        modifiers
    }

    fn extract_visibility(&self, modifiers: &[String]) -> String {
        for modifier in modifiers {
            match modifier.as_str() {
                "public" | "private" | "protected" | "internal" => return modifier.clone(),
                "protected internal" | "private protected" => return modifier.clone(),
                _ => {}
            }
        }
        "internal".to_string() // Default C# visibility
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
                if child.kind() == "parameter" {
                    // Extract parameter name
                    let name = child
                        .child_by_field_name("name")
                        .map(|n| self.node_text(n))
                        .unwrap_or_else(|| "param".to_string());

                    // Extract type
                    let type_annotation =
                        child.child_by_field_name("type").map(|n| self.node_text(n));

                    // Check for params keyword (variadic)
                    let is_variadic = self
                        .extract_modifiers(child)
                        .contains(&"params".to_string());

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
            "identifier" | "qualified_name" => self.node_text(node),
            "generic_name" => {
                // For generic types like List<string>, get the base name
                if let Some(name_node) = node.child_by_field_name("name") {
                    self.node_text(name_node)
                } else {
                    // Try first identifier child
                    let mut cursor = node.walk();
                    for child in node.children(&mut cursor) {
                        if child.kind() == "identifier" {
                            return self.node_text(child);
                        }
                    }
                    self.node_text(node)
                }
            }
            "simple_base_type" | "base_type" => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    let type_name = self.extract_type_name(child);
                    if !type_name.is_empty() {
                        return type_name;
                    }
                }
                String::new()
            }
            _ => self.node_text(node),
        }
    }

    fn extract_type_parameters(&self, node: Node) -> Vec<String> {
        let mut type_params = Vec::new();
        // Find type_parameter_list child node
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "type_parameter_list" {
                let mut param_cursor = child.walk();
                for param_child in child.children(&mut param_cursor) {
                    if param_child.kind() == "type_parameter" {
                        // Extract the identifier from type_parameter
                        if let Some(id) = param_child.child_by_field_name("name") {
                            type_params.push(self.node_text(id));
                        } else {
                            // Try to get the text directly
                            let mut id_cursor = param_child.walk();
                            for id_child in param_child.children(&mut id_cursor) {
                                if id_child.kind() == "identifier" {
                                    type_params.push(self.node_text(id_child));
                                    break;
                                }
                            }
                        }
                    }
                }
                break;
            }
        }
        type_params
    }

    fn extract_doc_comment(&self, node: Node) -> Option<String> {
        // Look for preceding XML documentation comment (///)
        if let Some(prev) = node.prev_sibling() {
            if prev.kind() == "comment" {
                let comment = self.node_text(prev);
                if comment.starts_with("///") {
                    return Some(comment);
                }
            }
        }
        None
    }

    fn extract_attributes(&self, node: Node) -> Vec<String> {
        let mut attributes = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "attribute_list" {
                let mut attr_cursor = child.walk();
                for attr in child.children(&mut attr_cursor) {
                    if attr.kind() == "attribute" {
                        attributes.push(self.node_text(attr));
                    }
                }
            }
        }
        attributes
    }

    fn has_test_attribute(&self, node: Node) -> bool {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "attribute_list" {
                let text = self.node_text(child);
                if text.contains("Test")
                    || text.contains("Fact")
                    || text.contains("Theory")
                    || text.contains("TestMethod")
                {
                    return true;
                }
            }
        }
        false
    }

    fn qualify_name(&self, name: &str) -> String {
        if let Some(ref ns) = self.current_namespace {
            format!("{}.{}", ns, name)
        } else {
            name.to_string()
        }
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
                // Check if there's an else branch: if_statement has two block children
                // when an else clause is present (the anonymous `else` token + second block).
                // We detect it by checking for the `else` keyword child.
                let has_else = {
                    let mut cursor = node.walk();
                    let x = node.children(&mut cursor).any(|c| c.kind() == "else");
                    x
                };
                if has_else {
                    builder.add_branch();
                }
            }
            // Each switch_section represents one case or default branch
            "switch_section" => {
                builder.add_branch();
                builder.enter_scope();
            }
            "conditional_expression" => {
                // Ternary operator ?:
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
                // Check for && and ||
                if let Some(op) = node.child_by_field_name("operator") {
                    let op_text = self.node_text(op);
                    if op_text == "&&" || op_text == "||" {
                        builder.add_logical_operator();
                    }
                }
            }
            "catch_clause" => {
                builder.add_exception_handler();
            }
            "finally_clause" => {
                builder.add_exception_handler();
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_for_complexity(child, builder);
        }

        // Exit scope for control structures that entered one
        match node.kind() {
            "if_statement" | "for_statement" | "while_statement" | "do_statement"
            | "foreach_statement" | "switch_section" => {
                builder.exit_scope();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_visit(source: &[u8]) -> CSharpVisitor<'_> {
        use tree_sitter::Parser;

        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_c_sharp::language())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = CSharpVisitor::new(source);
        visitor.visit_node(tree.root_node());
        visitor
    }

    #[test]
    fn test_visitor_basics() {
        let visitor = CSharpVisitor::new(b"");
        assert_eq!(visitor.functions.len(), 0);
        assert_eq!(visitor.classes.len(), 0);
        assert_eq!(visitor.traits.len(), 0);
    }

    #[test]
    fn test_visitor_class_extraction() {
        let source = b"public class Person { public string Name; }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "Person");
    }

    #[test]
    fn test_visitor_interface_extraction() {
        let source = b"public interface IReader { string Read(); }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.traits.len(), 1);
        assert_eq!(visitor.traits[0].name, "IReader");
    }

    #[test]
    fn test_visitor_method_extraction() {
        let source = b"public class Calculator { public int Add(int a, int b) { return a + b; } }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "Add");
        assert_eq!(
            visitor.functions[0].parent_class,
            Some("Calculator".to_string())
        );
    }

    #[test]
    fn test_visitor_using_extraction() {
        let source = b"using System;\nusing System.Collections.Generic;";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 2);
        assert_eq!(visitor.imports[0].imported, "System");
        assert_eq!(visitor.imports[1].imported, "System.Collections.Generic");
    }

    #[test]
    fn test_debug_tree_structure() {
        use tree_sitter::Parser;

        fn print_tree(source: &[u8], node: tree_sitter::Node, indent: usize) {
            let indent_str = "  ".repeat(indent);
            let text = node
                .utf8_text(source)
                .unwrap_or("")
                .lines()
                .next()
                .unwrap_or("");
            let text_display = if text.len() > 50 { &text[..50] } else { text };
            println!("{}{}: {:?}", indent_str, node.kind(), text_display);

            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                print_tree(source, child, indent + 1);
            }
        }

        println!("\n=== inheritance ===");
        let source = b"class Dog : Animal {}";
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_c_sharp::language())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();
        print_tree(source, tree.root_node(), 0);

        println!("\n=== generic type ===");
        let source2 = b"public class Container<T> { private T value; }";
        let tree2 = parser.parse(source2, None).unwrap();
        print_tree(source2, tree2.root_node(), 0);

        println!("\n=== if/else ===");
        let source3 = b"class C { void M() { if (x > 0) { } else { } } }";
        let tree3 = parser.parse(source3, None).unwrap();
        print_tree(source3, tree3.root_node(), 0);

        println!("\n=== switch ===");
        let source4 = b"class C { void M() { switch (n) { case 1: break; default: break; } } }";
        let tree4 = parser.parse(source4, None).unwrap();
        print_tree(source4, tree4.root_node(), 0);

        println!("\n=== foreach ===");
        let source5 = b"class C { void M() { foreach (var x in items) { } } }";
        let tree5 = parser.parse(source5, None).unwrap();
        print_tree(source5, tree5.root_node(), 0);
    }

    #[test]
    fn test_visitor_inheritance() {
        let source = b"class Animal {}\nclass Dog : Animal {}";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 2);
        assert_eq!(visitor.inheritance.len(), 1);
        assert_eq!(visitor.inheritance[0].child, "Dog");
        assert_eq!(visitor.inheritance[0].parent, "Animal");
    }

    #[test]
    fn test_visitor_implements() {
        let source = b"interface IShape { double Area(); }\nclass Circle : IShape { public double Area() { return 0.0; } }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.traits.len(), 1);
        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.implementations.len(), 1);
        assert_eq!(visitor.implementations[0].implementor, "Circle");
        assert_eq!(visitor.implementations[0].trait_name, "IShape");
    }

    #[test]
    fn test_visitor_enum() {
        let source = b"public enum Status { Pending, Active, Completed }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "Status");
        assert!(visitor.classes[0].attributes.contains(&"enum".to_string()));
    }

    #[test]
    fn test_visitor_struct() {
        let source = b"public struct Point { public int X; public int Y; }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "Point");
        assert!(visitor.classes[0]
            .attributes
            .contains(&"struct".to_string()));
    }

    #[test]
    fn test_visitor_record() {
        let source = b"public record Person(string Name, int Age);";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "Person");
        assert!(visitor.classes[0]
            .attributes
            .contains(&"record".to_string()));
    }

    #[test]
    fn test_visitor_namespace() {
        let source = b"namespace MyApp.Models { public class User {} }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "MyApp.Models.User");
    }

    #[test]
    fn test_visitor_abstract_class() {
        let source = b"public abstract class BaseController { public abstract void Handle(); }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert!(visitor.classes[0].is_abstract);
    }

    #[test]
    fn test_visitor_static_method() {
        let source = b"public class Helper { public static string Format(string s) { return s; } }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert!(visitor.functions[0].is_static);
    }

    #[test]
    fn test_visitor_async_method() {
        let source = b"public class Service { public async Task DoWork() { } }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert!(visitor.functions[0].is_async);
    }

    #[test]
    fn test_visitor_visibility_modifiers() {
        let source = b"public class Foo { private void Bar() {} protected void Baz() {} public void Qux() {} }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 3);
        assert_eq!(visitor.functions[0].visibility, "private");
        assert_eq!(visitor.functions[1].visibility, "protected");
        assert_eq!(visitor.functions[2].visibility, "public");
    }

    #[test]
    fn test_visitor_method_call_extraction() {
        let source = b"
public class MyClass
{
    public void Caller()
    {
        Helper();
        Process();
    }

    public void Helper() {}
    public void Process() {}
}
";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.functions.len(), 3);
        assert_eq!(visitor.calls.len(), 2);

        // Check calls: Caller -> Helper, Caller -> Process
        assert!(visitor
            .calls
            .iter()
            .any(|c| c.caller == "Caller" && c.callee == "Helper"));
        assert!(visitor
            .calls
            .iter()
            .any(|c| c.caller == "Caller" && c.callee == "Process"));
    }

    #[test]
    fn test_visitor_this_method_call() {
        let source = b"
public class MyClass
{
    public void Caller()
    {
        this.Helper();
    }

    public void Helper() {}
}
";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.calls.len(), 1);
        assert_eq!(visitor.calls[0].caller, "Caller");
        assert_eq!(visitor.calls[0].callee, "Helper");
    }

    #[test]
    fn test_visitor_static_call_extraction() {
        let source = b"
public class Calculator
{
    public void Calculate()
    {
        Math.Abs(-1);
        Helper.Format();
    }
}
";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.calls.len(), 2);
        assert!(visitor
            .calls
            .iter()
            .any(|c| c.caller == "Calculate" && c.callee == "Math.Abs"));
        assert!(visitor
            .calls
            .iter()
            .any(|c| c.caller == "Calculate" && c.callee == "Helper.Format"));
    }

    #[test]
    fn test_visitor_constructor_call() {
        let source = b"
public class Factory
{
    public void Create()
    {
        new List<int>();
        new Dictionary<string, int>();
    }
}
";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.calls.len(), 2);
        assert!(visitor
            .calls
            .iter()
            .any(|c| c.caller == "Create" && c.callee.starts_with("new ")));
    }

    #[test]
    fn test_visitor_test_attribute() {
        let source = b"
using NUnit.Framework;
public class MyTest
{
    [Test]
    public void TestSomething() {}
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
public class Test
{
    void Caller()
    {
        Helper();
    }
    void Helper() {}
}
";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.calls.len(), 1);
        assert_eq!(visitor.calls[0].caller, "Caller");
        assert_eq!(visitor.calls[0].callee, "Helper");
        assert_eq!(visitor.calls[0].call_site_line, 6);
        assert!(visitor.calls[0].is_direct);
    }

    #[test]
    fn test_complexity_simple_function() {
        // A function with no branches has CC=1
        let source = b"
public class MyClass
{
    public int SimpleAdd(int a, int b)
    {
        return a + b;
    }
}
";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert_eq!(complexity.cyclomatic_complexity, 1);
        assert_eq!(complexity.branches, 0);
        assert_eq!(complexity.loops, 0);
        assert_eq!(complexity.logical_operators, 0);
    }

    #[test]
    fn test_complexity_if_else_and_loop() {
        // if/else adds 2 branches, foreach adds 1 loop: CC = 1 + 2 + 1 = 4
        let source = b"
public class MyClass
{
    public string Classify(int[] items)
    {
        foreach (var item in items)
        {
            if (item > 0)
            {
                return \"positive\";
            }
            else
            {
                return \"non-positive\";
            }
        }
        return \"empty\";
    }
}
";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        // branches: if_statement + else_clause = 2, loops: for_each_statement = 1
        assert!(
            complexity.branches >= 2,
            "expected branches >= 2, got {}",
            complexity.branches
        );
        assert!(
            complexity.loops >= 1,
            "expected loops >= 1, got {}",
            complexity.loops
        );
        assert!(
            complexity.cyclomatic_complexity > 1,
            "expected CC > 1, got {}",
            complexity.cyclomatic_complexity
        );
        assert!(
            complexity.max_nesting_depth >= 2,
            "expected nesting >= 2, got {}",
            complexity.max_nesting_depth
        );
    }

    #[test]
    fn test_complexity_try_catch_finally() {
        // try/catch/finally: catch adds 1, finally adds 1 exception handler: CC = 1 + 2 = 3
        let source = b"
public class MyClass
{
    public void LoadData(string path)
    {
        try
        {
            var data = ReadFile(path);
            Process(data);
        }
        catch (Exception ex)
        {
            Log(ex.Message);
        }
        finally
        {
            Cleanup();
        }
    }
}
";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(
            complexity.exception_handlers >= 2,
            "expected exception_handlers >= 2, got {}",
            complexity.exception_handlers
        );
        assert!(
            complexity.cyclomatic_complexity >= 3,
            "expected CC >= 3, got {}",
            complexity.cyclomatic_complexity
        );
    }

    #[test]
    fn test_complexity_logical_operators() {
        // Two logical operators (&&, ||): CC = 1 + 1 (if) + 2 (logical) = 4
        let source = b"
public class MyClass
{
    public bool IsValid(int x, int y, int z)
    {
        if (x > 0 && y > 0 || z > 0)
        {
            return true;
        }
        return false;
    }
}
";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(
            complexity.logical_operators >= 2,
            "expected logical_operators >= 2, got {}",
            complexity.logical_operators
        );
    }

    #[test]
    fn test_complexity_switch() {
        // switch with 3 cases (including default): CC = 1 + 3 = 4
        let source = b"
public class MyClass
{
    public string Describe(int n)
    {
        switch (n)
        {
            case 1:
                return \"one\";
            case 2:
                return \"two\";
            default:
                return \"other\";
        }
    }
}
";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(
            complexity.branches >= 3,
            "expected branches >= 3 (case + case + default), got {}",
            complexity.branches
        );
    }

    #[test]
    fn test_complexity_abstract_method_has_none() {
        // Abstract methods have no body, so complexity should be None
        let source = b"
public abstract class MyClass
{
    public abstract void DoWork();
}
";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert!(
            visitor.functions[0].complexity.is_none(),
            "abstract methods should have no complexity"
        );
    }
}
