// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting Ruby entities

use codegraph_parser_api::{
    truncate_body_prefix, CallRelation, ClassEntity, ComplexityBuilder, ComplexityMetrics,
    FunctionEntity, ImplementationRelation, ImportRelation, InheritanceRelation, Parameter,
    TraitEntity,
};
use tree_sitter::Node;

pub struct RubyVisitor<'a> {
    pub source: &'a [u8],
    pub functions: Vec<FunctionEntity>,
    pub classes: Vec<ClassEntity>,
    pub traits: Vec<TraitEntity>,
    pub imports: Vec<ImportRelation>,
    pub calls: Vec<CallRelation>,
    pub inheritance: Vec<InheritanceRelation>,
    pub implementations: Vec<ImplementationRelation>,
    current_module: Option<String>,
    current_class: Option<String>,
    current_function: Option<String>,
}

impl<'a> RubyVisitor<'a> {
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
            current_module: None,
            current_class: None,
            current_function: None,
        }
    }

    fn node_text(&self, node: Node) -> String {
        node.utf8_text(self.source).unwrap_or("").to_string()
    }

    pub fn visit_node(&mut self, node: Node) {
        let should_recurse = match node.kind() {
            "method" => {
                self.visit_method(node);
                false // Don't recurse, visit_method handles body
            }
            "singleton_method" => {
                self.visit_singleton_method(node);
                false
            }
            "class" => {
                self.visit_class(node);
                false // visit_class handles body itself
            }
            "singleton_class" => {
                self.visit_singleton_class(node);
                false
            }
            "module" => {
                self.visit_module(node);
                false // visit_module handles body itself
            }
            "call" => {
                self.visit_call(node);
                true // Recurse to find nested calls
            }
            _ => true, // Recurse into other nodes
        };

        if should_recurse {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                self.visit_node(child);
            }
        }
    }

    fn visit_method(&mut self, node: Node) {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_else(|| "method".to_string());

        let parameters = self.extract_parameters(node);
        let doc_comment = self.extract_doc_comment(node);

        // Determine visibility based on context
        let visibility = "public".to_string(); // Default in Ruby

        // Calculate complexity from method body
        let complexity = {
            let mut body_node = None;
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "body_statement" {
                    body_node = Some(child);
                    break;
                }
            }
            body_node.map(|body| self.calculate_complexity(body))
        };

        let body_prefix = node
            .child_by_field_name("body")
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(truncate_body_prefix)
            .map(|t| t.to_string());

        let func = FunctionEntity {
            name: name.clone(),
            signature: self.extract_method_signature(node),
            visibility,
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_async: false,
            is_test: self.has_test_annotation(&name),
            is_static: false,
            is_abstract: false,
            parameters,
            return_type: None, // Ruby doesn't have return types
            doc_comment,
            attributes: Vec::new(),
            parent_class: self.current_class.clone(),
            complexity,
            body_prefix,
        };

        self.functions.push(func);

        // Set current function context and visit body to extract calls
        let previous_function = self.current_function.take();
        self.current_function = Some(name);

        // Look for body_statement (Ruby's method body node)
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "body_statement" {
                self.visit_method_body(child);
                break;
            }
        }

        self.current_function = previous_function;
    }

    fn visit_singleton_method(&mut self, node: Node) {
        // Singleton methods: def self.method_name
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_else(|| "method".to_string());

        let parameters = self.extract_parameters(node);
        let doc_comment = self.extract_doc_comment(node);

        // Calculate complexity from method body
        let complexity = {
            let mut body_node = None;
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "body_statement" {
                    body_node = Some(child);
                    break;
                }
            }
            body_node.map(|body| self.calculate_complexity(body))
        };

        let body_prefix = node
            .child_by_field_name("body")
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(truncate_body_prefix)
            .map(|t| t.to_string());

        let func = FunctionEntity {
            name: name.clone(),
            signature: self.extract_method_signature(node),
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_async: false,
            is_test: self.has_test_annotation(&name),
            is_static: true, // Singleton methods are class methods
            is_abstract: false,
            parameters,
            return_type: None,
            doc_comment,
            attributes: vec!["singleton".to_string()],
            parent_class: self.current_class.clone(),
            complexity,
            body_prefix,
        };

        self.functions.push(func);

        // Set current function context and visit body to extract calls
        let previous_function = self.current_function.take();
        self.current_function = Some(name);

        // Look for body_statement (Ruby's method body node)
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "body_statement" {
                self.visit_method_body(child);
                break;
            }
        }

        self.current_function = previous_function;
    }

    fn visit_class(&mut self, node: Node) {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_else(|| "Class".to_string());

        let qualified_name = self.qualify_name(&name);
        let doc_comment = self.extract_doc_comment(node);

        // Extract superclass
        let mut base_classes = Vec::new();
        if let Some(superclass) = node.child_by_field_name("superclass") {
            // The superclass node contains "< ClassName" - extract the constant
            let mut cursor = superclass.walk();
            for child in superclass.children(&mut cursor) {
                if child.kind() == "constant" || child.kind() == "scope_resolution" {
                    let parent_name = self.node_text(child);
                    base_classes.push(parent_name.clone());
                    self.inheritance.push(InheritanceRelation {
                        child: qualified_name.clone(),
                        parent: parent_name,
                        order: 0,
                    });
                    break;
                }
            }
        }

        let body_prefix = node
            .child_by_field_name("body")
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(truncate_body_prefix)
            .map(|t| t.to_string());

        let class_entity = ClassEntity {
            name: qualified_name.clone(),
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_abstract: false,
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

        // Set current class context for method extraction
        let previous_class = self.current_class.take();
        self.current_class = Some(qualified_name.clone());

        // Visit class body to extract methods and module inclusions
        if let Some(body) = node.child_by_field_name("body") {
            self.visit_class_body(body, &qualified_name);
        }

        self.current_class = previous_class;
    }

    fn visit_singleton_class(&mut self, node: Node) {
        // class << self - singleton class block
        // Visit body to extract class methods
        if let Some(body) = node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for child in body.children(&mut cursor) {
                match child.kind() {
                    "method" => {
                        // Methods in singleton_class are class methods
                        self.visit_method(child);
                        // Mark the last added method as static
                        if let Some(last_func) = self.functions.last_mut() {
                            last_func.is_static = true;
                            last_func.attributes.push("singleton".to_string());
                        }
                    }
                    _ => self.visit_node(child),
                }
            }
        }
    }

    fn visit_class_body(&mut self, node: Node, class_name: &str) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "method" => self.visit_method(child),
                "singleton_method" => self.visit_singleton_method(child),
                "singleton_class" => self.visit_singleton_class(child),
                "class" => self.visit_class(child), // Nested class
                "module" => self.visit_module(child), // Nested module
                "call" => {
                    // Check for include/extend/prepend
                    self.check_module_inclusion(child, class_name);
                    self.visit_call(child);
                }
                _ => self.visit_node(child),
            }
        }
    }

    fn check_module_inclusion(&mut self, node: Node, class_name: &str) {
        if let Some(method_node) = node.child_by_field_name("method") {
            let method_name = self.node_text(method_node);
            if method_name == "include" || method_name == "extend" || method_name == "prepend" {
                // Extract the module name from arguments
                if let Some(args) = node.child_by_field_name("arguments") {
                    let mut cursor = args.walk();
                    for arg in args.children(&mut cursor) {
                        if arg.kind() == "constant" || arg.kind() == "scope_resolution" {
                            let module_name = self.node_text(arg);
                            self.implementations.push(ImplementationRelation {
                                implementor: class_name.to_string(),
                                trait_name: module_name,
                            });
                        }
                    }
                }
            }
        }
    }

    fn visit_module(&mut self, node: Node) {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_else(|| "Module".to_string());

        let qualified_name = self.qualify_name(&name);
        let doc_comment = self.extract_doc_comment(node);

        // Extract methods to determine required_methods
        let required_methods = Vec::new();

        let trait_entity = TraitEntity {
            name: qualified_name.clone(),
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            required_methods,
            parent_traits: Vec::new(),
            doc_comment,
            attributes: Vec::new(),
        };

        self.traits.push(trait_entity);

        // Set current module context
        let previous_module = self.current_module.take();
        let previous_class = self.current_class.take();
        self.current_module = Some(qualified_name.clone());
        self.current_class = Some(qualified_name); // Modules can have methods

        // Visit module body to extract methods
        if let Some(body) = node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for child in body.children(&mut cursor) {
                match child.kind() {
                    "method" => self.visit_method(child),
                    "singleton_method" => self.visit_singleton_method(child),
                    "class" => self.visit_class(child),
                    "module" => self.visit_module(child),
                    _ => self.visit_node(child),
                }
            }
        }

        self.current_module = previous_module;
        self.current_class = previous_class;
    }

    fn visit_call(&mut self, node: Node) {
        // Check for require/require_relative statements
        if let Some(method_node) = node.child_by_field_name("method") {
            let method_name = self.node_text(method_node);
            if method_name == "require" || method_name == "require_relative" {
                self.visit_require(node, &method_name);
                return;
            }
        }

        // Extract call relationship if we're inside a function
        if let Some(ref caller) = self.current_function.clone() {
            let callee = self.extract_callee_name(node);

            // Skip empty callees or self-references
            if !callee.is_empty() && callee != "self" {
                let call = CallRelation {
                    caller: caller.clone(),
                    callee,
                    call_site_line: node.start_position().row + 1,
                    is_direct: true,
                    struct_type: None,
                    field_name: None,
                };
                self.calls.push(call);
            }
        }
    }

    fn visit_require(&mut self, node: Node, method_name: &str) {
        if let Some(args) = node.child_by_field_name("arguments") {
            let mut cursor = args.walk();
            for arg in args.children(&mut cursor) {
                if arg.kind() == "string" || arg.kind() == "string_content" {
                    let mut imported = self.node_text(arg);
                    // Remove quotes
                    imported = imported.trim_matches(|c| c == '\'' || c == '"').to_string();

                    let import = ImportRelation {
                        importer: self
                            .current_module
                            .clone()
                            .unwrap_or_else(|| "main".to_string()),
                        imported,
                        symbols: Vec::new(),
                        is_wildcard: false,
                        alias: if method_name == "require_relative" {
                            Some("require_relative".to_string())
                        } else {
                            None
                        },
                    };

                    self.imports.push(import);
                }
            }
        }
    }

    fn extract_callee_name(&self, node: Node) -> String {
        // For a call node, extract the method being called
        if let Some(method_node) = node.child_by_field_name("method") {
            let method_name = self.node_text(method_node);

            // Check if there's a receiver
            if let Some(receiver) = node.child_by_field_name("receiver") {
                let receiver_text = self.node_text(receiver);
                if receiver_text != "self" {
                    return method_name; // Just return method name for instance calls
                }
            }
            return method_name;
        }
        String::new()
    }

    fn visit_method_body(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "call" => {
                    self.visit_call(child);
                    self.visit_method_body(child);
                }
                "identifier" => {
                    // In Ruby, a bare identifier can be a method call
                    // Record it as a potential call if we're in a function context
                    if let Some(ref caller) = self.current_function.clone() {
                        let callee = self.node_text(child);
                        // Skip if it looks like a local variable (lowercase, no special chars)
                        // But simple method calls like `helper` are also lowercase, so we include them
                        if !callee.is_empty() && callee != "self" {
                            let call = CallRelation {
                                caller: caller.clone(),
                                callee,
                                call_site_line: child.start_position().row + 1,
                                is_direct: true,
                                struct_type: None,
                                field_name: None,
                            };
                            self.calls.push(call);
                        }
                    }
                }
                _ => {
                    self.visit_method_body(child);
                }
            }
        }
    }

    fn extract_parameters(&self, node: Node) -> Vec<Parameter> {
        let mut params = Vec::new();
        if let Some(params_node) = node.child_by_field_name("parameters") {
            let mut cursor = params_node.walk();
            for child in params_node.children(&mut cursor) {
                match child.kind() {
                    "identifier" => {
                        params.push(Parameter::new(self.node_text(child)));
                    }
                    "optional_parameter" => {
                        if let Some(name_node) = child.child_by_field_name("name") {
                            let mut param = Parameter::new(self.node_text(name_node));
                            if let Some(value_node) = child.child_by_field_name("value") {
                                param = param.with_default(self.node_text(value_node));
                            }
                            params.push(param);
                        }
                    }
                    "splat_parameter" => {
                        // *args
                        let mut inner_cursor = child.walk();
                        for inner in child.children(&mut inner_cursor) {
                            if inner.kind() == "identifier" {
                                params.push(Parameter::new(self.node_text(inner)).variadic());
                            }
                        }
                    }
                    "hash_splat_parameter" => {
                        // **kwargs - treat as variadic
                        let mut inner_cursor = child.walk();
                        for inner in child.children(&mut inner_cursor) {
                            if inner.kind() == "identifier" {
                                params.push(Parameter::new(self.node_text(inner)).variadic());
                            }
                        }
                    }
                    "block_parameter" => {
                        // &block
                        let mut inner_cursor = child.walk();
                        for inner in child.children(&mut inner_cursor) {
                            if inner.kind() == "identifier" {
                                params.push(Parameter::new(self.node_text(inner)));
                            }
                        }
                    }
                    "keyword_parameter" => {
                        if let Some(name_node) = child.child_by_field_name("name") {
                            let mut param = Parameter::new(self.node_text(name_node));
                            if let Some(value_node) = child.child_by_field_name("value") {
                                param = param.with_default(self.node_text(value_node));
                            }
                            params.push(param);
                        }
                    }
                    _ => {}
                }
            }
        }
        params
    }

    fn extract_method_signature(&self, node: Node) -> String {
        self.node_text(node)
            .lines()
            .next()
            .unwrap_or("")
            .to_string()
    }

    fn extract_doc_comment(&self, node: Node) -> Option<String> {
        // Look for preceding comment node
        if let Some(prev) = node.prev_sibling() {
            if prev.kind() == "comment" {
                return Some(self.node_text(prev));
            }
        }
        None
    }

    fn has_test_annotation(&self, name: &str) -> bool {
        name.starts_with("test_") || name.starts_with("it_") || name.starts_with("should_")
    }

    fn calculate_complexity(&self, body: Node) -> ComplexityMetrics {
        let mut builder = ComplexityBuilder::new();
        self.visit_for_complexity(body, &mut builder);
        builder.build()
    }

    fn visit_for_complexity(&self, node: Node, builder: &mut ComplexityBuilder) {
        match node.kind() {
            "if" | "unless" => {
                builder.add_branch();
                builder.enter_scope();
            }
            "elsif" => {
                builder.add_branch();
                // elsif does not create an additional scope level on top of its parent if
            }
            "else" => {
                builder.add_branch();
            }
            "case" => {
                builder.enter_scope();
            }
            "when" => {
                builder.add_branch();
            }
            "conditional" => {
                // Ternary: cond ? a : b
                builder.add_branch();
            }
            "for" => {
                builder.add_loop();
                builder.enter_scope();
            }
            "while" | "until" => {
                builder.add_loop();
                builder.enter_scope();
            }
            "do_block" | "block" => {
                // Blocks passed to iterators (each, map, select, etc.)
                builder.add_loop();
                builder.enter_scope();
            }
            "rescue" => {
                builder.add_exception_handler();
            }
            "ensure" => {
                builder.add_exception_handler();
            }
            "binary" => {
                // Check for && || and or
                if let Some(op) = node.child_by_field_name("operator") {
                    let op_text = self.node_text(op);
                    if op_text == "&&" || op_text == "||" || op_text == "and" || op_text == "or" {
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

        // Exit scope for constructs that entered one
        match node.kind() {
            "if" | "unless" | "for" | "while" | "until" | "case" | "do_block" | "block" => {
                builder.exit_scope();
            }
            _ => {}
        }
    }

    fn qualify_name(&self, name: &str) -> String {
        if let Some(ref module) = self.current_module {
            format!("{}::{}", module, name)
        } else {
            name.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph_parser_api::BODY_PREFIX_MAX_CHARS;

    fn parse_and_visit(source: &[u8]) -> RubyVisitor<'_> {
        use tree_sitter::Parser;

        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_ruby::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = RubyVisitor::new(source);
        visitor.visit_node(tree.root_node());
        visitor
    }

    #[test]
    fn test_visitor_basics() {
        let visitor = RubyVisitor::new(b"");
        assert_eq!(visitor.functions.len(), 0);
        assert_eq!(visitor.classes.len(), 0);
        assert_eq!(visitor.traits.len(), 0);
    }

    #[test]
    fn test_visitor_method_extraction() {
        let source = b"def greet(name)\n  puts \"Hello, #{name}\"\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "greet");
    }

    #[test]
    fn test_visitor_class_extraction() {
        let source = b"class Person\n  attr_accessor :name\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "Person");
    }

    #[test]
    fn test_visitor_module_extraction() {
        let source = b"module Loggable\n  def log(msg)\n    puts msg\n  end\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.traits.len(), 1);
        assert_eq!(visitor.traits[0].name, "Loggable");
    }

    #[test]
    fn test_visitor_class_method_extraction() {
        let source = b"class Calculator\n  def add(a, b)\n    a + b\n  end\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "add");
        assert_eq!(
            visitor.functions[0].parent_class,
            Some("Calculator".to_string())
        );
    }

    #[test]
    fn test_visitor_require_extraction() {
        let source = b"require 'json'\nrequire_relative './helper'";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 2);
        assert_eq!(visitor.imports[0].imported, "json");
        assert_eq!(visitor.imports[1].imported, "./helper");
    }

    #[test]
    fn test_visitor_require_vs_require_relative_alias() {
        let source = b"require 'json'\nrequire_relative './helper'";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 2);
        // require → alias is None (external gem)
        assert_eq!(visitor.imports[0].alias, None);
        // require_relative → alias marks it as relative
        assert_eq!(
            visitor.imports[1].alias,
            Some("require_relative".to_string())
        );
    }

    #[test]
    fn test_visitor_inheritance() {
        let source = b"class Animal\nend\nclass Dog < Animal\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 2);
        assert_eq!(visitor.inheritance.len(), 1);
        assert_eq!(visitor.inheritance[0].child, "Dog");
        assert_eq!(visitor.inheritance[0].parent, "Animal");
    }

    #[test]
    fn test_visitor_module_inclusion() {
        let source = b"module Walkable\nend\nclass Person\n  include Walkable\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.traits.len(), 1);
        assert_eq!(visitor.classes.len(), 1);
        assert!(visitor
            .implementations
            .iter()
            .any(|i| i.implementor == "Person" && i.trait_name == "Walkable"));
    }

    #[test]
    fn test_visitor_singleton_method() {
        let source = b"class Helper\n  def self.format(str)\n    str.strip\n  end\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.functions.len(), 1);
        assert!(visitor.functions[0].is_static);
    }

    #[test]
    fn test_visitor_method_call_extraction() {
        let source = b"
class MyClass
  def caller
    helper
    process
  end

  def helper
  end

  def process
  end
end
";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.functions.len(), 3);
        assert_eq!(visitor.calls.len(), 2);

        assert!(visitor
            .calls
            .iter()
            .any(|c| c.caller == "caller" && c.callee == "helper"));
        assert!(visitor
            .calls
            .iter()
            .any(|c| c.caller == "caller" && c.callee == "process"));
    }

    #[test]
    fn test_visitor_call_line_numbers() {
        let source = b"
class Test
  def caller
    helper
  end
  def helper
  end
end
";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.calls.len(), 1);
        assert_eq!(visitor.calls[0].caller, "caller");
        assert_eq!(visitor.calls[0].callee, "helper");
        assert_eq!(visitor.calls[0].call_site_line, 4);
        assert!(visitor.calls[0].is_direct);
    }

    #[test]
    fn test_visitor_optional_parameters() {
        let source = b"def greet(name, greeting = 'Hello')\n  puts \"#{greeting}, #{name}\"\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].parameters.len(), 2);
    }

    #[test]
    fn test_visitor_nested_module() {
        let source = b"module Outer\n  module Inner\n    def method\n    end\n  end\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.traits.len(), 2);
        assert!(visitor.traits.iter().any(|t| t.name == "Outer"));
        assert!(visitor.traits.iter().any(|t| t.name == "Outer::Inner"));
    }

    #[test]
    fn test_complexity_simple_method() {
        // A method with no control flow has CC=1
        let source = b"def greet(name)\n  \"Hello, #{name}\"\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert_eq!(complexity.cyclomatic_complexity, 1);
        assert_eq!(complexity.branches, 0);
        assert_eq!(complexity.loops, 0);
        assert_eq!(complexity.logical_operators, 0);
    }

    #[test]
    fn test_complexity_if_elsif_and_each() {
        let source = b"
def classify(items)
  items.each do |item|
    if item > 10
      :big
    elsif item > 5
      :medium
    else
      :small
    end
  end
end
";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        // do_block=1 loop, if=1 branch, elsif=1 branch, else=1 branch => CC = 1+1+3 = 5
        assert!(
            complexity.loops >= 1,
            "expected at least 1 loop (each do block)"
        );
        assert!(
            complexity.branches >= 3,
            "expected if + elsif + else = 3 branches"
        );
        assert!(complexity.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_complexity_rescue_ensure() {
        let source = b"
def risky_operation
  begin
    do_something
  rescue ArgumentError => e
    handle_arg_error(e)
  rescue => e
    handle_generic(e)
  ensure
    cleanup
  end
end
";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        // 2 rescue + 1 ensure = 3 exception handlers => CC = 1+3 = 4
        assert!(
            complexity.exception_handlers >= 3,
            "expected rescue + rescue + ensure = 3 exception handlers, got {}",
            complexity.exception_handlers
        );
        assert!(complexity.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_visitor_singleton_method_attributes() {
        // def self.foo carries a "singleton" attribute in addition to is_static
        let source = b"class Helper\n  def self.build\n  end\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert!(visitor.functions[0].is_static);
        assert!(visitor.functions[0]
            .attributes
            .iter()
            .any(|a| a == "singleton"));
        assert_eq!(
            visitor.functions[0].parent_class,
            Some("Helper".to_string())
        );
    }

    #[test]
    fn test_visitor_singleton_class_block() {
        // class << self promotes contained methods to static class methods
        let source = b"class Widget\n  class << self\n    def create\n    end\n  end\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "create");
        assert!(visitor.functions[0].is_static);
        assert!(visitor.functions[0]
            .attributes
            .iter()
            .any(|a| a == "singleton"));
    }

    #[test]
    fn test_visitor_class_qualified_in_module() {
        // A class nested in a module gets a module-qualified name
        let source = b"module Outer\n  class Inner\n  end\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.traits.len(), 1);
        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "Outer::Inner");
    }

    #[test]
    fn test_visitor_method_line_numbers() {
        // line_start/line_end are 1-indexed and span the whole method
        let source = b"def greet\n  puts 1\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].line_start, 1);
        assert_eq!(visitor.functions[0].line_end, 3);
    }

    #[test]
    fn test_visitor_method_signature_first_line() {
        // signature keeps only the first physical line of the definition
        let source = b"def greet(name)\n  puts name\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].signature, "def greet(name)");
    }

    #[test]
    fn test_visitor_is_test_detection() {
        // Methods named test_/it_/should_ are flagged as tests
        let source = b"def test_addition\nend\ndef helper\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 2);
        let test_fn = visitor
            .functions
            .iter()
            .find(|f| f.name == "test_addition")
            .unwrap();
        let helper_fn = visitor
            .functions
            .iter()
            .find(|f| f.name == "helper")
            .unwrap();
        assert!(test_fn.is_test);
        assert!(!helper_fn.is_test);
    }

    #[test]
    fn test_visitor_splat_parameter_variadic() {
        // *args is captured as a single variadic parameter
        let source = b"def collect(*args)\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].parameters.len(), 1);
        assert_eq!(visitor.functions[0].parameters[0].name, "args");
        assert!(visitor.functions[0].parameters[0].is_variadic);
    }

    #[test]
    fn test_visitor_keyword_parameters() {
        // Keyword parameters (name:) are extracted like ordinary params
        let source = b"def configure(host:, port: 80)\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].parameters.len(), 2);
        assert_eq!(visitor.functions[0].parameters[0].name, "host");
        assert_eq!(visitor.functions[0].parameters[1].name, "port");
    }

    #[test]
    fn test_visitor_doc_comment_extraction() {
        // A comment immediately preceding a method becomes its doc_comment
        let source = b"# Greets the user\ndef greet\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert!(visitor.functions[0]
            .doc_comment
            .as_deref()
            .unwrap_or("")
            .contains("Greets the user"));
    }

    #[test]
    fn test_visitor_scope_resolution_superclass() {
        // A namespaced superclass (Animals::Base) is recorded as inheritance
        let source = b"class Dog < Animals::Base\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.inheritance.len(), 1);
        assert_eq!(visitor.inheritance[0].child, "Dog");
        assert_eq!(visitor.inheritance[0].parent, "Animals::Base");
        assert_eq!(visitor.classes[0].base_classes, vec!["Animals::Base"]);
    }

    #[test]
    fn test_visitor_extend_inclusion() {
        // extend is treated the same as include for implementation relations
        let source = b"module Helpers\nend\nclass Service\n  extend Helpers\nend";
        let visitor = parse_and_visit(source);

        assert!(visitor
            .implementations
            .iter()
            .any(|i| i.implementor == "Service" && i.trait_name == "Helpers"));
    }

    #[test]
    fn test_visitor_require_importer_is_module() {
        // A require inside a module records that module as the importer
        let source = b"module App\n  require 'json'\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "json");
        assert_eq!(visitor.imports[0].importer, "App");
    }

    #[test]
    fn test_complexity_while_loop() {
        let source = b"def countdown(n)\n  while n > 0\n    n -= 1\n  end\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(complexity.loops >= 1);
        assert!(complexity.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_complexity_case_when() {
        let source = b"def label(n)\n  case n\n  when 1 then :one\n  when 2 then :two\n  end\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(complexity.branches >= 2, "expected two when branches");
        assert!(complexity.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_complexity_logical_operators() {
        let source = b"def valid?(a, b)\n  a && b || false\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(
            complexity.logical_operators >= 2,
            "expected && and || counted, got {}",
            complexity.logical_operators
        );
    }

    #[test]
    fn test_visitor_body_prefix_truncated() {
        // An oversized method body is truncated to exactly BODY_PREFIX_MAX_CHARS
        let filler = "a".repeat(BODY_PREFIX_MAX_CHARS + 100);
        let src = format!("def big\n  x = \"{}\"\nend", filler);
        let visitor = parse_and_visit(src.as_bytes());

        assert_eq!(visitor.functions.len(), 1);
        let body_prefix = visitor.functions[0].body_prefix.as_ref().unwrap();
        assert_eq!(body_prefix.chars().count(), BODY_PREFIX_MAX_CHARS);
    }

    #[test]
    fn test_visitor_method_body_prefix_content() {
        // A short method body is captured verbatim as body_prefix
        let source = b"def greet\n  puts 1\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let body_prefix = visitor.functions[0].body_prefix.as_ref().unwrap();
        assert!(body_prefix.contains("puts 1"));
    }

    #[test]
    fn test_visitor_hash_splat_parameter_variadic() {
        // **kwargs is captured as a single variadic parameter
        let source = b"def opts(**kwargs)\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].parameters.len(), 1);
        assert_eq!(visitor.functions[0].parameters[0].name, "kwargs");
        assert!(visitor.functions[0].parameters[0].is_variadic);
    }

    #[test]
    fn test_visitor_block_parameter() {
        // &block is captured as an ordinary (non-variadic) parameter
        let source = b"def each_item(&block)\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].parameters.len(), 1);
        assert_eq!(visitor.functions[0].parameters[0].name, "block");
        assert!(!visitor.functions[0].parameters[0].is_variadic);
    }

    #[test]
    fn test_complexity_until_loop() {
        // until is counted as a loop like while
        let source = b"def wait(n)\n  until n == 0\n    n -= 1\n  end\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(complexity.loops >= 1);
        assert!(complexity.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_complexity_unless_branch() {
        // unless adds a branch like if
        let source = b"def check(x)\n  unless x\n    puts 1\n  end\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(complexity.branches >= 1);
        assert!(complexity.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_complexity_ternary_conditional() {
        // A ternary (cond ? a : b) parses as a conditional node and adds a branch
        let source = b"def pick(x)\n  x ? 1 : 2\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(complexity.branches >= 1);
        assert!(complexity.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_complexity_for_loop() {
        // A `for x in coll` loop is counted as a loop
        let source = b"def loop_it(items)\n  for i in items\n    puts i\n  end\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(complexity.loops >= 1);
        assert!(complexity.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_visitor_prepend_inclusion() {
        // prepend is treated like include/extend for implementation relations
        let source = b"module M\nend\nclass C\n  prepend M\nend";
        let visitor = parse_and_visit(source);

        assert!(visitor
            .implementations
            .iter()
            .any(|i| i.implementor == "C" && i.trait_name == "M"));
    }

    #[test]
    fn test_visitor_scope_resolution_module_inclusion() {
        // A namespaced module (Foo::Bar) included in a class is recorded verbatim
        let source = b"class C\n  include Foo::Bar\nend";
        let visitor = parse_and_visit(source);

        assert!(visitor
            .implementations
            .iter()
            .any(|i| i.implementor == "C" && i.trait_name == "Foo::Bar"));
    }

    #[test]
    fn test_visitor_require_in_class_importer_main() {
        // A require inside a class (not a module) defaults importer to "main"
        let source = b"class App\n  require 'json'\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "json");
        assert_eq!(visitor.imports[0].importer, "main");
    }

    #[test]
    fn test_has_test_annotation_prefixes() {
        let visitor = RubyVisitor::new(b"");
        // Accepted prefixes (RSpec/minitest naming conventions)
        assert!(visitor.has_test_annotation("test_login"));
        assert!(visitor.has_test_annotation("it_returns_ok"));
        assert!(visitor.has_test_annotation("should_validate"));
        // Exact prefix boundary: the underscore is part of the required prefix
        assert!(!visitor.has_test_annotation("test"));
        assert!(!visitor.has_test_annotation("it"));
        assert!(!visitor.has_test_annotation("should"));
        // Non-matching / prefix-in-the-middle names are rejected
        assert!(!visitor.has_test_annotation("run_test_case"));
        assert!(!visitor.has_test_annotation("greet"));
        assert!(!visitor.has_test_annotation(""));
    }

    #[test]
    fn test_qualify_name_with_and_without_module() {
        let mut visitor = RubyVisitor::new(b"");
        // No enclosing module: name is returned verbatim
        assert_eq!(visitor.qualify_name("greet"), "greet");
        // Inside a module: name is prefixed with the module path using ::
        visitor.current_module = Some("Loggable".to_string());
        assert_eq!(visitor.qualify_name("log"), "Loggable::log");
        // Nested module path is preserved as-is
        visitor.current_module = Some("A::B".to_string());
        assert_eq!(visitor.qualify_name("c"), "A::B::c");
    }
}
