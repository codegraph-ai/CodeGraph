// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting R entities

use codegraph_parser_api::{
    truncate_body_prefix, CallRelation, ComplexityBuilder, ComplexityMetrics, FunctionEntity,
    ImportRelation, Parameter,
};
use tree_sitter::Node;

pub(crate) struct RVisitor<'a> {
    pub source: &'a [u8],
    pub functions: Vec<FunctionEntity>,
    pub imports: Vec<ImportRelation>,
    pub calls: Vec<CallRelation>,
    current_function: Option<String>,
}

impl<'a> RVisitor<'a> {
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
            "binary_operator" => {
                // Check for function assignment: name <- function(...) { ... }
                self.visit_binary_operator(node);
                return;
            }
            "call" => {
                self.visit_call(node);
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child);
        }
    }

    fn visit_binary_operator(&mut self, node: Node) {
        // Pattern: name <- function(params) body
        // or: name = function(params) body
        let text = self.node_text(node);
        if !text.contains("function") {
            // Still recurse for nested function assignments
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                self.visit_node(child);
            }
            return;
        }

        // Get operator
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();

        if children.len() < 3 {
            return;
        }

        let lhs = children[0];
        let operator = children[1];
        let op_text = self.node_text(operator);

        // Must be <- or = or <<-
        if op_text != "<-" && op_text != "=" && op_text != "<<-" {
            // Recurse
            for child in &children {
                self.visit_node(*child);
            }
            return;
        }

        let rhs = children[2];

        // Check if RHS is a function_definition
        if rhs.kind() != "function_definition" {
            // Recurse
            for child in &children {
                self.visit_node(*child);
            }
            return;
        }

        let name = self.node_text(lhs);
        if name.is_empty() {
            return;
        }

        let doc_comment = self.extract_doc_comment(node);
        let parameters = self.extract_parameters(rhs);

        let body_prefix = rhs
            .child_by_field_name("body")
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| truncate_body_prefix(t).to_string());

        let complexity = rhs
            .child_by_field_name("body")
            .map(|body| self.calculate_complexity(body));

        let signature = format!(
            "{} <- function({})",
            name,
            parameters
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );

        let func = FunctionEntity {
            name: name.clone(),
            signature,
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_async: false,
            is_test: name.starts_with("test_") || name.starts_with("test."),
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

        // Visit body for calls
        let previous_function = self.current_function.take();
        self.current_function = Some(name);

        if let Some(body) = rhs.child_by_field_name("body") {
            self.visit_body_for_calls(body);
        }

        self.current_function = previous_function;
    }

    fn visit_call(&mut self, node: Node) {
        if let Some(func_node) = node.child_by_field_name("function") {
            let func_name = self.node_text(func_node);

            // Check for library/require/source imports
            if func_name == "library" || func_name == "require" || func_name == "source" {
                if let Some(args) = node.child_by_field_name("arguments") {
                    let mut cursor = args.walk();
                    for arg in args.children(&mut cursor) {
                        // The grammar wraps each call argument in an `argument`
                        // node; unwrap it to reach the identifier/string value.
                        let value = if arg.kind() == "argument" {
                            arg.named_child(0).unwrap_or(arg)
                        } else {
                            arg
                        };
                        if value.kind() == "identifier" || value.kind() == "string" {
                            let module = self.node_text(value);
                            let module = module.trim_matches(|c| c == '"' || c == '\'').to_string();
                            if !module.is_empty() && module != "(" && module != ")" && module != ","
                            {
                                self.imports.push(ImportRelation {
                                    importer: "main".to_string(),
                                    imported: module,
                                    symbols: Vec::new(),
                                    is_wildcard: false,
                                    alias: Some(func_name.clone()),
                                });
                                break;
                            }
                        }
                    }
                }
            }

            // Track calls within functions
            if let Some(ref caller) = self.current_function.clone() {
                if !func_name.is_empty()
                    && func_name != "library"
                    && func_name != "require"
                    && func_name != "source"
                {
                    self.calls.push(CallRelation {
                        caller: caller.clone(),
                        callee: func_name,
                        call_site_line: node.start_position().row + 1,
                        is_direct: true,
                        struct_type: None,
                        field_name: None,
                    });
                }
            }
        }
    }

    fn visit_body_for_calls(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "call" {
                self.visit_call(child);
            }
            self.visit_body_for_calls(child);
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
                    "parameter" => {
                        // Variadic `...` parses as a `dots` child of the
                        // `parameter` node rather than a top-level `dots`, so
                        // detect it here and emit a variadic parameter.
                        if child.child(0).is_some_and(|c| c.kind() == "dots") {
                            params.push(Parameter::new("...").variadic());
                        } else if let Some(name_node) = child.child_by_field_name("name") {
                            let mut param = Parameter::new(self.node_text(name_node));
                            if let Some(default_node) = child.child_by_field_name("default") {
                                param = param.with_default(self.node_text(default_node));
                            }
                            params.push(param);
                        }
                    }
                    "dots" => {
                        params.push(Parameter::new("...").variadic());
                    }
                    _ => {}
                }
            }
        }
        params
    }

    fn extract_doc_comment(&self, node: Node) -> Option<String> {
        if let Some(prev) = node.prev_sibling() {
            if prev.kind() == "comment" {
                let text = self.node_text(prev);
                if text.starts_with("#'") {
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
            "for_statement" | "while_statement" | "repeat_statement" => {
                builder.add_loop();
                builder.enter_scope();
            }
            "binary_operator" => {
                let text = self.node_text(node);
                if text.contains("&&")
                    || text.contains("||")
                    || text.contains("&")
                    || text.contains("|")
                {
                    // Only count && and || (not & and | which are vectorized)
                    let op_node = node.child(1);
                    if let Some(op) = op_node {
                        let op_text = self.node_text(op);
                        if op_text == "&&" || op_text == "||" {
                            builder.add_logical_operator();
                        }
                    }
                }
            }
            "call" => {
                // tryCatch is R's exception handling
                if let Some(func_node) = node.child_by_field_name("function") {
                    let name = self.node_text(func_node);
                    if name == "tryCatch" || name == "try" {
                        builder.add_exception_handler();
                    }
                }
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_for_complexity(child, builder);
        }

        match node.kind() {
            "if_statement" | "for_statement" | "while_statement" | "repeat_statement" => {
                builder.exit_scope();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_visit(source: &[u8]) -> RVisitor<'_> {
        use tree_sitter::Parser;

        let mut parser = Parser::new();
        parser.set_language(&crate::ts_r::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = RVisitor::new(source);
        visitor.visit_node(tree.root_node());
        visitor
    }

    #[test]
    fn test_visitor_function_extraction() {
        let source = b"add <- function(a, b) {\n    a + b\n}";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "add");
    }

    #[test]
    fn test_visitor_library_extraction() {
        let source = b"library(ggplot2)";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "ggplot2");
    }

    #[test]
    fn test_empty_source_is_empty() {
        let visitor = parse_and_visit(b"");
        assert!(visitor.functions.is_empty());
        assert!(visitor.imports.is_empty());
        assert!(visitor.calls.is_empty());
    }

    #[test]
    fn test_function_metadata_defaults() {
        let source = b"add <- function(a, b) {\n    a + b\n}";
        let visitor = parse_and_visit(source);

        let func = &visitor.functions[0];
        assert_eq!(func.visibility, "public");
        assert!(!func.is_async);
        assert!(!func.is_static);
        assert!(!func.is_abstract);
        assert!(!func.is_test);
        assert_eq!(func.return_type, None);
        assert_eq!(func.parent_class, None);
        assert_eq!(func.line_start, 1);
        assert_eq!(func.line_end, 3);
    }

    #[test]
    fn test_signature_lists_parameter_names() {
        let source = b"add <- function(a, b) {\n    a + b\n}";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions[0].signature, "add <- function(a, b)");
    }

    #[test]
    fn test_parameter_extraction() {
        let source = b"add <- function(a, b) {\n    a + b\n}";
        let visitor = parse_and_visit(source);

        let params = &visitor.functions[0].parameters;
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].name, "a");
        assert_eq!(params[1].name, "b");
    }

    #[test]
    fn test_default_parameter_captured() {
        let source = b"f <- function(x, y = 10) {\n    x + y\n}";
        let visitor = parse_and_visit(source);

        let params = &visitor.functions[0].parameters;
        assert_eq!(params.len(), 2);
        assert_eq!(params[1].name, "y");
        assert_eq!(params[1].default_value.as_deref(), Some("10"));
    }

    #[test]
    fn test_variadic_dots_parameter() {
        let source = b"f <- function(x, ...) {\n    x\n}";
        let visitor = parse_and_visit(source);

        let params = &visitor.functions[0].parameters;
        assert_eq!(params.len(), 2);
        assert_eq!(params[1].name, "...");
        assert!(params[1].is_variadic);
    }

    #[test]
    fn test_equals_assignment_form() {
        let source = b"greet = function(name) {\n    name\n}";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "greet");
    }

    #[test]
    fn test_super_assign_form() {
        let source = b"counter <<- function() {\n    1\n}";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "counter");
    }

    #[test]
    fn test_is_test_prefix_detection() {
        let source = b"test_addition <- function() {\n    1\n}";
        let visitor = parse_and_visit(source);
        assert!(visitor.functions[0].is_test);
    }

    #[test]
    fn test_is_test_dot_prefix_detection() {
        let source = b"test.addition <- function() {\n    1\n}";
        let visitor = parse_and_visit(source);
        assert!(visitor.functions[0].is_test);
    }

    #[test]
    fn test_doc_comment_roxygen() {
        let source = b"#' Adds two numbers\nadd <- function(a, b) {\n    a + b\n}";
        let visitor = parse_and_visit(source);
        assert_eq!(
            visitor.functions[0].doc_comment.as_deref(),
            Some("#' Adds two numbers")
        );
    }

    #[test]
    fn test_plain_comment_is_not_doc() {
        let source = b"# just a note\nadd <- function(a, b) {\n    a + b\n}";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions[0].doc_comment, None);
    }

    #[test]
    fn test_body_prefix_present() {
        let source = b"add <- function(a, b) {\n    a + b\n}";
        let visitor = parse_and_visit(source);
        assert!(visitor.functions[0].body_prefix.is_some());
    }

    #[test]
    fn test_require_import_alias() {
        let source = b"require(dplyr)";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "dplyr");
        assert_eq!(visitor.imports[0].alias.as_deref(), Some("require"));
    }

    #[test]
    fn test_source_import_string_stripped() {
        let source = b"source(\"helpers.R\")";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "helpers.R");
        assert_eq!(visitor.imports[0].alias.as_deref(), Some("source"));
    }

    #[test]
    fn test_call_within_function_tracked() {
        let source = b"main <- function() {\n    helper()\n}";
        let visitor = parse_and_visit(source);

        assert!(visitor
            .calls
            .iter()
            .any(|c| c.caller == "main" && c.callee == "helper"));
    }

    #[test]
    fn test_library_call_not_tracked_as_call() {
        let source = b"main <- function() {\n    library(ggplot2)\n}";
        let visitor = parse_and_visit(source);
        assert!(!visitor.calls.iter().any(|c| c.callee == "library"));
    }

    #[test]
    fn test_complexity_increases_with_branch() {
        let plain = b"f <- function(x) {\n    x\n}";
        let branchy =
            b"g <- function(x) {\n    if (x > 0) {\n        1\n    } else {\n        2\n    }\n}";

        let plain_v = parse_and_visit(plain);
        let branchy_v = parse_and_visit(branchy);

        let plain_c = plain_v.functions[0]
            .complexity
            .as_ref()
            .unwrap()
            .cyclomatic_complexity;
        let branchy_c = branchy_v.functions[0]
            .complexity
            .as_ref()
            .unwrap()
            .cyclomatic_complexity;
        assert!(branchy_c > plain_c);
    }

    #[test]
    fn test_multiple_functions_extracted() {
        let source = b"a <- function() {\n    1\n}\nb <- function() {\n    2\n}";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 2);
        assert_eq!(visitor.functions[0].name, "a");
        assert_eq!(visitor.functions[1].name, "b");
    }

    #[test]
    fn test_non_function_assignment_ignored() {
        let source = b"x <- 42";
        let visitor = parse_and_visit(source);
        assert!(visitor.functions.is_empty());
    }

    fn complexity_of(source: &[u8]) -> u32 {
        let v = parse_and_visit(source);
        v.functions[0]
            .complexity
            .as_ref()
            .unwrap()
            .cyclomatic_complexity
    }

    #[test]
    fn test_for_loop_complexity() {
        let plain = b"f <- function(x) {\n    x\n}";
        let looped = b"g <- function(x) {\n    for (i in 1:x) {\n        i\n    }\n}";
        assert!(complexity_of(looped) > complexity_of(plain));
    }

    #[test]
    fn test_while_loop_complexity() {
        let plain = b"f <- function(x) {\n    x\n}";
        let looped = b"g <- function(x) {\n    while (x > 0) {\n        x <- x - 1\n    }\n}";
        assert!(complexity_of(looped) > complexity_of(plain));
    }

    #[test]
    fn test_repeat_loop_complexity() {
        let plain = b"f <- function(x) {\n    x\n}";
        let looped = b"g <- function(x) {\n    repeat {\n        break\n    }\n}";
        assert!(complexity_of(looped) > complexity_of(plain));
    }

    #[test]
    fn test_logical_and_operator_complexity() {
        let plain = b"f <- function(a, b) {\n    a\n}";
        let logical = b"g <- function(a, b) {\n    a && b\n}";
        assert!(complexity_of(logical) > complexity_of(plain));
    }

    #[test]
    fn test_vectorized_and_not_counted() {
        // A single `&` is R's vectorized AND and must not raise complexity,
        // unlike the scalar `&&`.
        let plain = b"f <- function(a, b) {\n    a\n}";
        let vectorized = b"g <- function(a, b) {\n    a & b\n}";
        assert_eq!(complexity_of(vectorized), complexity_of(plain));
    }

    #[test]
    fn test_trycatch_exception_handler_complexity() {
        let plain = b"f <- function(x) {\n    x\n}";
        let guarded = b"g <- function(x) {\n    tryCatch(x, error = function(e) NULL)\n}";
        assert!(complexity_of(guarded) > complexity_of(plain));
    }

    #[test]
    fn test_call_metadata_defaults() {
        let source = b"main <- function() {\n    helper()\n}";
        let visitor = parse_and_visit(source);

        let call = visitor
            .calls
            .iter()
            .find(|c| c.callee == "helper")
            .expect("helper call recorded");
        assert_eq!(call.caller, "main");
        assert_eq!(call.call_site_line, 2);
        assert!(call.is_direct);
        assert_eq!(call.struct_type, None);
        assert_eq!(call.field_name, None);
    }

    #[test]
    fn test_import_default_fields() {
        let source = b"library(ggplot2)";
        let visitor = parse_and_visit(source);

        let import = &visitor.imports[0];
        assert_eq!(import.importer, "main");
        assert!(import.symbols.is_empty());
        assert!(!import.is_wildcard);
        assert_eq!(import.alias.as_deref(), Some("library"));
    }

    #[test]
    fn test_multiple_imports_order_preserved() {
        let source = b"library(ggplot2)\nrequire(dplyr)\nsource(\"utils.R\")";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 3);
        assert_eq!(visitor.imports[0].imported, "ggplot2");
        assert_eq!(visitor.imports[1].imported, "dplyr");
        assert_eq!(visitor.imports[2].imported, "utils.R");
    }

    #[test]
    fn test_body_prefix_contains_body_text() {
        let source = b"add <- function(a, b) {\n    a + b\n}";
        let visitor = parse_and_visit(source);
        let prefix = visitor.functions[0].body_prefix.as_deref().unwrap();
        assert!(prefix.contains("a + b"));
    }

    #[test]
    fn test_line_numbering_offset_by_leading_blanks() {
        let source = b"\n\nadd <- function(a, b) {\n    a + b\n}";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions[0].line_start, 3);
        assert_eq!(visitor.functions[0].line_end, 5);
    }

    #[test]
    fn test_nested_function_assignment_not_extracted() {
        // Once an outer function is found, its body is traversed only by
        // visit_body_for_calls (call tracking), not visit_node, so a nested
        // `inner <- function()` assignment is never emitted as its own entity.
        let source =
            b"outer <- function() {\n    inner <- function() {\n        1\n    }\n    inner()\n}";
        let visitor = parse_and_visit(source);

        let names: Vec<&str> = visitor.functions.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["outer"]);
        // The nested call is still attributed to the outer function.
        assert!(visitor
            .calls
            .iter()
            .any(|c| c.caller == "outer" && c.callee == "inner"));
    }
}
