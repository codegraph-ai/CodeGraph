// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting Dart entities

use codegraph_parser_api::{
    truncate_body_prefix, CallRelation, ClassEntity, ComplexityBuilder, ComplexityMetrics,
    FunctionEntity, ImplementationRelation, ImportRelation, InheritanceRelation, Parameter,
    TraitEntity,
};
use tree_sitter::Node;

/// Find a direct child node with the given kind.
fn find_child_by_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let count = node.child_count();
    for i in 0..count {
        if let Some(child) = node.child(i) {
            if child.kind() == kind {
                return Some(child);
            }
        }
    }
    None
}

pub(crate) struct DartVisitor<'a> {
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

impl<'a> DartVisitor<'a> {
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

    pub fn visit_node(&mut self, node: Node) {
        match node.kind() {
            "function_signature" => {
                // Top-level function declaration
                if self.current_class.is_none() {
                    self.visit_function(node);
                }
            }
            "method_signature" => {
                self.visit_method(node);
                return; // Don't recurse into function_signature children
            }
            "class_definition" | "class_declaration" => {
                self.visit_class(node);
                return; // Don't recurse; visit_class handles body
            }
            "import_or_export" => {
                self.visit_import(node);
            }
            "enum_declaration" => {
                self.visit_enum(node);
                return;
            }
            "mixin_declaration" => {
                self.visit_mixin(node);
                return;
            }
            // ABI 15 grammar parses all constructs properly; no ERROR fallback needed
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child);
        }
    }

    fn visit_function(&mut self, node: Node) {
        let name = self.extract_function_name(node);
        if name.is_empty() {
            return;
        }

        let signature = self.node_text(node);
        let doc_comment = self.extract_doc_comment(node);
        let parameters = self.extract_parameters(node);
        let return_type = self.extract_return_type(node);

        // function_body is a sibling of function_signature (both children of source_file)
        let body_node = node.next_sibling().filter(|n| n.kind() == "function_body");

        let body_prefix = body_node
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| truncate_body_prefix(t).to_string());

        let complexity = body_node.map(|body| self.calculate_complexity(body));

        let is_async = signature.contains("async")
            || body_node
                .map(|b| self.node_text(b).starts_with("async"))
                .unwrap_or(false);

        let line_end = body_node
            .map(|b| b.end_position().row + 1)
            .unwrap_or_else(|| node.end_position().row + 1);

        let func = FunctionEntity {
            name: name.clone(),
            signature: signature.lines().next().unwrap_or("").to_string(),
            visibility: if name.starts_with('_') {
                "private"
            } else {
                "public"
            }
            .to_string(),
            line_start: node.start_position().row + 1,
            line_end,
            is_async,
            is_test: false,
            is_static: false,
            is_abstract: body_node.is_none(),
            parameters,
            return_type,
            doc_comment,
            attributes: Vec::new(),
            parent_class: None,
            complexity,
            body_prefix,
        };

        self.functions.push(func);

        // Visit body for calls
        let previous_function = self.current_function.take();
        self.current_function = Some(name);

        if let Some(body) = body_node {
            self.visit_body_for_calls(body);
        }

        self.current_function = previous_function;
    }

    fn visit_method(&mut self, node: Node) {
        // method_signature wraps function_signature (or getter_signature);
        // extract name/params/return_type from the inner signature node.
        let inner_sig = find_child_by_kind(node, "function_signature");
        let sig_node = inner_sig.unwrap_or(node);

        let name = self.extract_function_name(sig_node);
        if name.is_empty() {
            // Try getter_signature
            if let Some(getter) = find_child_by_kind(node, "getter_signature") {
                let getter_name = self.extract_function_name(getter);
                if !getter_name.is_empty() {
                    self.visit_getter(node, getter, &getter_name);
                }
            }
            return;
        }

        let parent_node = node.parent(); // class_member
        let signature = self.node_text(sig_node);
        let doc_comment = parent_node
            .map(|p| self.extract_doc_comment(p))
            .unwrap_or_else(|| self.extract_doc_comment(node));
        let parameters = self.extract_parameters(sig_node);
        let return_type = self.extract_return_type(sig_node);

        // function_body is a sibling of method_signature inside class_member
        let body_node = node.next_sibling().filter(|n| n.kind() == "function_body");

        let body_prefix = body_node
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| truncate_body_prefix(t).to_string());

        let complexity = body_node.map(|body| self.calculate_complexity(body));

        let is_static = signature.contains("static ");
        let is_abstract = body_node.is_none();

        let is_async = body_node
            .map(|b| self.node_text(b).starts_with("async"))
            .unwrap_or(false);

        let line_start = parent_node
            .map(|p| p.start_position().row + 1)
            .unwrap_or_else(|| node.start_position().row + 1);
        let line_end = body_node
            .map(|b| b.end_position().row + 1)
            .unwrap_or_else(|| {
                parent_node
                    .map(|p| p.end_position().row + 1)
                    .unwrap_or_else(|| node.end_position().row + 1)
            });

        let func = FunctionEntity {
            name: name.clone(),
            signature: signature.lines().next().unwrap_or("").to_string(),
            visibility: if name.starts_with('_') {
                "private"
            } else {
                "public"
            }
            .to_string(),
            line_start,
            line_end,
            is_async,
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

        let previous_function = self.current_function.take();
        self.current_function = Some(name);

        if let Some(body) = body_node {
            self.visit_body_for_calls(body);
        }

        self.current_function = previous_function;
    }

    fn visit_getter(&mut self, method_sig_node: Node, getter_node: Node, name: &str) {
        let parent_node = method_sig_node.parent();
        let signature = self.node_text(getter_node);
        let doc_comment = parent_node
            .map(|p| self.extract_doc_comment(p))
            .unwrap_or_else(|| self.extract_doc_comment(method_sig_node));
        let return_type = self.extract_return_type(getter_node);

        let body_node = method_sig_node
            .next_sibling()
            .filter(|n| n.kind() == "function_body");

        let body_prefix = body_node
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| truncate_body_prefix(t).to_string());

        let complexity = body_node.map(|body| self.calculate_complexity(body));

        let line_start = parent_node
            .map(|p| p.start_position().row + 1)
            .unwrap_or_else(|| method_sig_node.start_position().row + 1);
        let line_end = body_node
            .map(|b| b.end_position().row + 1)
            .unwrap_or_else(|| method_sig_node.end_position().row + 1);

        let func = FunctionEntity {
            name: name.to_string(),
            signature: signature.lines().next().unwrap_or("").to_string(),
            visibility: if name.starts_with('_') {
                "private"
            } else {
                "public"
            }
            .to_string(),
            line_start,
            line_end,
            is_async: false,
            is_test: false,
            is_static: false,
            is_abstract: body_node.is_none(),
            parameters: Vec::new(),
            return_type,
            doc_comment,
            attributes: vec!["getter".to_string()],
            parent_class: self.current_class.clone(),
            complexity,
            body_prefix,
        };

        self.functions.push(func);
    }

    fn visit_class(&mut self, node: Node) {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_else(|| "Class".to_string());

        let doc_comment = self.extract_doc_comment(node);
        let is_abstract = {
            let text = self.node_text(node);
            text.starts_with("abstract ")
        };

        // Extract superclass
        let mut base_classes = Vec::new();
        if let Some(superclass) = node.child_by_field_name("superclass") {
            let sc_name = self.node_text(superclass);
            let sc_name = sc_name.trim_start_matches("extends ");
            if !sc_name.is_empty() {
                let parent_name = sc_name
                    .split('<')
                    .next()
                    .unwrap_or(sc_name)
                    .trim()
                    .to_string();
                base_classes.push(parent_name.clone());
                self.inheritance.push(InheritanceRelation {
                    child: name.clone(),
                    parent: parent_name,
                    order: 0,
                });
            }
        }

        // Extract implemented interfaces
        if let Some(interfaces) = node.child_by_field_name("interfaces") {
            let iface_text = self.node_text(interfaces);
            let iface_text = iface_text.trim_start_matches("implements ");
            for iface in iface_text.split(',') {
                let iface_name = iface.split('<').next().unwrap_or(iface).trim().to_string();
                if !iface_name.is_empty() {
                    self.implementations.push(ImplementationRelation {
                        implementor: name.clone(),
                        trait_name: iface_name,
                    });
                }
            }
        }

        let body_prefix = node
            .child_by_field_name("body")
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| truncate_body_prefix(t).to_string());

        let class_entity = ClassEntity {
            name: name.clone(),
            visibility: if name.starts_with('_') {
                "private"
            } else {
                "public"
            }
            .to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_abstract,
            is_interface: false,
            base_classes,
            implemented_traits: Vec::new(),
            methods: Vec::new(),
            fields: Vec::new(),
            doc_comment,
            attributes: Vec::new(),
            type_parameters: Vec::new(),
            body_prefix,
        };

        self.classes.push(class_entity);

        let previous_class = self.current_class.take();
        self.current_class = Some(name);

        if let Some(body) = node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for child in body.children(&mut cursor) {
                self.visit_node(child);
            }
        }

        self.current_class = previous_class;
    }

    fn visit_enum(&mut self, node: Node) {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_else(|| "Enum".to_string());

        let doc_comment = self.extract_doc_comment(node);

        let class_entity = ClassEntity {
            name: name.clone(),
            visibility: if name.starts_with('_') {
                "private"
            } else {
                "public"
            }
            .to_string(),
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
            body_prefix: None,
        };

        self.classes.push(class_entity);
    }

    fn visit_mixin(&mut self, node: Node) {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_else(|| "Mixin".to_string());

        let doc_comment = self.extract_doc_comment(node);

        let trait_entity = TraitEntity {
            name: name.clone(),
            visibility: if name.starts_with('_') {
                "private"
            } else {
                "public"
            }
            .to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            required_methods: Vec::new(),
            parent_traits: Vec::new(),
            doc_comment,
            attributes: vec!["mixin".to_string()],
        };

        self.traits.push(trait_entity);

        let previous_class = self.current_class.take();
        self.current_class = Some(name);

        if let Some(body) = node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for child in body.children(&mut cursor) {
                self.visit_node(child);
            }
        }

        self.current_class = previous_class;
    }

    fn visit_import(&mut self, node: Node) {
        let text = self.node_text(node);
        // import 'package:foo/bar.dart' as baz;
        // export 'package:foo/bar.dart';
        if !text.starts_with("import") {
            return;
        }

        // Extract the URI string
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "import_specification" || child.kind() == "library_import" {
                let mut inner_cursor = child.walk();
                for inner in child.children(&mut inner_cursor) {
                    if inner.kind() == "string_literal" || inner.kind() == "configurable_uri" {
                        let uri = self.node_text(inner);
                        let uri = uri.trim_matches(|c| c == '\'' || c == '"').to_string();
                        if !uri.is_empty() {
                            self.imports.push(ImportRelation {
                                importer: "main".to_string(),
                                imported: uri,
                                symbols: Vec::new(),
                                is_wildcard: false,
                                alias: None,
                            });
                            return;
                        }
                    }
                }
            }
            // Also check for direct string literal child
            if child.kind() == "string_literal" || child.kind() == "configurable_uri" {
                let uri = self.node_text(child);
                let uri = uri.trim_matches(|c| c == '\'' || c == '"').to_string();
                if !uri.is_empty() {
                    self.imports.push(ImportRelation {
                        importer: "main".to_string(),
                        imported: uri,
                        symbols: Vec::new(),
                        is_wildcard: false,
                        alias: None,
                    });
                    return;
                }
            }
        }

        // Fallback: extract URI from text
        if let Some(start) = text.find('\'').or_else(|| text.find('"')) {
            let quote_char = text.as_bytes()[start] as char;
            if let Some(end) = text[start + 1..].find(quote_char) {
                let uri = &text[start + 1..start + 1 + end];
                self.imports.push(ImportRelation {
                    importer: "main".to_string(),
                    imported: uri.to_string(),
                    symbols: Vec::new(),
                    is_wildcard: false,
                    alias: None,
                });
            }
        }
    }

    fn visit_body_for_calls(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "identifier" || child.kind() == "selector" {
                if let Some(ref caller) = self.current_function.clone() {
                    // Check if this identifier is being called (next sibling is arguments)
                    if let Some(next) = child.next_sibling() {
                        if next.kind() == "selector_suffix"
                            || next.kind() == "argument_part"
                            || next.kind() == "arguments"
                        {
                            let callee = self.node_text(child);
                            if !callee.is_empty() {
                                self.calls.push(CallRelation {
                                    caller: caller.clone(),
                                    callee,
                                    call_site_line: child.start_position().row + 1,
                                    is_direct: true,
                                    struct_type: None,
                                    field_name: None,
                                });
                            }
                        }
                    }
                }
            }
            self.visit_body_for_calls(child);
        }
    }

    fn extract_function_name(&self, node: Node) -> String {
        node.child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_default()
    }

    fn extract_parameters(&self, node: Node) -> Vec<Parameter> {
        let mut params = Vec::new();
        if let Some(params_node) = node.child_by_field_name("parameters") {
            let mut cursor = params_node.walk();
            for child in params_node.children(&mut cursor) {
                if child.kind() == "formal_parameter" || child.kind() == "constructor_param" {
                    let text = self.node_text(child);
                    let parts: Vec<&str> = text.split_whitespace().collect();
                    if let Some(name) = parts.last() {
                        let name = name.trim_end_matches(',');
                        params.push(Parameter::new(name));
                    }
                }
            }
        }
        params
    }

    fn extract_return_type(&self, node: Node) -> Option<String> {
        node.child_by_field_name("return_type")
            .map(|n| self.node_text(n))
    }

    fn extract_doc_comment(&self, node: Node) -> Option<String> {
        if let Some(prev) = node.prev_sibling() {
            if prev.kind() == "comment" || prev.kind() == "documentation_comment" {
                return Some(self.node_text(prev));
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
            "else" => {
                builder.add_branch();
            }
            "switch_statement" => {
                builder.enter_scope();
            }
            "switch_case" | "default_case" => {
                builder.add_branch();
            }
            "for_statement" | "while_statement" | "do_statement" => {
                builder.add_loop();
                builder.enter_scope();
            }
            "catch_clause" => {
                builder.add_exception_handler();
            }
            "finally_clause" => {
                builder.add_exception_handler();
            }
            "conditional_expression" => {
                builder.add_branch();
            }
            "binary_expression" => {
                let text = self.node_text(node);
                if text.contains("&&") || text.contains("||") {
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
            "if_statement" | "for_statement" | "while_statement" | "do_statement"
            | "switch_statement" => {
                builder.exit_scope();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_visit(source: &[u8]) -> DartVisitor<'_> {
        use tree_sitter::Parser;

        let mut parser = Parser::new();
        parser.set_language(&crate::ts_dart::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = DartVisitor::new(source);
        visitor.visit_node(tree.root_node());
        visitor
    }

    #[test]
    fn test_visitor_function_extraction() {
        let source = b"void greet(String name) {\n  print('Hello, $name');\n}";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "greet");
    }

    #[test]
    fn test_visitor_class_extraction() {
        let source = b"class Person {\n  String name;\n  int age;\n}";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "Person");
    }

    #[test]
    fn test_visitor_import_extraction() {
        let source = b"import 'dart:io';";
        let visitor = parse_and_visit(source);

        assert!(!visitor.imports.is_empty());
    }

}
