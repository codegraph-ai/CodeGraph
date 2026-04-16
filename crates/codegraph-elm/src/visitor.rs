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
}
