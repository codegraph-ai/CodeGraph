// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting Rust entities using tree-sitter
//!
//! This module implements a tree-sitter based visitor that walks the Rust AST
//! and extracts functions, structs, enums, traits, and their relationships.

use codegraph_parser_api::{
    CallRelation, ClassEntity, ComplexityBuilder, ComplexityMetrics, Field, FunctionEntity,
    ImplementationRelation, ImportRelation, InheritanceRelation, Parameter, ParserConfig,
    TraitEntity, TypeReference, BODY_PREFIX_MAX_CHARS,
    truncate_body_prefix,
};
use tree_sitter::Node;

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Visitor that extracts entities and relationships from Rust AST
pub struct RustVisitor<'a> {
    pub source: &'a [u8],
    pub config: ParserConfig,
    pub functions: Vec<FunctionEntity>,
    pub classes: Vec<ClassEntity>,
    pub traits: Vec<TraitEntity>,
    pub imports: Vec<ImportRelation>,
    pub calls: Vec<CallRelation>,
    pub implementations: Vec<ImplementationRelation>,
    pub inheritance: Vec<InheritanceRelation>,
    pub type_references: Vec<TypeReference>,
    current_class: Option<String>,
    current_function: Option<String>,
}

impl<'a> RustVisitor<'a> {
    pub fn new(source: &'a [u8], config: ParserConfig) -> Self {
        Self {
            source,
            config,
            functions: Vec::new(),
            classes: Vec::new(),
            traits: Vec::new(),
            imports: Vec::new(),
            calls: Vec::new(),
            implementations: Vec::new(),
            inheritance: Vec::new(),
            type_references: Vec::new(),
            current_class: None,
            current_function: None,
        }
    }

    /// Get text from a node
    fn node_text(&self, node: Node) -> String {
        node.utf8_text(self.source).unwrap_or("").to_string()
    }

    /// Main visitor entry point
    pub fn visit_node(&mut self, node: Node) {
        match node.kind() {
            "function_item" => {
                // Only visit top-level functions (not inside impl/trait blocks)
                if self.current_class.is_none() {
                    self.visit_function(node);
                    // Don't recurse — visit_function handles body for call extraction
                    return;
                }
            }
            "struct_item" => self.visit_struct(node),
            "enum_item" => self.visit_enum(node),
            "trait_item" => {
                self.visit_trait(node);
                // Don't recurse into trait body - methods already extracted
                return;
            }
            "impl_item" => {
                self.visit_impl(node);
                // Don't recurse into impl body - methods already extracted
                return;
            }
            "use_declaration" => self.visit_use(node),
            "mod_item" => self.visit_mod(node),
            "call_expression" => self.visit_call_expression(node),
            _ => {}
        }

        // Recursively visit children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child);
        }
    }

    /// Extract visibility from a visibility_modifier node
    fn extract_visibility(&self, node: Node) -> String {
        // Look for visibility_modifier child
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "visibility_modifier" {
                let text = self.node_text(child);
                if text.starts_with("pub(crate)") {
                    return "internal".to_string();
                } else if text.starts_with("pub(super)") {
                    return "protected".to_string();
                } else if text.starts_with("pub") {
                    return "public".to_string();
                }
            }
        }
        "private".to_string()
    }

    /// Check if a function has the #[test] attribute
    fn has_test_attribute(&self, node: Node) -> bool {
        // First, check for attributes as previous siblings (e.g., #[test] before fn)
        let mut current = node;
        while let Some(prev) = current.prev_sibling() {
            if prev.kind() == "attribute_item" {
                let attr_text = self.node_text(prev);
                if attr_text.contains("test") {
                    return true;
                }
            } else {
                // Stop when we hit a non-attribute node
                break;
            }
            current = prev;
        }

        // Also check children (for inner attributes)
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "attribute_item" || child.kind() == "attribute" {
                let attr_text = self.node_text(child);
                if attr_text.contains("test") {
                    return true;
                }
            }
        }
        false
    }

    /// Check if function has async keyword
    fn is_async(&self, node: Node) -> bool {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "async" || self.node_text(child) == "async" {
                return true;
            }
        }
        false
    }

    /// Extract doc comments (/// or //!) from preceding nodes
    fn extract_doc_comment(&self, node: Node) -> Option<String> {
        let mut docs = Vec::new();
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            if child.kind() == "attribute_item" {
                let text = self.node_text(child);
                // Check for #[doc = "..."] style attributes
                if text.contains("doc") {
                    if let Some(start) = text.find('"') {
                        if let Some(end) = text.rfind('"') {
                            if start < end {
                                docs.push(text[start + 1..end].to_string());
                            }
                        }
                    }
                }
            } else if child.kind() == "line_comment" {
                let text = self.node_text(child);
                if let Some(rest) = text
                    .strip_prefix("///")
                    .or_else(|| text.strip_prefix("//!"))
                {
                    docs.push(rest.trim().to_string());
                }
            }
        }

        if docs.is_empty() {
            None
        } else {
            Some(docs.join("\n"))
        }
    }

    /// Extract parameters from a function's parameter list
    fn extract_parameters(&self, node: Node) -> Vec<Parameter> {
        let mut params = Vec::new();

        if let Some(params_node) = node.child_by_field_name("parameters") {
            let mut cursor = params_node.walk();
            for child in params_node.children(&mut cursor) {
                match child.kind() {
                    "self_parameter" => {
                        params.push(Parameter {
                            name: "self".to_string(),
                            type_annotation: Some("Self".to_string()),
                            default_value: None,
                            is_variadic: false,
                        });
                    }
                    "parameter" => {
                        let name = child
                            .child_by_field_name("pattern")
                            .map(|n| self.node_text(n))
                            .unwrap_or_else(|| "unknown".to_string());

                        let type_annotation =
                            child.child_by_field_name("type").map(|n| self.node_text(n));

                        params.push(Parameter {
                            name,
                            type_annotation,
                            default_value: None,
                            is_variadic: false,
                        });
                    }
                    _ => {}
                }
            }
        }

        params
    }

    /// Extract return type from function signature
    fn extract_return_type(&self, node: Node) -> Option<String> {
        node.child_by_field_name("return_type").map(|n| {
            self.node_text(n)
                .trim_start_matches("->")
                .trim()
                .to_string()
        })
    }

    /// Extract the first line as signature
    fn extract_signature(&self, node: Node) -> String {
        self.node_text(node)
            .lines()
            .next()
            .unwrap_or("")
            .to_string()
    }

    /// Extract type parameters from generics
    fn extract_type_parameters(&self, node: Node) -> Vec<String> {
        let mut params = Vec::new();

        if let Some(type_params) = node.child_by_field_name("type_parameters") {
            let mut cursor = type_params.walk();
            for child in type_params.children(&mut cursor) {
                match child.kind() {
                    "type_identifier" => {
                        params.push(self.node_text(child));
                    }
                    "type_parameter" => {
                        // tree-sitter-rust 0.24+: type_parameter wraps the type_identifier
                        if let Some(name) = child.child_by_field_name("name") {
                            params.push(self.node_text(name));
                        }
                    }
                    "constrained_type_parameter" => {
                        // Get just the type name from T: Trait
                        if let Some(name) = child.child_by_field_name("left") {
                            params.push(self.node_text(name));
                        }
                    }
                    _ => {}
                }
            }
        }

        params
    }

    /// Visit a function declaration
    fn visit_function(&mut self, node: Node) {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_else(|| "anonymous".to_string());

        // Set current function context for call extraction
        let previous_function = self.current_function.clone();
        self.current_function = Some(name.clone());

        let visibility = self.extract_visibility(node);

        // Skip private functions if configured
        if self.config.skip_private && visibility == "private" {
            self.current_function = previous_function;
            return;
        }

        let is_test = self.has_test_attribute(node);

        // Skip test functions if configured
        if self.config.skip_tests && is_test {
            self.current_function = previous_function;
            return;
        }

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
            name: name.clone(),
            signature: self.extract_signature(node),
            visibility,
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_async: self.is_async(node),
            is_test,
            is_static: false,
            is_abstract: false,
            parameters: self.extract_parameters(node),
            return_type: self.extract_return_type(node),
            doc_comment: self.extract_doc_comment(node),
            attributes: Vec::new(),
            parent_class: self.current_class.clone(),
            complexity,
            body_prefix,
        };
        self.functions.push(func);

        // Extract type references from parameter and return type annotations
        self.extract_type_refs_from_function(&name, node);

        // Visit body for call extraction
        if let Some(body) = node.child_by_field_name("body") {
            self.visit_body_for_calls(body);
        }

        self.current_function = previous_function;
    }

    /// Visit a struct declaration
    fn visit_struct(&mut self, node: Node) {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_else(|| "Struct".to_string());

        let visibility = self.extract_visibility(node);

        // Skip private structs if configured
        if self.config.skip_private && visibility == "private" {
            return;
        }

        // Extract fields from field_declaration_list
        let mut fields = Vec::new();
        let line = node.start_position().row + 1;
        if let Some(body) = node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for child in body.children(&mut cursor) {
                if child.kind() == "field_declaration" {
                    let field_name = child
                        .child_by_field_name("name")
                        .map(|n| self.node_text(n))
                        .unwrap_or_else(|| "unnamed".to_string());

                    let field_type = child.child_by_field_name("type").map(|n| self.node_text(n));

                    // Extract type references from field type annotations
                    if let Some(type_node) = child.child_by_field_name("type") {
                        for type_name in self.extract_type_names(type_node) {
                            self.type_references.push(TypeReference::new(
                                name.clone(),
                                type_name,
                                line,
                            ));
                        }
                    }

                    let field_vis = self.extract_visibility(child);

                    fields.push(Field {
                        name: field_name,
                        type_annotation: field_type,
                        visibility: field_vis,
                        is_static: false,
                        is_constant: false,
                        default_value: None,
                    });
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
            name: name.clone(),
            visibility,
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_abstract: false,
            is_interface: false,
            base_classes: Vec::new(),
            implemented_traits: Vec::new(),
            methods: Vec::new(),
            fields,
            doc_comment: self.extract_doc_comment(node),
            attributes: Vec::new(),
            type_parameters: self.extract_type_parameters(node),
            body_prefix,
        };
        self.classes.push(class);
    }

    /// Visit an enum declaration
    fn visit_enum(&mut self, node: Node) {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_else(|| "Enum".to_string());

        let visibility = self.extract_visibility(node);

        // Skip private enums if configured
        if self.config.skip_private && visibility == "private" {
            return;
        }

        // Treat enums as classes with an "enum" attribute
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
            visibility,
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_abstract: false,
            is_interface: false,
            base_classes: Vec::new(),
            implemented_traits: Vec::new(),
            methods: Vec::new(),
            fields: Vec::new(),
            doc_comment: self.extract_doc_comment(node),
            attributes: vec!["enum".to_string()],
            type_parameters: self.extract_type_parameters(node),
            body_prefix,
        };
        self.classes.push(class);
    }

    /// Visit a trait declaration
    fn visit_trait(&mut self, node: Node) {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_else(|| "Trait".to_string());

        let visibility = self.extract_visibility(node);

        // Skip private traits if configured
        if self.config.skip_private && visibility == "private" {
            return;
        }

        // Extract required methods from the trait body
        let mut required_methods = Vec::new();
        if let Some(body) = node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for child in body.children(&mut cursor) {
                if child.kind() == "function_signature_item" || child.kind() == "function_item" {
                    let method_name = child
                        .child_by_field_name("name")
                        .map(|n| self.node_text(n))
                        .unwrap_or_else(|| "method".to_string());

                    let func = FunctionEntity {
                        name: method_name,
                        signature: self.extract_signature(child),
                        visibility: "public".to_string(),
                        line_start: child.start_position().row + 1,
                        line_end: child.end_position().row + 1,
                        is_async: self.is_async(child),
                        is_test: false,
                        is_static: false,
                        is_abstract: true,
                        parameters: self.extract_parameters(child),
                        return_type: self.extract_return_type(child),
                        doc_comment: self.extract_doc_comment(child),
                        attributes: Vec::new(),
                        parent_class: Some(name.clone()),
                        complexity: None,
                        body_prefix: None,
                    };

                    required_methods.push(func);
                }
            }
        }

        // Extract parent traits (supertraits)
        let mut parent_traits = Vec::new();
        if let Some(bounds) = node.child_by_field_name("bounds") {
            let mut cursor = bounds.walk();
            for child in bounds.children(&mut cursor) {
                if child.kind() == "type_identifier" {
                    parent_traits.push(self.node_text(child));
                }
            }
        }

        let trait_entity = TraitEntity {
            name: name.clone(),
            visibility,
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            required_methods,
            parent_traits,
            doc_comment: self.extract_doc_comment(node),
            attributes: Vec::new(),
        };

        self.traits.push(trait_entity);
    }

    /// Visit an impl block
    fn visit_impl(&mut self, node: Node) {
        // Extract the implementing type
        let implementor = node
            .child_by_field_name("type")
            .map(|n| {
                // Handle generic types like Type<T> - extract just the base name
                let text = self.node_text(n);
                text.split('<').next().unwrap_or(&text).trim().to_string()
            })
            .unwrap_or_else(|| "unknown".to_string());

        // Check if this is a trait implementation
        if let Some(trait_node) = node.child_by_field_name("trait") {
            let trait_name = self.node_text(trait_node);
            let trait_name = trait_name
                .split('<')
                .next()
                .unwrap_or(&trait_name)
                .trim()
                .to_string();

            let impl_rel = ImplementationRelation {
                implementor: implementor.clone(),
                trait_name,
            };

            self.implementations.push(impl_rel);
        }

        // Set current class context for methods
        let previous_class = self.current_class.clone();
        self.current_class = Some(implementor.clone());

        // Extract methods from impl block body
        if let Some(body) = node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for child in body.children(&mut cursor) {
                if child.kind() == "function_item" {
                    let method_name = child
                        .child_by_field_name("name")
                        .map(|n| self.node_text(n))
                        .unwrap_or_else(|| "method".to_string());

                    let visibility = self.extract_visibility(child);
                    let parameters = self.extract_parameters(child);

                    // Check if it's a static method (no self parameter)
                    let is_static = !parameters.iter().any(|p| p.name == "self");

                    // Calculate complexity from method body
                    let complexity = child
                        .child_by_field_name("body")
                        .map(|body| self.calculate_complexity(body));

                    let func = FunctionEntity {
                        name: method_name.clone(),
                        signature: self.extract_signature(child),
                        visibility,
                        line_start: child.start_position().row + 1,
                        line_end: child.end_position().row + 1,
                        is_async: self.is_async(child),
                        is_test: false,
                        is_static,
                        is_abstract: false,
                        parameters,
                        return_type: self.extract_return_type(child),
                        doc_comment: self.extract_doc_comment(child),
                        attributes: Vec::new(),
                        parent_class: Some(implementor.clone()),
                        complexity,
                        body_prefix: child
                            .child_by_field_name("body")
                            .and_then(|b| b.utf8_text(self.source).ok())
                            .filter(|t| !t.is_empty())
                            .map(|t| {
                                truncate_body_prefix(t)
                            })
                            .map(|t| t.to_string()),
                    };

                    self.functions.push(func);

                    // Extract type references from method parameter and return type annotations
                    self.extract_type_refs_from_function(&method_name, child);

                    // Extract calls from method body
                    let previous_function = self.current_function.clone();
                    self.current_function = Some(method_name);
                    if let Some(body) = child.child_by_field_name("body") {
                        self.visit_body_for_calls(body);
                    }
                    self.current_function = previous_function;
                }
            }
        }

        // Restore previous class context
        self.current_class = previous_class;
    }

    /// Visit a use declaration
    fn visit_use(&mut self, node: Node) {
        // Extract the use tree
        if let Some(use_tree) = node.child_by_field_name("argument") {
            let import_path = self.node_text(use_tree);

            let import = ImportRelation {
                importer: "current_module".to_string(),
                imported: import_path,
                symbols: Vec::new(),
                is_wildcard: false,
                alias: None,
            };

            self.imports.push(import);
        }
    }

    /// Visit a call expression and record the caller→callee relationship
    fn visit_call_expression(&mut self, node: Node) {
        let caller = match &self.current_function {
            Some(name) => name.clone(),
            None => return,
        };

        let callee = self.extract_callee_name(node);
        if callee.is_empty() || callee == "self" {
            return;
        }

        self.calls.push(CallRelation::new(
            caller,
            callee,
            node.start_position().row + 1,
        ));
    }

    /// Extract the callee function name from a call expression node
    fn extract_callee_name(&self, node: Node) -> String {
        if let Some(func_node) = node.child_by_field_name("function") {
            match func_node.kind() {
                "identifier" => self.node_text(func_node),
                "scoped_identifier" => {
                    // e.g., std::mem::swap — take last segment
                    if let Some(name) = func_node.child_by_field_name("name") {
                        self.node_text(name)
                    } else {
                        self.node_text(func_node)
                    }
                }
                "field_expression" => {
                    // e.g., self.method() or obj.method()
                    if let Some(field) = func_node.child_by_field_name("field") {
                        self.node_text(field)
                    } else {
                        self.node_text(func_node)
                    }
                }
                _ => self.node_text(func_node),
            }
        } else {
            String::new()
        }
    }

    /// Calculate cyclomatic complexity for a function body
    fn calculate_complexity(&self, body: Node) -> ComplexityMetrics {
        let mut builder = ComplexityBuilder::new();
        self.visit_for_complexity(body, &mut builder);
        builder.build()
    }

    /// Recursively walk AST counting complexity-contributing nodes
    fn visit_for_complexity(&self, node: Node, builder: &mut ComplexityBuilder) {
        match node.kind() {
            // Branches
            "if_expression" => {
                builder.add_branch();
                builder.enter_scope();
            }
            "else_clause" => {
                builder.add_branch();
            }
            // Match expression (each arm is a decision path)
            "match_expression" => {
                builder.enter_scope();
            }
            "match_arm" => {
                builder.add_branch();
            }
            // Loops
            "loop_expression" => {
                builder.add_loop();
                builder.enter_scope();
            }
            "while_expression" | "while_let_expression" => {
                builder.add_loop();
                builder.enter_scope();
            }
            "for_expression" => {
                builder.add_loop();
                builder.enter_scope();
            }
            // Logical operators (&& / ||)
            "binary_expression" => {
                if let Some(op) = node.child_by_field_name("operator") {
                    let op_text = self.node_text(op);
                    if op_text == "&&" || op_text == "||" {
                        builder.add_logical_operator();
                    }
                }
            }
            // ? operator (early return on error)
            "try_expression" => {
                builder.add_early_return();
            }
            // Closures add a scope level
            "closure_expression" => {
                builder.enter_scope();
            }
            _ => {}
        }

        // Recurse into children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_for_complexity(child, builder);
        }

        // Exit scope for control structures
        match node.kind() {
            "if_expression"
            | "match_expression"
            | "loop_expression"
            | "while_expression"
            | "while_let_expression"
            | "for_expression"
            | "closure_expression" => {
                builder.exit_scope();
            }
            _ => {}
        }
    }

    /// Recursively visit a node's children looking for call expressions
    fn visit_body_for_calls(&mut self, node: Node) {
        if node.kind() == "call_expression" {
            self.visit_call_expression(node);
        } else if node.kind() == "macro_invocation" {
            // Macro bodies are opaque token trees — tree-sitter doesn't parse
            // the content as Rust AST. Extract call-like patterns heuristically
            // by scanning for `identifier (` sequences in the token tree text.
            self.extract_calls_from_macro(node);
            return; // Don't recurse into token_tree children
        } else if node.kind() == "scoped_identifier" {
            // Detect method references like `Self::method_name` used as values
            // (e.g., `.filter_map(Self::parse_kind_str)`)
            self.visit_method_reference(node);
        } else if node.kind() == "field_expression" {
            // Detect method references like `self.method_name` used as values
            // (only when NOT part of a call expression — calls are handled above)
            if node.parent().map(|p| p.kind()) != Some("call_expression") {
                self.visit_field_reference(node);
            }
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_body_for_calls(child);
        }
    }

    /// Detect `Self::method_name` used as a value (method reference / function pointer).
    /// E.g., `.filter_map(Self::parse_kind_str)` — this is NOT a call_expression.
    fn visit_method_reference(&mut self, node: Node) {
        let caller = match &self.current_function {
            Some(name) => name.clone(),
            None => return,
        };

        // Only match Self:: prefix (not arbitrary paths like std::mem::swap)
        if let Some(path) = node.child_by_field_name("path") {
            let path_text = self.node_text(path);
            if path_text == "Self" || path_text == "self" {
                if let Some(name) = node.child_by_field_name("name") {
                    let method_name = self.node_text(name);
                    if !method_name.is_empty() {
                        // Skip if this scoped_identifier is the function part of a call_expression
                        // (that case is already handled by visit_call_expression)
                        if node.parent().map(|p| p.kind()) == Some("call_expression") {
                            if let Some(parent) = node.parent() {
                                if parent
                                    .child_by_field_name("function")
                                    .map(|f| f.id() == node.id())
                                    .unwrap_or(false)
                                {
                                    return;
                                }
                            }
                        }
                        self.calls.push(CallRelation::new(
                            caller,
                            method_name,
                            node.start_position().row + 1,
                        ));
                    }
                }
            }
        }
    }

    /// Detect `self.method_name` used as a value (not a call).
    fn visit_field_reference(&mut self, node: Node) {
        let caller = match &self.current_function {
            Some(name) => name.clone(),
            None => return,
        };

        // Check that the object is `self`
        if let Some(object) = node.child_by_field_name("value") {
            if self.node_text(object) == "self" {
                if let Some(field) = node.child_by_field_name("field") {
                    let method_name = self.node_text(field);
                    if !method_name.is_empty() {
                        self.calls.push(CallRelation::new(
                            caller,
                            method_name,
                            node.start_position().row + 1,
                        ));
                    }
                }
            }
        }
    }

    /// Extract call-like patterns from macro invocation token trees.
    ///
    /// Since tree-sitter treats macro bodies as opaque token trees, we use
    /// a regex-like scan to find `identifier(` patterns. This catches calls
    /// inside `tokio::select!`, `tokio::spawn(async { ... })`, etc.
    fn extract_calls_from_macro(&mut self, node: Node) {
        let caller = match &self.current_function {
            Some(name) => name.clone(),
            None => return,
        };

        let text = self.node_text(node);

        // Simple state machine to find `name(` patterns, handling:
        // - simple: foo(...)
        // - method: self.foo(...)
        // - scoped: Self::foo(...)
        // - chained: a.b.foo(...)  → extract "foo"
        let bytes = text.as_bytes();
        let len = bytes.len();
        let mut i = 0;

        while i < len {
            // Skip non-identifier starts
            if !is_ident_start(bytes[i]) {
                i += 1;
                continue;
            }

            // Collect identifier
            let start = i;
            while i < len && is_ident_continue(bytes[i]) {
                i += 1;
            }
            let ident = &text[start..i];

            // Skip whitespace
            while i < len && bytes[i] == b' ' {
                i += 1;
            }

            if i < len && bytes[i] == b'(' {
                // Found `ident(` — this looks like a call
                let callee = if ident == "self" || ident == "Self" {
                    // Skip — not a real callee name
                    i += 1;
                    continue;
                } else {
                    ident.to_string()
                };

                // Skip common non-function identifiers
                if !callee.is_empty()
                    && callee != "if"
                    && callee != "for"
                    && callee != "while"
                    && callee != "match"
                    && callee != "loop"
                    && callee != "return"
                    && callee != "let"
                    && callee != "mut"
                    && callee != "async"
                    && callee != "await"
                    && callee != "move"
                    && callee != "Some"
                    && callee != "None"
                    && callee != "Ok"
                    && callee != "Err"
                {
                    self.calls.push(CallRelation::new(
                        caller.clone(),
                        callee,
                        node.start_position().row + 1,
                    ));
                }
            } else if i < len && bytes[i] == b':' && i + 1 < len && bytes[i + 1] == b':' {
                // Skip past `::` for scoped paths like Self::method
                i += 2;
                continue;
            } else if i < len && bytes[i] == b'.' {
                // Skip past `.` for method chains like self.method
                i += 1;
                continue;
            }

            i += 1;
        }
    }

    /// Extract type references from a function/method's parameter and return type annotations.
    fn extract_type_refs_from_function(&mut self, name: &str, node: Node) {
        let line = node.start_position().row + 1;

        if let Some(params_node) = node.child_by_field_name("parameters") {
            let mut cursor = params_node.walk();
            for child in params_node.children(&mut cursor) {
                if child.kind() == "parameter" {
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

        if let Some(return_type_node) = node.child_by_field_name("return_type") {
            for type_name in self.extract_type_names(return_type_node) {
                self.type_references
                    .push(TypeReference::new(name.to_string(), type_name, line));
            }
        }
    }

    /// Extract user-defined type names from a Rust type annotation node.
    /// Skips primitive types and standard library containers, returns only
    /// identifiers that could be user-defined types/structs/enums/traits.
    fn extract_type_names(&self, type_node: Node) -> Vec<String> {
        let mut names = Vec::new();
        match type_node.kind() {
            "type_identifier" => {
                let name = self.node_text(type_node);
                if !Self::is_builtin_rust_type(&name) {
                    names.push(name);
                }
            }
            "scoped_type_identifier" => {
                // e.g., module::Type — take just the final name segment
                if let Some(name_node) = type_node.child_by_field_name("name") {
                    let name = self.node_text(name_node);
                    if !Self::is_builtin_rust_type(&name) {
                        names.push(name);
                    }
                }
            }
            "generic_type" => {
                // e.g., Vec<MyStruct>, Result<Output, MyError>
                // Recurse into base type and type arguments
                let mut cursor = type_node.walk();
                for child in type_node.children(&mut cursor) {
                    names.extend(self.extract_type_names(child));
                }
            }
            "type_arguments" => {
                // <T, U> — recurse into each argument
                let mut cursor = type_node.walk();
                for child in type_node.children(&mut cursor) {
                    names.extend(self.extract_type_names(child));
                }
            }
            "reference_type" => {
                // &T or &mut T
                if let Some(inner) = type_node.child_by_field_name("type") {
                    names.extend(self.extract_type_names(inner));
                }
            }
            "pointer_type" => {
                // *const T or *mut T
                if let Some(inner) = type_node.child_by_field_name("type") {
                    names.extend(self.extract_type_names(inner));
                }
            }
            "tuple_type" => {
                // (A, B, C)
                let mut cursor = type_node.walk();
                for child in type_node.children(&mut cursor) {
                    names.extend(self.extract_type_names(child));
                }
            }
            "slice_type" => {
                // [T]
                if let Some(inner) = type_node.child_by_field_name("type") {
                    names.extend(self.extract_type_names(inner));
                }
            }
            "array_type" => {
                // [T; N] — extract element type
                if let Some(inner) = type_node.child_by_field_name("element") {
                    names.extend(self.extract_type_names(inner));
                }
            }
            "abstract_type" | "dynamic_type" => {
                // impl Trait / dyn Trait — extract the trait name
                let mut cursor = type_node.walk();
                for child in type_node.children(&mut cursor) {
                    names.extend(self.extract_type_names(child));
                }
            }
            _ => {}
        }
        names
    }

    /// Returns true for Rust primitive and standard-library types that are
    /// not user-defined and therefore uninteresting as type references.
    fn is_builtin_rust_type(name: &str) -> bool {
        matches!(
            name,
            "i8" | "i16"
                | "i32"
                | "i64"
                | "i128"
                | "isize"
                | "u8"
                | "u16"
                | "u32"
                | "u64"
                | "u128"
                | "usize"
                | "f32"
                | "f64"
                | "bool"
                | "char"
                | "str"
                | "String"
                | "Vec"
                | "Option"
                | "Result"
                | "Box"
                | "Rc"
                | "Arc"
                | "HashMap"
                | "HashSet"
                | "BTreeMap"
                | "BTreeSet"
                | "Cow"
                | "PhantomData"
                | "Cell"
                | "RefCell"
                | "Mutex"
                | "RwLock"
                | "Self"
        )
    }

    /// Visit a mod declaration (`mod foo;`)
    ///
    /// Skips inline mods (`mod foo { ... }`) since they don't reference external files.
    /// Uses `importer: "mod_declaration"` as a marker to distinguish from `use` imports.
    fn visit_mod(&mut self, node: Node) {
        // Skip inline mods — they have a body (declaration_list child)
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "declaration_list" {
                return;
            }
        }

        // Extract module name
        if let Some(name_node) = node.child_by_field_name("name") {
            let module_name = self.node_text(name_node);
            let import = ImportRelation {
                importer: "mod_declaration".to_string(),
                imported: module_name,
                symbols: Vec::new(),
                is_wildcard: false,
                alias: None,
            };
            self.imports.push(import);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::Parser;

    fn parse_and_visit(source: &str) -> RustVisitor<'_> {
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_rust::LANGUAGE.into()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = RustVisitor::new(source.as_bytes(), ParserConfig::default());
        visitor.visit_node(tree.root_node());
        visitor
    }

    #[test]
    fn test_visitor_function() {
        let source = r#"
fn hello() {
    println!("Hello");
}
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "hello");
        assert_eq!(visitor.functions[0].line_start, 2);
        assert_eq!(visitor.functions[0].line_end, 4);
    }

    #[test]
    fn test_visitor_struct() {
        let source = r#"
pub struct MyStruct {
    pub field1: String,
    field2: i32,
}
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "MyStruct");
        assert_eq!(visitor.classes[0].visibility, "public");
        assert_eq!(visitor.classes[0].fields.len(), 2);
        assert_eq!(visitor.classes[0].line_start, 2);
    }

    #[test]
    fn test_visitor_trait() {
        let source = r#"
pub trait MyTrait {
    fn method(&self);
}
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.traits.len(), 1);
        assert_eq!(visitor.traits[0].name, "MyTrait");
        assert_eq!(visitor.traits[0].required_methods.len(), 1);
    }

    #[test]
    fn test_visitor_enum() {
        let source = r#"
pub enum Status {
    Active,
    Inactive,
    Pending,
}
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "Status");
        assert!(visitor.classes[0].attributes.contains(&"enum".to_string()));
    }

    #[test]
    fn test_visitor_impl_block() {
        let source = r#"
struct MyStruct;

impl MyStruct {
    fn new() -> Self {
        MyStruct
    }

    fn method(&self) {}
}
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.classes.len(), 1);
        // Should extract 2 methods from the impl block
        let impl_methods: Vec<_> = visitor
            .functions
            .iter()
            .filter(|f| f.parent_class == Some("MyStruct".to_string()))
            .collect();
        assert_eq!(impl_methods.len(), 2);
    }

    #[test]
    fn test_visitor_async_function() {
        let source = r#"
async fn fetch() -> String {
    "data".to_string()
}
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 1);
        assert!(visitor.functions[0].is_async);
    }

    #[test]
    fn test_visitor_use_statements() {
        let source = r#"
use std::collections::HashMap;
use std::io::{self, Read};
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 2);
    }

    #[test]
    fn test_visitor_generic_struct() {
        let source = r#"
pub struct Wrapper<T> {
    value: T,
}
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "Wrapper");
        assert!(!visitor.classes[0].type_parameters.is_empty());
    }

    #[test]
    fn test_visitor_trait_impl() {
        let source = r#"
pub trait Display {
    fn display(&self);
}

pub struct Item;

impl Display for Item {
    fn display(&self) {}
}
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.traits.len(), 1);
        assert_eq!(visitor.classes.len(), 1);
        assert!(!visitor.implementations.is_empty());
        assert_eq!(visitor.implementations[0].implementor, "Item");
        assert_eq!(visitor.implementations[0].trait_name, "Display");
    }

    #[test]
    fn test_visitor_function_with_attributes() {
        let source = r#"
#[test]
#[ignore]
fn test_something() {}
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 1);
        assert!(visitor.functions[0].is_test);
    }

    #[test]
    fn test_visitor_visibility_modifiers() {
        let source = r#"
pub fn public_fn() {}
fn private_fn() {}
pub(crate) fn crate_fn() {}
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 3);

        let public_count = visitor
            .functions
            .iter()
            .filter(|f| f.visibility == "public")
            .count();
        assert!(public_count >= 1);

        let internal_count = visitor
            .functions
            .iter()
            .filter(|f| f.visibility == "internal")
            .count();
        assert!(internal_count >= 1);
    }

    #[test]
    fn test_visitor_mod_declarations() {
        let source = r#"
mod scanner;
pub mod codegraph_parsers;
mod inline {
    fn foo() {}
}
"#;
        let visitor = parse_and_visit(source);
        // Should have 2 imports (inline mod skipped)
        assert_eq!(visitor.imports.len(), 2);
        assert_eq!(visitor.imports[0].imported, "scanner");
        assert_eq!(visitor.imports[0].importer, "mod_declaration");
        assert_eq!(visitor.imports[1].imported, "codegraph_parsers");
        assert_eq!(visitor.imports[1].importer, "mod_declaration");
    }

    #[test]
    fn test_visitor_mod_and_use() {
        let source = r#"
mod scanner;
use crate::graph::scanner::FileScanner;
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 2);
        // mod declaration
        assert_eq!(visitor.imports[0].importer, "mod_declaration");
        assert_eq!(visitor.imports[0].imported, "scanner");
        // use statement
        assert_eq!(visitor.imports[1].importer, "current_module");
        assert_eq!(
            visitor.imports[1].imported,
            "crate::graph::scanner::FileScanner"
        );
    }

    #[test]
    fn test_visitor_multiple_items() {
        let source = r#"
use std::fmt;

pub trait Trait1 {
    fn method1(&self);
}

pub struct Struct1 {
    field: i32,
}

pub enum Enum1 {
    Variant1,
    Variant2,
}

pub fn function1() {}

impl Struct1 {
    fn new() -> Self {
        Struct1 { field: 0 }
    }
}
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.traits.len(), 1);
        assert!(visitor.classes.len() >= 2); // Struct1 and Enum1
        assert!(!visitor.functions.is_empty());
        assert!(!visitor.imports.is_empty());
    }

    #[test]
    fn test_accurate_line_numbers() {
        let source = "fn first() {}\n\nfn second() {}";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 2);
        assert_eq!(visitor.functions[0].name, "first");
        assert_eq!(visitor.functions[0].line_start, 1);
        assert_eq!(visitor.functions[1].name, "second");
        assert_eq!(visitor.functions[1].line_start, 3);
    }

    #[test]
    fn test_visitor_call_extraction() {
        let source = r#"
fn caller() {
    callee();
    other_func(42);
}

fn callee() {}
fn other_func(x: i32) {}
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.calls.len(), 2, "Should extract 2 calls");
        assert_eq!(visitor.calls[0].caller, "caller");
        assert_eq!(visitor.calls[0].callee, "callee");
        assert_eq!(visitor.calls[1].caller, "caller");
        assert_eq!(visitor.calls[1].callee, "other_func");
    }

    #[test]
    fn test_visitor_method_call_extraction() {
        let source = r#"
struct Foo;

impl Foo {
    fn method(&self) {
        self.other();
        helper();
    }

    fn other(&self) {}
}

fn helper() {}
"#;
        let visitor = parse_and_visit(source);
        assert!(
            visitor.calls.len() >= 2,
            "Should extract calls from impl methods, got {}",
            visitor.calls.len()
        );
        let callers: Vec<&str> = visitor.calls.iter().map(|c| c.caller.as_str()).collect();
        assert!(callers.contains(&"method"));
    }

    #[test]
    fn test_visitor_scoped_and_self_calls() {
        // Verify Self::method(), self.method(), and closure calls are all extracted
        let source = r#"
struct Foo;

impl Foo {
    fn caller(&self) {
        Self::associated();
        self.instance_method();
        let cb = || { standalone(); };
    }
    fn associated() {}
    fn instance_method(&self) {}
}
fn standalone() {}
"#;
        let visitor = parse_and_visit(source);
        let calls: Vec<String> = visitor
            .calls
            .iter()
            .map(|c| format!("{} -> {}", c.caller, c.callee))
            .collect();
        assert!(
            calls.contains(&"caller -> associated".to_string()),
            "Should extract Self::associated()"
        );
        assert!(
            calls.contains(&"caller -> instance_method".to_string()),
            "Should extract self.instance_method()"
        );
        assert!(
            calls.contains(&"caller -> standalone".to_string()),
            "Should extract closure call"
        );
    }

    #[test]
    fn test_visitor_calls_inside_macro() {
        // tokio::select!, vec![], println!() etc. — calls inside macro bodies
        let source = r#"
fn outer() {
    some_macro! {
        inner();
    }
    direct();
}

fn inner() {}
fn direct() {}
"#;
        let visitor = parse_and_visit(source);
        let calls: Vec<String> = visitor
            .calls
            .iter()
            .map(|c| format!("{} -> {}", c.caller, c.callee))
            .collect();
        eprintln!("Calls (macro): {:?}", calls);
        assert!(
            calls.contains(&"outer -> direct".to_string()),
            "Direct call should work"
        );
        // Calls inside macros are now extracted via heuristic token scanning
        assert!(
            calls.contains(&"outer -> inner".to_string()),
            "Should extract calls from macro token trees"
        );
    }

    #[test]
    fn test_visitor_method_references() {
        // Method references passed as values (not called directly)
        // e.g., `.filter_map(Self::parse_kind_str)` or `.map(self.transform)`
        let source = r#"
struct Foo;

impl Foo {
    fn parse_kind_str(s: &str) -> Option<i32> { None }
    fn transform(&self, x: i32) -> i32 { x }

    fn caller(&self) {
        let items: Vec<_> = vec!["a"].iter().filter_map(Self::parse_kind_str).collect();
        Self::parse_kind_str("test");
    }
}
"#;
        let visitor = parse_and_visit(source);
        let calls: Vec<String> = visitor
            .calls
            .iter()
            .map(|c| format!("{} -> {}", c.caller, c.callee))
            .collect();
        eprintln!("Calls (method refs): {:?}", calls);
        // The method reference Self::parse_kind_str should appear as a call
        assert!(
            calls.contains(&"caller -> parse_kind_str".to_string()),
            "Should detect Self::parse_kind_str method reference. Got: {:?}",
            calls
        );
    }

    #[test]
    fn test_visitor_scoped_call_extraction() {
        let source = r#"
fn caller() {
    std::mem::swap(&mut a, &mut b);
}
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.calls.len(), 1);
        assert_eq!(visitor.calls[0].caller, "caller");
        assert_eq!(visitor.calls[0].callee, "swap");
    }

    #[test]
    fn test_visitor_no_calls_outside_function() {
        let source = r#"
fn standalone() {}
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(
            visitor.calls.len(),
            0,
            "No calls outside of function bodies"
        );
    }

    // --- Complexity tests ---

    #[test]
    fn test_complexity_simple_function() {
        let source = r#"
fn simple() {
    println!("hello");
}
"#;
        let visitor = parse_and_visit(source);
        let func = &visitor.functions[0];
        let cx = func.complexity.as_ref().expect("complexity should be set");
        // No branches, loops, or logical ops → CC = 1
        assert_eq!(cx.cyclomatic_complexity, 1);
        assert_eq!(cx.grade(), 'A');
    }

    #[test]
    fn test_complexity_if_else() {
        let source = r#"
fn check(x: i32) -> &'static str {
    if x > 0 {
        "positive"
    } else if x < 0 {
        "negative"
    } else {
        "zero"
    }
}
"#;
        let visitor = parse_and_visit(source);
        let func = &visitor.functions[0];
        let cx = func.complexity.as_ref().unwrap();
        // if (+1) + else_clause with if (+1 for else, +1 for nested if) + else (+1) = 4 branches
        // CC = 1 + 4 = 5
        assert!(
            cx.cyclomatic_complexity >= 4,
            "if/else if/else should have CC >= 4, got {}",
            cx.cyclomatic_complexity
        );
        assert_eq!(cx.grade(), 'A');
    }

    #[test]
    fn test_complexity_match() {
        let source = r#"
fn classify(x: i32) -> &'static str {
    match x {
        0 => "zero",
        1..=10 => "small",
        11..=100 => "medium",
        _ => "large",
    }
}
"#;
        let visitor = parse_and_visit(source);
        let func = &visitor.functions[0];
        let cx = func.complexity.as_ref().unwrap();
        // 4 match arms → CC = 1 + 4 = 5
        assert_eq!(cx.branches, 4);
        assert_eq!(cx.cyclomatic_complexity, 5);
    }

    #[test]
    fn test_complexity_loops() {
        let source = r#"
fn process(items: &[i32]) {
    for item in items {
        println!("{}", item);
    }
    let mut i = 0;
    while i < 10 {
        i += 1;
    }
    loop {
        break;
    }
}
"#;
        let visitor = parse_and_visit(source);
        let func = &visitor.functions[0];
        let cx = func.complexity.as_ref().unwrap();
        // 3 loops → CC = 1 + 3 = 4
        assert_eq!(cx.loops, 3);
        assert_eq!(cx.cyclomatic_complexity, 4);
    }

    #[test]
    fn test_complexity_logical_operators() {
        let source = r#"
fn validate(x: i32, y: i32) -> bool {
    x > 0 && y > 0 || x < 100
}
"#;
        let visitor = parse_and_visit(source);
        let func = &visitor.functions[0];
        let cx = func.complexity.as_ref().unwrap();
        // && and || → 2 logical operators, CC = 1 + 2 = 3
        assert_eq!(cx.logical_operators, 2);
        assert_eq!(cx.cyclomatic_complexity, 3);
    }

    #[test]
    fn test_complexity_try_operator() {
        let source = r#"
fn read_file(path: &str) -> Result<String, std::io::Error> {
    let content = std::fs::read_to_string(path)?;
    let trimmed = content.trim().parse::<i32>()?;
    Ok(trimmed.to_string())
}
"#;
        let visitor = parse_and_visit(source);
        let func = &visitor.functions[0];
        let cx = func.complexity.as_ref().unwrap();
        // 2 ? operators → 2 early returns, CC = 1 (? counted as early_returns, not in CC formula)
        assert_eq!(cx.early_returns, 2);
    }

    #[test]
    fn test_complexity_nested() {
        let source = r#"
fn nested(items: &[i32]) {
    for item in items {
        if *item > 0 {
            match item {
                1 => {},
                _ => {},
            }
        }
    }
}
"#;
        let visitor = parse_and_visit(source);
        let func = &visitor.functions[0];
        let cx = func.complexity.as_ref().unwrap();
        // for(+1 loop) + if(+1 branch) + 2 match arms(+2 branches) = CC = 1+1+1+2 = 5
        assert_eq!(cx.max_nesting_depth, 3, "for > if > match = 3 levels");
        assert!(cx.cyclomatic_complexity >= 4);
    }

    #[test]
    fn test_complexity_impl_method() {
        let source = r#"
struct Foo;

impl Foo {
    fn complex_method(&self, x: i32) -> i32 {
        if x > 0 {
            for i in 0..x {
                if i % 2 == 0 && i > 5 {
                    return i;
                }
            }
        }
        0
    }
}
"#;
        let visitor = parse_and_visit(source);
        let method = visitor
            .functions
            .iter()
            .find(|f| f.name == "complex_method")
            .expect("method should exist");
        let cx = method.complexity.as_ref().unwrap();
        // if(+1) + for(+1) + if(+1) + &&(+1) = CC = 1+4 = 5
        assert!(cx.cyclomatic_complexity >= 4);
        assert!(cx.max_nesting_depth >= 3);
    }

    // --- Type reference tests ---

    #[test]
    fn test_type_refs_function_params() {
        let source = r#"
struct MyInput;
struct MyOutput;

fn process(input: MyInput) -> MyOutput {
    MyOutput
}
"#;
        let visitor = parse_and_visit(source);
        let type_names: Vec<&str> = visitor
            .type_references
            .iter()
            .map(|r| r.type_name.as_str())
            .collect();
        assert!(
            type_names.contains(&"MyInput"),
            "Should extract param type MyInput, got {:?}",
            type_names
        );
        assert!(
            type_names.contains(&"MyOutput"),
            "Should extract return type MyOutput, got {:?}",
            type_names
        );
        // Referrer should be the function name
        let process_refs: Vec<_> = visitor
            .type_references
            .iter()
            .filter(|r| r.referrer == "process")
            .collect();
        assert_eq!(process_refs.len(), 2);
    }

    #[test]
    fn test_type_refs_generic_types() {
        let source = r#"
struct Report;
struct Error;

fn generate(items: Vec<Report>) -> Result<Report, Error> {
    Ok(Report)
}
"#;
        let visitor = parse_and_visit(source);
        let type_names: Vec<&str> = visitor
            .type_references
            .iter()
            .filter(|r| r.referrer == "generate")
            .map(|r| r.type_name.as_str())
            .collect();
        // Vec and Result are builtins, but Report and Error are user-defined
        assert!(
            type_names.contains(&"Report"),
            "Should extract Report from Vec<Report> and Result<Report, Error>"
        );
        assert!(
            type_names.contains(&"Error"),
            "Should extract Error from Result<Report, Error>"
        );
        assert!(
            !type_names.contains(&"Vec"),
            "Vec is builtin and should be filtered"
        );
        assert!(
            !type_names.contains(&"Result"),
            "Result is builtin and should be filtered"
        );
    }

    #[test]
    fn test_type_refs_struct_fields() {
        let source = r#"
struct Address;
struct Company;

struct Person {
    address: Address,
    employer: Company,
    age: u32,
}
"#;
        let visitor = parse_and_visit(source);
        let struct_refs: Vec<&str> = visitor
            .type_references
            .iter()
            .filter(|r| r.referrer == "Person")
            .map(|r| r.type_name.as_str())
            .collect();
        assert!(
            struct_refs.contains(&"Address"),
            "Should extract Address from struct field"
        );
        assert!(
            struct_refs.contains(&"Company"),
            "Should extract Company from struct field"
        );
        assert!(
            !struct_refs.contains(&"u32"),
            "u32 is builtin and should be filtered"
        );
    }

    #[test]
    fn test_type_refs_reference_types() {
        let source = r#"
struct Config;

fn init(cfg: &Config) {}
fn update(cfg: &mut Config) {}
"#;
        let visitor = parse_and_visit(source);
        let type_names: Vec<&str> = visitor
            .type_references
            .iter()
            .map(|r| r.type_name.as_str())
            .collect();
        assert!(
            type_names.contains(&"Config"),
            "Should extract Config from &Config and &mut Config"
        );
    }

    #[test]
    fn test_type_refs_method_in_impl() {
        let source = r#"
struct MyStruct;
struct Input;
struct Output;

impl MyStruct {
    fn process(&self, input: Input) -> Output {
        Output
    }
}
"#;
        let visitor = parse_and_visit(source);
        let method_refs: Vec<&str> = visitor
            .type_references
            .iter()
            .filter(|r| r.referrer == "process")
            .map(|r| r.type_name.as_str())
            .collect();
        assert!(
            method_refs.contains(&"Input"),
            "Should extract Input from method param"
        );
        assert!(
            method_refs.contains(&"Output"),
            "Should extract Output from method return type"
        );
    }

    #[test]
    fn test_type_refs_primitives_filtered() {
        let source = r#"
fn add(a: i32, b: u64) -> f64 { 0.0 }
"#;
        let visitor = parse_and_visit(source);
        assert!(
            visitor.type_references.is_empty(),
            "Primitive types should not produce type references, got {:?}",
            visitor.type_references
        );
    }

}
