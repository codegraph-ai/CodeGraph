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
            "function_definition" | "function_declaration" => {
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
    use codegraph_parser_api::BODY_PREFIX_MAX_CHARS;

    fn parse_and_visit(source: &[u8]) -> ScalaVisitor<'_> {
        use tree_sitter::Parser;

        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_scala::LANGUAGE.into())
            .unwrap();
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

    #[test]
    fn test_empty_source_is_empty() {
        let visitor = parse_and_visit(b"");
        assert!(visitor.functions.is_empty());
        assert!(visitor.classes.is_empty());
        assert!(visitor.traits.is_empty());
        assert!(visitor.imports.is_empty());
        assert!(visitor.calls.is_empty());
        assert!(visitor.inheritance.is_empty());
        assert!(visitor.implementations.is_empty());
    }

    #[test]
    fn test_function_metadata_defaults() {
        let source = b"def add(a: Int, b: Int): Int = a + b";
        let visitor = parse_and_visit(source);

        let f = &visitor.functions[0];
        assert_eq!(f.visibility, "public");
        assert!(!f.is_async);
        assert!(!f.is_test);
        // top-level function has no enclosing class -> is_static true
        assert!(f.is_static);
        assert!(f.parent_class.is_none());
        assert_eq!(f.line_start, 1);
        assert_eq!(f.line_end, 1);
        assert!(f.attributes.is_empty());
    }

    #[test]
    fn test_function_signature_first_line() {
        let source = b"def add(a: Int, b: Int): Int = {\n  a + b\n}";
        let visitor = parse_and_visit(source);
        assert_eq!(
            visitor.functions[0].signature,
            "def add(a: Int, b: Int): Int = {"
        );
    }

    #[test]
    fn test_function_parameters_with_types() {
        let source = b"def add(a: Int, b: String): Int = 0";
        let visitor = parse_and_visit(source);
        let params = &visitor.functions[0].parameters;
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].name, "a");
        assert_eq!(params[0].type_annotation.as_deref(), Some("Int"));
        assert_eq!(params[1].name, "b");
        assert_eq!(params[1].type_annotation.as_deref(), Some("String"));
    }

    #[test]
    fn test_function_return_type() {
        let source = b"def five(): Int = 5";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions[0].return_type.as_deref(), Some("Int"));
    }

    #[test]
    fn test_function_is_test_prefix() {
        let source = b"def testAdd(): Unit = ()";
        let visitor = parse_and_visit(source);
        assert!(visitor.functions[0].is_test);
    }

    #[test]
    fn test_function_body_prefix_present() {
        let source = b"def work(): Int = {\n  val x = 1\n  x\n}";
        let visitor = parse_and_visit(source);
        assert!(visitor.functions[0].body_prefix.is_some());
    }

    #[test]
    fn test_abstract_method_has_no_body() {
        // a def with no body inside a trait is abstract
        let source = b"trait T {\n  def greet(name: String): String\n}";
        let visitor = parse_and_visit(source);
        let f = visitor
            .functions
            .iter()
            .find(|f| f.name == "greet")
            .expect("greet method extracted");
        assert!(f.is_abstract);
        assert!(f.body_prefix.is_none());
        assert!(f.complexity.is_none());
    }

    #[test]
    fn test_method_parent_class_and_not_static() {
        let source = b"class Calc {\n  def add(a: Int): Int = a\n}";
        let visitor = parse_and_visit(source);
        let f = visitor
            .functions
            .iter()
            .find(|f| f.name == "add")
            .expect("method extracted");
        assert_eq!(f.parent_class.as_deref(), Some("Calc"));
        assert!(!f.is_static);
    }

    #[test]
    fn test_function_baseline_complexity() {
        let source = b"def straight(): Int = {\n  val x = 1\n  x\n}";
        let visitor = parse_and_visit(source);
        let c = visitor.functions[0].complexity.as_ref().unwrap();
        assert_eq!(c.cyclomatic_complexity, 1);
    }

    #[test]
    fn test_if_expression_raises_complexity() {
        let source = b"def choose(x: Int): Int = {\n  if (x > 0) 1 else 2\n}";
        let visitor = parse_and_visit(source);
        let c = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(c.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_for_expression_raises_complexity() {
        let source = b"def loop(n: Int): Int = {\n  for (i <- 0 until n) println(i)\n  0\n}";
        let visitor = parse_and_visit(source);
        let c = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(c.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_match_expression_raises_complexity() {
        let source =
            b"def kind(x: Int): String = x match {\n  case 0 => \"zero\"\n  case _ => \"other\"\n}";
        let visitor = parse_and_visit(source);
        let c = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(c.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_class_metadata_defaults() {
        let source = b"class Person(val name: String)";
        let visitor = parse_and_visit(source);
        let c = &visitor.classes[0];
        assert_eq!(c.visibility, "public");
        assert!(!c.is_abstract);
        assert!(!c.is_interface);
        assert_eq!(c.line_start, 1);
    }

    #[test]
    fn test_abstract_class() {
        let source = b"abstract class Shape {\n  def area(): Double\n}";
        let visitor = parse_and_visit(source);
        let c = visitor
            .classes
            .iter()
            .find(|c| c.name == "Shape")
            .expect("class extracted");
        assert!(c.is_abstract);
    }

    #[test]
    fn test_case_class_attribute() {
        let source = b"case class Point(x: Int, y: Int)";
        let visitor = parse_and_visit(source);
        let c = &visitor.classes[0];
        assert!(c.attributes.contains(&"case".to_string()));
    }

    #[test]
    fn test_class_extends_inheritance() {
        let source = b"class Dog extends Animal";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.inheritance.len(), 1);
        assert_eq!(visitor.inheritance[0].child, "Dog");
        assert_eq!(visitor.inheritance[0].parent, "Animal");
    }

    #[test]
    fn test_import_wildcard_underscore() {
        let source = b"import scala.collection.mutable._";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 1);
        assert!(visitor.imports[0].is_wildcard);
        assert_eq!(visitor.imports[0].importer, "main");
    }

    #[test]
    fn test_import_selector_wildcard() {
        let source = b"import scala.collection.mutable.{Map, Set}";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 1);
        // contains ".{" so flagged as wildcard by the visitor's heuristic
        assert!(visitor.imports[0].is_wildcard);
    }

    #[test]
    fn test_non_wildcard_import() {
        let source = b"import scala.collection.mutable.ListBuffer";
        let visitor = parse_and_visit(source);
        assert!(!visitor.imports[0].is_wildcard);
        assert!(visitor.imports[0].symbols.is_empty());
        assert!(visitor.imports[0].alias.is_none());
    }

    #[test]
    fn test_object_attribute() {
        let source = b"object Config {\n  val port = 8080\n}";
        let visitor = parse_and_visit(source);
        let c = &visitor.classes[0];
        assert_eq!(c.name, "Config");
        assert!(c.attributes.contains(&"object".to_string()));
        assert!(!c.is_abstract);
    }

    #[test]
    fn test_multiple_functions() {
        let source = b"def a(): Int = 1\ndef b(): Int = 2\ndef c(): Int = 3";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 3);
    }

    #[test]
    fn test_function_line_offset_by_blank_lines() {
        let source = b"\n\ndef add(a: Int, b: Int): Int = a + b";
        let visitor = parse_and_visit(source);
        // two leading blank lines push the 1-indexed def onto line 3
        assert_eq!(visitor.functions[0].line_start, 3);
        assert_eq!(visitor.functions[0].line_end, 3);
    }

    #[test]
    fn test_function_body_prefix_truncated() {
        // build a body whose text far exceeds BODY_PREFIX_MAX_CHARS
        let filler = "x + ".repeat(BODY_PREFIX_MAX_CHARS);
        let source = format!("def big(): Int = {{\n  {filler}0\n}}");
        let visitor = parse_and_visit(source.as_bytes());
        let bp = visitor.functions[0].body_prefix.as_ref().unwrap();
        assert_eq!(bp.chars().count(), BODY_PREFIX_MAX_CHARS);
    }

    #[test]
    fn test_while_expression_raises_complexity() {
        let source = b"def loop(): Unit = {\n  while (true) println(1)\n}";
        let visitor = parse_and_visit(source);
        let c = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(c.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_logical_operator_raises_complexity() {
        let source = b"def both(a: Boolean, b: Boolean): Boolean = {\n  a && b\n}";
        let visitor = parse_and_visit(source);
        let c = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(c.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_call_extraction_default_metadata() {
        let source = b"def helper(): Int = 1\ndef caller(): Int = {\n  helper()\n}";
        let visitor = parse_and_visit(source);
        let call = visitor
            .calls
            .iter()
            .find(|c| c.callee == "helper")
            .expect("helper() call extracted");
        assert_eq!(call.caller, "caller");
        assert!(call.is_direct);
        assert!(call.struct_type.is_none());
        assert!(call.field_name.is_none());
        // call sits on the 3rd line of the source (1-indexed)
        assert_eq!(call.call_site_line, 3);
    }

    #[test]
    fn test_trait_metadata_defaults() {
        let source = b"trait Greeter {\n  def greet(): String\n}";
        let visitor = parse_and_visit(source);
        let t = &visitor.traits[0];
        assert_eq!(t.visibility, "public");
        assert_eq!(t.line_start, 1);
        assert!(t.parent_traits.is_empty());
        assert!(t.required_methods.is_empty());
        assert!(t.attributes.is_empty());
    }

    #[test]
    fn test_object_body_prefix_present() {
        let source = b"object Config {\n  val port = 8080\n}";
        let visitor = parse_and_visit(source);
        let c = &visitor.classes[0];
        assert!(c.body_prefix.is_some());
        assert!(c.body_prefix.as_ref().unwrap().contains("port"));
    }

    #[test]
    fn test_object_method_parented_and_not_static() {
        let source = b"object Util {\n  def helper(): Int = 1\n}";
        let visitor = parse_and_visit(source);
        let f = visitor
            .functions
            .iter()
            .find(|f| f.name == "helper")
            .expect("object method extracted");
        // methods inside an object take the object as their enclosing class
        assert_eq!(f.parent_class.as_deref(), Some("Util"));
        assert!(!f.is_static);
    }

    #[test]
    fn test_class_body_prefix_present() {
        let source = b"class Calc {\n  def add(a: Int): Int = a\n}";
        let visitor = parse_and_visit(source);
        let c = visitor
            .classes
            .iter()
            .find(|c| c.name == "Calc")
            .expect("class extracted");
        assert!(c.body_prefix.is_some());
    }

    #[test]
    fn test_multiple_imports_preserved() {
        let source = b"import scala.collection.mutable.ListBuffer\nimport java.util.HashMap";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 2);
        assert_eq!(
            visitor.imports[0].imported,
            "scala.collection.mutable.ListBuffer"
        );
        assert_eq!(visitor.imports[1].imported, "java.util.HashMap");
    }

    #[test]
    fn test_case_class_line_end_spans_body() {
        let source = b"class Box {\n  def a(): Int = 1\n  def b(): Int = 2\n}";
        let visitor = parse_and_visit(source);
        let c = visitor
            .classes
            .iter()
            .find(|c| c.name == "Box")
            .expect("class extracted");
        // class spans 4 physical lines (1-indexed closing brace on line 4)
        assert_eq!(c.line_start, 1);
        assert_eq!(c.line_end, 4);
    }

    #[test]
    fn test_function_not_test_when_no_prefix() {
        let source = b"def addition(): Boolean = true";
        let visitor = parse_and_visit(source);
        // is_test is a pure name-prefix check, so a non-"test" name is false
        assert!(!visitor.functions[0].is_test);
    }

    #[test]
    fn test_function_no_parameters() {
        let source = b"def now(): Long = 0L";
        let visitor = parse_and_visit(source);
        assert!(visitor.functions[0].parameters.is_empty());
    }

    #[test]
    fn test_doc_comment_block_attached() {
        let source = b"/** Adds two numbers. */\ndef add(a: Int, b: Int): Int = a + b";
        let visitor = parse_and_visit(source);
        let doc = visitor.functions[0]
            .doc_comment
            .as_ref()
            .expect("doc comment");
        assert!(doc.contains("Adds two numbers"));
    }

    #[test]
    fn test_doc_comment_triple_slash_attached() {
        let source = b"/// A doc line\ndef f(): Int = 1";
        let visitor = parse_and_visit(source);
        // extract_doc_comment accepts a /// prefixed comment as well as /**
        assert!(visitor.functions[0].doc_comment.is_some());
    }

    #[test]
    fn test_plain_line_comment_not_doc() {
        let source = b"// just a note\ndef f(): Int = 1";
        let visitor = parse_and_visit(source);
        // only /** and /// prefixes count as doc comments
        assert!(visitor.functions[0].doc_comment.is_none());
    }

    #[test]
    fn test_catch_clause_raises_complexity() {
        let source =
            b"def risky(): Int = {\n  try {\n    1\n  } catch {\n    case _: Exception => 0\n  }\n}";
        let visitor = parse_and_visit(source);
        let c = visitor.functions[0].complexity.as_ref().unwrap();
        // a catch_clause is recorded as an exception handler, raising CC above baseline
        assert!(c.exception_handlers >= 1);
        assert!(c.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_finally_clause_raises_exception_handlers() {
        let source = b"def risky(): Int = {\n  try {\n    1\n  } finally {\n    println(1)\n  }\n}";
        let visitor = parse_and_visit(source);
        let c = visitor.functions[0].complexity.as_ref().unwrap();
        // a finally_clause (no catch) is recorded as an exception handler, a branch
        // the catch-only test never reaches
        assert!(c.exception_handlers >= 1);
    }

    #[test]
    fn test_match_with_three_cases_accumulates_branches() {
        let source = b"def kind(x: Int): String = x match {\n  case 0 => \"a\"\n  case 1 => \"b\"\n  case _ => \"c\"\n}";
        let visitor = parse_and_visit(source);
        let c = visitor.functions[0].complexity.as_ref().unwrap();
        // three case clauses each add a branch: 1 + 3 = 4
        assert_eq!(c.cyclomatic_complexity, 4);
    }

    #[test]
    fn test_trait_method_parented_to_trait() {
        let source = b"trait Greeter {\n  def greet(): String = \"hi\"\n}";
        let visitor = parse_and_visit(source);
        let f = visitor
            .functions
            .iter()
            .find(|f| f.name == "greet")
            .expect("trait method extracted");
        // a def inside a trait body takes the trait as its enclosing class
        assert_eq!(f.parent_class.as_deref(), Some("Greeter"));
        assert!(!f.is_static);
    }

    #[test]
    fn test_class_extends_with_constructor_args() {
        let source = b"class Dog extends Animal(4)";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.inheritance.len(), 1);
        // parent name is split on '(' so the constructor args are dropped
        assert_eq!(visitor.inheritance[0].parent, "Animal");
        assert_eq!(visitor.inheritance[0].child, "Dog");
    }

    #[test]
    fn test_call_inside_class_method_attributed_to_method() {
        let source =
            b"class Svc {\n  def helper(): Int = 1\n  def run(): Int = {\n    helper()\n  }\n}";
        let visitor = parse_and_visit(source);
        let call = visitor
            .calls
            .iter()
            .find(|c| c.callee == "helper")
            .expect("helper() call extracted");
        // the call is attributed to the enclosing method, not the class
        assert_eq!(call.caller, "run");
        assert!(call.is_direct);
    }

    #[test]
    fn test_two_calls_in_one_body_recorded_separately() {
        let source = b"def a(): Int = 1\ndef b(): Int = 2\ndef caller(): Int = {\n  a()\n  b()\n}";
        let visitor = parse_and_visit(source);
        let from_caller: Vec<_> = visitor
            .calls
            .iter()
            .filter(|c| c.caller == "caller")
            .collect();
        assert_eq!(from_caller.len(), 2);
    }

    #[test]
    fn test_top_level_call_has_no_caller() {
        // a call outside any function has no current_function, so it is dropped
        let source = b"println(\"hi\")";
        let visitor = parse_and_visit(source);
        assert!(visitor.calls.is_empty());
    }
}
