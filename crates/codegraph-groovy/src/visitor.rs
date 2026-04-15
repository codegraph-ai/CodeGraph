// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting Groovy entities

use codegraph_parser_api::{
    truncate_body_prefix, CallRelation, ClassEntity, ComplexityBuilder, ComplexityMetrics,
    FunctionEntity, ImportRelation, Parameter,
};
use tree_sitter::Node;

pub(crate) struct GroovyVisitor<'a> {
    pub source: &'a [u8],
    pub functions: Vec<FunctionEntity>,
    pub classes: Vec<ClassEntity>,
    pub imports: Vec<ImportRelation>,
    pub calls: Vec<CallRelation>,
    current_class: Option<String>,
    current_function: Option<String>,
}

impl<'a> GroovyVisitor<'a> {
    pub fn new(source: &'a [u8]) -> Self {
        Self {
            source,
            functions: Vec::new(),
            classes: Vec::new(),
            imports: Vec::new(),
            calls: Vec::new(),
            current_class: None,
            current_function: None,
        }
    }

    fn node_text(&self, node: Node) -> String {
        node.utf8_text(self.source).unwrap_or("").to_string()
    }

    pub fn visit_node(&mut self, node: Node) {
        match node.kind() {
            "import_declaration" => {
                self.visit_import(node);
                return;
            }
            "class_declaration" => {
                self.visit_class(node);
                return;
            }
            "method_declaration" => {
                if self.current_class.is_none() {
                    self.visit_top_level_method(node);
                    return;
                }
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child);
        }
    }

    fn visit_import(&mut self, node: Node) {
        // import groovy.json.JsonSlurper
        // The node text is the full "import groovy.json.JsonSlurper"
        let text = self.node_text(node);
        let path = text
            .trim_start_matches("import")
            .trim()
            .trim_end_matches(';')
            .trim()
            .to_string();

        if !path.is_empty() {
            self.imports.push(ImportRelation {
                importer: "main".to_string(),
                imported: path,
                symbols: Vec::new(),
                is_wildcard: false,
                alias: None,
            });
        }
    }

    fn visit_class(&mut self, node: Node) {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_default();

        if name.is_empty() {
            return;
        }

        let visibility = self.extract_modifiers_visibility(node);
        let is_abstract = self.has_modifier(node, "abstract");

        let doc_comment = self.extract_doc_comment(node);

        let body_prefix = node
            .child_by_field_name("body")
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| truncate_body_prefix(t).to_string());

        let mut class_entity = ClassEntity {
            name: name.clone(),
            visibility,
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_abstract,
            is_interface: false,
            base_classes: Vec::new(),
            implemented_traits: Vec::new(),
            doc_comment,
            attributes: Vec::new(),
            type_parameters: Vec::new(),
            body_prefix,
            methods: Vec::new(),
            fields: Vec::new(),
        };

        let previous_class = self.current_class.take();
        self.current_class = Some(name.clone());

        if let Some(body) = node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for child in body.children(&mut cursor) {
                match child.kind() {
                    "method_declaration" | "constructor_declaration" => {
                        if let Some(method) = self.extract_method(child) {
                            class_entity.methods.push(method);
                        }
                    }
                    _ => {}
                }
            }
        }

        self.current_class = previous_class;
        self.classes.push(class_entity);
    }

    fn visit_top_level_method(&mut self, node: Node) {
        if let Some(func) = self.extract_method(node) {
            let previous_function = self.current_function.take();
            self.current_function = Some(func.name.clone());
            self.functions.push(func);
            self.current_function = previous_function;
        }
    }

    fn extract_method(&self, node: Node) -> Option<FunctionEntity> {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))?;

        if name.is_empty() {
            return None;
        }

        // Build signature from the first line of the method text
        let signature = self
            .node_text(node)
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .to_string();

        let visibility = self.extract_modifiers_visibility(node);
        let is_static = self.has_modifier(node, "static");
        let is_abstract = self.has_modifier(node, "abstract");

        let doc_comment = self.extract_doc_comment(node);
        let parameters = self.extract_parameters(node);

        let body_prefix = node
            .child_by_field_name("body")
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| truncate_body_prefix(t).to_string());

        let complexity = node
            .child_by_field_name("body")
            .map(|body| self.calculate_complexity(body));

        // Return type: `type` field (may be `def` keyword text or an actual type)
        let return_type = node
            .child_by_field_name("type")
            .map(|n| self.node_text(n));

        let is_test = self.node_is_test(node);

        Some(FunctionEntity {
            name,
            signature,
            visibility,
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_async: false,
            is_test,
            is_static,
            is_abstract,
            parameters,
            return_type,
            doc_comment,
            attributes: Vec::new(),
            parent_class: self.current_class.clone(),
            complexity,
            body_prefix,
        })
    }

    fn extract_modifiers_visibility(&self, node: Node) -> String {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "modifiers" {
                let text = self.node_text(child);
                if text.contains("private") {
                    return "private".to_string();
                } else if text.contains("protected") {
                    return "protected".to_string();
                } else if text.contains("public") {
                    return "public".to_string();
                }
            }
        }
        // Groovy default is public
        "public".to_string()
    }

    fn has_modifier(&self, node: Node, modifier: &str) -> bool {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "modifiers" {
                return self.node_text(child).contains(modifier);
            }
        }
        false
    }

    fn node_is_test(&self, node: Node) -> bool {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "modifiers" {
                let text = self.node_text(child);
                if text.contains("@Test") {
                    return true;
                }
            }
        }
        false
    }

    fn extract_parameters(&self, node: Node) -> Vec<Parameter> {
        let mut params = Vec::new();
        if let Some(params_node) = node.child_by_field_name("parameters") {
            let mut cursor = params_node.walk();
            for child in params_node.children(&mut cursor) {
                if child.kind() == "formal_parameter" {
                    // formal_parameter has `name` and `type` fields
                    if let Some(name_node) = child.child_by_field_name("name") {
                        let name = self.node_text(name_node);
                        if !name.is_empty() {
                            let type_name = child
                                .child_by_field_name("type")
                                .map(|t| self.node_text(t));
                            let mut p = Parameter::new(name);
                            if let Some(t) = type_name {
                                p = p.with_type(t);
                            }
                            params.push(p);
                        }
                    }
                }
            }
        }
        params
    }

    fn extract_doc_comment(&self, node: Node) -> Option<String> {
        if let Some(prev) = node.prev_sibling() {
            let kind = prev.kind();
            if kind == "block_comment" || kind == "line_comment" {
                let text = self.node_text(prev);
                if text.starts_with("/**") || text.starts_with("///") {
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
            "if_statement" => {
                builder.add_branch();
                builder.enter_scope();
            }
            "else_clause" => {
                builder.add_branch();
            }
            "for_statement" | "enhanced_for_statement" | "while_statement"
            | "do_while_statement" => {
                builder.add_loop();
                builder.enter_scope();
            }
            "switch_expression" | "switch_statement" => {
                builder.add_branch();
                builder.enter_scope();
            }
            "catch_clause" => {
                builder.add_branch();
            }
            "ternary_expression" => {
                builder.add_branch();
            }
            "return_statement" => {
                builder.add_early_return();
            }
            // Logical operators appear as binary_expression children
            "&&" | "||" => {
                builder.add_logical_operator();
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_for_complexity(child, builder);
        }

        match node.kind() {
            "if_statement"
            | "for_statement"
            | "enhanced_for_statement"
            | "while_statement"
            | "do_while_statement"
            | "switch_expression"
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

    fn parse_and_visit(source: &[u8]) -> GroovyVisitor<'_> {
        use tree_sitter::Parser;

        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_groovy::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = GroovyVisitor::new(source);
        visitor.visit_node(tree.root_node());
        visitor
    }

    #[test]
    fn test_visitor_class_extraction() {
        let source = br#"
class UserService {
    def greet(String name) {
        println "Hello, ${name}"
    }
}
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "UserService");
        assert_eq!(visitor.classes[0].methods.len(), 1);
        assert_eq!(visitor.classes[0].methods[0].name, "greet");
    }

    #[test]
    fn test_visitor_import_extraction() {
        let source = b"import groovy.json.JsonSlurper\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "groovy.json.JsonSlurper");
    }

    #[test]
    fn test_visitor_method_visibility() {
        let source = br#"
class Svc {
    private void validate(String s) {}
    def publicMethod() {}
}
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.classes.len(), 1);
        let methods = &visitor.classes[0].methods;
        assert_eq!(methods.len(), 2);
        let validate = methods.iter().find(|m| m.name == "validate").unwrap();
        assert_eq!(validate.visibility, "private");
        let public_m = methods.iter().find(|m| m.name == "publicMethod").unwrap();
        assert_eq!(public_m.visibility, "public");
    }

    #[test]
    fn test_visitor_parameters() {
        let source = br#"
class Svc {
    def createUser(String name, String email) {}
}
"#;
        let visitor = parse_and_visit(source);
        let method = &visitor.classes[0].methods[0];
        assert_eq!(method.parameters.len(), 2);
        assert_eq!(method.parameters[0].name, "name");
        assert_eq!(method.parameters[1].name, "email");
    }
}
