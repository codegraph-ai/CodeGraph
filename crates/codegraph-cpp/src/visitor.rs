// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting C++ entities

use codegraph_parser_api::{
    CallRelation, ClassEntity, ComplexityBuilder, ComplexityMetrics, FunctionEntity,
    ImplementationRelation, ImportRelation, InheritanceRelation, Parameter, TraitEntity,
    BODY_PREFIX_MAX_CHARS,
};
use tree_sitter::Node;

pub struct CppVisitor<'a> {
    pub source: &'a [u8],
    pub functions: Vec<FunctionEntity>,
    pub classes: Vec<ClassEntity>,
    pub traits: Vec<TraitEntity>,
    pub imports: Vec<ImportRelation>,
    pub calls: Vec<CallRelation>,
    pub inheritance: Vec<InheritanceRelation>,
    pub implementations: Vec<ImplementationRelation>,
    current_namespace: Vec<String>,
    current_class: Option<String>,
    current_function: Option<String>,
}

impl<'a> CppVisitor<'a> {
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
            current_namespace: Vec::new(),
            current_class: None,
            current_function: None,
        }
    }

    fn node_text(&self, node: Node) -> String {
        node.utf8_text(self.source).unwrap_or("").to_string()
    }

    pub fn visit_node(&mut self, node: Node) {
        let should_recurse = match node.kind() {
            "namespace_definition" => {
                self.visit_namespace(node);
                false
            }
            "class_specifier" => {
                self.visit_class(node);
                false
            }
            "struct_specifier" => {
                self.visit_struct(node);
                false
            }
            "function_definition" => {
                if self.current_class.is_none() {
                    self.visit_function(node);
                }
                false
            }
            "preproc_include" => {
                self.visit_include(node);
                false
            }
            "template_declaration" => {
                self.visit_template(node);
                false
            }
            "enum_specifier" => {
                self.visit_enum(node);
                false
            }
            "call_expression" => {
                self.visit_call_expression(node);
                true
            }
            _ => true,
        };

        if should_recurse {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                self.visit_node(child);
            }
        }
    }

    fn visit_namespace(&mut self, node: Node) {
        let name = self.extract_namespace_name(node);

        if let Some(ref ns_name) = name {
            self.current_namespace.push(ns_name.clone());
        }

        // Visit namespace body
        if let Some(body) = node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for child in body.children(&mut cursor) {
                self.visit_node(child);
            }
        } else {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                self.visit_node(child);
            }
        }

        if name.is_some() {
            self.current_namespace.pop();
        }
    }

    fn extract_namespace_name(&self, node: Node) -> Option<String> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "namespace_identifier" | "identifier" => {
                    return Some(self.node_text(child));
                }
                _ => {}
            }
        }
        None
    }

    fn visit_class(&mut self, node: Node) {
        self.visit_class_or_struct(node, false);
    }

    fn visit_struct(&mut self, node: Node) {
        self.visit_class_or_struct(node, true);
    }

    fn visit_class_or_struct(&mut self, node: Node, is_struct: bool) {
        let name = self.extract_type_name(node);
        if name.is_none() {
            return;
        }
        let name = name.unwrap();

        let qualified_name = self.qualify_name(&name);
        let previous_class = self.current_class.take();
        self.current_class = Some(qualified_name.clone());

        let mut base_classes = Vec::new();
        self.extract_base_classes(node, &qualified_name, &mut base_classes);

        let type_params = self.extract_template_params_from_parent(node);
        let doc_comment = self.extract_doc_comment(node);
        let is_abstract = self.is_abstract_class(node);

        let mut attributes = Vec::new();
        if is_struct {
            attributes.push("struct".to_string());
        }

        let body_prefix = node
            .child_by_field_name("body")
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| {
                if t.len() > BODY_PREFIX_MAX_CHARS {
                    &t[..BODY_PREFIX_MAX_CHARS]
                } else {
                    t
                }
            })
            .map(|t| t.to_string());

        let class_entity = ClassEntity {
            name: qualified_name.clone(),
            visibility: if is_struct {
                "public".to_string()
            } else {
                "private".to_string()
            },
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_abstract,
            is_interface: false,
            base_classes,
            implemented_traits: Vec::new(),
            methods: Vec::new(),
            fields: Vec::new(),
            doc_comment,
            attributes,
            type_parameters: type_params,
            body_prefix,
        };

        self.classes.push(class_entity);

        // Visit class body
        if let Some(body) = node.child_by_field_name("body") {
            self.visit_class_body(body);
        }

        self.current_class = previous_class;
    }

    fn visit_class_body(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "function_definition" => self.visit_method(child),
                "declaration" => self.visit_declaration(child),
                "field_declaration" => self.visit_field_declaration(child),
                "class_specifier" => self.visit_class(child),
                "struct_specifier" => self.visit_struct(child),
                _ => {}
            }
        }
    }

    fn visit_field_declaration(&mut self, node: Node) {
        // Check if this is a method declaration
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "function_declarator" {
                self.visit_method_declaration(node, child);
                return;
            }
        }
    }

    fn visit_method_declaration(&mut self, decl_node: Node, func_declarator: Node) {
        let name = self.extract_declarator_name(func_declarator);
        if name.is_none() {
            return;
        }
        let name = name.unwrap();

        let params = self.extract_parameters(func_declarator);
        let return_type = self.extract_return_type(decl_node);
        let visibility = self.extract_visibility(decl_node);
        let is_static = self.has_storage_class(decl_node, "static");
        let is_virtual = self.has_specifier(decl_node, "virtual");

        let mut func = FunctionEntity::new(
            &name,
            decl_node.start_position().row + 1,
            decl_node.end_position().row + 1,
        )
        .with_visibility(&visibility)
        .with_signature(self.node_text(decl_node).lines().next().unwrap_or(""));

        func.parameters = params;
        func.return_type = return_type;
        func.is_static = is_static;
        func.parent_class = self.current_class.clone();
        func.body_prefix = None; // Declaration only, no body

        if is_virtual {
            func.attributes.push("virtual".to_string());
        }

        self.functions.push(func);
    }

    fn visit_declaration(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "function_declarator" {
                self.visit_method_declaration(node, child);
                return;
            }
            if child.kind() == "init_declarator" {
                let mut inner_cursor = child.walk();
                for inner_child in child.children(&mut inner_cursor) {
                    if inner_child.kind() == "function_declarator" {
                        self.visit_method_declaration(node, inner_child);
                        return;
                    }
                }
            }
        }
    }

    fn visit_function(&mut self, node: Node) {
        let declarator = node.child_by_field_name("declarator");
        if declarator.is_none() {
            return;
        }
        let declarator = declarator.unwrap();

        let name = self.extract_declarator_name(declarator);
        if name.is_none() {
            return;
        }
        let name = name.unwrap();

        let qualified_name = if let Some(ref class) = self.current_class {
            format!("{}::{}", class, name)
        } else {
            self.qualify_name(&name)
        };

        let params = self.extract_parameters(declarator);
        let return_type = self.extract_return_type(node);
        let is_static = self.has_storage_class(node, "static");
        let is_virtual = self.has_specifier(node, "virtual");
        let doc_comment = self.extract_doc_comment(node);

        let mut func = FunctionEntity::new(
            &qualified_name,
            node.start_position().row + 1,
            node.end_position().row + 1,
        )
        .with_visibility(self.extract_visibility(node))
        .with_signature(self.node_text(node).lines().next().unwrap_or(""));

        func.parameters = params;
        func.return_type = return_type;
        func.is_static = is_static;
        func.is_async = self.is_coroutine(node);
        func.doc_comment = doc_comment;
        func.parent_class = self.current_class.clone();
        func.complexity = node
            .child_by_field_name("body")
            .map(|body| self.calculate_complexity(body));
        func.body_prefix = node
            .child_by_field_name("body")
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| {
                if t.len() > BODY_PREFIX_MAX_CHARS {
                    &t[..BODY_PREFIX_MAX_CHARS]
                } else {
                    t
                }
            })
            .map(|t| t.to_string());

        if is_virtual {
            func.attributes.push("virtual".to_string());
        }

        self.functions.push(func);

        // Set current function context and visit body
        let previous_function = self.current_function.take();
        self.current_function = Some(qualified_name);

        if let Some(body) = node.child_by_field_name("body") {
            self.visit_function_body(body);
        }

        self.current_function = previous_function;
    }

    fn visit_method(&mut self, node: Node) {
        let declarator = node.child_by_field_name("declarator");
        if declarator.is_none() {
            return;
        }
        let declarator = declarator.unwrap();

        let name = self.extract_declarator_name(declarator);
        if name.is_none() {
            return;
        }
        let name = name.unwrap();

        let params = self.extract_parameters(declarator);
        let return_type = self.extract_return_type(node);
        let visibility = self.extract_visibility(node);
        let is_static = self.has_storage_class(node, "static");
        let is_virtual = self.has_specifier(node, "virtual");
        let is_const = self.is_const_method(declarator);
        let doc_comment = self.extract_doc_comment(node);

        let mut func = FunctionEntity::new(
            &name,
            node.start_position().row + 1,
            node.end_position().row + 1,
        )
        .with_visibility(&visibility)
        .with_signature(self.node_text(node).lines().next().unwrap_or(""));

        func.parameters = params;
        func.return_type = return_type;
        func.is_static = is_static;
        func.is_async = self.is_coroutine(node);
        func.doc_comment = doc_comment;
        func.parent_class = self.current_class.clone();
        func.complexity = node
            .child_by_field_name("body")
            .map(|body| self.calculate_complexity(body));
        func.body_prefix = node
            .child_by_field_name("body")
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| {
                if t.len() > BODY_PREFIX_MAX_CHARS {
                    &t[..BODY_PREFIX_MAX_CHARS]
                } else {
                    t
                }
            })
            .map(|t| t.to_string());

        if is_virtual {
            func.attributes.push("virtual".to_string());
        }
        if is_const {
            func.attributes.push("const".to_string());
        }

        self.functions.push(func);

        // Set current function context and visit body
        let previous_function = self.current_function.take();
        self.current_function = Some(name);

        if let Some(body) = node.child_by_field_name("body") {
            self.visit_function_body(body);
        }

        self.current_function = previous_function;
    }

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

    fn visit_call_expression(&mut self, node: Node) {
        let caller = match &self.current_function {
            Some(name) => name.clone(),
            None => return,
        };

        let callee = self.extract_callee_name(node);
        if callee.is_empty() {
            return;
        }

        let call = CallRelation {
            caller,
            callee,
            call_site_line: node.start_position().row + 1,
            is_direct: true,
            struct_type: None,
            field_name: None,
        };

        self.calls.push(call);
    }

    fn extract_callee_name(&self, node: Node) -> String {
        if let Some(function) = node.child_by_field_name("function") {
            match function.kind() {
                "identifier" | "field_identifier" => self.node_text(function),
                "qualified_identifier" => self.node_text(function),
                "field_expression" => {
                    if let Some(field) = function.child_by_field_name("field") {
                        self.node_text(field)
                    } else {
                        self.node_text(function)
                    }
                }
                "template_function" => {
                    if let Some(name) = function.child_by_field_name("name") {
                        self.node_text(name)
                    } else {
                        self.node_text(function)
                    }
                }
                _ => self.node_text(function),
            }
        } else {
            String::new()
        }
    }

    fn visit_template(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "class_specifier" => self.visit_class(child),
                "struct_specifier" => self.visit_struct(child),
                "function_definition" => {
                    if self.current_class.is_none() {
                        self.visit_function(child);
                    }
                }
                "declaration" => self.visit_declaration(child),
                _ => {}
            }
        }
    }

    fn visit_enum(&mut self, node: Node) {
        let name = self.extract_type_name(node);
        if name.is_none() {
            return;
        }
        let name = name.unwrap();

        let qualified_name = self.qualify_name(&name);
        let is_enum_class = self.is_enum_class(node);
        let doc_comment = self.extract_doc_comment(node);

        let mut attributes = vec!["enum".to_string()];
        if is_enum_class {
            attributes.push("enum_class".to_string());
        }

        let body_prefix = node
            .child_by_field_name("body")
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| {
                if t.len() > BODY_PREFIX_MAX_CHARS {
                    &t[..BODY_PREFIX_MAX_CHARS]
                } else {
                    t
                }
            })
            .map(|t| t.to_string());

        let enum_entity = ClassEntity {
            name: qualified_name,
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
            attributes,
            type_parameters: Vec::new(),
            body_prefix,
        };

        self.classes.push(enum_entity);
    }

    fn visit_include(&mut self, node: Node) {
        let mut path = String::new();
        let mut is_system = false;

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "system_lib_string" => {
                    path = self.node_text(child);
                    path = path
                        .trim_start_matches('<')
                        .trim_end_matches('>')
                        .to_string();
                    is_system = true;
                }
                "string_literal" => {
                    path = self.node_text(child);
                    path = path.trim_matches('"').to_string();
                    is_system = false;
                }
                _ => {}
            }
        }

        if !path.is_empty() {
            let import = ImportRelation {
                importer: "file".to_string(),
                imported: path,
                symbols: Vec::new(),
                is_wildcard: true,
                alias: if is_system {
                    Some("system".to_string())
                } else {
                    None
                },
            };
            self.imports.push(import);
        }
    }

    // Helper methods

    fn extract_type_name(&self, node: Node) -> Option<String> {
        if let Some(name_node) = node.child_by_field_name("name") {
            return Some(self.node_text(name_node));
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "type_identifier" || child.kind() == "identifier" {
                return Some(self.node_text(child));
            }
        }
        None
    }

    fn extract_declarator_name(&self, node: Node) -> Option<String> {
        if let Some(decl) = node.child_by_field_name("declarator") {
            return self.extract_declarator_name(decl);
        }

        match node.kind() {
            "identifier" | "field_identifier" | "destructor_name" => Some(self.node_text(node)),
            "qualified_identifier" => node
                .child_by_field_name("name")
                .map(|name| self.node_text(name)),
            "function_declarator" | "pointer_declarator" | "reference_declarator" => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if let Some(name) = self.extract_declarator_name(child) {
                        return Some(name);
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn extract_parameters(&self, node: Node) -> Vec<Parameter> {
        let mut params = Vec::new();

        if let Some(param_list) = node.child_by_field_name("parameters") {
            let mut cursor = param_list.walk();
            for child in param_list.children(&mut cursor) {
                if child.kind() == "parameter_declaration" {
                    let param_type = self.extract_param_type(child);
                    let param_name = self.extract_param_name(child);

                    let mut param = Parameter::new(param_name);
                    param = param.with_type(param_type);
                    params.push(param);
                }
            }
        }

        params
    }

    fn extract_param_type(&self, node: Node) -> String {
        if let Some(type_node) = node.child_by_field_name("type") {
            return self.node_text(type_node);
        }

        let mut cursor = node.walk();
        let mut type_parts = Vec::new();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "primitive_type"
                | "type_identifier"
                | "sized_type_specifier"
                | "template_type"
                | "qualified_identifier" => {
                    type_parts.push(self.node_text(child));
                }
                _ => {}
            }
        }

        if type_parts.is_empty() {
            "void".to_string()
        } else {
            type_parts.join(" ")
        }
    }

    fn extract_param_name(&self, node: Node) -> String {
        if let Some(decl) = node.child_by_field_name("declarator") {
            if let Some(name) = self.extract_declarator_name(decl) {
                return name;
            }
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "identifier" {
                return self.node_text(child);
            }
        }

        String::new()
    }

    fn extract_return_type(&self, node: Node) -> Option<String> {
        if let Some(type_node) = node.child_by_field_name("type") {
            let type_str = self.node_text(type_node);
            if type_str != "void" {
                return Some(type_str);
            }
            return None;
        }

        let mut cursor = node.walk();
        let mut type_parts = Vec::new();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "primitive_type"
                | "type_identifier"
                | "sized_type_specifier"
                | "template_type"
                | "qualified_identifier" => {
                    type_parts.push(self.node_text(child));
                }
                "function_declarator" | "pointer_declarator" => break,
                _ => {}
            }
        }

        if type_parts.is_empty() || type_parts == vec!["void"] {
            None
        } else {
            Some(type_parts.join(" "))
        }
    }

    fn extract_base_classes(
        &mut self,
        node: Node,
        class_name: &str,
        base_classes: &mut Vec<String>,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "base_class_clause" {
                self.extract_base_class_items(child, class_name, base_classes);
            }
        }
    }

    fn extract_base_class_items(
        &mut self,
        node: Node,
        class_name: &str,
        base_classes: &mut Vec<String>,
    ) {
        let mut cursor = node.walk();
        let mut order = 0;
        for child in node.children(&mut cursor) {
            // tree-sitter-cpp 0.22 may use type_identifier directly or base_class_specifier
            match child.kind() {
                "base_class_specifier" => {
                    if let Some(type_name) = self.extract_base_type_name(child) {
                        base_classes.push(type_name.clone());
                        self.inheritance.push(InheritanceRelation {
                            child: class_name.to_string(),
                            parent: type_name,
                            order,
                        });
                        order += 1;
                    }
                }
                "type_identifier" | "qualified_identifier" | "template_type" => {
                    let type_name = self.node_text(child);
                    base_classes.push(type_name.clone());
                    self.inheritance.push(InheritanceRelation {
                        child: class_name.to_string(),
                        parent: type_name,
                        order,
                    });
                    order += 1;
                }
                _ => {}
            }
        }
    }

    fn extract_base_type_name(&self, node: Node) -> Option<String> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "type_identifier" | "qualified_identifier" | "template_type" => {
                    return Some(self.node_text(child));
                }
                _ => {}
            }
        }
        None
    }

    fn extract_template_params_from_parent(&self, node: Node) -> Vec<String> {
        if let Some(parent) = node.parent() {
            if parent.kind() == "template_declaration" {
                return self.extract_template_params(parent);
            }
        }
        Vec::new()
    }

    fn extract_template_params(&self, node: Node) -> Vec<String> {
        let mut params = Vec::new();

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "template_parameter_list" {
                let mut param_cursor = child.walk();
                for param in child.children(&mut param_cursor) {
                    if param.kind() == "type_parameter_declaration" {
                        if let Some(name) = param.child_by_field_name("name") {
                            params.push(self.node_text(name));
                        } else {
                            let mut inner_cursor = param.walk();
                            for inner in param.children(&mut inner_cursor) {
                                if inner.kind() == "type_identifier" {
                                    params.push(self.node_text(inner));
                                }
                            }
                        }
                    }
                }
            }
        }

        params
    }

    fn is_abstract_class(&self, node: Node) -> bool {
        if let Some(body) = node.child_by_field_name("body") {
            let source = self.node_text(body);
            return source.contains("= 0");
        }
        false
    }

    fn is_enum_class(&self, node: Node) -> bool {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "class" {
                return true;
            }
        }
        false
    }

    fn has_storage_class(&self, node: Node, storage_class: &str) -> bool {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "storage_class_specifier" && self.node_text(child) == storage_class {
                return true;
            }
        }
        false
    }

    fn has_specifier(&self, node: Node, specifier: &str) -> bool {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "virtual" && specifier == "virtual" {
                return true;
            }
            if child.kind() == "function_specifier" && self.node_text(child) == specifier {
                return true;
            }
        }
        false
    }

    /// Check if a function is a C++20 coroutine by looking for co_await/co_return/co_yield
    fn is_coroutine(&self, node: Node) -> bool {
        // Check return type for coroutine-related types
        if let Some(ret) = self.extract_return_type(node) {
            let ret_lower = ret.to_lowercase();
            if ret_lower.contains("coroutine_handle")
                || ret_lower.contains("task")
                || ret_lower.contains("generator")
                || ret_lower.contains("future")
                || ret_lower.contains("lazy")
            {
                return true;
            }
        }

        // Check function body for coroutine keywords
        if let Some(body) = node.child_by_field_name("body") {
            return self.has_coroutine_keyword(body);
        }
        false
    }

    /// Recursively check for co_await, co_return, co_yield in a node
    fn has_coroutine_keyword(&self, node: Node) -> bool {
        let kind = node.kind();
        if kind == "co_await_expression"
            || kind == "co_return_statement"
            || kind == "co_yield_expression"
        {
            return true;
        }

        // Also check text for keywords (some tree-sitter versions may not have dedicated nodes)
        if kind == "identifier" || kind == "expression_statement" {
            let text = self.node_text(node);
            if text == "co_await" || text == "co_return" || text == "co_yield" {
                return true;
            }
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if self.has_coroutine_keyword(child) {
                return true;
            }
        }
        false
    }

    fn is_const_method(&self, declarator: Node) -> bool {
        let mut cursor = declarator.walk();
        for child in declarator.children(&mut cursor) {
            if child.kind() == "type_qualifier" && self.node_text(child) == "const" {
                return true;
            }
        }
        false
    }

    fn extract_visibility(&self, node: Node) -> String {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "access_specifier" {
                let text = self.node_text(child);
                return text.trim_end_matches(':').to_string();
            }
        }
        "private".to_string()
    }

    fn extract_doc_comment(&self, node: Node) -> Option<String> {
        if let Some(prev) = node.prev_sibling() {
            if prev.kind() == "comment" {
                let text = self.node_text(prev);
                if text.starts_with("///") || text.starts_with("/**") {
                    return Some(text);
                }
            }
        }
        None
    }

    fn qualify_name(&self, name: &str) -> String {
        if self.current_namespace.is_empty() {
            name.to_string()
        } else {
            format!("{}::{}", self.current_namespace.join("::"), name)
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
            }
            "else_clause" => {
                builder.add_branch();
            }
            "for_statement" => {
                builder.add_loop();
                builder.enter_scope();
            }
            "for_range_loop" => {
                // C++ range-based for: for (auto x : container)
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
            "switch_statement" => {
                builder.enter_scope();
            }
            "case_statement" => {
                builder.add_branch();
            }
            "default_statement" => {
                builder.add_branch();
            }
            "conditional_expression" => {
                // Ternary operator ?:
                builder.add_branch();
            }
            "catch_clause" => {
                builder.add_exception_handler();
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
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_for_complexity(child, builder);
        }

        // Exit scope for control structures that entered one
        match node.kind() {
            "if_statement" | "for_statement" | "for_range_loop" | "while_statement"
            | "do_statement" | "switch_statement" => {
                builder.exit_scope();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::Parser;

    fn parse_and_visit(source: &[u8]) -> CppVisitor<'_> {
        let mut parser = Parser::new();
        let language = tree_sitter_cpp::language();
        parser.set_language(&language).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = CppVisitor::new(source);
        visitor.visit_node(tree.root_node());
        visitor
    }

    #[test]
    fn test_visitor_basics() {
        let visitor = CppVisitor::new(b"");
        assert_eq!(visitor.functions.len(), 0);
        assert_eq!(visitor.classes.len(), 0);
        assert_eq!(visitor.traits.len(), 0);
    }

    #[test]
    fn test_visitor_class_extraction() {
        let source = b"class Person { public: std::string name; };";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "Person");
    }

    #[test]
    fn test_visitor_struct_extraction() {
        let source = b"struct Point { int x; int y; };";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "Point");
        assert!(visitor.classes[0]
            .attributes
            .contains(&"struct".to_string()));
    }

    #[test]
    fn test_visitor_namespace() {
        let source = b"namespace myns { class MyClass {}; }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "myns::MyClass");
    }

    #[test]
    fn test_visitor_function_extraction() {
        let source = b"int add(int a, int b) { return a + b; }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "add");
    }

    #[test]
    fn test_visitor_include_extraction() {
        let source = b"#include <iostream>\n#include \"myheader.h\"";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 2);
        assert!(visitor.imports.iter().any(|i| i.imported == "iostream"));
        assert!(visitor.imports.iter().any(|i| i.imported == "myheader.h"));
    }

    #[test]
    fn test_visitor_inheritance() {
        let source = b"class Animal {};\nclass Dog : public Animal {};";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 2);
        assert_eq!(visitor.inheritance.len(), 1);
        assert_eq!(visitor.inheritance[0].child, "Dog");
        assert_eq!(visitor.inheritance[0].parent, "Animal");
    }

    #[test]
    fn test_visitor_enum() {
        let source = b"enum Color { Red, Green, Blue };";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "Color");
        assert!(visitor.classes[0].attributes.contains(&"enum".to_string()));
    }

    #[test]
    fn test_visitor_enum_class() {
        let source = b"enum class Status { Active, Inactive };";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert!(visitor.classes[0]
            .attributes
            .contains(&"enum_class".to_string()));
    }

    #[test]
    fn test_complexity_simple_function() {
        // A function with no control flow has CC=1
        let source = b"int add(int a, int b) { return a + b; }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert_eq!(complexity.cyclomatic_complexity, 1);
        assert_eq!(complexity.branches, 0);
        assert_eq!(complexity.loops, 0);
    }

    #[test]
    fn test_complexity_if_else_and_loop() {
        // if + else + for = 2 branches + 1 loop => CC = 1 + 2 + 1 = 4
        let source =
            b"void process(int x) { if (x > 0) { for (int i = 0; i < x; i++) {} } else {} }";
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
    fn test_complexity_nested_control_flow() {
        // Nested if inside while, plus logical operator => higher nesting depth
        let source = b"bool find(int* arr, int n, int val) { \
            while (n > 0) { \
                if (arr[n-1] == val || arr[0] == val) { return true; } \
                n--; \
            } \
            return false; \
        }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(complexity.loops >= 1);
        assert!(complexity.branches >= 1);
        assert!(complexity.logical_operators >= 1);
        assert!(complexity.max_nesting_depth >= 2);
        assert!(complexity.cyclomatic_complexity > 2);
    }

    #[test]
    fn test_complexity_range_based_for_and_catch() {
        // C++-specific: range-based for and catch clause
        let source = b"void run(std::vector<int>& v) { \
            try { \
                for (auto x : v) { if (x < 0) {} } \
            } catch (...) {} \
        }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(
            complexity.loops >= 1,
            "expected range-based for to count as a loop"
        );
        assert!(
            complexity.exception_handlers >= 1,
            "expected catch to count as exception handler"
        );
        assert!(complexity.branches >= 1);
    }

    #[test]
    fn test_complexity_method() {
        // Complexity is also captured for class methods
        let source = b"class Foo { int bar(int x) { if (x > 0) { return x; } return 0; } };";
        let visitor = parse_and_visit(source);

        // bar is extracted as a function with parent_class = "Foo"
        let bar = visitor.functions.iter().find(|f| f.name == "bar");
        assert!(bar.is_some(), "method 'bar' not found");
        let complexity = bar.unwrap().complexity.as_ref().unwrap();
        assert!(complexity.branches >= 1);
        assert!(complexity.cyclomatic_complexity > 1);
    }
}
