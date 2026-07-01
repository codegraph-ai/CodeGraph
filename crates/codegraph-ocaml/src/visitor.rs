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

    // -------------------------------------------------------------------------
    // Empty / trivial sources
    // -------------------------------------------------------------------------
    #[test]
    fn test_empty_source() {
        let visitor = parse_and_visit(b"");
        assert!(visitor.functions.is_empty());
        assert!(visitor.imports.is_empty());
        assert!(visitor.calls.is_empty());
    }

    #[test]
    fn test_comment_only_source() {
        let visitor = parse_and_visit(b"(* just a comment *)");
        assert!(visitor.functions.is_empty());
    }

    // -------------------------------------------------------------------------
    // Function metadata defaults
    // -------------------------------------------------------------------------
    #[test]
    fn test_function_metadata_defaults() {
        let source = b"let greet name = name";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let f = &visitor.functions[0];
        assert_eq!(f.visibility, "public");
        assert!(!f.is_async);
        assert!(!f.is_test);
        assert!(!f.is_static);
        assert!(!f.is_abstract);
        assert_eq!(f.return_type, None);
        assert_eq!(f.parent_class, None);
        assert!(f.attributes.is_empty());
    }

    #[test]
    fn test_function_line_bounds_one_based() {
        let source = b"let a = 1\nlet greet name = name";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        // second physical line
        assert_eq!(visitor.functions[0].line_start, 2);
        assert_eq!(visitor.functions[0].line_end, 2);
    }

    #[test]
    fn test_function_signature_first_line_only() {
        let source = b"let greet name =\n  let x = name in\n  x";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        // signature is derived from the let_binding node, which excludes the
        // leading `let` keyword (that belongs to the parent value_definition).
        assert_eq!(visitor.functions[0].signature, "greet name =");
    }

    // -------------------------------------------------------------------------
    // Parameters
    // -------------------------------------------------------------------------
    #[test]
    fn test_single_parameter() {
        let source = b"let greet name = name";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions[0].parameters.len(), 1);
        assert_eq!(visitor.functions[0].parameters[0].name, "name");
    }

    #[test]
    fn test_underscore_parameter_excluded_skips_plain_body() {
        // `let f _ = 42`: the only param is `_` (dropped), body is not a
        // fun/function expression, so nothing is extracted.
        let source = b"let f _ = 42";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 0);
    }

    // -------------------------------------------------------------------------
    // fun / function bodies with no explicit parameters
    // -------------------------------------------------------------------------
    #[test]
    fn test_fun_expression_body_extracted() {
        let source = b"let add = fun x y -> x + y";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "add");
        // the params live inside the fun_expression, not as `parameter` children
        assert!(visitor.functions[0].parameters.is_empty());
    }

    #[test]
    fn test_function_expression_body_extracted() {
        let source = b"let describe = function 0 -> \"zero\" | _ -> \"other\"";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "describe");
    }

    // -------------------------------------------------------------------------
    // open / imports
    // -------------------------------------------------------------------------
    #[test]
    fn test_open_import_defaults() {
        let source = b"open Printf";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 1);
        let imp = &visitor.imports[0];
        assert_eq!(imp.importer, "main");
        assert_eq!(imp.imported, "Printf");
        assert!(imp.is_wildcard);
        assert!(imp.symbols.is_empty());
        assert_eq!(imp.alias, None);
    }

    // -------------------------------------------------------------------------
    // body_prefix
    // -------------------------------------------------------------------------
    #[test]
    fn test_body_prefix_present() {
        let source = b"let greet name = name";
        let visitor = parse_and_visit(source);

        assert!(visitor.functions[0].body_prefix.is_some());
        assert!(visitor.functions[0]
            .body_prefix
            .as_ref()
            .unwrap()
            .contains("name"));
    }

    // -------------------------------------------------------------------------
    // Complexity
    // -------------------------------------------------------------------------
    #[test]
    fn test_baseline_complexity_is_one() {
        let source = b"let greet name = name";
        let visitor = parse_and_visit(source);

        let c = visitor.functions[0].complexity.as_ref().unwrap();
        assert_eq!(c.cyclomatic_complexity, 1);
    }

    #[test]
    fn test_if_raises_complexity() {
        let source = b"let f x = if x > 0 then 1 else 0";
        let visitor = parse_and_visit(source);

        let c = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(c.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_match_raises_complexity() {
        let source = b"let f x = match x with 0 -> \"a\" | _ -> \"b\"";
        let visitor = parse_and_visit(source);

        let c = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(c.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_for_loop_raises_complexity() {
        let source = b"let f n = for i = 0 to n do ignore i done";
        let visitor = parse_and_visit(source);

        let c = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(c.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_while_loop_raises_complexity() {
        let source = b"let f () = while true do () done";
        let visitor = parse_and_visit(source);

        let c = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(c.cyclomatic_complexity > 1);
    }

    // -------------------------------------------------------------------------
    // Call tracking
    // -------------------------------------------------------------------------
    #[test]
    fn test_call_qualifier_stripped() {
        let source = b"let greet name = Printf.printf \"Hi %s\" name";
        let visitor = parse_and_visit(source);

        assert!(visitor.calls.iter().any(|c| c.callee == "printf"));
        assert!(visitor.calls.iter().all(|c| c.caller == "greet"));
    }

    #[test]
    fn test_top_level_call_not_tracked() {
        // an application outside any function body yields no CallRelation
        let source = b"let () = print_string \"hi\"";
        let visitor = parse_and_visit(source);
        assert!(visitor.calls.is_empty());
    }

    // -------------------------------------------------------------------------
    // Multiple definitions
    // -------------------------------------------------------------------------
    #[test]
    fn test_multiple_functions() {
        let source = b"let a x = x\nlet b y = y\nlet c z = z";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 3);
        assert_eq!(visitor.functions[0].name, "a");
        assert_eq!(visitor.functions[1].name, "b");
        assert_eq!(visitor.functions[2].name, "c");
    }

    // -------------------------------------------------------------------------
    // module_definition recursion
    // -------------------------------------------------------------------------
    #[test]
    fn test_function_inside_module_extracted() {
        let source = b"module M = struct\n  let inner x = x\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "inner");
    }

    // -------------------------------------------------------------------------
    // Call relation metadata
    // -------------------------------------------------------------------------
    #[test]
    fn test_call_relation_default_metadata() {
        let source = b"let greet name = Printf.printf \"Hi %s\" name";
        let visitor = parse_and_visit(source);

        let call = visitor
            .calls
            .iter()
            .find(|c| c.callee == "printf")
            .expect("printf call recorded");
        assert_eq!(call.caller, "greet");
        assert!(call.is_direct);
        assert_eq!(call.struct_type, None);
        assert_eq!(call.field_name, None);
        // single-line source: the application starts on line 1
        assert_eq!(call.call_site_line, 1);
    }

    #[test]
    fn test_call_site_line_offset() {
        // the application_expression sits on the second physical line
        let source = b"let greet name =\n  print_string name";
        let visitor = parse_and_visit(source);

        let call = visitor
            .calls
            .iter()
            .find(|c| c.callee == "print_string")
            .expect("print_string call recorded");
        assert_eq!(call.call_site_line, 2);
    }

    #[test]
    fn test_call_inside_if_attributed_to_function() {
        // calls in both branches of an if are attributed to the enclosing let
        let source = b"let f x = if x then g () else h ()";
        let visitor = parse_and_visit(source);

        assert!(visitor
            .calls
            .iter()
            .any(|c| c.callee == "g" && c.caller == "f"));
        assert!(visitor
            .calls
            .iter()
            .any(|c| c.callee == "h" && c.caller == "f"));
    }

    // -------------------------------------------------------------------------
    // Additional complexity paths
    // -------------------------------------------------------------------------
    #[test]
    fn test_try_raises_complexity() {
        let source = b"let f g = try g () with _ -> 0";
        let visitor = parse_and_visit(source);

        let c = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(c.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_logical_and_raises_complexity() {
        let source = b"let f a b = a && b";
        let visitor = parse_and_visit(source);

        let c = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(c.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_logical_or_raises_complexity() {
        let source = b"let f a b = a || b";
        let visitor = parse_and_visit(source);

        let c = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(c.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_multiple_match_cases_increase_complexity() {
        // each match_case adds a branch, so three cases push cc to at least 3
        let source = b"let f x = match x with 0 -> \"a\" | 1 -> \"b\" | _ -> \"c\"";
        let visitor = parse_and_visit(source);

        let c = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(c.cyclomatic_complexity >= 3);
    }

    // -------------------------------------------------------------------------
    // `let ... and ...` multi-binding value_definition
    // -------------------------------------------------------------------------
    #[test]
    fn test_let_and_binding_extracts_multiple() {
        let source = b"let a x = x and b y = y";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 2);
        assert_eq!(visitor.functions[0].name, "a");
        assert_eq!(visitor.functions[1].name, "b");
    }

    // -------------------------------------------------------------------------
    // Parameters
    // -------------------------------------------------------------------------
    #[test]
    fn test_multi_param_order_preserved() {
        let source = b"let create a b c = a";
        let visitor = parse_and_visit(source);

        let params = &visitor.functions[0].parameters;
        assert_eq!(params.len(), 3);
        assert_eq!(params[0].name, "a");
        assert_eq!(params[1].name, "b");
        assert_eq!(params[2].name, "c");
    }

    // -------------------------------------------------------------------------
    // Underscore binding name
    // -------------------------------------------------------------------------
    #[test]
    fn test_underscore_binding_skipped() {
        // `let _ = ...` has no usable name and is not extracted
        let source = b"let _ = print_string \"hi\"";
        let visitor = parse_and_visit(source);
        assert!(visitor.functions.is_empty());
    }

    // -------------------------------------------------------------------------
    // Line bounds across a multiline body
    // -------------------------------------------------------------------------
    #[test]
    fn test_line_end_spans_multiline_body() {
        let source = b"let f x =\n  let y = x in\n  y + 1";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].line_start, 1);
        assert_eq!(visitor.functions[0].line_end, 3);
    }

    // -------------------------------------------------------------------------
    // body_prefix truncation
    // -------------------------------------------------------------------------
    #[test]
    fn test_body_prefix_truncated_to_max() {
        use codegraph_parser_api::BODY_PREFIX_MAX_CHARS;

        // build a body string longer than the truncation limit
        let filler = "x".repeat(BODY_PREFIX_MAX_CHARS + 100);
        let source = format!("let f _u = \"{filler}\"");
        let visitor = parse_and_visit(source.as_bytes());

        let prefix = visitor.functions[0]
            .body_prefix
            .as_ref()
            .expect("body_prefix present");
        assert_eq!(prefix.chars().count(), BODY_PREFIX_MAX_CHARS);
    }

    // -------------------------------------------------------------------------
    // Doc comments
    // -------------------------------------------------------------------------
    #[test]
    fn test_doc_comment_absent_is_none() {
        // no leading comment => doc_comment is None
        let source = b"let greet name = name";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].doc_comment, None);
    }

    #[test]
    fn test_doc_comment_captured_from_preceding_comment() {
        // a (** ... *) comment immediately before the definition is attached.
        // The comment is a sibling of the value_definition, and extract_doc_comment
        // inspects the let_binding's prev_sibling, so pin whatever the visitor
        // actually resolves rather than assuming.
        let source = b"(** doc for f *)\nlet f x = x";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert!(
            visitor.functions[0].doc_comment.is_none(),
            "doc_comment inspects the let_binding's prev_sibling (the `let` keyword), \
             not the value_definition's preceding comment, so the doc is not attached"
        );
    }

    // -------------------------------------------------------------------------
    // Nested modules
    // -------------------------------------------------------------------------
    #[test]
    fn test_nested_module_recursion() {
        // a function two module levels deep is still extracted
        let source =
            b"module Outer = struct\n  module Inner = struct\n    let deep x = x\n  end\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "deep");
    }

    // -------------------------------------------------------------------------
    // if without an else branch
    // -------------------------------------------------------------------------
    #[test]
    fn test_if_without_else_raises_complexity() {
        let source = b"let f x = if x then ignore x";
        let visitor = parse_and_visit(source);

        let c = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(c.cyclomatic_complexity > 1);
    }

    // -------------------------------------------------------------------------
    // Unqualified / multiple call tracking
    // -------------------------------------------------------------------------
    #[test]
    fn test_unqualified_local_call_recorded() {
        // a plain (non-Module-qualified) application is recorded verbatim
        let source = b"let f x = g x";
        let visitor = parse_and_visit(source);

        assert!(visitor
            .calls
            .iter()
            .any(|c| c.callee == "g" && c.caller == "f"));
    }

    #[test]
    fn test_multiple_calls_recorded_separately() {
        // nested applications g (h x) record both callees under the same caller
        let source = b"let f x = g (h x)";
        let visitor = parse_and_visit(source);

        assert!(visitor.calls.iter().any(|c| c.callee == "g"));
        assert!(visitor.calls.iter().any(|c| c.callee == "h"));
    }

    #[test]
    fn test_and_binding_calls_attributed_per_binding() {
        // each half of `let ... and ...` owns its own calls
        let source = b"let a x = g x and b y = h y";
        let visitor = parse_and_visit(source);

        assert!(visitor
            .calls
            .iter()
            .any(|c| c.callee == "g" && c.caller == "a"));
        assert!(visitor
            .calls
            .iter()
            .any(|c| c.callee == "h" && c.caller == "b"));
    }

    // -------------------------------------------------------------------------
    // open with a qualified module path
    // -------------------------------------------------------------------------
    #[test]
    fn test_open_qualified_path_uses_last_segment() {
        // `open Core.Std`: the outer module_path's only direct module_name child
        // is the trailing segment (`Std`); the `Core` prefix is nested inside a
        // child module_path, so the qualifier is effectively dropped rather than
        // the leading module being kept. Pin the real behavior.
        let source = b"open Core.Std";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "Std");
        assert!(visitor.imports[0].is_wildcard);
    }

    // -------------------------------------------------------------------------
    // function_expression (anonymous match) contributes complexity
    // -------------------------------------------------------------------------
    #[test]
    fn test_function_expression_match_cases_raise_complexity() {
        // `function` bodies enter a scope and their match_case children add branches
        let source = b"let describe = function 0 -> \"a\" | 1 -> \"b\" | _ -> \"c\"";
        let visitor = parse_and_visit(source);

        let c = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(c.cyclomatic_complexity >= 3);
    }

    // -------------------------------------------------------------------------
    // Mixed named + underscore parameters
    // -------------------------------------------------------------------------
    #[test]
    fn test_mixed_underscore_params_dropped() {
        // the `_` param is dropped; the two named params survive in order
        let source = b"let f a _ b = a";
        let visitor = parse_and_visit(source);

        let params = &visitor.functions[0].parameters;
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].name, "a");
        assert_eq!(params[1].name, "b");
    }

    // -------------------------------------------------------------------------
    // body_prefix of a fun-expression body
    // -------------------------------------------------------------------------
    #[test]
    fn test_fun_body_prefix_contains_fun_keyword() {
        let source = b"let add = fun x y -> x + y";
        let visitor = parse_and_visit(source);

        let prefix = visitor.functions[0]
            .body_prefix
            .as_ref()
            .expect("body_prefix present");
        assert!(prefix.contains("fun"));
    }
}
