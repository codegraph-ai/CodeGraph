// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting OCaml entities

use codegraph_parser_api::{
    truncate_body_prefix, CallRelation, ComplexityBuilder, ComplexityMetrics, FunctionEntity,
    ImportRelation, Parameter,
};
use tree_sitter::Node;

pub(crate) struct OcamlVisitor<'a> {
    pub source: &'a [u8],
    pub functions: Vec<FunctionEntity>,
    pub imports: Vec<ImportRelation>,
    pub calls: Vec<CallRelation>,
    current_function: Option<String>,
}

impl<'a> OcamlVisitor<'a> {
    pub fn new(source: &'a [u8]) -> Self {
        Self {
            source,
            functions: Vec::new(),
            imports: Vec::new(),
            calls: Vec::new(),
            current_function: None,
        }
    }

    fn node_text(&self, node: Node) -> String {
        node.utf8_text(self.source).unwrap_or("").to_string()
    }

    pub fn visit_node(&mut self, node: Node) {
        match node.kind() {
            // top-level let bindings
            "value_definition" => {
                self.visit_value_definition(node);
                return;
            }
            // open Module
            "open_module" => {
                self.visit_open_module(node);
                // don't return — no children to descend into that matter
            }
            // module Module = struct ... end  — recurse into body
            "module_definition" => {
                self.visit_children(node);
                return;
            }
            _ => {}
        }

        self.visit_children(node);
    }

    fn visit_children(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child);
        }
    }

    // -------------------------------------------------------------------------
    // value_definition: let [rec] <let_binding> [and <let_binding> ...]
    // -------------------------------------------------------------------------
    fn visit_value_definition(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "let_binding" {
                self.visit_let_binding(child);
            }
        }
    }

    // -------------------------------------------------------------------------
    // let_binding:
    //   - field "pattern": value_name  (the binding name)
    //   - children of kind "parameter": each has field "pattern": value_pattern
    //   - field "body": expression
    // -------------------------------------------------------------------------
    fn visit_let_binding(&mut self, node: Node) {
        // Extract name from the pattern field
        let name = match node.child_by_field_name("pattern") {
            Some(p) if p.kind() == "value_name" => self.node_text(p),
            _ => return,
        };

        if name.is_empty() || name == "_" {
            return;
        }

        let params = self.extract_parameters(node);
        let body_node = node.child_by_field_name("body");

        // Emit only if there are explicit parameters.
        // (Plain `let x = 42` has no parameters and should be skipped.)
        if params.is_empty() {
            // Also skip if body is not a fun/function expression
            let is_fun = body_node
                .map(|b| {
                    matches!(
                        b.kind(),
                        "fun_expression" | "function_expression" | "fun" | "function"
                    )
                })
                .unwrap_or(false);
            if !is_fun {
                return;
            }
        }

        let signature = {
            let full = self.node_text(node);
            full.lines().next().unwrap_or("").to_string()
        };

        let doc_comment = self.extract_doc_comment(node);

        let body_prefix = body_node
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| truncate_body_prefix(t).to_string());

        let complexity = body_node.map(|b| self.calculate_complexity(b));

        let func = FunctionEntity {
            name: name.clone(),
            signature,
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_async: false,
            is_test: false,
            is_static: false,
            is_abstract: false,
            parameters: params,
            return_type: None,
            doc_comment,
            attributes: Vec::new(),
            parent_class: None,
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

    // -------------------------------------------------------------------------
    // open_module: open <module_path>
    // -------------------------------------------------------------------------
    fn visit_open_module(&mut self, node: Node) {
        // child field "module": module_path > module_name
        if let Some(module_path) = node.child_by_field_name("module") {
            // module_path may wrap a module_name
            let name = if module_path.kind() == "module_path" {
                // First child of module_path is typically module_name
                let mut cursor = module_path.walk();
                let found = module_path
                    .children(&mut cursor)
                    .find(|c| c.kind() == "module_name")
                    .map(|n| self.node_text(n));
                found.unwrap_or_else(|| self.node_text(module_path))
            } else {
                self.node_text(module_path)
            };

            if !name.is_empty() {
                self.imports.push(ImportRelation {
                    importer: "main".to_string(),
                    imported: name,
                    symbols: Vec::new(),
                    is_wildcard: true,
                    alias: None,
                });
            }
        }
    }

    // -------------------------------------------------------------------------
    // Call tracking: application_expression
    //   - field "function": value_path  (possibly qualified: Module.fn)
    //   - further "argument" fields
    // -------------------------------------------------------------------------
    fn visit_body_for_calls(&mut self, node: Node) {
        if node.kind() == "application_expression" {
            self.visit_application(node);
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_body_for_calls(child);
        }
    }

    fn visit_application(&mut self, node: Node) {
        let Some(ref caller) = self.current_function.clone() else {
            return;
        };

        if let Some(func_node) = node.child_by_field_name("function") {
            let callee_text = self.node_text(func_node);
            // Strip module qualifiers: Printf.printf -> printf
            let callee = callee_text
                .rsplit('.')
                .next()
                .unwrap_or(&callee_text)
                .to_string();

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

    // -------------------------------------------------------------------------
    // Parameters: children of let_binding with kind "parameter"
    //   parameter has field "pattern": value_pattern  (the name)
    // -------------------------------------------------------------------------
    fn extract_parameters(&self, node: Node) -> Vec<Parameter> {
        let mut params = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "parameter" {
                // Try field "pattern" first, fall back to raw text
                let text = if let Some(pat) = child.child_by_field_name("pattern") {
                    self.node_text(pat)
                } else {
                    self.node_text(child)
                };
                let trimmed = text.trim().to_string();
                if !trimmed.is_empty() && trimmed != "_" {
                    params.push(Parameter::new(trimmed));
                }
            }
        }
        params
    }

    fn extract_doc_comment(&self, node: Node) -> Option<String> {
        // OCaml doc comments are (* ... *) or (** ... *)
        if let Some(prev) = node.prev_sibling() {
            if prev.kind() == "comment" || prev.kind() == "doc_comment" {
                let text = self.node_text(prev);
                if text.starts_with("(**") || text.starts_with("(*") {
                    return Some(text);
                }
            }
        }
        None
    }

    // -------------------------------------------------------------------------
    // Complexity
    // -------------------------------------------------------------------------
    fn calculate_complexity(&self, body: Node) -> ComplexityMetrics {
        let mut builder = ComplexityBuilder::new();
        self.visit_for_complexity(body, &mut builder);
        builder.build()
    }

    fn visit_for_complexity(&self, node: Node, builder: &mut ComplexityBuilder) {
        match node.kind() {
            // if/then/else: one branch per if, extra per else
            "if_expression" => {
                builder.add_branch();
                builder.enter_scope();
            }
            // match cases each count as a branch
            "match_expression" => {
                builder.enter_scope();
            }
            "match_case" => {
                builder.add_branch();
            }
            // function (anonymous match)
            "function_expression" => {
                builder.enter_scope();
            }
            "for_expression" | "while_expression" => {
                builder.add_loop();
                builder.enter_scope();
            }
            "try_expression" => {
                builder.add_branch();
                builder.enter_scope();
            }
            "infix_expression" => {
                let text = self.node_text(node);
                if text.contains(" && ") || text.contains(" || ") {
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
            "if_expression"
            | "match_expression"
            | "function_expression"
            | "for_expression"
            | "while_expression"
            | "try_expression" => {
                builder.exit_scope();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_visit(source: &[u8]) -> OcamlVisitor<'_> {
        use tree_sitter::Parser;

        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_ocaml::LANGUAGE_OCAML.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = OcamlVisitor::new(source);
        visitor.visit_node(tree.root_node());
        visitor
    }

    #[test]
    fn test_visitor_function_extraction() {
        let source = b"let greet name =\n  Printf.printf \"Hello, %s\\n\" name";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "greet");
    }

    #[test]
    fn test_visitor_open_extraction() {
        let source = b"open Printf\nopen List";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 2);
        assert_eq!(visitor.imports[0].imported, "Printf");
        assert_eq!(visitor.imports[1].imported, "List");
    }

    #[test]
    fn test_visitor_multi_param_function() {
        let source = b"let create_user name email =\n  { name; email }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "create_user");
        assert_eq!(visitor.functions[0].parameters.len(), 2);
    }

    #[test]
    fn test_visitor_plain_value_not_extracted() {
        // `let x = 42` should not be extracted as a function
        let source = b"let x = 42";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 0);
    }
}
