// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting Go entities

use codegraph_parser_api::{
    CallRelation, ClassEntity, ComplexityBuilder, ComplexityMetrics, FunctionEntity,
    ImportRelation, Parameter, TraitEntity, TypeReference, BODY_PREFIX_MAX_CHARS,
};
use tree_sitter::Node;

/// Built-in Go primitive types — not recorded as type references
const GO_PRIMITIVES: &[&str] = &[
    "int",
    "int8",
    "int16",
    "int32",
    "int64",
    "uint",
    "uint8",
    "uint16",
    "uint32",
    "uint64",
    "uintptr",
    "float32",
    "float64",
    "complex64",
    "complex128",
    "bool",
    "byte",
    "rune",
    "string",
    "error",
    "any",
];

pub struct GoVisitor<'a> {
    pub source: &'a [u8],
    pub functions: Vec<FunctionEntity>,
    pub structs: Vec<ClassEntity>,
    pub interfaces: Vec<TraitEntity>,
    pub imports: Vec<ImportRelation>,
    pub calls: Vec<CallRelation>,
    pub type_references: Vec<TypeReference>,
    current_function: Option<String>,
}

impl<'a> GoVisitor<'a> {
    pub fn new(source: &'a [u8]) -> Self {
        Self {
            source,
            functions: Vec::new(),
            structs: Vec::new(),
            interfaces: Vec::new(),
            imports: Vec::new(),
            calls: Vec::new(),
            type_references: Vec::new(),
            current_function: None,
        }
    }

    fn node_text(&self, node: Node) -> String {
        node.utf8_text(self.source).unwrap_or("").to_string()
    }

    pub fn visit_node(&mut self, node: Node) {
        match node.kind() {
            "function_declaration" => {
                self.visit_function(node);
                // Don't recurse — visit_function handles body for call extraction
                return;
            }
            "method_declaration" => {
                self.visit_method(node);
                // Don't recurse — visit_method handles body for call extraction
                return;
            }
            "type_declaration" => self.visit_type_declaration(node),
            "import_declaration" => self.visit_import(node),
            "call_expression" => self.visit_call_expression(node),
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child);
        }
    }

    fn visit_function(&mut self, node: Node) {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_else(|| "anonymous".to_string());

        let previous_function = self.current_function.clone();
        self.current_function = Some(name.clone());

        let complexity = node
            .child_by_field_name("body")
            .map(|body| self.calculate_complexity(body));

        // In Go, exported names start with uppercase
        let visibility = if name.starts_with(|c: char| c.is_uppercase()) {
            "public"
        } else {
            "private"
        };

        let parameters = self.extract_parameters(node);
        let return_type = self.extract_return_type(node);
        let is_test = name.starts_with("Test") || name.starts_with("Benchmark");

        self.extract_type_refs_from_signature(&name, node);

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

        let func = FunctionEntity {
            name,
            signature: self
                .node_text(node)
                .lines()
                .next()
                .unwrap_or("")
                .to_string(),
            visibility: visibility.to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_async: false,
            is_test,
            is_static: false,
            is_abstract: false,
            parameters,
            return_type,
            doc_comment: None,
            attributes: Vec::new(),
            parent_class: None,
            complexity,
            body_prefix,
        };
        self.functions.push(func);

        if let Some(body) = node.child_by_field_name("body") {
            self.visit_body_for_calls(body);
        }

        self.current_function = previous_function;
    }

    fn visit_method(&mut self, node: Node) {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_else(|| "method".to_string());

        let previous_function = self.current_function.clone();
        self.current_function = Some(name.clone());

        let complexity = node
            .child_by_field_name("body")
            .map(|body| self.calculate_complexity(body));

        // Extract receiver type for parent_class
        let parent_class = node.child_by_field_name("receiver").and_then(|recv| {
            // receiver is a parameter_list containing a parameter_declaration
            let mut cursor = recv.walk();
            for child in recv.children(&mut cursor) {
                if child.kind() == "parameter_declaration" {
                    // The type is the last meaningful child (may be pointer_type or type_identifier)
                    if let Some(type_node) = child.child_by_field_name("type") {
                        let text = self.node_text(type_node);
                        return Some(text.trim_start_matches('*').to_string());
                    }
                }
            }
            None
        });

        let visibility = if name.starts_with(|c: char| c.is_uppercase()) {
            "public"
        } else {
            "private"
        };

        let parameters = self.extract_parameters(node);
        let return_type = self.extract_return_type(node);

        self.extract_type_refs_from_signature(&name, node);

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

        let func = FunctionEntity {
            name,
            signature: self
                .node_text(node)
                .lines()
                .next()
                .unwrap_or("")
                .to_string(),
            visibility: visibility.to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_async: false,
            is_test: false,
            is_static: false,
            is_abstract: false,
            parameters,
            return_type,
            doc_comment: None,
            attributes: Vec::new(),
            parent_class,
            complexity,
            body_prefix,
        };
        self.functions.push(func);

        if let Some(body) = node.child_by_field_name("body") {
            self.visit_body_for_calls(body);
        }

        self.current_function = previous_function;
    }

    /// Extract parameters from a function/method declaration
    fn extract_parameters(&self, node: Node) -> Vec<Parameter> {
        let mut params = Vec::new();

        if let Some(params_node) = node.child_by_field_name("parameters") {
            let mut cursor = params_node.walk();
            for child in params_node.children(&mut cursor) {
                match child.kind() {
                    "parameter_declaration" => {
                        let type_annotation =
                            child.child_by_field_name("type").map(|n| self.node_text(n));

                        // A parameter_declaration can have multiple names (a, b int)
                        let mut names = Vec::new();
                        let mut inner_cursor = child.walk();
                        for inner in child.children(&mut inner_cursor) {
                            if inner.kind() == "identifier" {
                                names.push(self.node_text(inner));
                            }
                        }

                        if names.is_empty() {
                            // Unnamed parameter (just a type)
                            params.push(Parameter {
                                name: String::new(),
                                type_annotation: type_annotation.clone(),
                                default_value: None,
                                is_variadic: false,
                            });
                        } else {
                            for param_name in names {
                                params.push(Parameter {
                                    name: param_name,
                                    type_annotation: type_annotation.clone(),
                                    default_value: None,
                                    is_variadic: false,
                                });
                            }
                        }
                    }
                    "variadic_parameter_declaration" => {
                        // Go variadic: nums ...int
                        let mut param_name = String::new();
                        let mut type_annotation = None;

                        let mut inner_cursor = child.walk();
                        for inner in child.children(&mut inner_cursor) {
                            if inner.kind() == "identifier" {
                                param_name = self.node_text(inner);
                            }
                        }
                        // The type is the child after "..."
                        if let Some(type_node) = child.child_by_field_name("type") {
                            type_annotation = Some(self.node_text(type_node));
                        }

                        params.push(Parameter {
                            name: param_name,
                            type_annotation,
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

    /// Extract return type from a function/method declaration
    fn extract_return_type(&self, node: Node) -> Option<String> {
        node.child_by_field_name("result").map(|n| {
            let text = self.node_text(n);
            // Clean up parenthesized multiple returns: (int, error) stays as-is
            text.trim().to_string()
        })
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
                // Count an `else` branch if this if_statement has one.
                // tree-sitter-go has no `else_clause` wrapper: the else branch
                // appears as a bare `else` keyword token followed by a sibling block.
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "else" {
                        builder.add_branch();
                    }
                }
            }
            "for_statement" => {
                // Go only has `for` — covers for, while-style, and range loops
                builder.add_loop();
                builder.enter_scope();
            }
            "expression_switch_statement" | "type_switch_statement" => {
                builder.enter_scope();
            }
            "expression_case" | "default_case" | "type_case" => {
                builder.add_branch();
            }
            "select_statement" => {
                builder.enter_scope();
            }
            "communication_case" => {
                // select { case <-ch: ... }
                builder.add_branch();
            }
            "defer_statement" => {
                // Go uses defer/recover instead of try/catch
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

        // Exit scope after visiting children
        match node.kind() {
            "if_statement"
            | "for_statement"
            | "expression_switch_statement"
            | "type_switch_statement"
            | "select_statement" => {
                builder.exit_scope();
            }
            _ => {}
        }
    }

    /// Visit a call expression and record the caller→callee relationship
    fn visit_call_expression(&mut self, node: Node) {
        let caller = match &self.current_function {
            Some(name) => name.clone(),
            None => return,
        };

        let callee = self.extract_callee_name(node);
        if callee.is_empty() {
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
                "selector_expression" => {
                    // e.g., fmt.Println, obj.Method
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

    /// Recursively visit a node's children looking for call expressions
    fn visit_body_for_calls(&mut self, node: Node) {
        if node.kind() == "call_expression" {
            self.visit_call_expression(node);
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_body_for_calls(child);
        }
    }

    /// Extract type references from function/method parameter types and return type.
    fn extract_type_refs_from_signature(&mut self, referrer: &str, node: Node) {
        let line = node.start_position().row + 1;

        if let Some(params_node) = node.child_by_field_name("parameters") {
            let mut cursor = params_node.walk();
            for child in params_node.children(&mut cursor) {
                match child.kind() {
                    "parameter_declaration" | "variadic_parameter_declaration" => {
                        if let Some(type_node) = child.child_by_field_name("type") {
                            for type_name in self.collect_type_identifiers(type_node) {
                                self.type_references.push(TypeReference::new(
                                    referrer.to_string(),
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

        if let Some(result_node) = node.child_by_field_name("result") {
            for type_name in self.collect_type_identifiers(result_node) {
                self.type_references.push(TypeReference::new(
                    referrer.to_string(),
                    type_name,
                    line,
                ));
            }
        }
    }

    /// Recursively collect user-defined type identifiers from a type AST node.
    /// Skips built-in primitive types.
    fn collect_type_identifiers(&self, node: Node) -> Vec<String> {
        let mut result = Vec::new();
        self.collect_type_identifiers_recursive(node, &mut result);
        result
    }

    fn collect_type_identifiers_recursive(&self, node: Node, out: &mut Vec<String>) {
        match node.kind() {
            "type_identifier" => {
                let name = self.node_text(node);
                if !GO_PRIMITIVES.contains(&name.as_str()) {
                    out.push(name);
                }
            }
            "qualified_type" => {
                // e.g., io.Reader — take the field (method name) portion
                if let Some(field) = node.child_by_field_name("name") {
                    let name = self.node_text(field);
                    if !GO_PRIMITIVES.contains(&name.as_str()) {
                        out.push(name);
                    }
                }
            }
            _ => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.collect_type_identifiers_recursive(child, out);
                }
            }
        }
    }

    fn visit_type_declaration(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "type_spec" {
                let name = child
                    .child_by_field_name("name")
                    .map(|n| self.node_text(n))
                    .unwrap_or_else(|| "Type".to_string());
                let type_node = child.child_by_field_name("type");

                if let Some(type_node) = type_node {
                    match type_node.kind() {
                        "struct_type" => {
                            let body_prefix = type_node
                                .utf8_text(self.source)
                                .ok()
                                .filter(|t| !t.is_empty())
                                .map(|t| {
                                    if t.len() > BODY_PREFIX_MAX_CHARS {
                                        &t[..BODY_PREFIX_MAX_CHARS]
                                    } else {
                                        t
                                    }
                                })
                                .map(|t| t.to_string());

                            let struct_entity = ClassEntity {
                                name,
                                visibility: "public".to_string(),
                                line_start: child.start_position().row + 1,
                                line_end: child.end_position().row + 1,
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
                            self.structs.push(struct_entity);
                        }
                        "interface_type" => {
                            let interface_entity = TraitEntity {
                                name,
                                visibility: "public".to_string(),
                                line_start: child.start_position().row + 1,
                                line_end: child.end_position().row + 1,
                                required_methods: Vec::new(),
                                parent_traits: Vec::new(),
                                doc_comment: None,
                                attributes: Vec::new(),
                            };
                            self.interfaces.push(interface_entity);
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    fn visit_import(&mut self, node: Node) {
        // Check if this is an import block or single import
        let mut cursor = node.walk();
        let mut found_specs = false;

        for child in node.children(&mut cursor) {
            match child.kind() {
                "import_spec_list" => {
                    // Import block: import ( ... )
                    found_specs = true;
                    let mut spec_cursor = child.walk();
                    for spec in child.children(&mut spec_cursor) {
                        if spec.kind() == "import_spec" {
                            self.extract_import_spec(spec);
                        }
                    }
                }
                "import_spec" => {
                    // Single import: import "fmt" or import f "fmt"
                    found_specs = true;
                    self.extract_import_spec(child);
                }
                _ => {}
            }
        }

        // Fallback for unexpected format
        if !found_specs {
            let import_text = self.node_text(node);
            let import = ImportRelation {
                importer: "current_package".to_string(),
                imported: import_text,
                symbols: Vec::new(),
                is_wildcard: false,
                alias: None,
            };
            self.imports.push(import);
        }
    }

    fn extract_import_spec(&mut self, node: Node) {
        let mut alias = None;
        let mut is_wildcard = false;
        let mut path = String::new();

        // Extract path and optional name (alias)
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            let kind = child.kind();
            let text = self.node_text(child);

            match kind {
                "interpreted_string_literal" => {
                    // This is the import path
                    // Remove quotes
                    path = text.trim_matches('"').to_string();
                }
                "package_identifier" | "identifier" | "dot" | "." => {
                    // This is the alias/name or special marker
                    if text == "." {
                        is_wildcard = true;
                    } else if text != "_" {
                        alias = Some(text);
                    }
                    // If it's "_", we ignore it (blank identifier)
                }
                _ => {
                    // Check text content for special cases
                    if text == "." {
                        is_wildcard = true;
                    } else if text != "_"
                        && !text.trim().is_empty()
                        && kind != "("
                        && kind != ")"
                        && kind != "\""
                    {
                        // Might be an unrecognized alias format
                        if !path.is_empty() {
                            // Only set alias if we haven't found the path yet would mean this comes before
                            // Actually in Go, alias comes before path
                        }
                    }
                }
            }
        }

        let import = ImportRelation {
            importer: "current_package".to_string(),
            imported: path,
            symbols: Vec::new(), // Go doesn't have named imports like TypeScript
            is_wildcard,
            alias,
        };
        self.imports.push(import);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_visitor_basics() {
        let visitor = GoVisitor::new(b"package main");
        assert_eq!(visitor.functions.len(), 0);
        assert_eq!(visitor.structs.len(), 0);
        assert_eq!(visitor.interfaces.len(), 0);
    }

    #[test]
    fn test_visitor_function_extraction() {
        use tree_sitter::Parser;

        let source = b"package main\nfunc greet(name string) string { return \"Hello\" }";
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_go::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = GoVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "greet");
    }

    #[test]
    fn test_visitor_struct_extraction() {
        use tree_sitter::Parser;

        let source = b"package main\ntype Person struct { Name string }";
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_go::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = GoVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.structs.len(), 1);
        assert_eq!(visitor.structs[0].name, "Person");
    }

    #[test]
    fn test_visitor_interface_extraction() {
        use tree_sitter::Parser;

        let source = b"package main\ntype Reader interface { Read() error }";
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_go::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = GoVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.interfaces.len(), 1);
        assert_eq!(visitor.interfaces[0].name, "Reader");
    }

    #[test]
    fn test_visitor_method_extraction() {
        use tree_sitter::Parser;

        let source = b"package main\nfunc (p Person) String() string { return \"\" }";
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_go::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = GoVisitor::new(source);
        visitor.visit_node(tree.root_node());

        // Methods are extracted as functions
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "String");
    }

    #[test]
    fn test_visitor_import_extraction() {
        use tree_sitter::Parser;

        let source = b"package main\nimport \"fmt\"";
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_go::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = GoVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.imports.len(), 1);
    }

    #[test]
    fn test_visitor_multiple_declarations() {
        use tree_sitter::Parser;

        let source = b"package main\ntype User struct {}\ntype Admin struct {}";
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_go::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = GoVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.structs.len(), 2);
        assert_eq!(visitor.structs[0].name, "User");
        assert_eq!(visitor.structs[1].name, "Admin");
    }

    // TDD: New tests for individual import extraction
    #[test]
    fn test_visitor_import_block_multiple_imports() {
        use tree_sitter::Parser;

        let source = b"package main\nimport (\n\t\"fmt\"\n\t\"os\"\n\t\"io\"\n)";
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_go::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = GoVisitor::new(source);
        visitor.visit_node(tree.root_node());

        // Should extract 3 individual imports, not 1 block
        assert_eq!(
            visitor.imports.len(),
            3,
            "Should extract 3 individual imports"
        );
        assert_eq!(visitor.imports[0].imported, "fmt");
        assert_eq!(visitor.imports[1].imported, "os");
        assert_eq!(visitor.imports[2].imported, "io");
    }

    #[test]
    fn test_visitor_import_with_alias() {
        use tree_sitter::Parser;

        let source = b"package main\nimport f \"fmt\"";
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_go::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = GoVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "fmt");
        assert_eq!(visitor.imports[0].alias, Some("f".to_string()));
        assert!(!visitor.imports[0].is_wildcard);
    }

    #[test]
    fn test_visitor_import_with_dot_wildcard() {
        use tree_sitter::Parser;

        let source = b"package main\nimport . \"fmt\"";
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_go::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = GoVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "fmt");
        assert!(visitor.imports[0].is_wildcard);
        assert_eq!(visitor.imports[0].alias, None);
    }

    #[test]
    fn test_visitor_import_with_blank_identifier() {
        use tree_sitter::Parser;

        let source = b"package main\nimport _ \"database/sql\"";
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_go::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = GoVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "database/sql");
        assert_eq!(visitor.imports[0].alias, None); // _ is ignored
        assert!(!visitor.imports[0].is_wildcard);
    }

    #[test]
    fn test_visitor_import_block_with_aliases() {
        use tree_sitter::Parser;

        let source = b"package main\nimport (\n\tf \"fmt\"\n\t. \"os\"\n\t_ \"encoding/json\"\n)";
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_go::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = GoVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.imports.len(), 3);

        // Import with alias
        assert_eq!(visitor.imports[0].imported, "fmt");
        assert_eq!(visitor.imports[0].alias, Some("f".to_string()));
        assert!(!visitor.imports[0].is_wildcard);

        // Import with dot (wildcard)
        assert_eq!(visitor.imports[1].imported, "os");
        assert!(visitor.imports[1].is_wildcard);
        assert_eq!(visitor.imports[1].alias, None);

        // Import with blank identifier
        assert_eq!(visitor.imports[2].imported, "encoding/json");
        assert_eq!(visitor.imports[2].alias, None);
        assert!(!visitor.imports[2].is_wildcard);
    }

    #[test]
    fn test_visitor_call_extraction() {
        use tree_sitter::Parser;

        let source = b"package main\nfunc caller() {\n\tcallee()\n\tfmt.Println(\"hello\")\n}\nfunc callee() {}";
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_go::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = GoVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.calls.len(), 2, "Should extract 2 calls");
        assert_eq!(visitor.calls[0].caller, "caller");
        assert_eq!(visitor.calls[0].callee, "callee");
        assert_eq!(visitor.calls[1].caller, "caller");
        assert_eq!(visitor.calls[1].callee, "Println");
    }

    #[test]
    fn test_visitor_method_call_extraction() {
        use tree_sitter::Parser;

        let source = b"package main\ntype Foo struct{}\nfunc (f Foo) Method() {\n\thelper()\n}\nfunc helper() {}";
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_go::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = GoVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.calls.len(), 1);
        assert_eq!(visitor.calls[0].caller, "Method");
        assert_eq!(visitor.calls[0].callee, "helper");
    }

    #[test]
    fn test_visitor_no_calls_outside_function() {
        use tree_sitter::Parser;

        let source = b"package main\nfunc standalone() {}";
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_go::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = GoVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.calls.len(), 0);
    }

    #[test]
    fn test_parameter_extraction() {
        use tree_sitter::Parser;

        let source = b"package main\nfunc add(a int, b int) int { return a + b }";
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_go::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = GoVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.functions.len(), 1);
        let func = &visitor.functions[0];
        assert_eq!(func.parameters.len(), 2, "Should extract 2 parameters");
        assert_eq!(func.parameters[0].name, "a");
        assert_eq!(func.parameters[0].type_annotation.as_deref(), Some("int"));
        assert_eq!(func.parameters[1].name, "b");
        assert_eq!(func.return_type.as_deref(), Some("int"));
    }

    #[test]
    fn test_variadic_parameter() {
        use tree_sitter::Parser;

        let source = b"package main\nfunc sum(nums ...int) int { return 0 }";
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_go::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = GoVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.functions.len(), 1);
        let func = &visitor.functions[0];
        assert_eq!(func.parameters.len(), 1);
        assert!(
            func.parameters[0].is_variadic,
            "Should detect variadic parameter"
        );
    }

    #[test]
    fn test_multiple_return_values() {
        use tree_sitter::Parser;

        let source = b"package main\nfunc divide(a, b float64) (float64, error) { return 0, nil }";
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_go::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = GoVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.functions.len(), 1);
        let func = &visitor.functions[0];
        assert!(func.return_type.is_some(), "Should extract return type");
        let ret = func.return_type.as_ref().unwrap();
        assert!(
            ret.contains("float64") && ret.contains("error"),
            "Should capture multiple return types: {}",
            ret
        );
    }

    #[test]
    fn test_method_parent_class() {
        use tree_sitter::Parser;

        let source = b"package main\ntype Foo struct{}\nfunc (f *Foo) Bar() string { return \"\" }";
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_go::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = GoVisitor::new(source);
        visitor.visit_node(tree.root_node());

        let method = visitor.functions.iter().find(|f| f.name == "Bar").unwrap();
        assert_eq!(
            method.parent_class.as_deref(),
            Some("Foo"),
            "Method should have parent_class set to receiver type"
        );
    }

    #[test]
    fn test_complexity_simple_function() {
        use tree_sitter::Parser;

        // A function with no branches — CC should be 1
        let source = b"package main\nfunc simple(x int) int { return x + 1 }";
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_go::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = GoVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert_eq!(
            complexity.cyclomatic_complexity, 1,
            "Simple function should have CC=1"
        );
        assert_eq!(complexity.branches, 0);
        assert_eq!(complexity.loops, 0);
    }

    #[test]
    fn test_complexity_if_else_and_for() {
        use tree_sitter::Parser;

        // if adds 1 branch, else adds 1 branch, for adds 1 loop → CC = 1 + 3 = 4
        let source = b"package main\nfunc process(n int) int {\n\tif n > 0 {\n\t\treturn n\n\t} else {\n\t\treturn -n\n\t}\n\tfor i := 0; i < n; i++ {}\n\treturn 0\n}";
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_go::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = GoVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(
            complexity.branches >= 2,
            "if + else should contribute >= 2 branches"
        );
        assert!(
            complexity.loops >= 1,
            "for loop should contribute >= 1 loop"
        );
        assert!(complexity.cyclomatic_complexity > 1, "CC should be > 1");
    }

    #[test]
    fn test_complexity_switch_and_select() {
        use tree_sitter::Parser;

        // switch with 2 cases + default → 3 branches; select with 2 cases → 2 more branches
        let source = br#"package main
func handle(x int, ch chan int) {
    switch x {
    case 1:
        return
    case 2:
        return
    default:
        return
    }
    select {
    case v := <-ch:
        _ = v
    default:
        return
    }
}"#;
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_go::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = GoVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        // switch: case 1, case 2, default → 3 branches
        // select: communication_case, default_case → 2 branches
        assert!(
            complexity.branches >= 5,
            "switch (3) + select (2) should contribute >= 5 branches, got {}",
            complexity.branches
        );
        assert!(complexity.cyclomatic_complexity > 1, "CC should be > 1");
    }

    #[test]
    fn test_type_refs_from_function_params() {
        use tree_sitter::Parser;

        let source =
            b"package main\ntype Config struct{}\nfunc setup(cfg Config) error { return nil }";
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_go::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = GoVisitor::new(source);
        visitor.visit_node(tree.root_node());

        // setup references Config (parameter) and error is primitive so skipped
        let refs: Vec<_> = visitor
            .type_references
            .iter()
            .filter(|r| r.referrer == "setup")
            .collect();
        assert!(
            refs.iter().any(|r| r.type_name == "Config"),
            "setup should reference Config, got: {:?}",
            refs
        );
        // error is a primitive — should not appear
        assert!(
            !refs.iter().any(|r| r.type_name == "error"),
            "error is primitive and should be excluded"
        );
    }

    #[test]
    fn test_type_refs_from_return_type() {
        use tree_sitter::Parser;

        let source =
            b"package main\ntype Response struct{}\nfunc fetch() Response { return Response{} }";
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_go::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = GoVisitor::new(source);
        visitor.visit_node(tree.root_node());

        let refs: Vec<_> = visitor
            .type_references
            .iter()
            .filter(|r| r.referrer == "fetch")
            .collect();
        assert!(
            refs.iter().any(|r| r.type_name == "Response"),
            "fetch should reference Response, got: {:?}",
            refs
        );
    }

    #[test]
    fn test_type_refs_method_receiver_not_counted() {
        use tree_sitter::Parser;

        // The receiver type should not create a type reference (it's not a param annotation)
        let source =
            b"package main\ntype Foo struct{}\ntype Bar struct{}\nfunc (f Foo) Do(b Bar) {}";
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_go::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = GoVisitor::new(source);
        visitor.visit_node(tree.root_node());

        // Do's parameter Bar should be a type ref
        let refs: Vec<_> = visitor
            .type_references
            .iter()
            .filter(|r| r.referrer == "Do")
            .collect();
        assert!(
            refs.iter().any(|r| r.type_name == "Bar"),
            "Do should reference Bar, got: {:?}",
            refs
        );
    }

    #[test]
    fn test_type_refs_pointer_type() {
        use tree_sitter::Parser;

        let source = b"package main\ntype Node struct{}\nfunc process(n *Node) *Node { return n }";
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_go::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = GoVisitor::new(source);
        visitor.visit_node(tree.root_node());

        let refs: Vec<_> = visitor
            .type_references
            .iter()
            .filter(|r| r.referrer == "process")
            .collect();
        assert!(
            refs.iter().any(|r| r.type_name == "Node"),
            "process should reference Node through pointer type, got: {:?}",
            refs
        );
    }

    #[test]
    fn test_type_refs_primitives_excluded() {
        use tree_sitter::Parser;

        let source = b"package main\nfunc add(a int, b int) int { return a + b }";
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_go::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = GoVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(
            visitor.type_references.len(),
            0,
            "Primitives only — no type references expected"
        );
    }

    #[test]
    fn test_complexity_logical_operators() {
        use tree_sitter::Parser;

        // && and || each add a logical operator count
        let source = b"package main\nfunc check(a, b, c bool) bool { return a && b || c }";
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_go::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = GoVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(
            complexity.logical_operators >= 2,
            "&& and || should each count, got {}",
            complexity.logical_operators
        );
    }
}
