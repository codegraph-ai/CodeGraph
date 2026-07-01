// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting Lua entities

use codegraph_parser_api::{
    truncate_body_prefix, CallRelation, ComplexityBuilder, ComplexityMetrics, FunctionEntity,
    ImportRelation, Parameter,
};
use tree_sitter::Node;

pub(crate) struct LuaVisitor<'a> {
    pub source: &'a [u8],
    pub functions: Vec<FunctionEntity>,
    pub imports: Vec<ImportRelation>,
    pub calls: Vec<CallRelation>,
    current_function: Option<String>,
}

impl<'a> LuaVisitor<'a> {
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
            "function_declaration" => {
                self.visit_function_declaration(node);
                return;
            }
            "variable_declaration" => {
                // Check for local function pattern or require calls
                self.visit_variable_declaration(node);
                return;
            }
            "function_call" => {
                self.visit_function_call(node);
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child);
        }
    }

    fn visit_function_declaration(&mut self, node: Node) {
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

        let body_prefix = node
            .child_by_field_name("body")
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| truncate_body_prefix(t).to_string());

        let complexity = node
            .child_by_field_name("body")
            .map(|body| self.calculate_complexity(body));

        let is_local = {
            let text = self.node_text(node);
            text.starts_with("local ")
        };

        let func = FunctionEntity {
            name: name.clone(),
            signature,
            visibility: if is_local { "private" } else { "public" }.to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_async: false,
            is_test: false,
            is_static: false,
            is_abstract: false,
            parameters,
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

        if let Some(body) = node.child_by_field_name("body") {
            self.visit_body_for_calls(body);
        }

        self.current_function = previous_function;
    }

    fn visit_variable_declaration(&mut self, node: Node) {
        // A Lua `local foo = ...` parses as
        //   variable_declaration -> assignment_statement
        //     -> variable_list -> identifier (the name)
        //     -> expression_list -> (function_definition | function_call | ...)
        // Locate the assigned name and any function_definition value.
        let mut name: Option<String> = None;
        let mut func_def: Option<Node> = None;

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() != "assignment_statement" {
                continue;
            }
            let mut acursor = child.walk();
            for part in child.children(&mut acursor) {
                match part.kind() {
                    "variable_list" if name.is_none() => {
                        if let Some(id) = part.named_child(0) {
                            name = Some(self.node_text(id));
                        }
                    }
                    "expression_list" => {
                        let mut ecursor = part.walk();
                        for expr in part.children(&mut ecursor) {
                            if expr.kind() == "function_definition" {
                                func_def = Some(expr);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // local foo = function(...) ... end
        if let (Some(name), Some(child)) = (name, func_def) {
            if !name.is_empty() {
                let signature = format!("local {} = function", name);
                let body_prefix = child
                    .child_by_field_name("body")
                    .and_then(|b| b.utf8_text(self.source).ok())
                    .filter(|t| !t.is_empty())
                    .map(|t| truncate_body_prefix(t).to_string());

                let complexity = child
                    .child_by_field_name("body")
                    .map(|body| self.calculate_complexity(body));

                let parameters = self.extract_parameters(child);

                let func = FunctionEntity {
                    name: name.clone(),
                    signature,
                    visibility: "private".to_string(),
                    line_start: node.start_position().row + 1,
                    line_end: child.end_position().row + 1,
                    is_async: false,
                    is_test: false,
                    is_static: false,
                    is_abstract: false,
                    parameters,
                    return_type: None,
                    doc_comment: None,
                    attributes: Vec::new(),
                    parent_class: None,
                    complexity,
                    body_prefix,
                };

                self.functions.push(func);

                let previous_function = self.current_function.take();
                self.current_function = Some(name);
                if let Some(body) = child.child_by_field_name("body") {
                    self.visit_body_for_calls(body);
                }
                self.current_function = previous_function;
                return;
            }
        }

        // Otherwise recurse so `require(...)` function calls and any nested
        // constructs are still picked up (recursion handles require extraction,
        // avoiding the earlier double-count from also scanning the raw text).
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child);
        }
    }

    fn visit_function_call(&mut self, node: Node) {
        // Check for require("module") calls
        let text = self.node_text(node);
        if text.starts_with("require") {
            self.extract_require_from_text(&text);
        }

        // Track calls if inside a function
        if let Some(ref caller) = self.current_function.clone() {
            if let Some(name_node) = node.child_by_field_name("name") {
                let callee = self.node_text(name_node);
                if !callee.is_empty() && callee != "require" {
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
    }

    fn extract_require_from_text(&mut self, text: &str) {
        // Match require("module") or require('module')
        if let Some(start) = text.find("require(") {
            let after = &text[start + 8..];
            let quote = after.chars().next();
            if let Some(q) = quote {
                if q == '"' || q == '\'' {
                    if let Some(end) = after[1..].find(q) {
                        let module = &after[1..1 + end];
                        self.imports.push(ImportRelation {
                            importer: "main".to_string(),
                            imported: module.to_string(),
                            symbols: Vec::new(),
                            is_wildcard: false,
                            alias: None,
                        });
                    }
                }
            }
        }
    }

    fn visit_body_for_calls(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "function_call" {
                self.visit_function_call(child);
            }
            self.visit_body_for_calls(child);
        }
    }

    fn extract_parameters(&self, node: Node) -> Vec<Parameter> {
        let mut params = Vec::new();
        if let Some(params_node) = node.child_by_field_name("parameters") {
            let mut cursor = params_node.walk();
            for child in params_node.children(&mut cursor) {
                if child.kind() == "identifier" {
                    params.push(Parameter::new(self.node_text(child)));
                } else if child.kind() == "vararg_expression" {
                    params.push(Parameter::new("...").variadic());
                }
            }
        }
        params
    }

    fn extract_doc_comment(&self, node: Node) -> Option<String> {
        if let Some(prev) = node.prev_sibling() {
            if prev.kind() == "comment" {
                let text = self.node_text(prev);
                if text.starts_with("---") || text.starts_with("--!") {
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
            "elseif_statement" => {
                builder.add_branch();
            }
            "else_statement" => {
                builder.add_branch();
            }
            "for_statement" | "for_generic_statement" | "while_statement" | "repeat_statement" => {
                builder.add_loop();
                builder.enter_scope();
            }
            "binary_expression" => {
                let text = self.node_text(node);
                if text.contains(" and ") || text.contains(" or ") {
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
            | "for_statement"
            | "for_generic_statement"
            | "while_statement"
            | "repeat_statement" => {
                builder.exit_scope();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_visit(source: &[u8]) -> LuaVisitor<'_> {
        use tree_sitter::Parser;

        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_lua::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = LuaVisitor::new(source);
        visitor.visit_node(tree.root_node());
        visitor
    }

    #[test]
    fn test_visitor_function_extraction() {
        let source = b"function greet(name)\n    print(\"Hello, \" .. name)\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "greet");
    }

    #[test]
    fn test_top_level_function_is_public() {
        let source = b"function greet()\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions[0].visibility, "public");
    }

    #[test]
    fn test_local_function_is_private() {
        let source = b"local function helper()\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "helper");
        assert_eq!(visitor.functions[0].visibility, "private");
    }

    #[test]
    fn test_signature_is_first_line() {
        let source = b"function greet(name)\n    print(name)\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions[0].signature, "function greet(name)");
    }

    #[test]
    fn test_parameter_extraction() {
        let source = b"function add(a, b)\nend";
        let visitor = parse_and_visit(source);

        let params = &visitor.functions[0].parameters;
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].name, "a");
        assert_eq!(params[1].name, "b");
        assert!(!params[0].is_variadic);
    }

    #[test]
    fn test_variadic_parameter_extraction() {
        let source = b"function log(fmt, ...)\nend";
        let visitor = parse_and_visit(source);

        let params = &visitor.functions[0].parameters;
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].name, "fmt");
        assert_eq!(params[1].name, "...");
        assert!(params[1].is_variadic);
    }

    #[test]
    fn test_require_extraction() {
        let source = b"local json = require(\"json\")";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "json");
    }

    #[test]
    fn test_require_single_quotes() {
        let source = b"local m = require('mymod')";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "mymod");
    }

    #[test]
    fn test_bare_require_call_extraction() {
        let source = b"require(\"setup\")";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "setup");
    }

    #[test]
    fn test_two_requires_in_one_statement() {
        // Recursion into function_call children catches both requires;
        // the earlier text-scan path only saw the first.
        let source = b"local a, b = require(\"x\"), require(\"y\")";
        let visitor = parse_and_visit(source);

        let mut names: Vec<_> = visitor.imports.iter().map(|i| i.imported.clone()).collect();
        names.sort();
        assert_eq!(names, vec!["x".to_string(), "y".to_string()]);
    }

    #[test]
    fn test_local_var_assigned_function_extracted() {
        let source = b"local adder = function(a, b)\n  return a + b\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let f = &visitor.functions[0];
        assert_eq!(f.name, "adder");
        assert_eq!(f.visibility, "private");
        assert_eq!(f.signature, "local adder = function");
        assert_eq!(f.parameters.len(), 2);
    }

    #[test]
    fn test_doc_comment_extraction() {
        let source = b"--- Greets a person.\nfunction greet(name)\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(
            visitor.functions[0].doc_comment.as_deref(),
            Some("--- Greets a person.")
        );
    }

    #[test]
    fn test_plain_comment_not_doc() {
        let source = b"-- ordinary comment\nfunction greet()\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions[0].doc_comment, None);
    }

    #[test]
    fn test_body_prefix_present() {
        let source = b"function greet()\n    print(\"hi\")\nend";
        let visitor = parse_and_visit(source);

        assert!(visitor.functions[0].body_prefix.is_some());
    }

    #[test]
    fn test_complexity_if_adds_branch() {
        let source = b"function check(x)\n  if x then\n    return 1\n  end\nend";
        let visitor = parse_and_visit(source);

        let c = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(c.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_complexity_loop_adds_branch() {
        let source = b"function loopy()\n  for i = 1, 10 do\n    print(i)\n  end\nend";
        let visitor = parse_and_visit(source);

        let c = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(c.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_call_tracking_inside_function() {
        let source = b"function outer()\n  inner()\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.calls.len(), 1);
        assert_eq!(visitor.calls[0].caller, "outer");
        assert_eq!(visitor.calls[0].callee, "inner");
    }

    #[test]
    fn test_require_excluded_from_calls() {
        let source = b"function setup()\n  require(\"cfg\")\nend";
        let visitor = parse_and_visit(source);

        assert!(visitor.calls.is_empty());
        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "cfg");
    }

    #[test]
    fn test_empty_source() {
        let source = b"";
        let visitor = parse_and_visit(source);

        assert!(visitor.functions.is_empty());
        assert!(visitor.imports.is_empty());
        assert!(visitor.calls.is_empty());
    }

    #[test]
    fn test_line_numbers_one_indexed() {
        // First physical line is line 1; a 3-line function spans 1..=3.
        let source = b"function greet()\n  print(\"hi\")\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions[0].line_start, 1);
        assert_eq!(visitor.functions[0].line_end, 3);
    }

    #[test]
    fn test_default_flags_are_false() {
        let source = b"function plain()\nend";
        let visitor = parse_and_visit(source);

        let f = &visitor.functions[0];
        assert!(!f.is_async);
        assert!(!f.is_test);
        assert!(!f.is_static);
        assert!(!f.is_abstract);
    }

    #[test]
    fn test_return_type_is_none() {
        // Lua has no static return types, so return_type is always None.
        let source = b"function f()\n  return 1\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions[0].return_type, None);
    }

    #[test]
    fn test_complexity_while_adds_branch() {
        let source = b"function spin()\n  while true do\n    break\n  end\nend";
        let visitor = parse_and_visit(source);

        let c = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(c.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_complexity_repeat_adds_branch() {
        let source = b"function again()\n  repeat\n    x = 1\n  until x\nend";
        let visitor = parse_and_visit(source);

        let c = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(c.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_complexity_generic_for_adds_branch() {
        let source = b"function iter(t)\n  for k, v in pairs(t) do\n    print(k)\n  end\nend";
        let visitor = parse_and_visit(source);

        let c = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(c.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_complexity_logical_operator() {
        let source = b"function both(a, b)\n  return a and b\nend";
        let visitor = parse_and_visit(source);

        let c = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(c.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_complexity_elseif_adds_branch() {
        let source =
            b"function pick(x)\n  if x then\n    return 1\n  elseif x then\n    return 2\n  end\nend";
        let visitor = parse_and_visit(source);

        let c = visitor.functions[0].complexity.as_ref().unwrap();
        // if branch + elseif branch push complexity above a single-branch body.
        assert!(c.cyclomatic_complexity > 2);
    }

    #[test]
    fn test_call_metadata() {
        let source = b"function outer()\n  inner()\nend";
        let visitor = parse_and_visit(source);

        let call = &visitor.calls[0];
        assert_eq!(call.call_site_line, 2);
        assert!(call.is_direct);
    }

    #[test]
    fn test_local_var_function_body_prefix() {
        let source = b"local adder = function(a, b)\n  return a + b\nend";
        let visitor = parse_and_visit(source);

        assert!(visitor.functions[0].body_prefix.is_some());
    }

    #[test]
    fn test_multiple_functions_in_source_order() {
        let source = b"function first()\nend\nfunction second()\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 2);
        assert_eq!(visitor.functions[0].name, "first");
        assert_eq!(visitor.functions[1].name, "second");
    }
}
