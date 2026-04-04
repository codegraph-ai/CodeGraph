// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting SystemVerilog/Verilog entities
//!
//! Uses tree-sitter-verilog which, despite its name, is the SystemVerilog
//! grammar (supports IEEE 1800-2012 constructs: modules, interfaces, classes,
//! programs, packages, tasks, functions, instantiations, imports).

use codegraph_parser_api::{
    CallRelation, ClassEntity, ComplexityBuilder, ComplexityMetrics, FunctionEntity,
    ImportRelation, Parameter, BODY_PREFIX_MAX_CHARS,
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
            .map(|t| {
                if t.len() > BODY_PREFIX_MAX_CHARS {
                    &t[..BODY_PREFIX_MAX_CHARS]
                } else {
                    t
                }
            })
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
}
