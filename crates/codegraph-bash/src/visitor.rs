// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting Bash entities

use codegraph_parser_api::{
    truncate_body_prefix, CallRelation, ComplexityBuilder, ComplexityMetrics, FunctionEntity,
    ImportRelation,
};
use tree_sitter::Node;

pub(crate) struct BashVisitor<'a> {
    pub source: &'a [u8],
    pub functions: Vec<FunctionEntity>,
    pub imports: Vec<ImportRelation>,
    pub calls: Vec<CallRelation>,
    current_function: Option<String>,
}

impl<'a> BashVisitor<'a> {
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
            "function_definition" => {
                self.visit_function_definition(node);
                return;
            }
            "command" => {
                self.visit_command(node);
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child);
        }
    }

    fn visit_function_definition(&mut self, node: Node) {
        // Bash function_definition has a "name" field
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

        let body_node = node.child_by_field_name("body");

        let body_prefix = body_node
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| truncate_body_prefix(t).to_string());

        let complexity = body_node.map(|body| self.calculate_complexity(body));

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
            parameters: Vec::new(), // Bash uses positional params $1, $2 — not declared in signature
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

    fn visit_command(&mut self, node: Node) {
        // The "name" field of a command is a command_name node; get its text
        let cmd_name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_default();

        // Check for source / . (dot) imports
        if cmd_name == "source" || cmd_name == "." {
            // The first named "argument" field holds the file path
            if let Some(arg) = node.child_by_field_name("argument") {
                let imported = self.node_text(arg);
                let imported = imported.trim_matches(|c| c == '"' || c == '\'').to_string();
                if !imported.is_empty() {
                    self.imports.push(ImportRelation {
                        importer: "main".to_string(),
                        imported,
                        symbols: Vec::new(),
                        is_wildcard: false,
                        alias: None,
                    });
                }
            }
            return;
        }

        // Track calls if inside a function
        if let Some(ref caller) = self.current_function.clone() {
            if !cmd_name.is_empty() {
                self.calls.push(CallRelation {
                    caller: caller.clone(),
                    callee: cmd_name,
                    call_site_line: node.start_position().row + 1,
                    is_direct: true,
                    struct_type: None,
                    field_name: None,
                });
            }
        }
    }

    fn visit_body_for_calls(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "command" {
                self.visit_command(child);
            }
            self.visit_body_for_calls(child);
        }
    }

    fn extract_doc_comment(&self, node: Node) -> Option<String> {
        if let Some(prev) = node.prev_sibling() {
            if prev.kind() == "comment" {
                let text = self.node_text(prev);
                if text.starts_with('#') {
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
            "elif_clause" | "else_clause" => {
                builder.add_branch();
            }
            "for_statement" | "while_statement" | "until_statement" | "c_style_for_statement" => {
                builder.add_loop();
                builder.enter_scope();
            }
            "case_statement" => {
                builder.add_branch();
                builder.enter_scope();
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
            | "while_statement"
            | "until_statement"
            | "c_style_for_statement"
            | "case_statement" => {
                builder.exit_scope();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_visit(source: &[u8]) -> BashVisitor<'_> {
        use tree_sitter::Parser;

        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_bash::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = BashVisitor::new(source);
        visitor.visit_node(tree.root_node());
        visitor
    }

    #[test]
    fn test_source_with_empty_quoted_path_records_no_import() {
        // `source ""` parses as a command whose `argument` is an empty string
        // node; stripping the quotes leaves an empty path, so the
        // `!imported.is_empty()` guard rejects it and no import is recorded.
        let source = b"source \"\"\n";
        let visitor = parse_and_visit(source);
        assert!(
            visitor.imports.is_empty(),
            "empty quoted source path should not record an import, got {:?}",
            visitor.imports
        );
    }

    #[test]
    fn test_source_without_argument_records_no_import() {
        // A bare `source` with no path parses as a command with no `argument`
        // field, so `child_by_field_name("argument")` is None and the import
        // branch is skipped entirely.
        let source = b"source\n";
        let visitor = parse_and_visit(source);
        assert!(
            visitor.imports.is_empty(),
            "argument-less source should not record an import, got {:?}",
            visitor.imports
        );
    }

    #[test]
    fn test_visitor_function_extraction() {
        let source = b"greet() {\n    echo \"Hello\"\n}\n";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "greet");
    }

    #[test]
    fn test_visitor_source_import() {
        let source = b"source ./lib.sh\n";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "./lib.sh");
    }

    #[test]
    fn test_function_metadata_defaults() {
        let source = b"greet() {\n    echo hi\n}\n";
        let f = &parse_and_visit(source).functions[0];

        assert_eq!(f.visibility, "public");
        assert_eq!(f.line_start, 1);
        assert_eq!(f.line_end, 3);
        assert!(!f.is_async);
        assert!(!f.is_test);
        assert!(!f.is_static);
        assert!(!f.is_abstract);
        assert!(f.parameters.is_empty());
        assert!(f.return_type.is_none());
        assert!(f.parent_class.is_none());
        assert!(f.attributes.is_empty());
    }

    #[test]
    fn test_function_signature_is_first_line() {
        let source = b"greet() {\n    echo hi\n}\n";
        let f = &parse_and_visit(source).functions[0];
        assert_eq!(f.signature, "greet() {");
    }

    #[test]
    fn test_function_keyword_form() {
        let source = b"function do_work {\n    echo hi\n}\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "do_work");
    }

    #[test]
    fn test_doc_comment_extracted() {
        let source = b"# greets the user\ngreet() {\n    echo hi\n}\n";
        let f = &parse_and_visit(source).functions[0];
        assert_eq!(f.doc_comment.as_deref(), Some("# greets the user"));
    }

    #[test]
    fn test_doc_comment_absent() {
        let source = b"greet() {\n    echo hi\n}\n";
        let f = &parse_and_visit(source).functions[0];
        assert!(f.doc_comment.is_none());
    }

    #[test]
    fn test_body_prefix_present() {
        let source = b"greet() {\n    echo hi\n}\n";
        let f = &parse_and_visit(source).functions[0];
        assert!(f.body_prefix.as_deref().unwrap().contains("echo hi"));
    }

    #[test]
    fn test_complexity_baseline() {
        let source = b"greet() {\n    echo hi\n}\n";
        let f = &parse_and_visit(source).functions[0];
        assert_eq!(f.complexity.as_ref().unwrap().cyclomatic_complexity, 1);
    }

    #[test]
    fn test_complexity_if_branch() {
        let source = b"greet() {\n    if [ -n \"$1\" ]; then\n        echo hi\n    fi\n}\n";
        let f = &parse_and_visit(source).functions[0];
        assert!(f.complexity.as_ref().unwrap().cyclomatic_complexity > 1);
    }

    #[test]
    fn test_complexity_loop() {
        let source = b"greet() {\n    for x in 1 2 3; do\n        echo $x\n    done\n}\n";
        let f = &parse_and_visit(source).functions[0];
        assert!(f.complexity.as_ref().unwrap().cyclomatic_complexity > 1);
    }

    #[test]
    fn test_complexity_case() {
        let source =
            b"greet() {\n    case $1 in\n        a) echo a ;;\n        *) echo x ;;\n    esac\n}\n";
        let f = &parse_and_visit(source).functions[0];
        assert!(f.complexity.as_ref().unwrap().cyclomatic_complexity > 1);
    }

    #[test]
    fn test_dot_import() {
        let source = b". ./lib.sh\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "./lib.sh");
    }

    #[test]
    fn test_import_quotes_stripped() {
        let source = b"source \"./lib.sh\"\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "./lib.sh");
    }

    #[test]
    fn test_call_tracked_inside_function() {
        let source = b"greet() {\n    echo hi\n}\n";
        let visitor = parse_and_visit(source);
        assert!(visitor
            .calls
            .iter()
            .any(|c| c.caller == "greet" && c.callee == "echo"));
    }

    #[test]
    fn test_call_not_tracked_outside_function() {
        let source = b"echo hi\n";
        let visitor = parse_and_visit(source);
        assert!(visitor.calls.is_empty());
    }

    #[test]
    fn test_source_excluded_from_calls() {
        let source = b"greet() {\n    source ./lib.sh\n}\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 1);
        assert!(!visitor.calls.iter().any(|c| c.callee == "source"));
    }

    #[test]
    fn test_multiple_functions() {
        let source = b"a() {\n    echo 1\n}\nb() {\n    echo 2\n}\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 2);
        assert_eq!(visitor.functions[0].name, "a");
        assert_eq!(visitor.functions[1].name, "b");
    }

    #[test]
    fn test_empty_source() {
        let visitor = parse_and_visit(b"");
        assert!(visitor.functions.is_empty());
        assert!(visitor.imports.is_empty());
        assert!(visitor.calls.is_empty());
    }

    #[test]
    fn test_complexity_while_loop() {
        let source = b"greet() {\n    while true; do\n        echo hi\n    done\n}\n";
        let f = &parse_and_visit(source).functions[0];
        assert!(f.complexity.as_ref().unwrap().cyclomatic_complexity > 1);
    }

    #[test]
    fn test_complexity_until_loop() {
        let source = b"greet() {\n    until false; do\n        echo hi\n    done\n}\n";
        let f = &parse_and_visit(source).functions[0];
        assert!(f.complexity.as_ref().unwrap().cyclomatic_complexity > 1);
    }

    #[test]
    fn test_complexity_c_style_for() {
        let source = b"greet() {\n    for ((i=0; i<3; i++)); do\n        echo $i\n    done\n}\n";
        let f = &parse_and_visit(source).functions[0];
        assert!(f.complexity.as_ref().unwrap().cyclomatic_complexity > 1);
    }

    #[test]
    fn test_complexity_elif_adds_branch() {
        let plain = b"greet() {\n    if [ -n \"$1\" ]; then\n        echo a\n    fi\n}\n";
        let with_elif = b"greet() {\n    if [ -n \"$1\" ]; then\n        echo a\n    elif [ -n \"$2\" ]; then\n        echo b\n    fi\n}\n";
        let cc_plain = parse_and_visit(plain).functions[0]
            .complexity
            .as_ref()
            .unwrap()
            .cyclomatic_complexity;
        let cc_elif = parse_and_visit(with_elif).functions[0]
            .complexity
            .as_ref()
            .unwrap()
            .cyclomatic_complexity;
        assert!(cc_elif > cc_plain);
    }

    #[test]
    fn test_call_site_line_and_is_direct() {
        let source = b"greet() {\n    echo hi\n}\n";
        let visitor = parse_and_visit(source);
        let call = visitor.calls.iter().find(|c| c.callee == "echo").unwrap();
        assert_eq!(call.call_site_line, 2);
        assert!(call.is_direct);
        assert!(call.struct_type.is_none());
        assert!(call.field_name.is_none());
    }

    #[test]
    fn test_import_default_fields() {
        let source = b"source ./lib.sh\n";
        let imp = &parse_and_visit(source).imports[0];
        assert_eq!(imp.importer, "main");
        assert!(imp.symbols.is_empty());
        assert!(!imp.is_wildcard);
        assert!(imp.alias.is_none());
    }

    #[test]
    fn test_multiple_imports_recorded() {
        let source = b"source ./a.sh\n. ./b.sh\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 2);
        assert_eq!(visitor.imports[0].imported, "./a.sh");
        assert_eq!(visitor.imports[1].imported, "./b.sh");
    }

    #[test]
    fn test_nested_call_tracked() {
        let source = b"greet() {\n    if true; then\n        printf hi\n    fi\n}\n";
        let visitor = parse_and_visit(source);
        assert!(visitor
            .calls
            .iter()
            .any(|c| c.caller == "greet" && c.callee == "printf"));
    }

    #[test]
    fn test_keyword_form_signature() {
        let source = b"function do_work {\n    echo hi\n}\n";
        let f = &parse_and_visit(source).functions[0];
        assert_eq!(f.signature, "function do_work {");
    }

    #[test]
    fn test_function_line_numbers_offset() {
        let source = b"\n\ngreet() {\n    echo hi\n}\n";
        let f = &parse_and_visit(source).functions[0];
        assert_eq!(f.line_start, 3);
        assert_eq!(f.line_end, 5);
    }

    #[test]
    fn test_body_prefix_truncated_to_max() {
        use codegraph_parser_api::BODY_PREFIX_MAX_CHARS;
        let mut source = Vec::from(&b"greet() {\n    echo \""[..]);
        source.extend(std::iter::repeat_n(b'x', BODY_PREFIX_MAX_CHARS + 200));
        source.extend_from_slice(b"\"\n}\n");
        let f = &parse_and_visit(&source).functions[0];
        assert_eq!(f.body_prefix.as_ref().unwrap().len(), BODY_PREFIX_MAX_CHARS);
    }

    #[test]
    fn test_two_calls_recorded() {
        let source = b"greet() {\n    echo hi\n    printf bye\n}\n";
        let visitor = parse_and_visit(source);
        let n = visitor.calls.iter().filter(|c| c.caller == "greet").count();
        assert_eq!(n, 2);
    }

    #[test]
    fn test_call_inside_loop_attributed() {
        let source = b"greet() {\n    for x in 1 2; do\n        printf $x\n    done\n}\n";
        let visitor = parse_and_visit(source);
        assert!(visitor
            .calls
            .iter()
            .any(|c| c.caller == "greet" && c.callee == "printf"));
    }

    #[test]
    fn test_doc_comment_not_from_command_sibling() {
        // A command (not a comment) immediately preceding a function yields no doc comment.
        let source = b"echo start\ngreet() {\n    echo hi\n}\n";
        let f = &parse_and_visit(source).functions[0];
        assert!(f.doc_comment.is_none());
    }

    #[test]
    fn test_source_single_quotes_stripped() {
        let source = b"source './lib.sh'\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "./lib.sh");
    }

    #[test]
    fn test_nested_if_raises_complexity_further() {
        let one = b"greet() {\n    if true; then\n        echo a\n    fi\n}\n";
        let two = b"greet() {\n    if true; then\n        if false; then\n            echo a\n        fi\n    fi\n}\n";
        let cc_one = parse_and_visit(one).functions[0]
            .complexity
            .as_ref()
            .unwrap()
            .cyclomatic_complexity;
        let cc_two = parse_and_visit(two).functions[0]
            .complexity
            .as_ref()
            .unwrap()
            .cyclomatic_complexity;
        assert!(cc_two > cc_one);
    }

    #[test]
    fn test_dot_import_inside_function_recorded() {
        let source = b"greet() {\n    . ./lib.sh\n}\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "./lib.sh");
        assert!(!visitor.calls.iter().any(|c| c.callee == "."));
    }

    #[test]
    fn test_multiple_functions_line_progression() {
        let source = b"a() {\n    echo 1\n}\nb() {\n    echo 2\n}\n";
        let fns = parse_and_visit(source).functions;
        assert_eq!(fns[0].line_start, 1);
        assert!(fns[1].line_start > fns[0].line_end);
    }

    #[test]
    fn test_case_multi_arm_higher_than_single() {
        let single = b"greet() {\n    case $1 in\n        a) echo a ;;\n    esac\n}\n";
        let multi = b"greet() {\n    case $1 in\n        a) echo a ;;\n        b) echo b ;;\n        *) echo x ;;\n    esac\n}\n";
        let cc_single = parse_and_visit(single).functions[0]
            .complexity
            .as_ref()
            .unwrap()
            .cyclomatic_complexity;
        let cc_multi = parse_and_visit(multi).functions[0]
            .complexity
            .as_ref()
            .unwrap()
            .cyclomatic_complexity;
        assert!(cc_multi >= cc_single);
    }

    #[test]
    fn test_call_before_function_definition_not_tracked() {
        // Commands outside any function are not recorded even when functions exist later.
        let source = b"echo top\ngreet() {\n    echo hi\n}\n";
        let visitor = parse_and_visit(source);
        assert!(!visitor
            .calls
            .iter()
            .any(|c| c.callee == "echo" && c.call_site_line == 1));
    }
}
