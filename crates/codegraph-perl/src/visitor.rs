// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting Perl entities

use codegraph_parser_api::{
    truncate_body_prefix, CallRelation, ClassEntity, ComplexityBuilder, ComplexityMetrics,
    FunctionEntity, ImportRelation, Parameter,
};
use tree_sitter::Node;

pub(crate) struct PerlVisitor<'a> {
    pub source: &'a [u8],
    pub functions: Vec<FunctionEntity>,
    pub classes: Vec<ClassEntity>,
    pub imports: Vec<ImportRelation>,
    pub calls: Vec<CallRelation>,
    current_function: Option<String>,
    current_package: Option<String>,
}

impl<'a> PerlVisitor<'a> {
    pub fn new(source: &'a [u8]) -> Self {
        Self {
            source,
            functions: Vec::new(),
            classes: Vec::new(),
            imports: Vec::new(),
            calls: Vec::new(),
            current_function: None,
            current_package: None,
        }
    }

    fn node_text(&self, node: Node) -> String {
        node.utf8_text(self.source).unwrap_or("").to_string()
    }

    pub fn visit_node(&mut self, node: Node) {
        match node.kind() {
            "package_statement" => {
                self.visit_package_statement(node);
                return;
            }
            "function_definition" => {
                self.visit_sub_declaration(node);
                return;
            }
            "use_no_statement" => {
                self.visit_use_statement(node);
            }
            "use_parent_statement" | "use_base_statement" => {
                self.visit_use_parent_statement(node);
            }
            "require_expression" | "require_statement" => {
                self.visit_require_expression(node);
            }
            "call_expression_with_spaced_args"
            | "call_expression_with_bareword"
            | "method_call_expression" => {
                self.visit_call_expression(node);
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child);
        }
    }

    fn visit_package_statement(&mut self, node: Node) {
        // package_statement: "package" package_expression ";"
        // package_expression contains the name
        let name = self.find_package_name(node);
        if name.is_empty() {
            return;
        }

        let line_start = node.start_position().row + 1;
        let line_end = node.end_position().row + 1;

        self.current_package = Some(name.clone());

        let class = ClassEntity {
            name: name.clone(),
            visibility: "public".to_string(),
            line_start,
            line_end,
            is_abstract: false,
            is_interface: false,
            base_classes: Vec::new(),
            implemented_traits: Vec::new(),
            fields: Vec::new(),
            doc_comment: self.extract_doc_comment(node),
            attributes: Vec::new(),
            type_parameters: Vec::new(),
            methods: Vec::new(),
            body_prefix: None,
        };
        self.classes.push(class);

        // Visit children (sub declarations inside package scope are handled at
        // top level since Perl's package scope extends to the next package decl)
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child);
        }
    }

    fn find_package_name(&self, node: Node) -> String {
        // package_statement has package_name child containing identifier(s)
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "package_name" {
                return self.node_text(child);
            }
        }
        String::new()
    }

    fn visit_sub_declaration(&mut self, node: Node) {
        // subroutine_declaration_statement has: name, prototype?, block
        let name = self.find_sub_name(node);
        if name.is_empty() {
            return;
        }

        let is_private = name.starts_with('_');
        let visibility = if is_private { "private" } else { "public" }.to_string();

        let full_name = if let Some(ref pkg) = self.current_package.clone() {
            format!("{}::{}", pkg, name)
        } else {
            name.clone()
        };

        let signature = format!("sub {}", name);

        let doc_comment = self.extract_doc_comment(node);

        let body_node = self.find_block(node);
        let body_prefix = body_node
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| truncate_body_prefix(t).to_string());

        let complexity = body_node.map(|b| self.calculate_complexity(b));

        let parameters = self.extract_perl_parameters(node);

        let parent_class = self.current_package.clone();

        let func = FunctionEntity {
            name: full_name.clone(),
            signature,
            visibility,
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_async: false,
            is_test: name.starts_with("test_") || name.starts_with("Test"),
            is_static: false,
            is_abstract: false,
            parameters,
            return_type: None,
            doc_comment,
            attributes: Vec::new(),
            parent_class,
            complexity,
            body_prefix,
        };

        self.functions.push(func);

        let prev_function = self.current_function.take();
        self.current_function = Some(full_name);

        if let Some(block) = body_node {
            self.visit_body_for_calls(block);
        }

        self.current_function = prev_function;
    }

    fn find_sub_name(&self, node: Node) -> String {
        // function_definition has a "name" field
        if let Some(name_node) = node.child_by_field_name("name") {
            return self.node_text(name_node);
        }
        // Fallback: look for identifier child
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "identifier" {
                return self.node_text(child);
            }
        }
        String::new()
    }

    #[allow(clippy::manual_find)] // Iterator::find can't return a cursor-borrowing Node
    fn find_block<'b>(&self, node: Node<'b>) -> Option<Node<'b>> {
        // function_definition has a "body" field
        if let Some(body) = node.child_by_field_name("body") {
            return Some(body);
        }
        // Fallback: look for block child. Cannot use Iterator::find here because
        // the returned Node borrows the TreeCursor, which must outlive the search.
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "block" {
                return Some(child);
            }
        }
        None
    }

    fn extract_perl_parameters(&self, node: Node) -> Vec<Parameter> {
        // Perl parameters aren't formally declared in the signature —
        // they come from @_. We look for prototype nodes for hint.
        let mut params = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "prototype" {
                let text = self.node_text(child);
                // prototype like ($self, $name) or ($$$)
                for part in text.trim_matches(|c| c == '(' || c == ')').split(',') {
                    let p = part.trim();
                    if !p.is_empty() {
                        params.push(Parameter::new(p));
                    }
                }
            }
        }
        params
    }

    fn visit_use_statement(&mut self, node: Node) {
        // use Module::Name; or use Module::Name qw(...);
        let module = self.extract_use_module(node);
        if !module.is_empty()
            && module != "strict"
            && module != "warnings"
            && module != "utf8"
            && module != "feature"
            && module != "constant"
            && module != "overload"
            && module != "vars"
            && module != "base"
            && module != "parent"
        {
            self.imports.push(ImportRelation {
                importer: self
                    .current_package
                    .clone()
                    .unwrap_or_else(|| "main".to_string()),
                imported: module,
                symbols: Vec::new(),
                is_wildcard: false,
                alias: None,
            });
        } else if module == "parent" || module == "base" {
            // use parent 'SomeClass'; — extract the parent class name
            let parent = self.extract_use_list(node);
            for p in parent {
                self.imports.push(ImportRelation {
                    importer: self
                        .current_package
                        .clone()
                        .unwrap_or_else(|| "main".to_string()),
                    imported: p,
                    symbols: Vec::new(),
                    is_wildcard: false,
                    alias: None,
                });
            }
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child);
        }
    }

    fn visit_use_parent_statement(&mut self, node: Node) {
        // use parent 'SomeClass'; / use base 'SomeClass'; — the grammar emits a
        // dedicated use_parent_statement/use_base_statement node, so extract the
        // quoted parent class name(s) as imports.
        for parent in self.extract_use_list(node) {
            self.imports.push(ImportRelation {
                importer: self
                    .current_package
                    .clone()
                    .unwrap_or_else(|| "main".to_string()),
                imported: parent,
                symbols: Vec::new(),
                is_wildcard: false,
                alias: None,
            });
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child);
        }
    }

    fn extract_use_module(&self, node: Node) -> String {
        // use_no_statement has package_name field
        if let Some(pkg) = node.child_by_field_name("package_name") {
            return self.node_text(pkg);
        }
        // Fallback: look for package_name or identifier child
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "package_name" | "package_expression" | "identifier" => {
                    return self.node_text(child);
                }
                _ => {}
            }
        }
        String::new()
    }

    fn extract_use_list(&self, node: Node) -> Vec<String> {
        let mut list = Vec::new();
        let text = self.node_text(node);
        // Extract quoted strings from the statement
        for part in text.split_whitespace() {
            let cleaned = part.trim_matches(|c| {
                c == '\'' || c == '"' || c == ',' || c == ';' || c == '(' || c == ')'
            });
            if !cleaned.is_empty() && cleaned.contains("::") {
                list.push(cleaned.to_string());
            }
        }
        list
    }

    fn visit_require_expression(&mut self, node: Node) {
        let text = self.node_text(node);
        // require 'Module/Name.pm' or require Module::Name
        let module = text
            .trim_start_matches("require")
            .trim()
            .trim_matches(|c| c == '\'' || c == '"' || c == ';')
            .replace('/', "::")
            .replace(".pm", "");
        if !module.is_empty() {
            self.imports.push(ImportRelation {
                importer: self
                    .current_package
                    .clone()
                    .unwrap_or_else(|| "main".to_string()),
                imported: module,
                symbols: Vec::new(),
                is_wildcard: false,
                alias: None,
            });
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child);
        }
    }

    fn visit_call_expression(&mut self, node: Node) {
        if let Some(ref caller) = self.current_function.clone() {
            // Get the function being called
            let callee = self.extract_callee_name(node);
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

    fn extract_callee_name(&self, node: Node) -> String {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "identifier" | "package_expression" => {
                    return self.node_text(child);
                }
                _ => {}
            }
        }
        String::new()
    }

    fn visit_body_for_calls(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "call_expression_with_spaced_args"
                | "call_expression_with_bareword"
                | "method_call_expression" => {
                    self.visit_call_expression(child);
                    self.visit_body_for_calls(child);
                }
                _ => {
                    self.visit_body_for_calls(child);
                }
            }
        }
    }

    fn extract_doc_comment(&self, node: Node) -> Option<String> {
        if let Some(prev) = node.prev_sibling() {
            if prev.kind() == "comment" || prev.kind() == "comments" {
                let text = self.node_text(prev);
                if text.starts_with("##") || text.starts_with("#!") || text.starts_with("# ") {
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
            "if_statement" | "unless_statement" => {
                builder.add_branch();
                builder.enter_scope();
            }
            "elsif_clause" | "else_clause" => {
                builder.add_branch();
            }
            "while_statement" | "until_statement" | "for_statement" | "foreach_statement" => {
                builder.add_loop();
                builder.enter_scope();
            }
            "binary_expression" => {
                let text = self.node_text(node);
                if text.contains(" && ")
                    || text.contains(" || ")
                    || text.contains(" and ")
                    || text.contains(" or ")
                {
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
            "if_statement" | "unless_statement" | "while_statement" | "until_statement"
            | "for_statement" | "foreach_statement" => {
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

    fn parse_and_visit(source: &[u8]) -> PerlVisitor<'_> {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&crate::ts_perl::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut visitor = PerlVisitor::new(source);
        visitor.visit_node(tree.root_node());
        visitor
    }

    #[test]
    fn test_visitor_function_extraction() {
        let source = b"sub greet {\n    print \"Hello\\n\";\n}\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "greet");
    }

    #[test]
    fn test_visitor_package_extraction() {
        let source = b"package MyApp::User;\nsub new { }\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.classes.len(), 1);
        assert!(visitor.classes[0].name.contains("MyApp"));
        assert_eq!(visitor.functions.len(), 1);
    }

    #[test]
    fn test_visitor_use_extraction() {
        let source = b"use Moose;\nuse Data::Dumper;\n";
        let visitor = parse_and_visit(source);
        assert!(!visitor.imports.is_empty());
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
    fn test_private_function_visibility() {
        let source = b"sub _helper {\n    return 1;\n}\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].visibility, "private");
    }

    #[test]
    fn test_public_function_visibility() {
        let source = b"sub run {\n    return 1;\n}\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions[0].visibility, "public");
    }

    #[test]
    fn test_signature_uses_bare_name_not_package_qualified() {
        let source = b"package Foo;\nsub greet {\n    return 1;\n}\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions[0].signature, "sub greet");
    }

    #[test]
    fn test_package_qualified_full_name_and_parent_class() {
        let source = b"package MyApp::User;\nsub load {\n    return 1;\n}\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "MyApp::User::load");
        assert_eq!(
            visitor.functions[0].parent_class.as_deref(),
            Some("MyApp::User")
        );
    }

    #[test]
    fn test_function_without_package_has_no_parent() {
        let source = b"sub standalone {\n    return 1;\n}\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions[0].name, "standalone");
        assert!(visitor.functions[0].parent_class.is_none());
    }

    #[test]
    fn test_is_test_prefix_detection() {
        let source = b"sub test_login {\n    return 1;\n}\n";
        let visitor = parse_and_visit(source);
        assert!(visitor.functions[0].is_test);
    }

    #[test]
    fn test_non_test_function_not_flagged() {
        let source = b"sub login {\n    return 1;\n}\n";
        let visitor = parse_and_visit(source);
        assert!(!visitor.functions[0].is_test);
    }

    #[test]
    fn test_function_flag_defaults() {
        let source = b"sub plain {\n    return 1;\n}\n";
        let visitor = parse_and_visit(source);
        let f = &visitor.functions[0];
        assert!(!f.is_async);
        assert!(!f.is_static);
        assert!(!f.is_abstract);
        assert!(f.return_type.is_none());
        assert!(f.attributes.is_empty());
    }

    #[test]
    fn test_body_prefix_present() {
        let source = b"sub greet {\n    print \"hi\";\n}\n";
        let visitor = parse_and_visit(source);
        assert!(visitor.functions[0].body_prefix.is_some());
    }

    #[test]
    fn test_complexity_baseline() {
        let source = b"sub straight {\n    my $x = 1;\n    return $x;\n}\n";
        let visitor = parse_and_visit(source);
        let c = visitor.functions[0].complexity.as_ref().unwrap();
        assert_eq!(c.cyclomatic_complexity, 1);
    }

    #[test]
    fn test_complexity_if_increases() {
        let source = b"sub branchy {\n    if ($x) {\n        return 1;\n    }\n    return 0;\n}\n";
        let visitor = parse_and_visit(source);
        let c = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(c.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_complexity_loop_increases() {
        let source = b"sub looper {\n    while ($x) {\n        $x--;\n    }\n    return 0;\n}\n";
        let visitor = parse_and_visit(source);
        let c = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(c.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_doc_comment_extracted() {
        let source = b"# This greets the user\nsub greet {\n    return 1;\n}\n";
        let visitor = parse_and_visit(source);
        assert!(visitor.functions[0].doc_comment.is_some());
    }

    #[test]
    fn test_no_doc_comment() {
        let source = b"sub greet {\n    return 1;\n}\n";
        let visitor = parse_and_visit(source);
        assert!(visitor.functions[0].doc_comment.is_none());
    }

    #[test]
    fn test_use_strict_and_warnings_excluded() {
        let source = b"use strict;\nuse warnings;\n";
        let visitor = parse_and_visit(source);
        assert!(visitor.imports.is_empty());
    }

    #[test]
    fn test_use_module_import_recorded() {
        let source = b"use Data::Dumper;\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "Data::Dumper");
        assert_eq!(visitor.imports[0].importer, "main");
    }

    #[test]
    fn test_use_import_importer_is_current_package() {
        let source = b"package MyApp;\nuse Data::Dumper;\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].importer, "MyApp");
    }

    #[test]
    fn test_use_parent_extracts_base_class() {
        let source = b"use parent 'Base::Class';\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "Base::Class");
    }

    #[test]
    fn test_require_expression_recorded() {
        let source = b"require Foo::Bar;\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "Foo::Bar");
    }

    #[test]
    fn test_call_tracking_inside_function() {
        let source = b"sub run {\n    helper();\n}\n";
        let visitor = parse_and_visit(source);
        assert!(
            visitor.calls.iter().any(|c| c.callee == "helper"),
            "expected a call to helper, got {:?}",
            visitor.calls
        );
        assert!(visitor.calls.iter().all(|c| c.caller == "run"));
    }

    #[test]
    fn test_multiple_functions_extracted() {
        let source = b"sub a {\n    return 1;\n}\nsub b {\n    return 2;\n}\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 2);
    }

    #[test]
    fn test_package_class_metadata() {
        let source = b"package MyApp::Thing;\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.classes.len(), 1);
        let c = &visitor.classes[0];
        assert_eq!(c.visibility, "public");
        assert!(!c.is_abstract);
        assert!(!c.is_interface);
        assert!(c.base_classes.is_empty());
    }

    #[test]
    fn test_line_numbers_offset_by_leading_blank_lines() {
        // Two blank lines push the sub to line 3 (1-indexed); the body's closing
        // brace lands on line 5.
        let source = b"\n\nsub greet {\n    return 1;\n}";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions[0].line_start, 3);
        assert_eq!(visitor.functions[0].line_end, 5);
    }

    #[test]
    fn test_complexity_unless_increases() {
        let source =
            b"sub guard {\n    unless ($x) {\n        return 1;\n    }\n    return 0;\n}\n";
        let visitor = parse_and_visit(source);
        let c = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(c.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_complexity_logical_operator_increases() {
        // A `&&` inside a binary_expression body raises cyclomatic complexity.
        let source = b"sub combine {\n    my $c = $a && $b;\n    return $c;\n}\n";
        let visitor = parse_and_visit(source);
        let c = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(c.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_body_prefix_truncated_to_max() {
        // A body whose block text exceeds BODY_PREFIX_MAX_CHARS is truncated exactly.
        let filler = "x".repeat(BODY_PREFIX_MAX_CHARS + 200);
        let source = format!("sub big {{\n    my $s = \"{}\";\n}}\n", filler);
        let visitor = parse_and_visit(source.as_bytes());
        let bp = visitor.functions[0].body_prefix.as_ref().unwrap();
        assert_eq!(bp.chars().count(), BODY_PREFIX_MAX_CHARS);
    }

    #[test]
    fn test_two_packages_emit_two_classes() {
        let source =
            b"package Foo;\nsub a {\n    return 1;\n}\npackage Bar;\nsub b {\n    return 2;\n}\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.classes.len(), 2);
    }

    #[test]
    fn test_function_attributed_to_enclosing_package() {
        // The sub after the second package declaration is qualified with that package.
        let source =
            b"package Foo;\nsub a {\n    return 1;\n}\npackage Bar;\nsub b {\n    return 2;\n}\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions[0].name, "Foo::a");
        assert_eq!(visitor.functions[1].name, "Bar::b");
        assert_eq!(visitor.functions[1].parent_class.as_deref(), Some("Bar"));
    }

    #[test]
    fn test_require_importer_is_current_package() {
        // A bareword require inside a package is attributed to that package.
        let source = b"package MyApp;\nrequire Foo::Bar;\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "Foo::Bar");
        assert_eq!(visitor.imports[0].importer, "MyApp");
    }

    #[test]
    fn test_call_site_line_recorded() {
        let source = b"sub run {\n    helper();\n}\n";
        let visitor = parse_and_visit(source);
        let call = visitor.calls.iter().find(|c| c.callee == "helper").unwrap();
        assert_eq!(call.call_site_line, 2);
        assert!(call.is_direct);
    }

    #[test]
    fn test_is_test_capital_test_prefix() {
        let source = b"sub TestLogin {\n    return 1;\n}\n";
        let visitor = parse_and_visit(source);
        assert!(visitor.functions[0].is_test);
    }

    #[test]
    fn test_nested_call_attributed_to_function() {
        // A call inside an if block still belongs to the enclosing sub.
        let source = b"sub run {\n    if ($x) {\n        helper();\n    }\n}\n";
        let visitor = parse_and_visit(source);
        assert!(visitor.calls.iter().any(|c| c.callee == "helper"));
        assert!(visitor.calls.iter().all(|c| c.caller == "run"));
    }

    #[test]
    fn test_foreach_complexity_gap() {
        // tree-sitter-perl emits `for_statement_2` for the `foreach my $i (@list)`
        // form, which the complexity visitor does not match (it only handles
        // for_statement/foreach_statement), so complexity stays at baseline 1.
        let source =
            b"sub iter {\n    foreach my $i (@list) {\n        print $i;\n    }\n    return 0;\n}\n";
        let visitor = parse_and_visit(source);
        let c = visitor.functions[0].complexity.as_ref().unwrap();
        assert_eq!(c.cyclomatic_complexity, 1);
    }

    #[test]
    fn test_complexity_until_increases() {
        let source =
            b"sub wait_loop {\n    until ($done) {\n        $done = check();\n    }\n    return 0;\n}\n";
        let visitor = parse_and_visit(source);
        let c = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(c.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_complexity_word_or_operator_increases() {
        // The word form `or` in a binary_expression raises complexity like `||`.
        let source = b"sub pick {\n    my $c = $a or $b;\n    return $c;\n}\n";
        let visitor = parse_and_visit(source);
        let c = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(c.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_complexity_elsif_adds_branches() {
        // if / elsif / else should push complexity to at least 3.
        let source = b"sub grade {\n    if ($x) {\n        return 1;\n    } elsif ($y) {\n        return 2;\n    } else {\n        return 3;\n    }\n}\n";
        let visitor = parse_and_visit(source);
        let c = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(c.cyclomatic_complexity >= 3);
    }

    #[test]
    fn test_method_invocation_call_gap() {
        // $obj->greet() parses as a `method_invocation` node, but the visitor only
        // records call_expression_with_spaced_args/bareword/method_call_expression,
        // so method calls are silently dropped - pinned as a regression test.
        let source = b"sub run {\n    $obj->greet();\n}\n";
        let visitor = parse_and_visit(source);
        assert!(
            visitor.calls.is_empty(),
            "method_invocation should not be recorded, got {:?}",
            visitor.calls
        );
    }

    #[test]
    fn test_require_quoted_file_path_gap() {
        // A quoted require ('Foo/Bar.pm') fails to parse - tree-sitter-perl wraps it
        // in an ERROR node with no require_expression/require_statement, so the
        // slash->:: / .pm-stripping normalization path never runs and no import is
        // recorded. Only bareword `require Foo::Bar` is handled.
        let source = b"require 'Foo/Bar.pm';\n";
        let visitor = parse_and_visit(source);
        assert!(visitor.imports.is_empty());
    }

    #[test]
    fn test_use_feature_excluded() {
        let source = b"use feature 'say';\n";
        let visitor = parse_and_visit(source);
        assert!(visitor.imports.is_empty());
    }

    #[test]
    fn test_use_constant_excluded() {
        let source = b"use constant PI => 3.14;\n";
        let visitor = parse_and_visit(source);
        assert!(visitor.imports.is_empty());
    }

    #[test]
    fn test_use_base_records_import() {
        let source = b"use base 'Some::Base';\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "Some::Base");
    }

    #[test]
    fn test_use_parent_without_colons_dropped() {
        // extract_use_list only keeps names containing "::", so a single-segment
        // parent name yields no import - a latent gap pinned as a regression test.
        let source = b"use parent 'Base';\n";
        let visitor = parse_and_visit(source);
        assert!(visitor.imports.is_empty());
    }

    #[test]
    fn test_comment_without_space_is_not_doc() {
        // extract_doc_comment requires "##", "#!", or "# " - a bare "#word" comment
        // is not attached as a doc_comment.
        let source = b"#nospace\nsub greet {\n    return 1;\n}\n";
        let visitor = parse_and_visit(source);
        assert!(visitor.functions[0].doc_comment.is_none());
    }

    #[test]
    fn test_empty_body_yields_brace_prefix() {
        // An empty {} block has non-empty text so body_prefix is Some, starting with
        // the braces (the block node text spans through the trailing newline).
        let source = b"sub noop {}\n";
        let visitor = parse_and_visit(source);
        let bp = visitor.functions[0].body_prefix.as_ref().unwrap();
        assert!(bp.starts_with("{}"), "unexpected body_prefix {:?}", bp);
    }
    #[test]
    fn test_complexity_symbolic_or_operator_increases() {
        // The `||` spelling in a binary_expression raises complexity like `&&`.
        let source = b"sub pick {\n    my $c = $a || $b;\n    return $c;\n}\n";
        let visitor = parse_and_visit(source);
        let c = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(c.logical_operators >= 1);
        assert!(c.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_complexity_word_and_operator_gap() {
        // tree-sitter-perl parses `$c = $a and $b` as a unary_expression wrapping
        // the assignment, so no binary_expression contains " and " and complexity
        // stays at baseline - a grammar-shape gap pinned as a regression test.
        let source = b"sub combine {\n    $c = $a and $b;\n    return $c;\n}\n";
        let visitor = parse_and_visit(source);
        let c = visitor.functions[0].complexity.as_ref().unwrap();
        assert_eq!(c.cyclomatic_complexity, 1);
    }

    #[test]
    fn test_complexity_word_and_operator_via_mixed_expression() {
        // In `my $c = $a or $b and $d` the outer binary_expression spans the whole
        // statement, so its text hits the " and " check (evaluated before " or ").
        let source = b"sub mixed {\n    my $c = $a or $b and $d;\n    return $c;\n}\n";
        let visitor = parse_and_visit(source);
        let c = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(c.logical_operators >= 1);
        assert!(c.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_complexity_c_style_for_gap() {
        // tree-sitter-perl emits `for_statement_1` for the C-style
        // `for (init; cond; incr)` form, which the complexity visitor does not
        // match (it only handles for_statement/foreach_statement), so complexity
        // stays at baseline 1.
        let source = b"sub loopy {\n    for (my $i = 0; $i < 10; $i++) {\n        print $i;\n    }\n    return 0;\n}\n";
        let visitor = parse_and_visit(source);
        let c = visitor.functions[0].complexity.as_ref().unwrap();
        assert_eq!(c.cyclomatic_complexity, 1);
    }
}
