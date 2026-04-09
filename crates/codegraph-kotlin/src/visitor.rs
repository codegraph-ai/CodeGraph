// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting Kotlin entities

use codegraph_parser_api::{
    CallRelation, ClassEntity, ComplexityBuilder, ComplexityMetrics, FunctionEntity,
    ImplementationRelation, ImportRelation, InheritanceRelation, Parameter, TraitEntity,
    BODY_PREFIX_MAX_CHARS,
    truncate_body_prefix,
};
use tree_sitter::Node;

pub struct KotlinVisitor<'a> {
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

impl<'a> KotlinVisitor<'a> {
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
            "package_header" => {
                self.visit_package(node);
                false
            }
            "import_header" => {
                self.visit_import(node);
                false
            }
            "class_declaration" => {
                // Check if this is an interface, enum, or regular class
                let is_interface = self.is_interface_declaration(node);
                let is_enum = self.is_enum_declaration(node);

                if is_interface {
                    self.visit_interface(node);
                } else if is_enum {
                    self.visit_enum_class(node);
                } else {
                    self.visit_class(node);
                }
                false // visitor handles body itself
            }
            "object_declaration" => {
                self.visit_object(node);
                false // visit_object handles body itself
            }
            "function_declaration" => {
                // Only visit if not in a class context (would be double-counted)
                if self.current_class.is_none() {
                    self.visit_function(node);
                }
                false
            }
            // Call expressions - extract call relationships
            "call_expression" | "navigation_expression" => {
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

    fn is_interface_declaration(&self, node: Node) -> bool {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "interface" {
                return true;
            }
        }
        false
    }

    fn is_enum_declaration(&self, node: Node) -> bool {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "enum" {
                return true;
            }
        }
        false
    }

    fn visit_package(&mut self, node: Node) {
        // package com.example.app
        if let Some(identifier) = node.child_by_field_name("identifier") {
            self.current_package = Some(self.node_text(identifier));
        } else {
            // Try to find identifier in children
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "identifier" {
                    self.current_package = Some(self.node_text(child));
                    break;
                }
            }
        }
    }

    fn visit_import(&mut self, node: Node) {
        // import java.util.List
        // import java.util.*
        let mut cursor = node.walk();
        let mut imported = String::new();
        let mut is_wildcard = false;
        let mut alias = None;

        for child in node.children(&mut cursor) {
            match child.kind() {
                "identifier" => {
                    imported = self.node_text(child);
                }
                "import_alias" => {
                    // import X as Y
                    let mut alias_cursor = child.walk();
                    for alias_child in child.children(&mut alias_cursor) {
                        if alias_child.kind() == "type_identifier"
                            || alias_child.kind() == "identifier"
                        {
                            alias = Some(self.node_text(alias_child));
                        }
                    }
                }
                "*" => {
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
                alias,
            };
            self.imports.push(import);
        }
    }

    fn visit_class(&mut self, node: Node) {
        // Get class name
        let name = self
            .find_class_name(node)
            .unwrap_or_else(|| "Class".to_string());

        let qualified_name = self.qualify_name(&name);
        let modifiers = self.extract_modifiers(node);
        let is_abstract = modifiers.contains(&"abstract".to_string());
        let visibility = self.extract_visibility(&modifiers);
        let doc_comment = self.extract_doc_comment(node);

        // Check for data class, enum class, sealed class
        let mut attributes = Vec::new();
        if modifiers.contains(&"data".to_string()) {
            attributes.push("data".to_string());
        }
        if modifiers.contains(&"enum".to_string()) {
            attributes.push("enum".to_string());
        }
        if modifiers.contains(&"sealed".to_string()) {
            attributes.push("sealed".to_string());
        }

        // Extract superclass and implemented interfaces from delegation_specifiers
        let mut base_classes = Vec::new();
        let mut implemented_traits = Vec::new();
        self.extract_supertypes(
            node,
            &qualified_name,
            &mut base_classes,
            &mut implemented_traits,
        );

        // Kotlin uses "class_body" as a child node, not a field
        let class_body_node = {
            let mut cursor = node.walk();
            let found = node
                .children(&mut cursor)
                .find(|c| c.kind() == "class_body");
            found
        };
        let body_prefix = class_body_node
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
            attributes,
            type_parameters: self.extract_type_parameters(node),
            body_prefix,
        };

        self.classes.push(class_entity);

        // Set current class context for method extraction
        let previous_class = self.current_class.take();
        self.current_class = Some(qualified_name);

        // Visit class body to extract methods (find class_body as child)
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "class_body" {
                self.visit_class_body(child);
            }
        }

        self.current_class = previous_class;
    }

    fn visit_object(&mut self, node: Node) {
        // object Singleton { ... }
        let name = self
            .find_class_name(node)
            .unwrap_or_else(|| "Object".to_string());

        let qualified_name = self.qualify_name(&name);
        let modifiers = self.extract_modifiers(node);
        let visibility = self.extract_visibility(&modifiers);
        let doc_comment = self.extract_doc_comment(node);

        // Extract implemented interfaces
        let mut implemented_traits = Vec::new();
        let mut base_classes = Vec::new();
        self.extract_supertypes(
            node,
            &qualified_name,
            &mut base_classes,
            &mut implemented_traits,
        );

        let class_body_node = {
            let mut cursor = node.walk();
            let found = node
                .children(&mut cursor)
                .find(|c| c.kind() == "class_body");
            found
        };
        let body_prefix = class_body_node
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| {
                truncate_body_prefix(t)
            })
            .map(|t| t.to_string());

        let object_entity = ClassEntity {
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
            attributes: vec!["object".to_string()],
            type_parameters: Vec::new(),
            body_prefix,
        };

        self.classes.push(object_entity);

        // Set current class context for method extraction
        let previous_class = self.current_class.take();
        self.current_class = Some(qualified_name);

        // Visit object body to extract methods
        if let Some(body) = node.child_by_field_name("class_body") {
            self.visit_class_body(body);
        }

        self.current_class = previous_class;
    }

    fn visit_interface(&mut self, node: Node) {
        let name = self
            .find_class_name(node)
            .unwrap_or_else(|| "Interface".to_string());

        let qualified_name = self.qualify_name(&name);
        let modifiers = self.extract_modifiers(node);
        let visibility = self.extract_visibility(&modifiers);
        let doc_comment = self.extract_doc_comment(node);

        // Extract parent interfaces
        let mut parent_traits = Vec::new();
        let mut base_classes = Vec::new();
        self.extract_supertypes(node, &qualified_name, &mut base_classes, &mut parent_traits);

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

        // Set current class context for method extraction
        let previous_class = self.current_class.take();
        self.current_class = Some(qualified_name);

        // Visit interface body
        if let Some(body) = node.child_by_field_name("class_body") {
            self.visit_class_body(body);
        }

        self.current_class = previous_class;
    }

    fn visit_enum_class(&mut self, node: Node) {
        let name = self
            .find_class_name(node)
            .unwrap_or_else(|| "Enum".to_string());

        let qualified_name = self.qualify_name(&name);
        let modifiers = self.extract_modifiers(node);
        let visibility = self.extract_visibility(&modifiers);
        let doc_comment = self.extract_doc_comment(node);

        // Extract implemented interfaces
        let mut implemented_traits = Vec::new();
        let mut base_classes = Vec::new();
        self.extract_supertypes(
            node,
            &qualified_name,
            &mut base_classes,
            &mut implemented_traits,
        );

        let class_body_node = {
            let mut cursor = node.walk();
            let found = node
                .children(&mut cursor)
                .find(|c| c.kind() == "enum_class_body");
            found
        };
        let body_prefix = class_body_node
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
            base_classes,
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

        // Visit enum body to extract methods (enum_class_body)
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "enum_class_body" {
                self.visit_enum_body(child);
            }
        }

        self.current_class = previous_class;
    }

    fn visit_enum_body(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "function_declaration" => self.visit_function(child),
                "class_declaration" => {
                    // Handle inner classes/interfaces/enums
                    let is_interface = self.is_interface_declaration(child);
                    let is_enum = self.is_enum_declaration(child);
                    if is_interface {
                        self.visit_interface(child);
                    } else if is_enum {
                        self.visit_enum_class(child);
                    } else {
                        self.visit_class(child);
                    }
                }
                _ => {}
            }
        }
    }

    fn visit_class_body(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "function_declaration" => self.visit_function(child),
                "class_declaration" => {
                    // Handle inner classes/interfaces/enums
                    let is_interface = self.is_interface_declaration(child);
                    let is_enum = self.is_enum_declaration(child);
                    if is_interface {
                        self.visit_interface(child);
                    } else if is_enum {
                        self.visit_enum_class(child);
                    } else {
                        self.visit_class(child);
                    }
                }
                "object_declaration" => self.visit_object(child), // Inner object/companion
                "companion_object" => self.visit_companion_object(child),
                _ => {}
            }
        }
    }

    fn visit_companion_object(&mut self, node: Node) {
        // Visit the body of companion object for static-like methods
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "class_body" {
                self.visit_class_body(child);
            }
        }
    }

    fn visit_function(&mut self, node: Node) {
        let name = node
            .child_by_field_name("simple_identifier")
            .or_else(|| self.find_simple_identifier(node))
            .map(|n| self.node_text(n))
            .unwrap_or_else(|| "function".to_string());

        let modifiers = self.extract_modifiers(node);
        let visibility = self.extract_visibility(&modifiers);
        let is_abstract = modifiers.contains(&"abstract".to_string());
        let return_type = self.extract_return_type(node);
        let parameters = self.extract_parameters(node);
        let doc_comment = self.extract_doc_comment(node);
        let is_suspend = modifiers.contains(&"suspend".to_string());

        let mut attributes = Vec::new();
        if is_suspend {
            attributes.push("suspend".to_string());
        }
        if modifiers.contains(&"inline".to_string()) {
            attributes.push("inline".to_string());
        }
        if modifiers.contains(&"operator".to_string()) {
            attributes.push("operator".to_string());
        }

        // Calculate complexity from function body (find function_body as a direct child)
        let func_body_node = {
            let mut cursor = node.walk();
            let found = node
                .children(&mut cursor)
                .find(|c| c.kind() == "function_body");
            found
        };
        let complexity = func_body_node.map(|body| self.calculate_complexity(body));
        let body_prefix = func_body_node
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
            is_async: is_suspend,
            is_test: self.has_test_annotation(node),
            is_static: false, // Kotlin doesn't have static, uses companion objects
            is_abstract,
            parameters,
            return_type,
            doc_comment,
            attributes,
            parent_class: self.current_class.clone(),
            complexity,
            body_prefix,
        };

        self.functions.push(func);

        // Set current function context and visit body to extract calls
        let previous_function = self.current_function.take();
        self.current_function = Some(name);

        // Find function_body as a direct child (not a field)
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "function_body" {
                self.visit_function_body(child);
                break;
            }
        }

        self.current_function = previous_function;
    }

    /// Visit a call expression and extract the call relationship
    fn visit_call_expression(&mut self, node: Node) {
        // Only record calls if we're inside a function
        let caller = match &self.current_function {
            Some(name) => name.clone(),
            None => return,
        };

        let callee = self.extract_callee_name(node);

        // Skip empty callees
        if callee.is_empty() || callee == "this" || callee == "super" {
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
            // Direct function call: foo(), bar()
            "call_expression" => {
                // The callee is the first child (before arguments)
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    match child.kind() {
                        "simple_identifier" => {
                            return self.node_text(child);
                        }
                        "navigation_expression" => {
                            // obj.method() - extract method name
                            return self.extract_navigation_callee(child);
                        }
                        _ => {}
                    }
                }
                String::new()
            }
            "navigation_expression" => self.extract_navigation_callee(node),
            _ => String::new(),
        }
    }

    fn extract_navigation_callee(&self, node: Node) -> String {
        // For obj.method - extract just the method name
        // Or for chained calls, extract the final method
        let mut result = String::new();
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            if child.kind() == "simple_identifier" {
                result = self.node_text(child);
            } else if child.kind() == "navigation_expression" {
                // Recurse into chained navigation
                result = self.extract_navigation_callee(child);
            }
        }

        result
    }

    /// Visit function body to extract calls
    fn visit_function_body(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "call_expression" => {
                    self.visit_call_expression(child);
                    self.visit_function_body(child);
                }
                _ => {
                    self.visit_function_body(child);
                }
            }
        }
    }

    fn extract_supertypes(
        &mut self,
        node: Node,
        class_name: &str,
        base_classes: &mut Vec<String>,
        implemented_traits: &mut Vec<String>,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            // Handle both delegation_specifiers (container) and delegation_specifier (direct)
            if child.kind() == "delegation_specifiers" {
                let mut spec_cursor = child.walk();
                for spec in child.children(&mut spec_cursor) {
                    if spec.kind() == "delegation_specifier" {
                        self.process_delegation_specifier(
                            spec,
                            class_name,
                            base_classes,
                            implemented_traits,
                        );
                    }
                }
            } else if child.kind() == "delegation_specifier" {
                // Direct delegation_specifier child (no container)
                self.process_delegation_specifier(
                    child,
                    class_name,
                    base_classes,
                    implemented_traits,
                );
            }
        }
    }

    fn process_delegation_specifier(
        &mut self,
        spec: Node,
        class_name: &str,
        base_classes: &mut Vec<String>,
        implemented_traits: &mut Vec<String>,
    ) {
        let type_name = self.extract_supertype_name(spec);
        if !type_name.is_empty() {
            // In Kotlin, we check if it has parentheses (constructor call) to determine
            // if it's a class or interface. If it has (), it's typically a class.
            let has_constructor_call = self.has_constructor_call(spec);

            if has_constructor_call {
                base_classes.push(type_name.clone());
                self.inheritance.push(InheritanceRelation {
                    child: class_name.to_string(),
                    parent: type_name,
                    order: base_classes.len() - 1,
                });
            } else {
                implemented_traits.push(type_name.clone());
                self.implementations.push(ImplementationRelation {
                    implementor: class_name.to_string(),
                    trait_name: type_name,
                });
            }
        }
    }

    fn extract_supertype_name(&self, node: Node) -> String {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "user_type" | "type_identifier" | "simple_identifier" => {
                    return self.extract_type_identifier(child);
                }
                "constructor_invocation" => {
                    // Extract the type from constructor invocation
                    let mut inv_cursor = child.walk();
                    for inv_child in child.children(&mut inv_cursor) {
                        if inv_child.kind() == "user_type" {
                            return self.extract_type_identifier(inv_child);
                        }
                    }
                }
                _ => {}
            }
        }
        String::new()
    }

    fn extract_type_identifier(&self, node: Node) -> String {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "type_identifier" || child.kind() == "simple_identifier" {
                return self.node_text(child);
            }
        }
        self.node_text(node)
    }

    fn has_constructor_call(&self, node: Node) -> bool {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "constructor_invocation" || child.kind() == "value_arguments" {
                return true;
            }
        }
        false
    }

    fn extract_interface_methods(&self, node: Node) -> Vec<FunctionEntity> {
        let mut methods = Vec::new();
        if let Some(body) = node.child_by_field_name("class_body") {
            let mut cursor = body.walk();
            for child in body.children(&mut cursor) {
                if child.kind() == "function_declaration" {
                    let name = child
                        .child_by_field_name("simple_identifier")
                        .or_else(|| self.find_simple_identifier(child))
                        .map(|n| self.node_text(n))
                        .unwrap_or_else(|| "method".to_string());

                    let modifiers = self.extract_modifiers(child);
                    let visibility = self.extract_visibility(&modifiers);
                    let return_type = self.extract_return_type(child);
                    let parameters = self.extract_parameters(child);

                    let func = FunctionEntity {
                        name,
                        signature: self.extract_function_signature(child),
                        visibility,
                        line_start: child.start_position().row + 1,
                        line_end: child.end_position().row + 1,
                        is_async: modifiers.contains(&"suspend".to_string()),
                        is_test: false,
                        is_static: false,
                        is_abstract: true,
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

    fn find_class_name(&self, node: Node) -> Option<String> {
        // First try the field name
        if let Some(name_node) = node.child_by_field_name("type_identifier") {
            return Some(self.node_text(name_node));
        }
        if let Some(name_node) = node.child_by_field_name("simple_identifier") {
            return Some(self.node_text(name_node));
        }

        // Fall back to searching children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "type_identifier" || child.kind() == "simple_identifier" {
                return Some(self.node_text(child));
            }
        }
        None
    }

    #[allow(clippy::manual_find)]
    fn find_simple_identifier<'b>(&self, node: Node<'b>) -> Option<Node<'b>> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "simple_identifier" {
                return Some(child);
            }
        }
        None
    }

    fn extract_modifiers(&self, node: Node) -> Vec<String> {
        let mut modifiers = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "modifiers" {
                let mut mod_cursor = child.walk();
                for modifier in child.children(&mut mod_cursor) {
                    let kind = modifier.kind();
                    if kind.ends_with("_modifier") || kind == "annotation" {
                        modifiers.push(self.node_text(modifier));
                    }
                }
            }
        }
        modifiers
    }

    fn extract_visibility(&self, modifiers: &[String]) -> String {
        for modifier in modifiers {
            let lower = modifier.to_lowercase();
            if lower.contains("public")
                || lower.contains("private")
                || lower.contains("protected")
                || lower.contains("internal")
            {
                return lower;
            }
        }
        "public".to_string() // Default Kotlin visibility
    }

    fn extract_return_type(&self, node: Node) -> Option<String> {
        // Look for : Type after parameters
        let mut cursor = node.walk();
        let mut found_colon = false;
        for child in node.children(&mut cursor) {
            if child.kind() == ":" {
                found_colon = true;
            } else if found_colon
                && (child.kind() == "user_type"
                    || child.kind() == "nullable_type"
                    || child.kind() == "type_identifier")
            {
                let type_str = self.node_text(child);
                if type_str != "Unit" {
                    return Some(type_str);
                }
                return None;
            }
        }
        None
    }

    fn extract_parameters(&self, node: Node) -> Vec<Parameter> {
        let mut params = Vec::new();
        if let Some(params_node) = node.child_by_field_name("function_value_parameters") {
            let mut cursor = params_node.walk();
            for child in params_node.children(&mut cursor) {
                if child.kind() == "parameter" {
                    if let Some(param) = self.extract_parameter(child) {
                        params.push(param);
                    }
                }
            }
        }
        params
    }

    fn extract_parameter(&self, node: Node) -> Option<Parameter> {
        let mut name = String::new();
        let mut type_annotation = None;
        let mut default_value = None;
        let mut is_vararg = false;

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "simple_identifier" => {
                    if name.is_empty() {
                        name = self.node_text(child);
                    }
                }
                "type" | "user_type" | "nullable_type" => {
                    type_annotation = Some(self.node_text(child));
                }
                "vararg" => {
                    is_vararg = true;
                }
                _ => {
                    // Check for default value (expression after =)
                    if child.kind().ends_with("_expression") || child.kind().ends_with("_literal") {
                        default_value = Some(self.node_text(child));
                    }
                }
            }
        }

        if name.is_empty() {
            return None;
        }

        let mut param = Parameter::new(name);
        if let Some(t) = type_annotation {
            param = param.with_type(t);
        }
        if let Some(d) = default_value {
            param = param.with_default(d);
        }
        if is_vararg {
            param = param.variadic();
        }
        Some(param)
    }

    fn extract_function_signature(&self, node: Node) -> String {
        self.node_text(node)
            .lines()
            .next()
            .unwrap_or("")
            .to_string()
    }

    fn extract_type_parameters(&self, node: Node) -> Vec<String> {
        let mut type_params = Vec::new();
        // Look for type_parameters as a direct child (not a field)
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "type_parameters" {
                let mut inner_cursor = child.walk();
                for inner in child.children(&mut inner_cursor) {
                    if inner.kind() == "type_parameter" {
                        // Extract just the type identifier from the type_parameter
                        let mut param_cursor = inner.walk();
                        for param_child in inner.children(&mut param_cursor) {
                            if param_child.kind() == "type_identifier" {
                                type_params.push(self.node_text(param_child));
                                break;
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
        // Look for preceding multiline comment (KDoc)
        if let Some(prev) = node.prev_sibling() {
            if prev.kind() == "multiline_comment" {
                let comment = self.node_text(prev);
                if comment.starts_with("/**") {
                    return Some(comment);
                }
            }
        }
        None
    }

    fn has_test_annotation(&self, node: Node) -> bool {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "modifiers" {
                let mut mod_cursor = child.walk();
                for modifier in child.children(&mut mod_cursor) {
                    if modifier.kind() == "annotation" {
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
            "if_expression" => {
                builder.add_branch();
                builder.enter_scope();

                // Check for an else branch by scanning children for the "else" keyword
                let has_else = {
                    let mut cursor = node.walk();
                    let x = node
                        .children(&mut cursor)
                        .any(|child| child.kind() == "else");
                    x
                };
                if has_else {
                    builder.add_branch();
                }
            }
            "when_expression" => {
                builder.enter_scope();
            }
            "when_entry" => {
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
            "do_while_statement" => {
                builder.add_loop();
                builder.enter_scope();
            }
            "catch_block" => {
                builder.add_exception_handler();
            }
            "finally_block" => {
                builder.add_exception_handler();
            }
            "conjunction_expression" => {
                // Kotlin && operator
                builder.add_logical_operator();
            }
            "disjunction_expression" => {
                // Kotlin || operator
                builder.add_logical_operator();
            }
            "elvis_expression" => {
                // Kotlin ?: (null-coalescing) acts as a branch
                builder.add_branch();
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_for_complexity(child, builder);
        }

        // Exit scope for control structures that entered one
        match node.kind() {
            "if_expression" | "when_expression" | "for_statement" | "while_statement"
            | "do_while_statement" => {
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

    fn parse_and_visit(source: &[u8]) -> KotlinVisitor<'_> {
        use tree_sitter::Parser;

        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_kotlin::language())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = KotlinVisitor::new(source);
        visitor.visit_node(tree.root_node());
        visitor
    }

    #[test]
    fn test_visitor_basics() {
        let visitor = KotlinVisitor::new(b"");
        assert_eq!(visitor.functions.len(), 0);
        assert_eq!(visitor.classes.len(), 0);
        assert_eq!(visitor.traits.len(), 0);
    }

    #[test]
    fn test_visitor_class_extraction() {
        let source = b"class Person { val name: String = \"\" }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "Person");
    }

    #[test]
    fn test_visitor_interface_extraction() {
        let source = b"interface Reader { fun read(): String }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.traits.len(), 1);
        assert_eq!(visitor.traits[0].name, "Reader");
    }

    #[test]
    fn test_visitor_function_extraction() {
        let source = b"fun add(a: Int, b: Int): Int { return a + b }";
        let visitor = parse_and_visit(source);

        assert!(!visitor.functions.is_empty());
        assert_eq!(visitor.functions[0].name, "add");
    }

    #[test]
    fn test_visitor_method_extraction() {
        let source = b"class Calculator { fun add(a: Int, b: Int): Int { return a + b } }";
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
        let source = b"import java.util.List\nimport java.util.ArrayList";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 2);
    }

    #[test]
    fn test_visitor_data_class() {
        let source = b"data class Person(val name: String, val age: Int)";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "Person");
        assert!(visitor.classes[0].attributes.contains(&"data".to_string()));
    }

    #[test]
    fn test_visitor_object() {
        let source = b"object Singleton { fun getInstance(): Singleton = this }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "Singleton");
        assert!(visitor.classes[0]
            .attributes
            .contains(&"object".to_string()));
    }

    #[test]
    fn test_visitor_enum_class() {
        let source = b"enum class Status { PENDING, ACTIVE, COMPLETED }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "Status");
        assert!(visitor.classes[0].attributes.contains(&"enum".to_string()));
    }

    #[test]
    fn test_visitor_package() {
        let source = b"package com.example.app\nclass App";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "com.example.app.App");
    }

    #[test]
    fn test_visitor_abstract_class() {
        let source = b"abstract class BaseController { abstract fun handle() }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert!(visitor.classes[0].is_abstract);
    }

    #[test]
    fn test_visitor_suspend_function() {
        let source = b"suspend fun fetchData(): String { return \"data\" }";
        let visitor = parse_and_visit(source);

        assert!(!visitor.functions.is_empty());
        assert!(visitor.functions[0].is_async);
        assert!(visitor.functions[0]
            .attributes
            .contains(&"suspend".to_string()));
    }

    #[test]
    fn test_visitor_visibility_modifiers() {
        let source = b"class Foo { private fun bar() {} protected fun baz() {} public fun qux() {} internal fun internal_fn() {} }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 4);
        // Note: The actual visibility depends on how tree-sitter-kotlin parses modifiers
    }

    #[test]
    fn test_visitor_method_call_extraction() {
        let source = b"
class MyClass {
    fun caller() {
        helper()
        process()
    }

    fun helper() {}
    fun process() {}
}
";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.functions.len(), 3);
        assert_eq!(visitor.calls.len(), 2);

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
    fn test_visitor_call_line_numbers() {
        let source = b"
class Test {
    fun caller() {
        helper()
    }
    fun helper() {}
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
    fn test_visitor_generic_class() {
        let source = b"class Container<T>(val value: T)";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert!(!visitor.classes[0].type_parameters.is_empty());
    }

    #[test]
    fn test_complexity_simple_function() {
        // A function with no branching should have CC=1
        let source = b"fun greet(name: String): String { return \"Hello, $name!\" }";
        let visitor = parse_and_visit(source);

        assert!(!visitor.functions.is_empty());
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert_eq!(complexity.cyclomatic_complexity, 1);
        assert_eq!(complexity.branches, 0);
        assert_eq!(complexity.loops, 0);
    }

    #[test]
    fn test_complexity_if_else_and_loop() {
        // A function with if/else and a for loop should have CC > 1
        let source = b"
fun classify(x: Int): String {
    val result = if (x > 0) {
        \"positive\"
    } else {
        \"non-positive\"
    }
    for (i in 0..x) {
        println(i)
    }
    return result
}
";
        let visitor = parse_and_visit(source);

        assert!(!visitor.functions.is_empty());
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        // if adds 1 branch, else adds 1 branch, for adds 1 loop: CC = 1 + 1 + 1 + 1 = 4
        assert!(
            complexity.branches >= 2,
            "Expected at least 2 branches, got {}",
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
    fn test_complexity_when_expression() {
        // A function with a when expression should count each when_entry as a branch
        let source = b"
fun describe(x: Int): String {
    return when (x) {
        1 -> \"one\"
        2 -> \"two\"
        3 -> \"three\"
        else -> \"other\"
    }
}
";
        let visitor = parse_and_visit(source);

        assert!(!visitor.functions.is_empty());
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        // 4 when_entry nodes (1, 2, 3, else): 4 branches total
        assert!(
            complexity.branches >= 4,
            "Expected at least 4 branches for when entries, got {}",
            complexity.branches
        );
        assert!(complexity.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_complexity_logical_operators() {
        // Logical && and || operators should increase complexity
        let source = b"
fun check(a: Int, b: Int, c: Int): Boolean {
    return a > 0 && b > 0 || c == 0
}
";
        let visitor = parse_and_visit(source);

        assert!(!visitor.functions.is_empty());
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        // disjunction (||) wraps conjunction (&&): 2 logical operators
        assert!(
            complexity.logical_operators >= 2,
            "Expected at least 2 logical operators, got {}",
            complexity.logical_operators
        );
    }

    #[test]
    fn test_complexity_exception_handling() {
        // catch and finally blocks should increase exception_handlers count
        let source = b"
fun riskyOp() {
    try {
        println(\"try\")
    } catch (e: Exception) {
        println(\"catch\")
    } finally {
        println(\"finally\")
    }
}
";
        let visitor = parse_and_visit(source);

        assert!(!visitor.functions.is_empty());
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        // 1 catch_block + 1 finally_block = 2 exception handlers
        assert!(
            complexity.exception_handlers >= 2,
            "Expected at least 2 exception handlers, got {}",
            complexity.exception_handlers
        );
    }

    #[test]
    fn test_complexity_while_and_do_while() {
        // while and do-while loops add to loop count
        let source = b"
fun loops(n: Int) {
    var i = 0
    while (i < n) { i++ }
    do { i-- } while (i > 0)
}
";
        let visitor = parse_and_visit(source);

        assert!(!visitor.functions.is_empty());
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(
            complexity.loops >= 2,
            "Expected at least 2 loops, got {}",
            complexity.loops
        );
    }

    #[test]
    fn test_complexity_nesting_depth() {
        // Nested control structures should track max nesting depth
        let source = b"
fun nested(x: Int, y: Int) {
    if (x > 0) {
        for (i in 0..x) {
            while (y > 0) {
                println(i)
            }
        }
    }
}
";
        let visitor = parse_and_visit(source);

        assert!(!visitor.functions.is_empty());
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        // if -> for -> while: nesting depth of 3
        assert!(
            complexity.max_nesting_depth >= 3,
            "Expected nesting depth >= 3, got {}",
            complexity.max_nesting_depth
        );
    }
}
