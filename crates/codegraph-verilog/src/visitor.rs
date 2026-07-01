// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting SystemVerilog/Verilog entities
//!
//! Uses tree-sitter-verilog which, despite its name, is the SystemVerilog
//! grammar (supports IEEE 1800-2012 constructs: modules, interfaces, classes,
//! programs, packages, tasks, functions, instantiations, imports).

use codegraph_parser_api::{
    truncate_body_prefix, CallRelation, ClassEntity, ComplexityBuilder, ComplexityMetrics,
    FunctionEntity, ImportRelation, Parameter,
};
use tree_sitter::Node;

pub struct VerilogVisitor<'a> {
    pub source: &'a [u8],
    pub modules: Vec<ClassEntity>,
    pub functions: Vec<FunctionEntity>,
    pub imports: Vec<ImportRelation>,
    pub calls: Vec<CallRelation>,
    current_module: Option<String>,
    current_function: Option<String>,
}

impl<'a> VerilogVisitor<'a> {
    pub fn new(source: &'a [u8]) -> Self {
        Self {
            source,
            modules: Vec::new(),
            functions: Vec::new(),
            imports: Vec::new(),
            calls: Vec::new(),
            current_module: None,
            current_function: None,
        }
    }

    fn node_text(&self, node: Node) -> String {
        node.utf8_text(self.source).unwrap_or("").to_string()
    }

    /// Extract parameters from a function_body_declaration or task_body_declaration.
    /// AST: body_declaration → tf_port_list → tf_port_item1 → port_identifier → simple_identifier
    fn extract_tf_parameters(&self, body_node: Node) -> Vec<Parameter> {
        let mut cursor = body_node.walk();
        let port_list = body_node
            .children(&mut cursor)
            .find(|c| c.kind() == "tf_port_list");

        let Some(port_list) = port_list else {
            return Vec::new();
        };

        let mut params = Vec::new();
        let mut pl_cursor = port_list.walk();
        for item in port_list.children(&mut pl_cursor) {
            if !item.kind().starts_with("tf_port_item") {
                continue;
            }
            // Find port_identifier → simple_identifier for the name
            let mut ic = item.walk();
            let name = item
                .children(&mut ic)
                .find(|c| c.kind() == "port_identifier")
                .and_then(|pi| self.find_identifier_in(pi));
            if let Some(name) = name {
                params.push(Parameter {
                    name,
                    type_annotation: None,
                    default_value: None,
                    is_variadic: false,
                });
            }
        }
        params
    }

    /// Find the first simple_identifier or escaped_identifier child
    fn find_identifier_in(&self, node: Node) -> Option<String> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "simple_identifier" | "escaped_identifier" => {
                    return Some(self.node_text(child));
                }
                _ => {}
            }
        }
        None
    }

    /// Recursively search for the first simple_identifier descendant (BFS up to depth 4)
    fn find_identifier_recursive(&self, node: Node, depth: usize) -> Option<String> {
        if depth == 0 {
            return None;
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "simple_identifier" | "escaped_identifier" => {
                    return Some(self.node_text(child));
                }
                _ => {}
            }
        }
        // Second pass: recurse
        let mut cursor2 = node.walk();
        for child in node.children(&mut cursor2) {
            if let Some(name) = self.find_identifier_recursive(child, depth - 1) {
                return Some(name);
            }
        }
        None
    }

    /// Find identifier in an SV declaration using the *_identifier or *_ansi_header child.
    /// For example, `interface_declaration` has `interface_ansi_header` which has
    /// `interface_identifier` which has `simple_identifier`.
    fn extract_sv_name(&self, node: Node, ansi_header_kind: &str, identifier_kind: &str) -> String {
        // Look for the specific identifier node first (most precise)
        let id_node: Option<Node> = {
            let mut cursor = node.walk();
            let found = node
                .children(&mut cursor)
                .find(|c| c.kind() == identifier_kind);
            found
        };
        if let Some(n) = id_node {
            if let Some(name) = self.find_identifier_in(n) {
                return name;
            }
        }
        // Try via the ansi header
        let header_node: Option<Node> = {
            let mut cursor = node.walk();
            let found = node
                .children(&mut cursor)
                .find(|c| c.kind() == ansi_header_kind);
            found
        };
        if let Some(h) = header_node {
            let id_in_header: Option<Node> = {
                let mut cursor = h.walk();
                let found = h
                    .children(&mut cursor)
                    .find(|c| c.kind() == identifier_kind);
                found
            };
            if let Some(n) = id_in_header {
                if let Some(name) = self.find_identifier_in(n) {
                    return name;
                }
            }
        }
        // Final fallback: recursive search
        self.find_identifier_recursive(node, 4)
            .unwrap_or_else(|| "unknown".to_string())
    }

    pub fn visit_node(&mut self, node: Node) {
        match node.kind() {
            "module_declaration" => {
                self.visit_module(node);
                return;
            }
            "interface_declaration" => {
                self.visit_interface(node);
                return;
            }
            "class_declaration" => {
                self.visit_class(node);
                return;
            }
            "program_declaration" => {
                self.visit_program(node);
                return;
            }
            "package_declaration" => {
                self.visit_package(node);
                return;
            }
            "function_declaration" => {
                self.visit_function(node);
                return;
            }
            "task_declaration" => {
                self.visit_task(node);
                return;
            }
            "include_compiler_directive" => {
                self.visit_include(node);
            }
            "package_import_declaration" => {
                self.visit_package_import(node);
            }
            "module_instantiation" => {
                self.visit_module_instantiation(node);
            }
            "interface_instantiation" => {
                self.visit_interface_instantiation(node);
            }
            "checker_instantiation" => {
                // The grammar sometimes parses module instantiations as checker_instantiations
                // due to Verilog parsing ambiguity (both use the same named port syntax)
                self.visit_checker_instantiation(node);
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child);
        }
    }

    fn visit_module(&mut self, node: Node) {
        // module_declaration -> module_header -> simple_identifier (module name)
        let name = {
            let mut cursor = node.walk();
            let header = node
                .children(&mut cursor)
                .find(|c| c.kind() == "module_header");
            header
                .and_then(|h| self.find_identifier_in(h))
                .or_else(|| self.find_identifier_recursive(node, 3))
                .unwrap_or_else(|| "unknown".to_string())
        };

        self.push_class_entity(node, name, false, false);
    }

    fn visit_interface(&mut self, node: Node) {
        // interface_declaration -> interface_ansi_header -> interface_identifier -> simple_identifier
        // OR interface_declaration -> interface_identifier -> simple_identifier (non-ansi)
        let name = self.extract_sv_name(node, "interface_ansi_header", "interface_identifier");
        self.push_class_entity(node, name, false, true);
    }

    fn visit_class(&mut self, node: Node) {
        // class_declaration -> class_identifier -> simple_identifier
        let name = self.extract_sv_name(node, "", "class_identifier");
        self.push_class_entity(node, name, false, false);
    }

    fn visit_program(&mut self, node: Node) {
        // program_declaration -> program_ansi_header -> program_identifier -> simple_identifier
        // OR program_declaration -> program_identifier -> simple_identifier (non-ansi)
        let name = self.extract_sv_name(node, "program_ansi_header", "program_identifier");
        self.push_class_entity(node, name, false, false);
    }

    fn visit_package(&mut self, node: Node) {
        // package_declaration -> package_identifier -> simple_identifier
        let name = self.extract_sv_name(node, "", "package_identifier");
        self.push_class_entity(node, name, false, false);
    }

    fn push_class_entity(
        &mut self,
        node: Node,
        name: String,
        is_abstract: bool,
        is_interface: bool,
    ) {
        let prev_module = self.current_module.clone();
        self.current_module = Some(name.clone());

        let body_prefix = node
            .utf8_text(self.source)
            .ok()
            .filter(|t| !t.is_empty())
            .map(truncate_body_prefix)
            .map(|t| t.to_string());
        let entity = ClassEntity {
            name,
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_abstract,
            is_interface,
            base_classes: Vec::new(),
            implemented_traits: Vec::new(),
            methods: Vec::new(),
            fields: Vec::new(),
            doc_comment: None,
            attributes: Vec::new(),
            type_parameters: Vec::new(),
            body_prefix,
        };
        self.modules.push(entity);

        // Visit children for functions/tasks/instantiations inside this construct
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child);
        }

        self.current_module = prev_module;
    }

    fn visit_function(&mut self, node: Node) {
        // function_declaration -> function_body_declaration -> function_identifier -> simple_identifier
        let mut cursor = node.walk();
        let body = node
            .children(&mut cursor)
            .find(|c| c.kind() == "function_body_declaration");

        let name = body
            .and_then(|b| {
                let mut bc = b.walk();
                let func_id = b
                    .children(&mut bc)
                    .find(|c| c.kind() == "function_identifier");
                func_id.and_then(|fi| self.find_identifier_in(fi))
            })
            .or_else(|| self.find_identifier_recursive(node, 4))
            .unwrap_or_else(|| "unknown_function".to_string());

        let parameters = body
            .map(|b| self.extract_tf_parameters(b))
            .unwrap_or_default();

        let prev_function = self.current_function.clone();
        self.current_function = Some(name.clone());

        let complexity = self.calculate_complexity(node);

        let body_prefix = node
            .utf8_text(self.source)
            .ok()
            .filter(|t| !t.is_empty())
            .map(truncate_body_prefix)
            .map(|t| t.to_string());
        let func = FunctionEntity {
            name,
            signature: self
                .node_text(node)
                .lines()
                .next()
                .unwrap_or("")
                .to_string(),
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_async: false,
            is_test: false,
            is_static: false,
            is_abstract: false,
            parameters,
            return_type: None,
            doc_comment: None,
            attributes: Vec::new(),
            parent_class: self.current_module.clone(),
            complexity: Some(complexity),
            body_prefix,
        };

        self.functions.push(func);
        self.current_function = prev_function;
    }

    fn visit_task(&mut self, node: Node) {
        // task_declaration -> task_body_declaration -> task_identifier -> simple_identifier
        let mut cursor = node.walk();
        let body = node
            .children(&mut cursor)
            .find(|c| c.kind() == "task_body_declaration");

        let name = body
            .and_then(|b| {
                let mut bc = b.walk();
                let task_id = b.children(&mut bc).find(|c| c.kind() == "task_identifier");
                task_id.and_then(|ti| self.find_identifier_in(ti))
            })
            .or_else(|| self.find_identifier_recursive(node, 4))
            .unwrap_or_else(|| "unknown_task".to_string());

        let parameters = body
            .map(|b| self.extract_tf_parameters(b))
            .unwrap_or_default();

        let prev_function = self.current_function.clone();
        self.current_function = Some(name.clone());

        let complexity = self.calculate_complexity(node);

        let body_prefix = node
            .utf8_text(self.source)
            .ok()
            .filter(|t| !t.is_empty())
            .map(truncate_body_prefix)
            .map(|t| t.to_string());
        let func = FunctionEntity {
            name,
            signature: self
                .node_text(node)
                .lines()
                .next()
                .unwrap_or("")
                .to_string(),
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_async: false,
            is_test: false,
            is_static: false,
            is_abstract: false,
            parameters,
            return_type: None,
            doc_comment: None,
            attributes: Vec::new(),
            parent_class: self.current_module.clone(),
            complexity: Some(complexity),
            body_prefix,
        };

        self.functions.push(func);
        self.current_function = prev_function;
    }

    fn visit_include(&mut self, node: Node) {
        // include_compiler_directive -> double_quoted_string
        let path = {
            let mut cursor = node.walk();
            let found = node
                .children(&mut cursor)
                .find(|c| c.kind() == "double_quoted_string")
                .map(|n| {
                    let text = self.node_text(n);
                    text.trim_matches('"').to_string()
                });
            found.unwrap_or_default()
        };

        if !path.is_empty() {
            self.imports.push(ImportRelation {
                importer: self
                    .current_module
                    .clone()
                    .unwrap_or_else(|| "file".to_string()),
                imported: path,
                symbols: Vec::new(),
                is_wildcard: false,
                alias: None,
            });
        }
    }

    fn visit_package_import(&mut self, node: Node) {
        // package_import_declaration -> package_import_item -> package_identifier
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "package_import_item" {
                // package_import_item has package_identifier and optional simple_identifier
                let mut ic = child.walk();
                let mut pkg_name = String::new();
                let mut is_wildcard = false;

                for item_child in child.children(&mut ic) {
                    match item_child.kind() {
                        "package_identifier" => {
                            pkg_name = self
                                .find_identifier_in(item_child)
                                .unwrap_or_else(|| self.node_text(item_child));
                        }
                        "simple_identifier" => {
                            // specific symbol import
                        }
                        "*" => {
                            is_wildcard = true;
                        }
                        _ => {
                            let text = self.node_text(item_child);
                            if text == "*" {
                                is_wildcard = true;
                            }
                        }
                    }
                }

                if !pkg_name.is_empty() {
                    self.imports.push(ImportRelation {
                        importer: self
                            .current_module
                            .clone()
                            .unwrap_or_else(|| "file".to_string()),
                        imported: pkg_name,
                        symbols: Vec::new(),
                        is_wildcard,
                        alias: None,
                    });
                }
            }
        }
    }

    fn visit_module_instantiation(&mut self, node: Node) {
        // module_instantiation -> simple_identifier (module type being instantiated)
        let module_type = {
            let mut cursor = node.walk();
            let result = node
                .children(&mut cursor)
                .find(|c| c.kind() == "simple_identifier" || c.kind() == "escaped_identifier")
                .map(|n| self.node_text(n))
                .unwrap_or_default();
            result
        };

        if !module_type.is_empty() {
            let caller = self
                .current_module
                .clone()
                .unwrap_or_else(|| "file".to_string());
            self.calls.push(CallRelation::new(
                caller,
                module_type,
                node.start_position().row + 1,
            ));
        }
    }

    fn visit_interface_instantiation(&mut self, node: Node) {
        // interface_instantiation -> interface_identifier -> simple_identifier
        let inst_type = {
            let id_node: Option<Node> = {
                let mut cursor = node.walk();
                let found = node
                    .children(&mut cursor)
                    .find(|c| c.kind() == "interface_identifier");
                found
            };
            if let Some(n) = id_node {
                self.find_identifier_in(n).unwrap_or_default()
            } else {
                self.find_identifier_recursive(node, 3).unwrap_or_default()
            }
        };

        if !inst_type.is_empty() {
            let caller = self
                .current_module
                .clone()
                .unwrap_or_else(|| "file".to_string());
            self.calls.push(CallRelation::new(
                caller,
                inst_type,
                node.start_position().row + 1,
            ));
        }
    }

    fn visit_checker_instantiation(&mut self, node: Node) {
        // checker_instantiation -> checker_identifier -> simple_identifier
        // The grammar uses checker_instantiation for what are often module instantiations
        // due to Verilog parsing ambiguity with named port connections
        let module_type = {
            let mut cursor = node.walk();
            let found = node
                .children(&mut cursor)
                .find(|c| c.kind() == "checker_identifier")
                .and_then(|ci| self.find_identifier_in(ci));
            found
                .or_else(|| {
                    // Fallback: look for simple_identifier directly
                    let mut c2 = node.walk();
                    let f = node
                        .children(&mut c2)
                        .find(|c| {
                            c.kind() == "simple_identifier" || c.kind() == "escaped_identifier"
                        })
                        .map(|n| self.node_text(n));
                    f
                })
                .unwrap_or_default()
        };

        if !module_type.is_empty() {
            let caller = self
                .current_module
                .clone()
                .unwrap_or_else(|| "file".to_string());
            self.calls.push(CallRelation::new(
                caller,
                module_type,
                node.start_position().row + 1,
            ));
        }
    }

    fn calculate_complexity(&self, node: Node) -> ComplexityMetrics {
        let mut builder = ComplexityBuilder::new();
        self.visit_for_complexity(node, &mut builder);
        builder.build()
    }

    fn visit_for_complexity(&self, node: Node, builder: &mut ComplexityBuilder) {
        match node.kind() {
            "conditional_statement" | "case_statement" => {
                builder.add_branch();
                builder.enter_scope();
            }
            "case_item" => {
                builder.add_branch();
            }
            "loop_statement" | "for_step_assignment" => {
                builder.add_loop();
                builder.enter_scope();
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_for_complexity(child, builder);
        }

        match node.kind() {
            "conditional_statement" | "case_statement" | "loop_statement" => {
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

    #[test]
    fn test_visitor_basics() {
        let visitor = VerilogVisitor::new(b"module top(); endmodule");
        assert_eq!(visitor.modules.len(), 0);
        assert_eq!(visitor.functions.len(), 0);
        assert_eq!(visitor.imports.len(), 0);
    }

    #[test]
    fn test_visitor_module_extraction() {
        use tree_sitter::Parser;
        let source = b"module counter (input clk); endmodule";
        let mut parser = Parser::new();
        parser.set_language(&crate::ts_verilog::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = VerilogVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.modules.len(), 1);
        assert_eq!(visitor.modules[0].name, "counter");
    }

    #[test]
    fn test_visitor_function_extraction() {
        use tree_sitter::Parser;
        let source =
            b"module top(); function integer add; input a, b; add = a + b; endfunction endmodule";
        let mut parser = Parser::new();
        parser.set_language(&crate::ts_verilog::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = VerilogVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert!(
            !visitor.functions.is_empty(),
            "Expected at least one function"
        );
    }

    #[test]
    fn test_function_parameter_extraction() {
        use tree_sitter::Parser;
        let source = b"module top();
  function automatic int add(input int a, input int b);
    return a + b;
  endfunction
endmodule";
        let mut parser = Parser::new();
        parser.set_language(&crate::ts_verilog::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = VerilogVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.functions.len(), 1);
        let params: Vec<&str> = visitor.functions[0]
            .parameters
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(params, vec!["a", "b"]);
    }

    #[test]
    fn test_task_parameter_extraction() {
        use tree_sitter::Parser;
        let source = b"module top();
  task my_task(input logic clk, output logic [7:0] data, inout wire en);
  endtask
endmodule";
        let mut parser = Parser::new();
        parser.set_language(&crate::ts_verilog::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = VerilogVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.functions.len(), 1);
        let params: Vec<&str> = visitor.functions[0]
            .parameters
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(params, vec!["clk", "data", "en"]);
    }

    #[test]
    fn test_visitor_sv_interface() {
        use tree_sitter::Parser;
        let source = b"interface my_bus; logic clk; endinterface";
        let mut parser = Parser::new();
        parser.set_language(&crate::ts_verilog::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = VerilogVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(
            visitor.modules.len(),
            1,
            "Expected 1 interface, got {:?}",
            visitor.modules.iter().map(|m| &m.name).collect::<Vec<_>>()
        );
        assert_eq!(visitor.modules[0].name, "my_bus");
        assert!(visitor.modules[0].is_interface);
    }

    #[test]
    fn test_visitor_sv_class() {
        use tree_sitter::Parser;
        let source = b"class Packet; int data; endclass";
        let mut parser = Parser::new();
        parser.set_language(&crate::ts_verilog::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = VerilogVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.modules.len(), 1);
        assert_eq!(visitor.modules[0].name, "Packet");
    }

    #[test]
    fn test_visitor_sv_package() {
        use tree_sitter::Parser;
        let source = b"package my_pkg; endpackage";
        let mut parser = Parser::new();
        parser.set_language(&crate::ts_verilog::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = VerilogVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.modules.len(), 1);
        assert_eq!(visitor.modules[0].name, "my_pkg");
    }

    #[test]
    fn test_visitor_sv_package_import() {
        use tree_sitter::Parser;
        let source = b"module top(); import my_pkg::*; endmodule";
        let mut parser = Parser::new();
        parser.set_language(&crate::ts_verilog::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = VerilogVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert!(!visitor.imports.is_empty(), "Expected package import");
        assert_eq!(visitor.imports[0].imported, "my_pkg");
        assert!(visitor.imports[0].is_wildcard);
    }

    #[test]
    fn test_visitor_sv_program() {
        use tree_sitter::Parser;
        let source = b"program my_test; initial begin end endprogram";
        let mut parser = Parser::new();
        parser.set_language(&crate::ts_verilog::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = VerilogVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.modules.len(), 1);
        assert_eq!(visitor.modules[0].name, "my_test");
    }

    #[test]
    fn test_visitor_module_instantiation() {
        use tree_sitter::Parser;
        let source = b"module top(); counter u1 (.clk(clk)); endmodule";
        let mut parser = Parser::new();
        parser.set_language(&crate::ts_verilog::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = VerilogVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert!(
            !visitor.calls.is_empty(),
            "Expected module instantiation call"
        );
        assert_eq!(visitor.calls[0].callee, "counter");
    }

    /// Parse `source` and return a populated visitor.
    fn parse_visit(source: &[u8]) -> VerilogVisitor<'_> {
        use tree_sitter::Parser;
        let mut parser = Parser::new();
        parser.set_language(&crate::ts_verilog::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();
        // Leak the tree so the borrow lives as long as the returned visitor's source ref.
        let tree = Box::leak(Box::new(tree));
        let mut visitor = VerilogVisitor::new(source);
        visitor.visit_node(tree.root_node());
        visitor
    }

    #[test]
    fn test_empty_source_extracts_nothing() {
        let v = parse_visit(b"");
        assert!(v.modules.is_empty());
        assert!(v.functions.is_empty());
        assert!(v.imports.is_empty());
        assert!(v.calls.is_empty());
    }

    #[test]
    fn test_function_metadata_defaults() {
        let source = b"module top();
  function int add(input int a, input int b);
    return a + b;
  endfunction
endmodule";
        let v = parse_visit(source);
        assert_eq!(v.functions.len(), 1);
        let f = &v.functions[0];
        // Verilog functions are always public, synchronous, non-static, non-abstract.
        assert_eq!(f.visibility, "public");
        assert!(!f.is_async);
        assert!(!f.is_static);
        assert!(!f.is_abstract);
        assert!(!f.is_test);
        // Verilog has no return-type extraction, and no doc/attributes.
        assert_eq!(f.return_type, None);
        assert_eq!(f.doc_comment, None);
        assert!(f.attributes.is_empty());
    }

    #[test]
    fn test_function_one_based_line_bounds_and_signature() {
        let source = b"module top();
  function int add(input int a);
    return a;
  endfunction
endmodule";
        let v = parse_visit(source);
        let f = &v.functions[0];
        // Function declaration begins on the 2nd line (row 1 -> line 2).
        assert_eq!(f.line_start, 2);
        assert!(f.line_end >= f.line_start);
        // Signature is the first line of the declaration text.
        assert!(f.signature.contains("function int add"));
        assert!(f.body_prefix.is_some());
    }

    #[test]
    fn test_function_parent_class_is_enclosing_module() {
        let source = b"module alu();
  function int neg(input int a);
    return -a;
  endfunction
endmodule";
        let v = parse_visit(source);
        assert_eq!(v.functions[0].parent_class.as_deref(), Some("alu"));
    }

    #[test]
    fn test_baseline_function_complexity() {
        let source = b"module top();
  function int id(input int a);
    return a;
  endfunction
endmodule";
        let v = parse_visit(source);
        let c = v.functions[0].complexity.as_ref().unwrap();
        // A straight-line function has no branches or loops.
        assert_eq!(c.branches, 0);
        assert_eq!(c.loops, 0);
    }

    #[test]
    fn test_conditional_raises_complexity() {
        let source = b"module top();
  function int pick(input int a);
    if (a > 0) return a;
    else return 0;
  endfunction
endmodule";
        let v = parse_visit(source);
        let c = v.functions[0].complexity.as_ref().unwrap();
        assert!(
            c.branches >= 1,
            "if statement should register a branch, got {}",
            c.branches
        );
        assert!(c.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_loop_raises_complexity() {
        let source = b"module top();
  function int sum(input int n);
    int total; total = 0;
    for (int i = 0; i < n; i = i + 1) total = total + i;
    return total;
  endfunction
endmodule";
        let v = parse_visit(source);
        let c = v.functions[0].complexity.as_ref().unwrap();
        assert!(c.loops >= 1, "for loop should register a loop");
    }

    #[test]
    fn test_task_extracted_as_function_with_name_and_parent() {
        let source = b"module dut();
  task reset(input logic clk);
  endtask
endmodule";
        let v = parse_visit(source);
        assert_eq!(v.functions.len(), 1);
        assert_eq!(v.functions[0].name, "reset");
        assert_eq!(v.functions[0].parent_class.as_deref(), Some("dut"));
    }

    #[test]
    fn test_include_directive_becomes_import() {
        let source = b"`include \"defs.svh\"\nmodule top(); endmodule";
        let v = parse_visit(source);
        assert!(
            v.imports.iter().any(|i| i.imported == "defs.svh"),
            "expected an include import for defs.svh, got {:?}",
            v.imports.iter().map(|i| &i.imported).collect::<Vec<_>>()
        );
        let inc = v.imports.iter().find(|i| i.imported == "defs.svh").unwrap();
        // Top-level include has no enclosing module -> importer defaults to "file".
        assert_eq!(inc.importer, "file");
        assert!(!inc.is_wildcard);
    }

    #[test]
    fn test_specific_symbol_package_import_is_not_wildcard() {
        let source = b"module top(); import my_pkg::my_type; endmodule";
        let v = parse_visit(source);
        let imp = v
            .imports
            .iter()
            .find(|i| i.imported == "my_pkg")
            .expect("expected my_pkg import");
        assert!(
            !imp.is_wildcard,
            "a specific symbol import (::my_type) is not a wildcard"
        );
        // Import inside a module records the module as the importer.
        assert_eq!(imp.importer, "top");
    }

    #[test]
    fn test_interface_is_flagged_but_module_is_not() {
        let mod_v = parse_visit(b"module top(); endmodule");
        assert!(!mod_v.modules[0].is_interface);
        let if_v = parse_visit(b"interface bus; endinterface");
        assert!(if_v.modules[0].is_interface);
    }

    #[test]
    fn test_multiple_modules_extracted() {
        let source = b"module a(); endmodule\nmodule b(); endmodule";
        let v = parse_visit(source);
        let names: Vec<&str> = v.modules.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"a"));
        assert!(names.contains(&"b"));
        assert_eq!(v.modules.len(), 2);
    }

    #[test]
    fn test_module_class_entity_defaults() {
        let v = parse_visit(b"module top(); endmodule");
        let m = &v.modules[0];
        assert_eq!(m.visibility, "public");
        assert!(!m.is_abstract);
        assert_eq!(m.line_start, 1);
        assert!(m.base_classes.is_empty());
        assert!(m.body_prefix.is_some());
    }

    #[test]
    fn test_case_statement_raises_complexity() {
        let source = b"module top();
  function int classify(input int a);
    case (a)
      0: return 0;
      1: return 1;
      default: return 2;
    endcase
  endfunction
endmodule";
        let v = parse_visit(source);
        let c = v.functions[0].complexity.as_ref().unwrap();
        // A case statement plus its case_items each register a branch.
        assert!(
            c.branches >= 1,
            "case statement should register at least one branch, got {}",
            c.branches
        );
        assert!(c.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_nested_loops_increase_loop_count() {
        let source = b"module top();
  function int grid(input int n);
    int total; total = 0;
    for (int i = 0; i < n; i = i + 1)
      for (int j = 0; j < n; j = j + 1)
        total = total + 1;
    return total;
  endfunction
endmodule";
        let v = parse_visit(source);
        let c = v.functions[0].complexity.as_ref().unwrap();
        // Two nested for loops each count toward the loop metric.
        assert!(
            c.loops >= 2,
            "two nested for loops should register two loops, got {}",
            c.loops
        );
    }

    #[test]
    fn test_module_instantiation_caller_is_enclosing_module() {
        let source = b"module top(); counter u1 (.clk(clk)); endmodule";
        let v = parse_visit(source);
        assert_eq!(v.calls[0].callee, "counter");
        // The instantiation is attributed to the module it appears inside.
        assert_eq!(v.calls[0].caller, "top");
    }

    #[test]
    fn test_top_level_package_import_importer_is_file() {
        let source = b"import glob_pkg::*;\nmodule top(); endmodule";
        let v = parse_visit(source);
        let imp = v
            .imports
            .iter()
            .find(|i| i.imported == "glob_pkg")
            .expect("expected glob_pkg import");
        // An import with no enclosing module defaults its importer to "file".
        assert_eq!(imp.importer, "file");
        assert!(imp.is_wildcard);
    }

    #[test]
    fn test_multiple_includes_preserve_order() {
        let source = b"`include \"a.svh\"\n`include \"b.svh\"\nmodule top(); endmodule";
        let v = parse_visit(source);
        let paths: Vec<&str> = v.imports.iter().map(|i| i.imported.as_str()).collect();
        assert_eq!(paths, vec!["a.svh", "b.svh"]);
    }

    #[test]
    fn test_wildcard_import_importer_is_enclosing_module() {
        let source = b"module top(); import my_pkg::*; endmodule";
        let v = parse_visit(source);
        let imp = &v.imports[0];
        assert!(imp.is_wildcard);
        // Inside a module the import records the module as the importer.
        assert_eq!(imp.importer, "top");
    }

    #[test]
    fn test_task_one_based_line_bounds_and_signature() {
        let source = b"module dut();
  task reset(input logic clk);
  endtask
endmodule";
        let v = parse_visit(source);
        let f = &v.functions[0];
        // The task begins on the 2nd line (row 1 -> line 2).
        assert_eq!(f.line_start, 2);
        assert!(f.line_end >= f.line_start);
        assert!(f.signature.contains("task reset"));
        assert!(f.body_prefix.is_some());
    }

    #[test]
    fn test_function_parameter_field_defaults() {
        let source = b"module top();
  function int add(input int a, input int b);
    return a + b;
  endfunction
endmodule";
        let v = parse_visit(source);
        let p = &v.functions[0].parameters[0];
        // Verilog parameter extraction captures the name only.
        assert_eq!(p.name, "a");
        assert_eq!(p.type_annotation, None);
        assert_eq!(p.default_value, None);
        assert!(!p.is_variadic);
    }

    #[test]
    fn test_class_method_parent_is_class() {
        let source = b"class Packet;
  function int size();
    return 8;
  endfunction
endclass";
        let v = parse_visit(source);
        assert_eq!(v.functions.len(), 1);
        assert_eq!(v.functions[0].name, "size");
        // A function declared inside a class is parented to that class.
        assert_eq!(v.functions[0].parent_class.as_deref(), Some("Packet"));
    }

    #[test]
    fn test_program_task_parent_is_program() {
        let source = b"program my_test;
  task run();
  endtask
endprogram";
        let v = parse_visit(source);
        assert_eq!(v.functions.len(), 1);
        // A task inside a program is parented to the program name.
        assert_eq!(v.functions[0].parent_class.as_deref(), Some("my_test"));
    }

    #[test]
    fn test_body_prefix_truncated_on_large_module() {
        // Build a module whose text exceeds BODY_PREFIX_MAX_CHARS.
        let mut src = String::from("module big();\n");
        for i in 0..400 {
            src.push_str(&format!("  wire signal_{i};\n"));
        }
        src.push_str("endmodule");
        let v = parse_visit(src.as_bytes());
        let bp = v.modules[0].body_prefix.as_ref().unwrap();
        assert!(bp.len() <= BODY_PREFIX_MAX_CHARS);
        assert!(src.len() > BODY_PREFIX_MAX_CHARS);
    }

    #[test]
    fn test_multiple_functions_in_module_order() {
        let source = b"module top();
  function int first(input int a);
    return a;
  endfunction
  function int second(input int b);
    return b;
  endfunction
endmodule";
        let v = parse_visit(source);
        let names: Vec<&str> = v.functions.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["first", "second"]);
    }
}
