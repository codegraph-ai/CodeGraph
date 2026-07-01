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
            // In-class methods are `method_declaration`; top-level script functions
            // parse as `function_definition` with the same name/parameters/body/type
            // fields, so both route through the same top-level extractor.
            "method_declaration" | "function_definition" => {
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
        let return_type = node.child_by_field_name("type").map(|n| self.node_text(n));

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
                            let type_name =
                                child.child_by_field_name("type").map(|t| self.node_text(t));
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
            "for_statement"
            | "enhanced_for_statement"
            | "while_statement"
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

    #[test]
    fn test_empty_source() {
        let visitor = parse_and_visit(b"");
        assert!(visitor.functions.is_empty());
        assert!(visitor.classes.is_empty());
        assert!(visitor.imports.is_empty());
        assert!(visitor.calls.is_empty());
    }

    #[test]
    fn test_top_level_method() {
        // A method outside any class is extracted as a free function.
        let source = br#"
def standalone(String a) {
    println a
}
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "standalone");
        assert_eq!(visitor.functions[0].parent_class, None);
        assert!(visitor.classes.is_empty());
    }

    #[test]
    fn test_method_parent_class() {
        let source = br#"
class Svc {
    def doWork() {}
}
"#;
        let visitor = parse_and_visit(source);
        let method = &visitor.classes[0].methods[0];
        assert_eq!(method.parent_class.as_deref(), Some("Svc"));
    }

    #[test]
    fn test_static_method() {
        let source = br#"
class Svc {
    static def factory() {}
    def instanceMethod() {}
}
"#;
        let visitor = parse_and_visit(source);
        let methods = &visitor.classes[0].methods;
        let factory = methods.iter().find(|m| m.name == "factory").unwrap();
        assert!(factory.is_static);
        let inst = methods.iter().find(|m| m.name == "instanceMethod").unwrap();
        assert!(!inst.is_static);
    }

    #[test]
    fn test_protected_visibility() {
        let source = br#"
class Svc {
    protected def helper() {}
}
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.classes[0].methods[0].visibility, "protected");
    }

    #[test]
    fn test_abstract_class_and_method() {
        let source = br#"
abstract class Base {
    abstract def compute()
}
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.classes.len(), 1);
        assert!(visitor.classes[0].is_abstract);
        assert!(!visitor.classes[0].is_interface);
        let method = &visitor.classes[0].methods[0];
        assert!(method.is_abstract);
    }

    #[test]
    fn test_class_line_bounds() {
        let source = br#"class One {
    def a() {}
}
"#;
        let visitor = parse_and_visit(source);
        let class = &visitor.classes[0];
        assert_eq!(class.line_start, 1);
        assert_eq!(class.line_end, 3);
    }

    #[test]
    fn test_wildcard_import_path() {
        // visit_import always sets is_wildcard=false; the `*` stays in the path.
        let source = b"import groovy.json.*\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "groovy.json.*");
        assert!(!visitor.imports[0].is_wildcard);
        assert_eq!(visitor.imports[0].importer, "main");
    }

    #[test]
    fn test_multiple_imports() {
        let source = b"import a.B\nimport c.D\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 2);
        assert_eq!(visitor.imports[0].imported, "a.B");
        assert_eq!(visitor.imports[1].imported, "c.D");
    }

    #[test]
    fn test_constructor_declaration() {
        let source = br#"
class Svc {
    Svc(String name) {}
}
"#;
        let visitor = parse_and_visit(source);
        // constructor_declaration is collected as a method
        let ctor = visitor.classes[0].methods.iter().find(|m| m.name == "Svc");
        assert!(ctor.is_some(), "constructor should be extracted");
    }

    #[test]
    fn test_method_body_prefix_and_signature() {
        let source = br#"
class Svc {
    def greet(String name) {
        println "hi"
    }
}
"#;
        let visitor = parse_and_visit(source);
        let method = &visitor.classes[0].methods[0];
        assert!(method.signature.contains("greet"));
        assert!(method.body_prefix.is_some());
    }

    #[test]
    fn test_complexity_baseline() {
        let source = br#"
class Svc {
    def simple() {
        return 1
    }
}
"#;
        let visitor = parse_and_visit(source);
        let method = &visitor.classes[0].methods[0];
        let complexity = method.complexity.as_ref().unwrap();
        assert_eq!(complexity.cyclomatic_complexity, 1);
    }

    #[test]
    fn test_complexity_if_raises() {
        let source = br#"
class Svc {
    def branchy(int x) {
        if (x > 0) {
            return 1
        }
        return 0
    }
}
"#;
        let visitor = parse_and_visit(source);
        let method = &visitor.classes[0].methods[0];
        let complexity = method.complexity.as_ref().unwrap();
        assert!(complexity.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_complexity_loop_raises() {
        let source = br#"
class Svc {
    def loopy(int n) {
        for (int i = 0; i < n; i++) {
            println i
        }
    }
}
"#;
        let visitor = parse_and_visit(source);
        let method = &visitor.classes[0].methods[0];
        let complexity = method.complexity.as_ref().unwrap();
        assert!(complexity.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_multiple_classes() {
        let source = br#"
class A {
    def a() {}
}
class B {
    def b() {}
}
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.classes.len(), 2);
        assert_eq!(visitor.classes[0].name, "A");
        assert_eq!(visitor.classes[1].name, "B");
    }
}
