// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting Scala entities

use codegraph_parser_api::{
    truncate_body_prefix, CallRelation, ClassEntity, ComplexityBuilder, ComplexityMetrics,
    FunctionEntity, ImplementationRelation, ImportRelation, InheritanceRelation, Parameter,
    TraitEntity,
};
use tree_sitter::Node;

pub(crate) struct ScalaVisitor<'a> {
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

impl<'a> ScalaVisitor<'a> {
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
            "function_definition" => {
                self.visit_function(node);
                return;
            }
            "class_definition" => {
                self.visit_class(node);
                return;
            }
            "object_definition" => {
                self.visit_object(node);
                return;
            }
            "trait_definition" => {
                self.visit_trait(node);
                return;
            }
            "import_declaration" => {
                self.visit_import(node);
            }
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
            .unwrap_or_default();

        if name.is_empty() {
            return;
        }

        let signature = self
            .node_text(node)
            .lines()
            .next()
            .unwrap_or("")
            .to_string();

        let doc_comment = self.extract_doc_comment(node);
        let parameters = self.extract_parameters(node);
        let return_type = node
            .child_by_field_name("return_type")
            .map(|n| self.node_text(n));

        let body_prefix = node
            .child_by_field_name("body")
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| truncate_body_prefix(t).to_string());

        let complexity = node
            .child_by_field_name("body")
            .map(|body| self.calculate_complexity(body));

        let is_abstract = node.child_by_field_name("body").is_none();

        let func = FunctionEntity {
            name: name.clone(),
            signature,
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_async: false,
            is_test: name.starts_with("test"),
            is_static: self.current_class.is_none(),
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

        if let Some(body) = node.child_by_field_name("body") {
            self.visit_body_for_calls(body);
        }

        self.current_function = previous_function;
    }

    fn visit_class(&mut self, node: Node) {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_else(|| "Class".to_string());

        let doc_comment = self.extract_doc_comment(node);
        let text = self.node_text(node);
        let is_abstract = text.starts_with("abstract ");

        // Extract extends clause
        if let Some(extends) = node.child_by_field_name("extend") {
            let parent_text = self.node_text(extends);
            let parent_name = parent_text
                .trim_start_matches("extends ")
                .split(|c: char| c.is_whitespace() || c == '(' || c == '{')
                .next()
                .unwrap_or("")
                .to_string();
            if !parent_name.is_empty() {
                self.inheritance.push(InheritanceRelation {
                    child: name.clone(),
                    parent: parent_name,
                    order: 0,
                });
            }
        }

        let body_prefix = node
            .child_by_field_name("body")
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| truncate_body_prefix(t).to_string());

        let mut attrs = Vec::new();
        if text.starts_with("case ") {
            attrs.push("case".to_string());
        }

        let class_entity = ClassEntity {
            name: name.clone(),
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_abstract,
            is_interface: false,
            base_classes: Vec::new(),
            implemented_traits: Vec::new(),
            methods: Vec::new(),
            fields: Vec::new(),
            doc_comment,
            attributes: attrs,
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

    fn visit_object(&mut self, node: Node) {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_else(|| "Object".to_string());

        let doc_comment = self.extract_doc_comment(node);

        let body_prefix = node
            .child_by_field_name("body")
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| truncate_body_prefix(t).to_string());

        let class_entity = ClassEntity {
            name: name.clone(),
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
            attributes: vec!["object".to_string()],
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

    fn visit_trait(&mut self, node: Node) {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_else(|| "Trait".to_string());

        let doc_comment = self.extract_doc_comment(node);

        let trait_entity = TraitEntity {
            name: name.clone(),
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            required_methods: Vec::new(),
            parent_traits: Vec::new(),
            doc_comment,
            attributes: Vec::new(),
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
        let import_path = text.trim_start_matches("import ").trim().to_string();

        if !import_path.is_empty() {
            self.imports.push(ImportRelation {
                importer: "main".to_string(),
                imported: import_path,
                symbols: Vec::new(),
                is_wildcard: text.contains("._") || text.contains(".{"),
                alias: None,
            });
        }
    }

    fn visit_body_for_calls(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "call_expression" || child.kind() == "generic_function" {
                if let Some(ref caller) = self.current_function.clone() {
                    let callee = child
                        .child_by_field_name("function")
                        .map(|n| self.node_text(n))
                        .unwrap_or_default();
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
            self.visit_body_for_calls(child);
        }
    }

    fn extract_parameters(&self, node: Node) -> Vec<Parameter> {
        let mut params = Vec::new();
        if let Some(params_node) = node.child_by_field_name("parameters") {
            let mut cursor = params_node.walk();
            for child in params_node.children(&mut cursor) {
                if child.kind() == "class_parameter" || child.kind() == "parameter" {
                    let name = child
                        .child_by_field_name("name")
                        .map(|n| self.node_text(n))
                        .unwrap_or_default();
                    if !name.is_empty() {
                        let param_type =
                            child.child_by_field_name("type").map(|t| self.node_text(t));
                        let mut param = Parameter::new(name);
                        if let Some(t) = param_type {
                            param = param.with_type(t);
                        }
                        params.push(param);
                    }
                }
            }
        }
        params
    }

    fn extract_doc_comment(&self, node: Node) -> Option<String> {
        if let Some(prev) = node.prev_sibling() {
            if prev.kind() == "comment" || prev.kind() == "block_comment" {
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
            "if_expression" => {
                builder.add_branch();
                builder.enter_scope();
            }
            "else_clause" => {
                builder.add_branch();
            }
            "match_expression" => {
                builder.enter_scope();
            }
            "case_clause" => {
                builder.add_branch();
            }
            "for_expression" | "while_expression" => {
                builder.add_loop();
                builder.enter_scope();
            }
            "catch_clause" => {
                builder.add_exception_handler();
            }
            "finally_clause" => {
                builder.add_exception_handler();
            }
            "infix_expression" => {
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
            "if_expression" | "for_expression" | "while_expression" | "match_expression" => {
                builder.exit_scope();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_visit(source: &[u8]) -> ScalaVisitor<'_> {
        use tree_sitter::Parser;

        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_scala::LANGUAGE.into()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = ScalaVisitor::new(source);
        visitor.visit_node(tree.root_node());
        visitor
    }

    #[test]
    fn test_visitor_function_extraction() {
        let source = b"def add(a: Int, b: Int): Int = a + b";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "add");
    }

    #[test]
    fn test_visitor_class_extraction() {
        let source = b"class Person(val name: String, val age: Int)";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "Person");
    }

    #[test]
    fn test_visitor_trait_extraction() {
        let source = b"trait Greeter {\n  def greet(name: String): String\n}";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.traits.len(), 1);
        assert_eq!(visitor.traits[0].name, "Greeter");
    }

    #[test]
    fn test_visitor_object_extraction() {
        let source =
            b"object Main {\n  def main(args: Array[String]): Unit = println(\"Hello\")\n}";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "Main");
        assert!(visitor.classes[0]
            .attributes
            .contains(&"object".to_string()));
    }

    #[test]
    fn test_visitor_import_extraction() {
        let source = b"import scala.collection.mutable.ListBuffer";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 1);
    }
}
