// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting Perl entities

use codegraph_parser_api::{
    truncate_body_prefix, CallRelation, ClassEntity, ComplexityBuilder, ComplexityMetrics,
    FunctionEntity, ImportRelation, Parameter,
};
use tree_sitter::Node;

pub(crate) struct PerlVisitor<'a> {
    pub source: &'a [u8],
    pub functions: Vec<FunctionEntity>,
    pub classes: Vec<ClassEntity>,
    pub imports: Vec<ImportRelation>,
    pub calls: Vec<CallRelation>,
    current_function: Option<String>,
    current_package: Option<String>,
}

impl<'a> PerlVisitor<'a> {
    pub fn new(source: &'a [u8]) -> Self {
        Self {
            source,
            functions: Vec::new(),
            classes: Vec::new(),
            imports: Vec::new(),
            calls: Vec::new(),
            current_function: None,
            current_package: None,
        }
    }

    fn node_text(&self, node: Node) -> String {
        node.utf8_text(self.source).unwrap_or("").to_string()
    }

    pub fn visit_node(&mut self, node: Node) {
        match node.kind() {
            "package_statement" => {
                self.visit_package_statement(node);
                return;
            }
            "function_definition" => {
                self.visit_sub_declaration(node);
                return;
            }
            "use_no_statement" => {
                self.visit_use_statement(node);
            }
            "require_expression" => {
                self.visit_require_expression(node);
            }
            "call_expression_with_spaced_args" | "call_expression_with_bareword" | "method_call_expression" => {
                self.visit_call_expression(node);
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child);
        }
    }

    fn visit_package_statement(&mut self, node: Node) {
        // package_statement: "package" package_expression ";"
        // package_expression contains the name
        let name = self.find_package_name(node);
        if name.is_empty() {
            return;
        }

        let line_start = node.start_position().row + 1;
        let line_end = node.end_position().row + 1;

        self.current_package = Some(name.clone());

        let class = ClassEntity {
            name: name.clone(),
            visibility: "public".to_string(),
            line_start,
            line_end,
            is_abstract: false,
            is_interface: false,
            base_classes: Vec::new(),
            implemented_traits: Vec::new(),
            fields: Vec::new(),
            doc_comment: self.extract_doc_comment(node),
            attributes: Vec::new(),
            type_parameters: Vec::new(),
            methods: Vec::new(),
            body_prefix: None,
        };
        self.classes.push(class);

        // Visit children (sub declarations inside package scope are handled at
        // top level since Perl's package scope extends to the next package decl)
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child);
        }
    }

    fn find_package_name(&self, node: Node) -> String {
        // package_statement has package_name child containing identifier(s)
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "package_name" {
                return self.node_text(child);
            }
        }
        String::new()
    }

    fn visit_sub_declaration(&mut self, node: Node) {
        // subroutine_declaration_statement has: name, prototype?, block
        let name = self.find_sub_name(node);
        if name.is_empty() {
            return;
        }

        let is_private = name.starts_with('_');
        let visibility = if is_private { "private" } else { "public" }.to_string();

        let full_name = if let Some(ref pkg) = self.current_package.clone() {
            format!("{}::{}", pkg, name)
        } else {
            name.clone()
        };

        let signature = format!("sub {}", name);

        let doc_comment = self.extract_doc_comment(node);

        let body_node = self.find_block(node);
        let body_prefix = body_node
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| truncate_body_prefix(t).to_string());

        let complexity = body_node.map(|b| self.calculate_complexity(b));

        let parameters = self.extract_perl_parameters(node);

        let parent_class = self.current_package.clone();

        let func = FunctionEntity {
            name: full_name.clone(),
            signature,
            visibility,
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_async: false,
            is_test: name.starts_with("test_") || name.starts_with("Test"),
            is_static: false,
            is_abstract: false,
            parameters,
            return_type: None,
            doc_comment,
            attributes: Vec::new(),
            parent_class,
            complexity,
            body_prefix,
        };

        self.functions.push(func);

        let prev_function = self.current_function.take();
        self.current_function = Some(full_name);

        if let Some(block) = body_node {
            self.visit_body_for_calls(block);
        }

        self.current_function = prev_function;
    }

    fn find_sub_name(&self, node: Node) -> String {
        // function_definition has a "name" field
        if let Some(name_node) = node.child_by_field_name("name") {
            return self.node_text(name_node);
        }
        // Fallback: look for identifier child
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "identifier" {
                return self.node_text(child);
            }
        }
        String::new()
    }

    fn find_block<'b>(&self, node: Node<'b>) -> Option<Node<'b>> {
        // function_definition has a "body" field
        if let Some(body) = node.child_by_field_name("body") {
            return Some(body);
        }
        // Fallback: look for block child
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "block" {
                return Some(child);
            }
        }
        None
    }

    fn extract_perl_parameters(&self, node: Node) -> Vec<Parameter> {
        // Perl parameters aren't formally declared in the signature —
        // they come from @_. We look for prototype nodes for hint.
        let mut params = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "prototype" {
                let text = self.node_text(child);
                // prototype like ($self, $name) or ($$$)
                for part in text.trim_matches(|c| c == '(' || c == ')').split(',') {
                    let p = part.trim();
                    if !p.is_empty() {
                        params.push(Parameter::new(p));
                    }
                }
            }
        }
        params
    }

    fn visit_use_statement(&mut self, node: Node) {
        // use Module::Name; or use Module::Name qw(...);
        let module = self.extract_use_module(node);
        if !module.is_empty()
            && module != "strict"
            && module != "warnings"
            && module != "utf8"
            && module != "feature"
            && module != "constant"
            && module != "overload"
            && module != "vars"
            && module != "base"
            && module != "parent"
        {
            self.imports.push(ImportRelation {
                importer: self.current_package.clone().unwrap_or_else(|| "main".to_string()),
                imported: module,
                symbols: Vec::new(),
                is_wildcard: false,
                alias: None,
            });
        } else if module == "parent" || module == "base" {
            // use parent 'SomeClass'; — extract the parent class name
            let parent = self.extract_use_list(node);
            for p in parent {
                self.imports.push(ImportRelation {
                    importer: self.current_package.clone().unwrap_or_else(|| "main".to_string()),
                    imported: p,
                    symbols: Vec::new(),
                    is_wildcard: false,
                    alias: None,
                });
            }
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child);
        }
    }

    fn extract_use_module(&self, node: Node) -> String {
        // use_no_statement has package_name field
        if let Some(pkg) = node.child_by_field_name("package_name") {
            return self.node_text(pkg);
        }
        // Fallback: look for package_name or identifier child
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "package_name" | "package_expression" | "identifier" => {
                    return self.node_text(child);
                }
                _ => {}
            }
        }
        String::new()
    }

    fn extract_use_list(&self, node: Node) -> Vec<String> {
        let mut list = Vec::new();
        let text = self.node_text(node);
        // Extract quoted strings from the statement
        for part in text.split_whitespace() {
            let cleaned = part
                .trim_matches(|c| c == '\'' || c == '"' || c == ',' || c == ';' || c == '(' || c == ')');
            if !cleaned.is_empty() && cleaned.contains("::") {
                list.push(cleaned.to_string());
            }
        }
        list
    }

    fn visit_require_expression(&mut self, node: Node) {
        let text = self.node_text(node);
        // require 'Module/Name.pm' or require Module::Name
        let module = text
            .trim_start_matches("require")
            .trim()
            .trim_matches(|c| c == '\'' || c == '"' || c == ';')
            .replace('/', "::")
            .replace(".pm", "");
        if !module.is_empty() {
            self.imports.push(ImportRelation {
                importer: self.current_package.clone().unwrap_or_else(|| "main".to_string()),
                imported: module,
                symbols: Vec::new(),
                is_wildcard: false,
                alias: None,
            });
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child);
        }
    }

    fn visit_call_expression(&mut self, node: Node) {
        if let Some(ref caller) = self.current_function.clone() {
            // Get the function being called
            let callee = self.extract_callee_name(node);
            if !callee.is_empty() {
                self.calls.push(CallRelation {
                    caller: caller.clone(),
                    callee,
                    call_site_line: node.start_position().row + 1,
                    is_direct: true,
                    struct_type: None,
                    field_name: None,
                });
            }
        }
    }

    fn extract_callee_name(&self, node: Node) -> String {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "identifier" | "package_expression" => {
                    return self.node_text(child);
                }
                _ => {}
            }
        }
        String::new()
    }

    fn visit_body_for_calls(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "call_expression" | "method_call_expression" => {
                    self.visit_call_expression(child);
                    self.visit_body_for_calls(child);
                }
                _ => {
                    self.visit_body_for_calls(child);
                }
            }
        }
    }

    fn extract_doc_comment(&self, node: Node) -> Option<String> {
        if let Some(prev) = node.prev_sibling() {
            if prev.kind() == "comment" {
                let text = self.node_text(prev);
                if text.starts_with("##") || text.starts_with("#!") || text.starts_with("# ") {
                    return Some(text);
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
            "if_statement" | "unless_statement" => {
                builder.add_branch();
                builder.enter_scope();
            }
            "elsif_clause" | "else_clause" => {
                builder.add_branch();
            }
            "while_statement"
            | "until_statement"
            | "for_statement"
            | "foreach_statement" => {
                builder.add_loop();
                builder.enter_scope();
            }
            "binary_expression" => {
                let text = self.node_text(node);
                if text.contains(" && ") || text.contains(" || ") || text.contains(" and ") || text.contains(" or ") {
                    builder.add_logical_operator();
                }
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_for_complexity(child, builder);
        }

        match node.kind() {
            "if_statement"
            | "unless_statement"
            | "while_statement"
            | "until_statement"
            | "for_statement"
            | "foreach_statement" => {
                builder.exit_scope();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_visit(source: &[u8]) -> PerlVisitor<'_> {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&crate::ts_perl::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut visitor = PerlVisitor::new(source);
        visitor.visit_node(tree.root_node());
        visitor
    }

    #[test]
    fn test_visitor_function_extraction() {
        let source = b"sub greet {\n    print \"Hello\\n\";\n}\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "greet");
    }

    #[test]
    fn test_visitor_package_extraction() {
        let source = b"package MyApp::User;\nsub new { }\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.classes.len(), 1);
        assert!(visitor.classes[0].name.contains("MyApp"));
        assert_eq!(visitor.functions.len(), 1);
    }

    #[test]
    fn test_visitor_use_extraction() {
        let source = b"use Moose;\nuse Data::Dumper;\n";
        let visitor = parse_and_visit(source);
        assert!(visitor.imports.len() >= 1);
    }
}
