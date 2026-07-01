// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting Elm entities.
//!
//! Node types verified against tree-sitter-elm 5.9 grammar by AST dump.
//!
//! Top-level `file` children:
//!   - `module_declaration`  — module Main exposing (..)
//!   - `import_clause`       — import Html exposing (div, text)
//!   - `type_annotation`     — foo : Type -> Type
//!   - `value_declaration`   — foo arg = body
//!   - `type_declaration`    — type Msg = Increment | Decrement
//!   - `type_alias_declaration` — type alias Model = { .. }
//!   - `port_annotation`     — port sendMessage : String -> Cmd msg

use codegraph_parser_api::{
    truncate_body_prefix, ClassEntity, ComplexityBuilder, ComplexityMetrics, FunctionEntity,
    ImportRelation, Parameter,
};
use tree_sitter::Node;

pub(crate) struct ElmVisitor<'a> {
    pub source: &'a [u8],
    pub functions: Vec<FunctionEntity>,
    /// Type declarations (type + type alias) stored as ClassEntity
    pub classes: Vec<ClassEntity>,
    pub imports: Vec<ImportRelation>,
    /// Maps function name -> type signature text (from `type_annotation` nodes)
    seen_annotations: std::collections::HashMap<String, String>,
}

impl<'a> ElmVisitor<'a> {
    pub fn new(source: &'a [u8]) -> Self {
        Self {
            source,
            functions: Vec::new(),
            classes: Vec::new(),
            imports: Vec::new(),
            seen_annotations: std::collections::HashMap::new(),
        }
    }

    fn node_text(&self, node: Node) -> String {
        node.utf8_text(self.source).unwrap_or("").to_string()
    }

    pub fn visit_node(&mut self, node: Node) {
        // Walk the top-level `file` node's children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "module_declaration" => {
                    // Nothing to extract for the graph; module name is used in extractor
                }
                "import_clause" => {
                    self.visit_import_clause(child);
                }
                "type_annotation" => {
                    self.visit_type_annotation(child);
                }
                "value_declaration" => {
                    self.visit_value_declaration(child);
                }
                "type_declaration" => {
                    self.visit_type_declaration(child);
                }
                "type_alias_declaration" => {
                    self.visit_type_alias_declaration(child);
                }
                "port_annotation" => {
                    self.visit_port_annotation(child);
                }
                _ => {}
            }
        }
    }

    // -----------------------------------------------------------------------
    // Module name extraction helper (used by extractor)
    // -----------------------------------------------------------------------

    /// Extract the module name from the `module_declaration` child.
    pub fn extract_module_name(root: Node, source: &[u8]) -> Option<String> {
        let mut cursor = root.walk();
        for child in root.children(&mut cursor) {
            if child.kind() == "module_declaration" {
                // upper_case_qid holds the qualified module name
                if let Some(qid) = child.child_by_field_name("name") {
                    return Some(qid.utf8_text(source).ok()?.to_string());
                }
                // Fallback: find first upper_case_qid named child
                let mut c2 = child.walk();
                for gc in child.named_children(&mut c2) {
                    if gc.kind() == "upper_case_qid" {
                        return Some(gc.utf8_text(source).ok()?.to_string());
                    }
                }
            }
        }
        None
    }

    // -----------------------------------------------------------------------
    // Imports
    // -----------------------------------------------------------------------

    fn visit_import_clause(&mut self, node: Node) {
        // upper_case_qid is the module name (e.g. "Html", "Html.Attributes")
        let module_name = {
            let mut c = node.walk();
            let mut name = String::new();
            for child in node.named_children(&mut c) {
                if child.kind() == "upper_case_qid" {
                    name = self.node_text(child);
                    break;
                }
            }
            name
        };

        if module_name.is_empty() {
            return;
        }

        // as clause: `import Foo as F`
        let alias = {
            let mut c = node.walk();
            let mut alias = None;
            for child in node.named_children(&mut c) {
                if child.kind() == "as_clause" {
                    // as_clause contains upper_case_identifier
                    let mut c2 = child.walk();
                    for gc in child.named_children(&mut c2) {
                        if gc.kind() == "upper_case_identifier" {
                            alias = Some(self.node_text(gc));
                            break;
                        }
                    }
                }
            }
            alias
        };

        // Exposed symbols from exposing_list
        let mut symbols: Vec<String> = Vec::new();
        let mut is_wildcard = false;
        {
            let mut c = node.walk();
            for child in node.named_children(&mut c) {
                if child.kind() == "exposing_list" {
                    let mut c2 = child.walk();
                    for item in child.named_children(&mut c2) {
                        match item.kind() {
                            "double_dot" => {
                                is_wildcard = true;
                            }
                            "exposed_value" | "exposed_type" | "exposed_operator" => {
                                let text = self.node_text(item);
                                if !text.is_empty() {
                                    symbols.push(text);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        self.imports.push(ImportRelation {
            importer: "main".to_string(),
            imported: module_name,
            symbols,
            is_wildcard,
            alias,
        });
    }

    // -----------------------------------------------------------------------
    // Type annotations (collect for use when building value_declaration)
    // -----------------------------------------------------------------------

    fn visit_type_annotation(&mut self, node: Node) {
        // lower_case_identifier is the function name
        let name = {
            let mut c = node.walk();
            let mut n = String::new();
            for child in node.named_children(&mut c) {
                if child.kind() == "lower_case_identifier" {
                    n = self.node_text(child);
                    break;
                }
            }
            n
        };
        if name.is_empty() {
            return;
        }
        let sig_text = self.node_text(node);
        self.seen_annotations.insert(name, sig_text);
    }

    // -----------------------------------------------------------------------
    // Value declarations (functions / constants)
    // -----------------------------------------------------------------------

    fn visit_value_declaration(&mut self, node: Node) {
        // function_declaration_left holds the name + parameters
        let decl_left = {
            let mut c = node.walk();
            let mut found = None;
            for child in node.named_children(&mut c) {
                if child.kind() == "function_declaration_left" {
                    found = Some(child);
                    break;
                }
            }
            found
        };

        let decl_left = match decl_left {
            Some(n) => n,
            None => return,
        };

        // Name: first lower_case_identifier inside function_declaration_left
        let name = {
            let mut c = decl_left.walk();
            let mut n = String::new();
            for child in decl_left.named_children(&mut c) {
                if child.kind() == "lower_case_identifier" {
                    n = self.node_text(child);
                    break;
                }
            }
            n
        };

        if name.is_empty() {
            return;
        }

        // Parameters: lower_pattern children of function_declaration_left
        let parameters = self.extract_parameters(decl_left);

        // Signature from collected type annotation, or first line of decl
        let signature = self
            .seen_annotations
            .get(&name)
            .cloned()
            .unwrap_or_else(|| {
                self.node_text(node)
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_string()
            });

        // Return type: last segment after `->` from annotation
        let return_type = self
            .seen_annotations
            .get(&name)
            .and_then(|sig| sig.split("->").last().map(|s| s.trim().to_string()));

        let doc_comment = self.extract_doc_comment(node);

        // Body prefix: everything after `=` (the expression child)
        let body_prefix = self.extract_body(node);

        let complexity = self.calculate_value_complexity(node);

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
            parameters,
            return_type,
            doc_comment,
            attributes: Vec::new(),
            parent_class: None,
            complexity: Some(complexity),
            body_prefix,
        };

        self.functions.push(func);
    }

    fn extract_parameters(&self, decl_left: Node) -> Vec<Parameter> {
        let mut params = Vec::new();
        let mut cursor = decl_left.walk();
        for child in decl_left.named_children(&mut cursor) {
            // lower_pattern children represent function parameters
            if child.kind() == "lower_pattern" {
                let mut c2 = child.walk();
                for gc in child.named_children(&mut c2) {
                    if gc.kind() == "lower_case_identifier" {
                        params.push(Parameter::new(self.node_text(gc)));
                        break;
                    }
                }
            }
        }
        params
    }

    fn extract_body(&self, value_decl: Node) -> Option<String> {
        // The body expression is the last named child that isn't function_declaration_left or eq
        let mut cursor = value_decl.walk();
        for child in value_decl.named_children(&mut cursor) {
            if child.kind() != "function_declaration_left" && child.kind() != "eq" {
                let text = child.utf8_text(self.source).ok()?;
                if !text.is_empty() {
                    return Some(truncate_body_prefix(text).to_string());
                }
            }
        }
        None
    }

    fn extract_doc_comment(&self, node: Node) -> Option<String> {
        // {- doc comments -} appear as prev_named_sibling of kind "block_comment"
        // or single-line `--` comments as "line_comment"
        if let Some(prev) = node.prev_named_sibling() {
            if prev.kind() == "block_comment" || prev.kind() == "line_comment" {
                let text = self.node_text(prev);
                if text.starts_with("{-|") || text.starts_with("--") {
                    return Some(text);
                }
            }
        }
        None
    }

    fn calculate_value_complexity(&self, node: Node) -> ComplexityMetrics {
        let mut builder = ComplexityBuilder::new();
        self.visit_for_complexity(node, &mut builder);
        builder.build()
    }

    fn visit_for_complexity(&self, node: Node, builder: &mut ComplexityBuilder) {
        match node.kind() {
            "case_of_expr" => {
                builder.add_branch();
                builder.enter_scope();
            }
            "case_of_branch" => {
                builder.add_branch();
            }
            "if_else_expr" => {
                builder.add_branch();
                builder.enter_scope();
            }
            "let_in_expr" => {
                builder.enter_scope();
            }
            "bin_op_expr" => {
                // Check for && / || operators
                let text = self.node_text(node);
                if text.contains("&&") || text.contains("||") {
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
            "case_of_expr" | "if_else_expr" | "let_in_expr" => {
                builder.exit_scope();
            }
            _ => {}
        }
    }

    // -----------------------------------------------------------------------
    // Type declarations  (type Msg = Increment | Decrement)
    // -----------------------------------------------------------------------

    fn visit_type_declaration(&mut self, node: Node) {
        let name = {
            let mut c = node.walk();
            let mut n = String::new();
            for child in node.named_children(&mut c) {
                if child.kind() == "upper_case_identifier" {
                    n = self.node_text(child);
                    break;
                }
            }
            n
        };

        if name.is_empty() {
            return;
        }

        let doc_comment = self.extract_doc_comment(node);

        self.classes.push(ClassEntity {
            name,
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_abstract: false,
            is_interface: false,
            base_classes: Vec::new(),
            implemented_traits: Vec::new(),
            methods: Vec::new(),
            fields: Vec::new(),
            doc_comment,
            attributes: Vec::new(),
            type_parameters: Vec::new(),
            body_prefix: None,
        });
    }

    // -----------------------------------------------------------------------
    // Type alias declarations  (type alias Model = { count : Int })
    // -----------------------------------------------------------------------

    fn visit_type_alias_declaration(&mut self, node: Node) {
        let name = {
            let mut c = node.walk();
            let mut n = String::new();
            for child in node.named_children(&mut c) {
                if child.kind() == "upper_case_identifier" {
                    n = self.node_text(child);
                    break;
                }
            }
            n
        };

        if name.is_empty() {
            return;
        }

        let doc_comment = self.extract_doc_comment(node);

        self.classes.push(ClassEntity {
            name,
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_abstract: false,
            is_interface: false,
            base_classes: Vec::new(),
            implemented_traits: Vec::new(),
            methods: Vec::new(),
            fields: Vec::new(),
            doc_comment,
            attributes: Vec::new(),
            type_parameters: Vec::new(),
            body_prefix: None,
        });
    }

    // -----------------------------------------------------------------------
    // Port declarations  (port sendMessage : String -> Cmd msg)
    // -----------------------------------------------------------------------

    fn visit_port_annotation(&mut self, node: Node) {
        let name = {
            let mut c = node.walk();
            let mut n = String::new();
            for child in node.named_children(&mut c) {
                if child.kind() == "lower_case_identifier" {
                    n = self.node_text(child);
                    break;
                }
            }
            n
        };

        if name.is_empty() {
            return;
        }

        let signature = self.node_text(node);
        let return_type = signature.split("->").last().map(|s| s.trim().to_string());

        let doc_comment = self.extract_doc_comment(node);

        self.functions.push(FunctionEntity {
            name: name.clone(),
            signature,
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_async: false,
            is_test: false,
            is_static: false,
            is_abstract: false,
            parameters: Vec::new(),
            return_type,
            doc_comment,
            attributes: vec!["port".to_string()],
            parent_class: None,
            complexity: None,
            body_prefix: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_visit(source: &[u8]) -> ElmVisitor<'_> {
        use tree_sitter::Parser;

        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_elm::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = ElmVisitor::new(source);
        visitor.visit_node(tree.root_node());
        visitor
    }

    #[test]
    fn test_visitor_function_extraction() {
        let source = b"module Main exposing (main)\n\nmain : String\nmain =\n    \"hello\"\n";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "main");
    }

    #[test]
    fn test_visitor_import_extraction() {
        let source = b"module Main exposing (main)\n\nimport Html exposing (Html, div)\nimport Browser\n\nmain = div [] []\n";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 2);
        assert!(visitor.imports.iter().any(|i| i.imported == "Html"));
        assert!(visitor.imports.iter().any(|i| i.imported == "Browser"));
    }

    #[test]
    fn test_visitor_type_declaration() {
        let source = b"module Main exposing (..)\n\ntype Msg\n    = Increment\n    | Decrement\n\nmain = 1\n";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "Msg");
    }

    #[test]
    fn test_visitor_type_alias() {
        let source = b"module Main exposing (..)\n\ntype alias Model =\n    { count : Int\n    }\n\nmain = 1\n";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "Model");
    }

    #[test]
    fn test_visitor_port_extraction() {
        let source =
            b"port module Main exposing (..)\n\nport sendMessage : String -> Cmd msg\n\nmain = 1\n";
        let visitor = parse_and_visit(source);

        assert!(
            visitor.functions.iter().any(|f| f.name == "sendMessage"),
            "Expected sendMessage port function, found: {:?}",
            visitor
                .functions
                .iter()
                .map(|f| &f.name)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_visitor_parameters() {
        let source = b"module Main exposing (..)\n\nupdate : Msg -> Model -> Model\nupdate msg model =\n    model\n";
        let visitor = parse_and_visit(source);

        let update = visitor.functions.iter().find(|f| f.name == "update");
        assert!(update.is_some(), "update function not found");
        let update = update.unwrap();
        assert_eq!(update.parameters.len(), 2);
        assert_eq!(update.parameters[0].name, "msg");
        assert_eq!(update.parameters[1].name, "model");
    }

    #[test]
    fn test_empty_source_is_empty() {
        let visitor = parse_and_visit(b"module Main exposing (..)\n");
        assert!(visitor.functions.is_empty());
        assert!(visitor.classes.is_empty());
        assert!(visitor.imports.is_empty());
    }

    #[test]
    fn test_import_alias() {
        let source = b"module Main exposing (..)\n\nimport Html.Attributes as Attr\n\nmain = 1\n";
        let visitor = parse_and_visit(source);

        let imp = visitor
            .imports
            .iter()
            .find(|i| i.imported == "Html.Attributes")
            .expect("import not found");
        assert_eq!(imp.alias.as_deref(), Some("Attr"));
        assert!(!imp.is_wildcard);
    }

    #[test]
    fn test_import_exposed_symbols() {
        let source =
            b"module Main exposing (..)\n\nimport Html exposing (Html, div, text)\n\nmain = 1\n";
        let visitor = parse_and_visit(source);

        let imp = visitor
            .imports
            .iter()
            .find(|i| i.imported == "Html")
            .expect("import not found");
        assert!(imp.symbols.iter().any(|s| s == "div"));
        assert!(imp.symbols.iter().any(|s| s == "text"));
        assert!(!imp.is_wildcard);
        assert_eq!(imp.alias, None);
    }

    #[test]
    fn test_import_wildcard() {
        let source = b"module Main exposing (..)\n\nimport Html exposing (..)\n\nmain = 1\n";
        let visitor = parse_and_visit(source);

        let imp = visitor
            .imports
            .iter()
            .find(|i| i.imported == "Html")
            .expect("import not found");
        assert!(imp.is_wildcard);
        assert!(imp.symbols.is_empty());
    }

    #[test]
    fn test_function_signature_and_return_type_from_annotation() {
        let source =
            b"module Main exposing (..)\n\ngreet : String -> String\ngreet name =\n    name\n";
        let visitor = parse_and_visit(source);

        let greet = visitor
            .functions
            .iter()
            .find(|f| f.name == "greet")
            .expect("greet not found");
        // Signature comes from the collected type_annotation, not the decl line.
        assert!(greet.signature.contains("greet : String -> String"));
        // Return type is the last `->` segment, trimmed.
        assert_eq!(greet.return_type.as_deref(), Some("String"));
        assert_eq!(greet.visibility, "public");
    }

    #[test]
    fn test_function_signature_fallback_without_annotation() {
        // No type_annotation, so signature falls back to the first decl line
        // and return_type stays None.
        let source = b"module Main exposing (..)\n\nanswer =\n    42\n";
        let visitor = parse_and_visit(source);

        let answer = visitor
            .functions
            .iter()
            .find(|f| f.name == "answer")
            .expect("answer not found");
        assert_eq!(answer.signature, "answer =");
        assert_eq!(answer.return_type, None);
    }

    #[test]
    fn test_function_body_prefix() {
        let source = b"module Main exposing (..)\n\ngreeting =\n    \"hello world\"\n";
        let visitor = parse_and_visit(source);

        let greeting = visitor
            .functions
            .iter()
            .find(|f| f.name == "greeting")
            .expect("greeting not found");
        let body = greeting.body_prefix.as_deref().unwrap_or("");
        assert!(body.contains("hello world"), "body_prefix was: {body:?}");
    }

    #[test]
    fn test_case_expression_raises_complexity() {
        let source = b"module Main exposing (..)\n\nclassify n =\n    case n of\n        0 -> \"zero\"\n        _ -> \"other\"\n";
        let visitor = parse_and_visit(source);

        let classify = visitor
            .functions
            .iter()
            .find(|f| f.name == "classify")
            .expect("classify not found");
        let cx = classify.complexity.as_ref().expect("complexity missing");
        assert!(
            cx.cyclomatic_complexity > 1,
            "expected case-of to raise complexity, got {}",
            cx.cyclomatic_complexity
        );
    }

    #[test]
    fn test_port_attributes_and_return_type() {
        let source =
            b"port module Main exposing (..)\n\nport sendMessage : String -> Cmd msg\n\nmain = 1\n";
        let visitor = parse_and_visit(source);

        let port = visitor
            .functions
            .iter()
            .find(|f| f.name == "sendMessage")
            .expect("sendMessage not found");
        assert!(port.attributes.iter().any(|a| a == "port"));
        assert_eq!(port.return_type.as_deref(), Some("Cmd msg"));
        assert!(port.complexity.is_none());
        assert!(port.parameters.is_empty());
    }

    #[test]
    fn test_extract_module_name() {
        use tree_sitter::Parser;
        let source = b"module Main.App exposing (..)\n\nmain = 1\n";
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_elm::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let name = ElmVisitor::extract_module_name(tree.root_node(), source);
        assert_eq!(name.as_deref(), Some("Main.App"));
    }

    #[test]
    fn test_type_alias_and_type_declaration_both_classes() {
        let source = b"module Main exposing (..)\n\ntype Msg\n    = Inc\n    | Dec\n\ntype alias Model =\n    { count : Int }\n\nmain = 1\n";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 2);
        assert!(visitor.classes.iter().any(|c| c.name == "Msg"));
        assert!(visitor.classes.iter().any(|c| c.name == "Model"));
        // Elm type declarations are plain data types, never abstract/interface.
        assert!(visitor.classes.iter().all(|c| !c.is_abstract));
        assert!(visitor.classes.iter().all(|c| !c.is_interface));
    }

    #[test]
    fn test_import_without_exposing() {
        // A plain `import Browser` has no exposing list, no alias, no symbols.
        let source = b"module Main exposing (..)\n\nimport Browser\n\nmain = 1\n";
        let visitor = parse_and_visit(source);

        let imp = visitor
            .imports
            .iter()
            .find(|i| i.imported == "Browser")
            .expect("import not found");
        assert!(imp.symbols.is_empty());
        assert!(!imp.is_wildcard);
        assert_eq!(imp.alias, None);
    }

    #[test]
    fn test_import_alias_and_exposing_combined() {
        // `import Foo as F exposing (bar)` carries both an alias and symbols.
        let source =
            b"module Main exposing (..)\n\nimport Html.Attributes as Attr exposing (class, id)\n\nmain = 1\n";
        let visitor = parse_and_visit(source);

        let imp = visitor
            .imports
            .iter()
            .find(|i| i.imported == "Html.Attributes")
            .expect("import not found");
        assert_eq!(imp.alias.as_deref(), Some("Attr"));
        assert!(imp.symbols.iter().any(|s| s == "class"));
        assert!(imp.symbols.iter().any(|s| s == "id"));
        assert!(!imp.is_wildcard);
    }

    #[test]
    fn test_function_line_numbers_are_one_indexed() {
        // `main` starts on physical line 3 (1-indexed) and spans to line 4.
        let source = b"module Main exposing (..)\n\nmain =\n    1\n";
        let visitor = parse_and_visit(source);

        let main = visitor
            .functions
            .iter()
            .find(|f| f.name == "main")
            .expect("main not found");
        assert_eq!(main.line_start, 3);
        assert_eq!(main.line_end, 4);
    }

    #[test]
    fn test_type_declaration_line_numbers_are_one_indexed() {
        let source = b"module Main exposing (..)\n\ntype Msg\n    = Inc\n    | Dec\n\nmain = 1\n";
        let visitor = parse_and_visit(source);

        let msg = visitor
            .classes
            .iter()
            .find(|c| c.name == "Msg")
            .expect("Msg not found");
        assert_eq!(msg.line_start, 3);
        assert!(msg.line_end >= msg.line_start);
    }

    #[test]
    fn test_function_doc_comment_block() {
        // A `{-| .. -}` block comment immediately preceding a declaration is
        // captured as the doc comment.
        let source =
            b"module Main exposing (..)\n\n{-| Greets the world. -}\ngreeting =\n    \"hi\"\n";
        let visitor = parse_and_visit(source);

        let greeting = visitor
            .functions
            .iter()
            .find(|f| f.name == "greeting")
            .expect("greeting not found");
        let doc = greeting.doc_comment.as_deref().unwrap_or("");
        assert!(doc.contains("Greets the world"), "doc_comment was: {doc:?}");
    }

    #[test]
    fn test_function_without_doc_comment_is_none() {
        let source = b"module Main exposing (..)\n\ngreeting =\n    \"hi\"\n";
        let visitor = parse_and_visit(source);

        let greeting = visitor
            .functions
            .iter()
            .find(|f| f.name == "greeting")
            .expect("greeting not found");
        assert_eq!(greeting.doc_comment, None);
    }

    #[test]
    fn test_if_else_raises_complexity() {
        let source = b"module Main exposing (..)\n\npick n =\n    if n > 0 then\n        \"pos\"\n    else\n        \"neg\"\n";
        let visitor = parse_and_visit(source);

        let pick = visitor
            .functions
            .iter()
            .find(|f| f.name == "pick")
            .expect("pick not found");
        let cx = pick.complexity.as_ref().expect("complexity missing");
        assert!(
            cx.cyclomatic_complexity > 1,
            "expected if-else to raise complexity, got {}",
            cx.cyclomatic_complexity
        );
    }

    #[test]
    fn test_logical_operator_raises_complexity() {
        let source = b"module Main exposing (..)\n\nboth a b =\n    a && b\n";
        let visitor = parse_and_visit(source);

        let both = visitor
            .functions
            .iter()
            .find(|f| f.name == "both")
            .expect("both not found");
        let cx = both.complexity.as_ref().expect("complexity missing");
        assert!(
            cx.cyclomatic_complexity > 1,
            "expected && to raise complexity, got {}",
            cx.cyclomatic_complexity
        );
    }

    #[test]
    fn test_multi_arrow_return_type_is_last_segment() {
        // For `add : Int -> Int -> Int` the return type is the final segment.
        let source = b"module Main exposing (..)\n\nadd : Int -> Int -> Int\nadd a b =\n    a\n";
        let visitor = parse_and_visit(source);

        let add = visitor
            .functions
            .iter()
            .find(|f| f.name == "add")
            .expect("add not found");
        assert_eq!(add.return_type.as_deref(), Some("Int"));
        assert_eq!(add.parameters.len(), 2);
    }

    #[test]
    fn test_value_declaration_default_flags() {
        // A plain value declaration is public, non-async, non-test, non-static.
        let source = b"module Main exposing (..)\n\nanswer =\n    42\n";
        let visitor = parse_and_visit(source);

        let answer = visitor
            .functions
            .iter()
            .find(|f| f.name == "answer")
            .expect("answer not found");
        assert_eq!(answer.visibility, "public");
        assert!(!answer.is_async);
        assert!(!answer.is_test);
        assert!(!answer.is_static);
        assert!(!answer.is_abstract);
        assert!(answer.parent_class.is_none());
        assert!(answer.attributes.is_empty());
    }

    #[test]
    fn test_body_prefix_truncated_to_max() {
        // A body longer than BODY_PREFIX_MAX_CHARS bytes is truncated to that many.
        use codegraph_parser_api::BODY_PREFIX_MAX_CHARS;
        let long = "x".repeat(BODY_PREFIX_MAX_CHARS + 100);
        let source = format!("module Main exposing (..)\n\nbig =\n    \"{long}\"\n");
        let visitor = parse_and_visit(source.as_bytes());

        let big = visitor
            .functions
            .iter()
            .find(|f| f.name == "big")
            .expect("big not found");
        let body = big.body_prefix.as_deref().expect("body_prefix missing");
        assert_eq!(body.len(), BODY_PREFIX_MAX_CHARS);
    }

    #[test]
    fn test_line_comment_doc_comment() {
        // A `--` single-line comment immediately preceding a declaration is
        // captured as the doc comment.
        let source = b"module Main exposing (..)\n\n-- greets politely\ngreeting =\n    \"hi\"\n";
        let visitor = parse_and_visit(source);

        let greeting = visitor
            .functions
            .iter()
            .find(|f| f.name == "greeting")
            .expect("greeting not found");
        let doc = greeting.doc_comment.as_deref().unwrap_or("");
        assert!(doc.starts_with("--"), "doc_comment was: {doc:?}");
        assert!(doc.contains("greets politely"), "doc_comment was: {doc:?}");
    }

    #[test]
    fn test_or_operator_raises_complexity() {
        let source = b"module Main exposing (..)\n\neither a b =\n    a || b\n";
        let visitor = parse_and_visit(source);

        let either = visitor
            .functions
            .iter()
            .find(|f| f.name == "either")
            .expect("either not found");
        let cx = either.complexity.as_ref().expect("complexity missing");
        assert!(
            cx.cyclomatic_complexity > 1,
            "expected || to raise complexity, got {}",
            cx.cyclomatic_complexity
        );
    }

    #[test]
    fn test_three_parameters_order_preserved() {
        let source = b"module Main exposing (..)\n\ncombine a b c =\n    a\n";
        let visitor = parse_and_visit(source);

        let combine = visitor
            .functions
            .iter()
            .find(|f| f.name == "combine")
            .expect("combine not found");
        assert_eq!(combine.parameters.len(), 3);
        assert_eq!(combine.parameters[0].name, "a");
        assert_eq!(combine.parameters[1].name, "b");
        assert_eq!(combine.parameters[2].name, "c");
    }

    #[test]
    fn test_simple_function_baseline_complexity() {
        // A branch-free value has the baseline cyclomatic complexity of 1.
        let source = b"module Main exposing (..)\n\nanswer =\n    42\n";
        let visitor = parse_and_visit(source);

        let answer = visitor
            .functions
            .iter()
            .find(|f| f.name == "answer")
            .expect("answer not found");
        let cx = answer.complexity.as_ref().expect("complexity missing");
        assert_eq!(cx.cyclomatic_complexity, 1);
    }

    #[test]
    fn test_multiple_case_branches_raise_complexity() {
        // Each case_of_branch adds a branch, so three arms exceed a two-arm case.
        let three = b"module Main exposing (..)\n\nc3 n =\n    case n of\n        0 -> \"a\"\n        1 -> \"b\"\n        _ -> \"c\"\n";
        let two = b"module Main exposing (..)\n\nc2 n =\n    case n of\n        0 -> \"a\"\n        _ -> \"b\"\n";
        let vx3 = parse_and_visit(three);
        let vx2 = parse_and_visit(two);

        let cx3 = vx3.functions[0].complexity.as_ref().unwrap();
        let cx2 = vx2.functions[0].complexity.as_ref().unwrap();
        assert!(
            cx3.cyclomatic_complexity > cx2.cyclomatic_complexity,
            "three arms ({}) should exceed two arms ({})",
            cx3.cyclomatic_complexity,
            cx2.cyclomatic_complexity
        );
    }

    #[test]
    fn test_type_annotation_without_declaration_creates_no_function() {
        // A lone `type_annotation` with no matching value_declaration produces
        // no FunctionEntity - annotations are only collected, never emitted.
        let source = b"module Main exposing (..)\n\nghost : Int -> Int\n";
        let visitor = parse_and_visit(source);

        assert!(visitor.functions.iter().all(|f| f.name != "ghost"));
    }

    #[test]
    fn test_single_type_annotation_return_type_is_whole_signature() {
        // With no `->` in the annotation, split("->").last() yields the whole
        // trimmed annotation text as the return type.
        let source = b"module Main exposing (..)\n\nanswer : Int\nanswer =\n    42\n";
        let visitor = parse_and_visit(source);

        let answer = visitor
            .functions
            .iter()
            .find(|f| f.name == "answer")
            .expect("answer not found");
        assert_eq!(answer.return_type.as_deref(), Some("answer : Int"));
    }

    #[test]
    fn test_multiple_functions_source_order() {
        let source =
            b"module Main exposing (..)\n\nfirst =\n    1\n\nsecond =\n    2\n\nthird =\n    3\n";
        let visitor = parse_and_visit(source);

        let names: Vec<&str> = visitor.functions.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["first", "second", "third"]);
    }

    #[test]
    fn test_port_line_numbers_one_indexed() {
        // The port declaration sits on physical line 3 (1-indexed).
        let source =
            b"port module Main exposing (..)\n\nport sendMessage : String -> Cmd msg\n\nmain = 1\n";
        let visitor = parse_and_visit(source);

        let port = visitor
            .functions
            .iter()
            .find(|f| f.name == "sendMessage")
            .expect("sendMessage not found");
        assert_eq!(port.line_start, 3);
        assert!(port.line_end >= port.line_start);
    }

    #[test]
    fn test_let_in_body_still_extracts_function() {
        // A `let .. in` body opens a scope but does not by itself add a branch;
        // the function is still extracted with its body captured.
        let source =
            b"module Main exposing (..)\n\ncompute =\n    let\n        x = 1\n    in\n    x\n";
        let visitor = parse_and_visit(source);

        let compute = visitor
            .functions
            .iter()
            .find(|f| f.name == "compute")
            .expect("compute not found");
        let cx = compute.complexity.as_ref().expect("complexity missing");
        assert_eq!(cx.cyclomatic_complexity, 1);
        let body = compute.body_prefix.as_deref().unwrap_or("");
        assert!(body.contains("let"), "body_prefix was: {body:?}");
    }
}
