// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting Swift entities

use codegraph_parser_api::{
    CallRelation, ClassEntity, ComplexityBuilder, ComplexityMetrics, FunctionEntity,
    ImplementationRelation, ImportRelation, InheritanceRelation, Parameter, TraitEntity,
    BODY_PREFIX_MAX_CHARS,
};
use tree_sitter::Node;

pub struct SwiftVisitor<'a> {
    pub source: &'a [u8],
    pub functions: Vec<FunctionEntity>,
    pub classes: Vec<ClassEntity>,
    pub traits: Vec<TraitEntity>,
    pub imports: Vec<ImportRelation>,
    pub calls: Vec<CallRelation>,
    pub inheritance: Vec<InheritanceRelation>,
    pub implementations: Vec<ImplementationRelation>,
    current_class: Option<String>,
    current_function: Option<String>,
}

impl<'a> SwiftVisitor<'a> {
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
            current_class: None,
            current_function: None,
        }
    }

    fn node_text(&self, node: Node) -> String {
        node.utf8_text(self.source).unwrap_or("").to_string()
    }

    /// Determines the type of declaration (class, struct, enum) based on keyword child
    fn get_declaration_type(&self, node: Node) -> String {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "struct" => return "struct".to_string(),
                "enum" => return "enum".to_string(),
                "class" => return "class".to_string(),
                _ => continue,
            }
        }
        "class".to_string() // default
    }

    pub fn visit_node(&mut self, node: Node) {
        let should_recurse = match node.kind() {
            "class_declaration" => {
                // tree-sitter-swift uses class_declaration for class, struct, and enum
                // We need to check the first child to determine the type
                let declaration_type = self.get_declaration_type(node);
                match declaration_type.as_str() {
                    "struct" => self.visit_struct(node),
                    "enum" => self.visit_enum(node),
                    _ => self.visit_class(node), // "class" or other
                }
                false
            }
            "struct_declaration" => {
                self.visit_struct(node);
                false
            }
            "protocol_declaration" => {
                self.visit_protocol(node);
                false
            }
            "function_declaration" => {
                if self.current_class.is_none() {
                    self.visit_function(node);
                }
                false
            }
            "import_declaration" => {
                self.visit_import(node);
                false
            }
            "enum_declaration" => {
                self.visit_enum(node);
                false
            }
            "extension_declaration" => {
                self.visit_extension(node);
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

    fn visit_class(&mut self, node: Node) {
        let name = self.extract_type_name(node);
        if name.is_none() {
            return;
        }
        let name = name.unwrap();

        let previous_class = self.current_class.take();
        self.current_class = Some(name.clone());

        let mut base_classes = Vec::new();
        let mut implemented_traits = Vec::new();
        self.extract_inheritance(node, &name, &mut base_classes, &mut implemented_traits);

        let type_params = self.extract_generic_params(node);
        let doc_comment = self.extract_doc_comment(node);
        let visibility = self.extract_visibility(node);

        let body_node = node.child_by_field_name("body").or_else(|| {
            let mut cursor = node.walk();
            let found = node
                .children(&mut cursor)
                .find(|c| c.kind() == "class_body");
            found
        });
        let body_prefix = body_node
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
            name: name.clone(),
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
            attributes: Vec::new(),
            type_parameters: type_params,
            body_prefix,
        };

        self.classes.push(class_entity);

        // Visit class body
        if let Some(body) = node.child_by_field_name("body") {
            self.visit_class_body(body);
        } else {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "class_body" {
                    self.visit_class_body(child);
                }
            }
        }

        self.current_class = previous_class;
    }

    fn visit_struct(&mut self, node: Node) {
        let name = self.extract_type_name(node);
        if name.is_none() {
            return;
        }
        let name = name.unwrap();

        let previous_class = self.current_class.take();
        self.current_class = Some(name.clone());

        let mut base_classes = Vec::new();
        let mut implemented_traits = Vec::new();
        self.extract_inheritance(node, &name, &mut base_classes, &mut implemented_traits);

        let type_params = self.extract_generic_params(node);
        let doc_comment = self.extract_doc_comment(node);
        let visibility = self.extract_visibility(node);

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
            name: name.clone(),
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
            attributes: vec!["struct".to_string()],
            type_parameters: type_params,
            body_prefix,
        };

        self.classes.push(class_entity);

        // Visit struct body
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "class_body" || child.kind() == "struct_body" {
                self.visit_class_body(child);
            }
        }

        self.current_class = previous_class;
    }

    fn visit_protocol(&mut self, node: Node) {
        let name = self.extract_type_name(node);
        if name.is_none() {
            return;
        }
        let name = name.unwrap();

        let doc_comment = self.extract_doc_comment(node);
        let visibility = self.extract_visibility(node);

        let mut required_methods = Vec::new();

        // Extract protocol methods
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "protocol_body" {
                let mut body_cursor = child.walk();
                for body_child in child.children(&mut body_cursor) {
                    if body_child.kind() == "protocol_function_declaration"
                        || body_child.kind() == "function_declaration"
                    {
                        if let Some(method) = self.extract_function_signature(body_child) {
                            required_methods.push(method);
                        }
                    }
                }
            }
        }

        let trait_entity = TraitEntity {
            name,
            visibility,
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            doc_comment,
            required_methods,
            parent_traits: Vec::new(),
            attributes: Vec::new(),
        };

        self.traits.push(trait_entity);
    }

    fn visit_class_body(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "function_declaration" => self.visit_method(child),
                "subscript_declaration" => self.visit_method(child),
                "init_declaration" | "initializer_declaration" => self.visit_init(child),
                "deinit_declaration" => self.visit_deinit(child),
                "class_declaration" => self.visit_class(child),
                "struct_declaration" => self.visit_struct(child),
                "enum_declaration" => self.visit_enum(child),
                _ => {}
            }
        }
    }

    fn visit_function(&mut self, node: Node) {
        let name = self.extract_function_name(node);
        if name.is_none() {
            return;
        }
        let name = name.unwrap();

        let params = self.extract_parameters(node);
        let return_type = self.extract_return_type(node);
        let visibility = self.extract_visibility(node);
        let is_static = self.has_modifier(node, "static");
        let is_async = self.has_modifier(node, "async");
        let doc_comment = self.extract_doc_comment(node);

        // Calculate complexity from function body
        let complexity = self.find_body_and_calculate_complexity(node);

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
        func.is_async = is_async;
        func.doc_comment = doc_comment;
        func.parent_class = self.current_class.clone();
        func.complexity = complexity;
        func.body_prefix = {
            let mut cursor = node.walk();
            let found = node
                .children(&mut cursor)
                .find(|c| c.kind() == "function_body" || c.kind() == "code_block");
            found
        }
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

        self.functions.push(func);

        // Track function context for call extraction
        let previous_function = self.current_function.take();
        self.current_function = Some(name);

        // Visit function body
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "function_body" || child.kind() == "code_block" {
                self.visit_function_body(child);
            }
        }

        self.current_function = previous_function;
    }

    fn visit_method(&mut self, node: Node) {
        let name = self.extract_function_name(node);
        if name.is_none() {
            return;
        }
        let name = name.unwrap();

        let params = self.extract_parameters(node);
        let return_type = self.extract_return_type(node);
        let visibility = self.extract_visibility(node);
        let is_static = self.has_modifier(node, "static") || self.has_modifier(node, "class");
        let is_async = self.has_modifier(node, "async");
        let doc_comment = self.extract_doc_comment(node);

        // Calculate complexity from method body
        let complexity = self.find_body_and_calculate_complexity(node);

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
        func.is_async = is_async;
        func.doc_comment = doc_comment;
        func.parent_class = self.current_class.clone();
        func.complexity = complexity;
        func.body_prefix = {
            let mut cursor = node.walk();
            let found = node
                .children(&mut cursor)
                .find(|c| c.kind() == "function_body" || c.kind() == "code_block");
            found
        }
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

        if self.has_modifier(node, "override") {
            func.attributes.push("override".to_string());
        }
        if self.has_modifier(node, "mutating") {
            func.attributes.push("mutating".to_string());
        }

        self.functions.push(func);

        // Track function context for call extraction
        let previous_function = self.current_function.take();
        self.current_function = Some(name);

        // Visit function body
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "function_body" || child.kind() == "code_block" {
                self.visit_function_body(child);
            }
        }

        self.current_function = previous_function;
    }

    fn visit_init(&mut self, node: Node) {
        let name = "init".to_string();

        let params = self.extract_parameters(node);
        let visibility = self.extract_visibility(node);
        let doc_comment = self.extract_doc_comment(node);

        // Calculate complexity from init body
        let complexity = self.find_body_and_calculate_complexity(node);

        let mut func = FunctionEntity::new(
            &name,
            node.start_position().row + 1,
            node.end_position().row + 1,
        )
        .with_visibility(&visibility)
        .with_signature(self.node_text(node).lines().next().unwrap_or(""));

        func.parameters = params;
        func.doc_comment = doc_comment;
        func.parent_class = self.current_class.clone();
        func.complexity = complexity;
        func.body_prefix = {
            let mut cursor = node.walk();
            let found = node
                .children(&mut cursor)
                .find(|c| c.kind() == "function_body" || c.kind() == "code_block");
            found
        }
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
        func.attributes.push("init".to_string());

        if self.has_modifier(node, "convenience") {
            func.attributes.push("convenience".to_string());
        }
        if self.has_modifier(node, "required") {
            func.attributes.push("required".to_string());
        }

        self.functions.push(func);

        // Track function context for call extraction
        let previous_function = self.current_function.take();
        self.current_function = Some(name);

        // Visit init body
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "function_body" || child.kind() == "code_block" {
                self.visit_function_body(child);
            }
        }

        self.current_function = previous_function;
    }

    fn visit_deinit(&mut self, node: Node) {
        let name = "deinit".to_string();
        let doc_comment = self.extract_doc_comment(node);

        let mut func = FunctionEntity::new(
            &name,
            node.start_position().row + 1,
            node.end_position().row + 1,
        )
        .with_visibility("internal")
        .with_signature(self.node_text(node).lines().next().unwrap_or(""));

        func.doc_comment = doc_comment;
        func.parent_class = self.current_class.clone();
        func.body_prefix = {
            let mut cursor = node.walk();
            let found = node
                .children(&mut cursor)
                .find(|c| c.kind() == "function_body" || c.kind() == "code_block");
            found
        }
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
        func.attributes.push("deinit".to_string());

        self.functions.push(func);
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
        // Try to get the function being called
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "simple_identifier" | "identifier" => {
                    return self.node_text(child);
                }
                "navigation_expression" => {
                    // Get the last identifier in a.b.c()
                    if let Some(suffix) = child.child_by_field_name("suffix") {
                        return self.node_text(suffix);
                    }
                    let mut inner_cursor = child.walk();
                    let mut last_id = String::new();
                    for inner_child in child.children(&mut inner_cursor) {
                        if inner_child.kind() == "simple_identifier"
                            || inner_child.kind() == "identifier"
                        {
                            last_id = self.node_text(inner_child);
                        }
                    }
                    if !last_id.is_empty() {
                        return last_id;
                    }
                }
                _ => {}
            }
        }
        String::new()
    }

    fn visit_enum(&mut self, node: Node) {
        let name = self.extract_type_name(node);
        if name.is_none() {
            return;
        }
        let name = name.unwrap();

        let doc_comment = self.extract_doc_comment(node);
        let visibility = self.extract_visibility(node);
        let type_params = self.extract_generic_params(node);

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
            name,
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
            type_parameters: type_params,
            body_prefix,
        };

        self.classes.push(class_entity);
    }

    fn visit_extension(&mut self, node: Node) {
        // Extract the type being extended
        let extended_type = self.extract_extension_type(node);
        if extended_type.is_none() {
            return;
        }
        let extended_type = extended_type.unwrap();

        let previous_class = self.current_class.take();
        self.current_class = Some(extended_type.clone());

        // Check if extension conforms to protocols
        let mut implemented_traits = Vec::new();
        self.extract_extension_protocols(node, &extended_type, &mut implemented_traits);

        // Visit extension body for methods
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "extension_body" || child.kind() == "class_body" {
                self.visit_class_body(child);
            }
        }

        self.current_class = previous_class;
    }

    fn visit_import(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "identifier" || child.kind() == "simple_identifier" {
                let import = ImportRelation {
                    importer: "file".to_string(),
                    imported: self.node_text(child),
                    symbols: Vec::new(),
                    is_wildcard: true,
                    alias: None,
                };
                self.imports.push(import);
                return;
            }
        }
    }

    // Helper methods

    fn extract_type_name(&self, node: Node) -> Option<String> {
        if let Some(name_node) = node.child_by_field_name("name") {
            return Some(self.node_text(name_node));
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "type_identifier"
                || child.kind() == "simple_identifier"
                || child.kind() == "identifier"
            {
                return Some(self.node_text(child));
            }
        }
        None
    }

    fn extract_function_name(&self, node: Node) -> Option<String> {
        if let Some(name_node) = node.child_by_field_name("name") {
            return Some(self.node_text(name_node));
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "simple_identifier" || child.kind() == "identifier" {
                return Some(self.node_text(child));
            }
        }
        None
    }

    fn extract_function_signature(&self, node: Node) -> Option<FunctionEntity> {
        let name = self.extract_function_name(node)?;
        let params = self.extract_parameters(node);
        let return_type = self.extract_return_type(node);

        let mut func = FunctionEntity::new(
            &name,
            node.start_position().row + 1,
            node.end_position().row + 1,
        )
        .with_signature(self.node_text(node).lines().next().unwrap_or(""));

        func.parameters = params;
        func.return_type = return_type;
        func.is_abstract = true;
        func.body_prefix = None;

        Some(func)
    }

    fn extract_parameters(&self, node: Node) -> Vec<Parameter> {
        let mut params = Vec::new();

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "parameter"
                || child.kind() == "function_parameter"
                || child.kind() == "parameter_clause"
            {
                if child.kind() == "parameter_clause" {
                    let mut param_cursor = child.walk();
                    for param_child in child.children(&mut param_cursor) {
                        if param_child.kind() == "parameter" {
                            if let Some(param) = self.extract_single_parameter(param_child) {
                                params.push(param);
                            }
                        }
                    }
                } else if let Some(param) = self.extract_single_parameter(child) {
                    params.push(param);
                }
            }
        }

        params
    }

    fn extract_single_parameter(&self, node: Node) -> Option<Parameter> {
        let mut name = String::new();
        let mut param_type = String::new();

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "simple_identifier" | "identifier" => {
                    if name.is_empty() {
                        name = self.node_text(child);
                    }
                }
                "type_annotation" => {
                    let mut type_cursor = child.walk();
                    for type_child in child.children(&mut type_cursor) {
                        if type_child.kind() != ":" {
                            param_type = self.node_text(type_child);
                            break;
                        }
                    }
                }
                _ => {}
            }
        }

        if name.is_empty() {
            return None;
        }

        let mut param = Parameter::new(name);
        if !param_type.is_empty() {
            param = param.with_type(param_type);
        }

        Some(param)
    }

    fn extract_return_type(&self, node: Node) -> Option<String> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "function_result" || child.kind() == "type_annotation" {
                let mut type_cursor = child.walk();
                for type_child in child.children(&mut type_cursor) {
                    if type_child.kind() != "->" && type_child.kind() != ":" {
                        let type_str = self.node_text(type_child);
                        if type_str != "Void" && !type_str.is_empty() {
                            return Some(type_str);
                        }
                    }
                }
            }
        }
        None
    }

    fn extract_inheritance(
        &mut self,
        node: Node,
        class_name: &str,
        base_classes: &mut Vec<String>,
        implemented_traits: &mut Vec<String>,
    ) {
        let mut cursor = node.walk();
        let mut order = 0;
        for child in node.children(&mut cursor) {
            if child.kind() == "inheritance_specifier" || child.kind() == "type_inheritance_clause"
            {
                let mut inherit_cursor = child.walk();
                for inherit_child in child.children(&mut inherit_cursor) {
                    if inherit_child.kind() == "type_identifier"
                        || inherit_child.kind() == "user_type"
                        || inherit_child.kind() == "simple_identifier"
                    {
                        let parent_name = self.node_text(inherit_child);

                        // In Swift, first inheritance is usually superclass, rest are protocols
                        if order == 0 {
                            base_classes.push(parent_name.clone());
                            self.inheritance.push(InheritanceRelation {
                                child: class_name.to_string(),
                                parent: parent_name,
                                order,
                            });
                        } else {
                            implemented_traits.push(parent_name.clone());
                            self.implementations.push(ImplementationRelation {
                                implementor: class_name.to_string(),
                                trait_name: parent_name,
                            });
                        }
                        order += 1;
                    }
                }
            }
        }
    }

    fn extract_extension_type(&self, node: Node) -> Option<String> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "type_identifier"
                || child.kind() == "user_type"
                || child.kind() == "simple_identifier"
            {
                return Some(self.node_text(child));
            }
        }
        None
    }

    fn extract_extension_protocols(
        &mut self,
        node: Node,
        extended_type: &str,
        implemented_traits: &mut Vec<String>,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "type_inheritance_clause" {
                let mut inherit_cursor = child.walk();
                for inherit_child in child.children(&mut inherit_cursor) {
                    if inherit_child.kind() == "type_identifier"
                        || inherit_child.kind() == "user_type"
                    {
                        let protocol_name = self.node_text(inherit_child);
                        implemented_traits.push(protocol_name.clone());
                        self.implementations.push(ImplementationRelation {
                            implementor: extended_type.to_string(),
                            trait_name: protocol_name,
                        });
                    }
                }
            }
        }
    }

    fn extract_generic_params(&self, node: Node) -> Vec<String> {
        let mut params = Vec::new();

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "type_parameters" || child.kind() == "generic_parameter_clause" {
                let mut param_cursor = child.walk();
                for param_child in child.children(&mut param_cursor) {
                    if param_child.kind() == "type_parameter"
                        || param_child.kind() == "simple_identifier"
                    {
                        params.push(self.node_text(param_child));
                    }
                }
            }
        }

        params
    }

    fn extract_visibility(&self, node: Node) -> String {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "modifiers" || child.kind() == "modifier" {
                let text = self.node_text(child);
                if text.contains("public") {
                    return "public".to_string();
                } else if text.contains("private") {
                    return "private".to_string();
                } else if text.contains("fileprivate") {
                    return "fileprivate".to_string();
                } else if text.contains("internal") {
                    return "internal".to_string();
                } else if text.contains("open") {
                    return "open".to_string();
                }
            }
            if child.kind() == "visibility_modifier" {
                return self.node_text(child);
            }
        }
        "internal".to_string() // Swift default visibility
    }

    fn has_modifier(&self, node: Node, modifier: &str) -> bool {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "modifiers" || child.kind() == "modifier" {
                let text = self.node_text(child);
                if text.contains(modifier) {
                    return true;
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

    /// Find the function body child node and calculate complexity from it.
    /// Returns None if no body node is found (e.g., abstract/protocol declarations).
    fn find_body_and_calculate_complexity(&self, node: Node) -> Option<ComplexityMetrics> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "function_body" || child.kind() == "code_block" {
                return Some(self.calculate_complexity(child));
            }
        }
        None
    }

    fn visit_for_complexity(&self, node: Node, builder: &mut ComplexityBuilder) {
        match node.kind() {
            "if_statement" => {
                builder.add_branch();
                builder.enter_scope();
            }
            "for_statement" => {
                builder.add_loop();
                builder.enter_scope();
            }
            "while_statement" => {
                builder.add_loop();
                builder.enter_scope();
            }
            "repeat_while_statement" => {
                builder.add_loop();
                builder.enter_scope();
            }
            "switch_statement" => {
                builder.enter_scope();
            }
            "switch_entry" => {
                // Each case (including default) is a branch
                builder.add_branch();
            }
            "guard_statement" => {
                // guard adds a branch (the else path)
                builder.add_branch();
                builder.enter_scope();
            }
            "ternary_expression" => {
                builder.add_branch();
            }
            "conjunction_expression" => {
                // a && b
                builder.add_logical_operator();
            }
            "disjunction_expression" => {
                // a || b
                builder.add_logical_operator();
            }
            "catch_block" => {
                builder.add_exception_handler();
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_for_complexity(child, builder);
        }

        // Exit scope for control structures that opened one
        match node.kind() {
            "if_statement"
            | "for_statement"
            | "while_statement"
            | "repeat_while_statement"
            | "switch_statement"
            | "guard_statement" => {
                builder.exit_scope();
            }
            _ => {}
        }
    }

    fn extract_doc_comment(&self, node: Node) -> Option<String> {
        if let Some(prev) = node.prev_sibling() {
            if prev.kind() == "comment" || prev.kind() == "multiline_comment" {
                let text = self.node_text(prev);
                if text.starts_with("///") || text.starts_with("/**") {
                    return Some(text);
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::Parser;

    fn parse_and_visit(source: &[u8]) -> SwiftVisitor<'_> {
        let mut parser = Parser::new();
        let language = tree_sitter_swift::language();
        parser.set_language(&language).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = SwiftVisitor::new(source);
        visitor.visit_node(tree.root_node());
        visitor
    }

    #[test]
    fn test_visitor_basics() {
        let visitor = SwiftVisitor::new(b"");
        assert_eq!(visitor.functions.len(), 0);
        assert_eq!(visitor.classes.len(), 0);
        assert_eq!(visitor.traits.len(), 0);
    }

    #[test]
    fn test_visitor_class_extraction() {
        let source = b"class Person { var name: String = \"\" }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "Person");
    }

    #[test]
    fn test_visitor_struct_extraction() {
        let source = b"struct Point { var x: Int; var y: Int }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "Point");
        assert!(visitor.classes[0]
            .attributes
            .contains(&"struct".to_string()));
    }

    #[test]
    fn test_visitor_protocol_extraction() {
        let source = b"protocol Drawable { func draw() }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.traits.len(), 1);
        assert_eq!(visitor.traits[0].name, "Drawable");
    }

    #[test]
    fn test_visitor_function_extraction() {
        let source = b"func greet(name: String) -> String { return name }";
        let visitor = parse_and_visit(source);

        assert!(!visitor.functions.is_empty());
    }

    #[test]
    fn test_visitor_import_extraction() {
        let source = b"import Foundation\nimport UIKit";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 2);
    }

    #[test]
    fn test_visitor_inheritance() {
        let source = b"class Animal {}\nclass Dog: Animal {}";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 2);
        assert_eq!(visitor.inheritance.len(), 1);
        assert_eq!(visitor.inheritance[0].child, "Dog");
        assert_eq!(visitor.inheritance[0].parent, "Animal");
    }

    #[test]
    fn test_visitor_enum() {
        let source = b"enum Color { case red, green, blue }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "Color");
        assert!(visitor.classes[0].attributes.contains(&"enum".to_string()));
    }

    // Complexity tests

    #[test]
    fn test_complexity_simple_function() {
        // A function with no control flow has CC=1
        let source = b"func simple(x: Int) -> Int { return x + 1 }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert_eq!(complexity.cyclomatic_complexity, 1);
        assert_eq!(complexity.branches, 0);
        assert_eq!(complexity.loops, 0);
        assert_eq!(complexity.logical_operators, 0);
    }

    #[test]
    fn test_complexity_if_else_and_for_in_loop() {
        // if + else-if + else + for-in loop: branches=2, loops=1 => CC=1+2+1=4
        let source = br#"
func process(x: Int) {
    if x > 0 {
        print("positive")
    } else if x < 0 {
        print("negative")
    } else {
        print("zero")
    }
    for i in 0..<x {
        print(i)
    }
}
"#;
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        // Two if_statement nodes (if x>0 and else-if x<0), one for_statement
        assert!(
            complexity.branches >= 2,
            "Expected >= 2 branches, got {}",
            complexity.branches
        );
        assert!(
            complexity.loops >= 1,
            "Expected >= 1 loop, got {}",
            complexity.loops
        );
        assert!(complexity.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_complexity_guard_and_switch() {
        // guard (branch) + switch with 3 entries (3 branches) => branches=4, CC=5
        let source = br#"
func classify(x: Int?) -> String {
    guard let val = x else { return "nil" }
    switch val {
    case 1:
        return "one"
    case 2:
        return "two"
    default:
        return "other"
    }
}
"#;
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        // 1 guard + 3 switch_entry nodes
        assert!(
            complexity.branches >= 4,
            "Expected >= 4 branches, got {}",
            complexity.branches
        );
        assert!(complexity.cyclomatic_complexity >= 5);
    }

    #[test]
    fn test_complexity_logical_operators() {
        // a && b counts as conjunction_expression, a || b as disjunction_expression
        let source = br#"
func check(a: Bool, b: Bool, c: Bool) -> Bool {
    return a && b || c
}
"#;
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(
            complexity.logical_operators >= 2,
            "Expected >= 2 logical operators, got {}",
            complexity.logical_operators
        );
    }

    #[test]
    fn test_complexity_catch_block() {
        // A do-catch adds one exception_handler
        let source = br#"
func risky() {
    do {
        print("try")
    } catch {
        print("error")
    }
}
"#;
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(
            complexity.exception_handlers >= 1,
            "Expected >= 1 exception handler, got {}",
            complexity.exception_handlers
        );
    }

    #[test]
    fn test_complexity_ternary() {
        // A ternary expression adds a branch
        let source = br#"
func sign(x: Int) -> String {
    return x > 0 ? "positive" : "non-positive"
}
"#;
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(
            complexity.branches >= 1,
            "Expected >= 1 branch from ternary, got {}",
            complexity.branches
        );
    }

    #[test]
    fn test_complexity_repeat_while() {
        // repeat-while adds a loop
        let source = br#"
func countDown() {
    var x = 10
    repeat {
        x -= 1
    } while x > 0
}
"#;
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(
            complexity.loops >= 1,
            "Expected >= 1 loop from repeat-while, got {}",
            complexity.loops
        );
    }

    #[test]
    fn test_complexity_nesting_depth() {
        // Nested if inside for inside if => depth >= 3
        let source = br#"
func nested(x: Int) {
    if x > 0 {
        for i in 0..<x {
            if i % 2 == 0 {
                print(i)
            }
        }
    }
}
"#;
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(
            complexity.max_nesting_depth >= 3,
            "Expected nesting depth >= 3, got {}",
            complexity.max_nesting_depth
        );
    }

    #[test]
    fn test_complexity_method_in_class() {
        // Methods in a class also get complexity scored
        let source = br#"
class Calculator {
    func compute(x: Int, y: Int) -> Int {
        if x > y {
            return x - y
        } else {
            return y - x
        }
    }
}
"#;
        let visitor = parse_and_visit(source);

        // The method should be extracted with complexity
        let method = visitor.functions.iter().find(|f| f.name == "compute");
        assert!(method.is_some(), "compute method not found");
        let complexity = method.unwrap().complexity.as_ref().unwrap();
        assert!(
            complexity.branches >= 1,
            "Expected >= 1 branch, got {}",
            complexity.branches
        );
        assert!(complexity.cyclomatic_complexity > 1);
    }
}
