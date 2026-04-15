// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting Haskell entities

use codegraph_parser_api::{
    truncate_body_prefix, CallRelation, ClassEntity, ComplexityBuilder, ComplexityMetrics,
    FunctionEntity, ImportRelation, Parameter,
};
use tree_sitter::Node;

pub(crate) struct HaskellVisitor<'a> {
    pub source: &'a [u8],
    pub functions: Vec<FunctionEntity>,
    pub classes: Vec<ClassEntity>,
    pub imports: Vec<ImportRelation>,
    pub calls: Vec<CallRelation>,
    /// Names of functions we've already seen a `signature` for (used to pick up return type)
    seen_signatures: std::collections::HashMap<String, String>,
    current_function: Option<String>,
}

impl<'a> HaskellVisitor<'a> {
    pub fn new(source: &'a [u8]) -> Self {
        Self {
            source,
            functions: Vec::new(),
            classes: Vec::new(),
            imports: Vec::new(),
            calls: Vec::new(),
            seen_signatures: std::collections::HashMap::new(),
            current_function: None,
        }
    }

    fn node_text(&self, node: Node) -> String {
        node.utf8_text(self.source).unwrap_or("").to_string()
    }

    pub fn visit_node(&mut self, node: Node) {
        match node.kind() {
            // Top-level imports block
            "imports" => {
                self.visit_imports(node);
                return;
            }
            // Top-level declarations block
            "declarations" => {
                self.visit_declarations(node);
                return;
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child);
        }
    }

    // -----------------------------------------------------------------------
    // Imports
    // -----------------------------------------------------------------------

    fn visit_imports(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "import" {
                self.visit_import(child);
            }
        }
    }

    fn visit_import(&mut self, node: Node) {
        // module field holds the qualified module name
        let module_name = node
            .child_by_field_name("module")
            .map(|n| self.node_text(n))
            .unwrap_or_default();

        if module_name.is_empty() {
            return;
        }

        // alias: `import qualified Data.Map as Map`
        let alias = node.child_by_field_name("alias").map(|n| self.node_text(n));

        // names: `import Data.Text (Text, pack)`
        let mut symbols: Vec<String> = Vec::new();
        if let Some(names_node) = node.child_by_field_name("names") {
            let mut c = names_node.walk();
            for name_node in names_node.children(&mut c) {
                if name_node.kind() == "import_name" {
                    let text = self.node_text(name_node);
                    if !text.is_empty() {
                        symbols.push(text);
                    }
                }
            }
        }

        let is_wildcard = symbols.is_empty() && alias.is_none();

        self.imports.push(ImportRelation {
            importer: "main".to_string(),
            imported: module_name,
            symbols,
            is_wildcard,
            alias,
        });
    }

    // -----------------------------------------------------------------------
    // Declarations
    // -----------------------------------------------------------------------

    fn visit_declarations(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "signature" => {
                    self.visit_signature(child);
                }
                "function" => {
                    self.visit_function(child);
                }
                "data_type" | "newtype" => {
                    self.visit_data_type(child);
                }
                "class" => {
                    self.visit_class(child);
                }
                "instance" => {
                    self.visit_instance(child);
                }
                _ => {}
            }
        }
    }

    /// Collect type signatures so we can attach them to function entities.
    fn visit_signature(&mut self, node: Node) {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_default();
        if name.is_empty() {
            return;
        }
        // The full type annotation text (everything after `::`)
        let sig_text = self.node_text(node);
        self.seen_signatures.insert(name, sig_text);
    }

    fn visit_function(&mut self, node: Node) {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_default();

        if name.is_empty() {
            return;
        }

        // Use the type signature as the canonical signature if available, else reconstruct.
        let signature = self.seen_signatures.get(&name).cloned().unwrap_or_else(|| {
            // Fallback: first line of the function definition
            self.node_text(node)
                .lines()
                .next()
                .unwrap_or("")
                .to_string()
        });

        // Parameters — the `patterns` field holds argument patterns
        let parameters = self.extract_function_parameters(node);

        // Return type — extracted from signature after last `->`
        let return_type = self.seen_signatures.get(&name).and_then(|sig| {
            // The sig looks like `name :: A -> B -> C`, last segment after last `->`
            sig.rsplit("->").next().map(|s| s.trim().to_string())
        });

        let doc_comment = self.extract_doc_comment(node);

        // body_prefix: the match expression
        let body_prefix = node
            .child_by_field_name("match")
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| truncate_body_prefix(t).to_string());

        let complexity = node
            .child_by_field_name("match")
            .map(|body| self.calculate_complexity(body));

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
            complexity,
            body_prefix,
        };

        self.functions.push(func);

        // Walk the match body for call relations
        let previous_function = self.current_function.take();
        self.current_function = Some(name);

        if let Some(match_node) = node.child_by_field_name("match") {
            self.visit_body_for_calls(match_node);
        }

        self.current_function = previous_function;
    }

    /// `data` or `newtype` declarations → ClassEntity (kind "data")
    fn visit_data_type(&mut self, node: Node) {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_default();

        if name.is_empty() {
            return;
        }

        let doc_comment = self.extract_doc_comment(node);

        let cls = ClassEntity {
            name,
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            doc_comment,
            attributes: Vec::new(),
            base_classes: Vec::new(),
            implemented_traits: Vec::new(),
            methods: Vec::new(),
            fields: Vec::new(),
            type_parameters: Vec::new(),
            is_abstract: false,
            is_interface: false,
            body_prefix: None,
        };

        self.classes.push(cls);
    }

    /// `class` declarations → ClassEntity (trait-like, is_interface = true)
    fn visit_class(&mut self, node: Node) {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_default();

        if name.is_empty() {
            return;
        }

        let doc_comment = self.extract_doc_comment(node);

        // Extract method signatures declared in the class body
        let mut methods: Vec<String> = Vec::new();
        if let Some(decls) = node.child_by_field_name("declarations") {
            let mut cursor = decls.walk();
            for child in decls.children(&mut cursor) {
                if child.kind() == "signature" {
                    if let Some(mn) = child.child_by_field_name("name") {
                        let method_name = self.node_text(mn);
                        if !method_name.is_empty() {
                            methods.push(method_name);
                        }
                    }
                }
            }
        }

        let cls = ClassEntity {
            name,
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            doc_comment,
            attributes: Vec::new(),
            base_classes: Vec::new(),
            implemented_traits: Vec::new(),
            methods: Vec::new(),
            fields: Vec::new(),
            type_parameters: Vec::new(),
            is_abstract: false,
            is_interface: true,
            body_prefix: None,
        };

        self.classes.push(cls);

        // Also register the method signatures as functions (abstract)
        for method in methods {
            if let Some(sig) = self.seen_signatures.get(&method).cloned() {
                let func = FunctionEntity {
                    name: method.clone(),
                    signature: sig,
                    visibility: "public".to_string(),
                    line_start: node.start_position().row + 1,
                    line_end: node.end_position().row + 1,
                    is_async: false,
                    is_test: false,
                    is_static: false,
                    is_abstract: true,
                    parameters: Vec::new(),
                    return_type: None,
                    doc_comment: None,
                    attributes: Vec::new(),
                    parent_class: Some(
                        node.child_by_field_name("name")
                            .map(|n| self.node_text(n))
                            .unwrap_or_default(),
                    ),
                    complexity: None,
                    body_prefix: None,
                };
                self.functions.push(func);
            }
        }
    }

    /// `instance` declarations → emit a FunctionEntity per implemented method
    fn visit_instance(&mut self, node: Node) {
        let type_class_name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_default();

        // The type being instantiated is in `patterns`
        let instance_type = node
            .child_by_field_name("patterns")
            .map(|n| self.node_text(n))
            .unwrap_or_default();

        if let Some(decls) = node.child_by_field_name("declarations") {
            let mut cursor = decls.walk();
            for child in decls.children(&mut cursor) {
                if child.kind() == "function" {
                    let method_name = child
                        .child_by_field_name("name")
                        .map(|n| self.node_text(n))
                        .unwrap_or_default();

                    if method_name.is_empty() {
                        continue;
                    }

                    let qualified_name = format!(
                        "{}.{}_{}",
                        type_class_name,
                        instance_type.trim(),
                        method_name
                    );

                    let signature = format!(
                        "instance {} {} -- {}",
                        type_class_name,
                        instance_type.trim(),
                        method_name
                    );

                    let parameters = self.extract_function_parameters(child);

                    let body_prefix = child
                        .child_by_field_name("match")
                        .and_then(|b| b.utf8_text(self.source).ok())
                        .filter(|t| !t.is_empty())
                        .map(|t| truncate_body_prefix(t).to_string());

                    let complexity = child
                        .child_by_field_name("match")
                        .map(|body| self.calculate_complexity(body));

                    let func = FunctionEntity {
                        name: qualified_name.clone(),
                        signature,
                        visibility: "public".to_string(),
                        line_start: child.start_position().row + 1,
                        line_end: child.end_position().row + 1,
                        is_async: false,
                        is_test: false,
                        is_static: false,
                        is_abstract: false,
                        parameters,
                        return_type: None,
                        doc_comment: None,
                        attributes: Vec::new(),
                        parent_class: Some(type_class_name.clone()),
                        complexity,
                        body_prefix,
                    };

                    self.functions.push(func);

                    // Track calls inside instance methods
                    let previous_function = self.current_function.take();
                    self.current_function = Some(qualified_name);

                    if let Some(match_node) = child.child_by_field_name("match") {
                        self.visit_body_for_calls(match_node);
                    }

                    self.current_function = previous_function;
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn extract_function_parameters(&self, node: Node) -> Vec<Parameter> {
        let mut params = Vec::new();
        if let Some(patterns_node) = node.child_by_field_name("patterns") {
            let mut cursor = patterns_node.walk();
            for child in patterns_node.children(&mut cursor) {
                let kind = child.kind();
                // variable patterns are the simple argument names
                if kind == "variable" || kind == "as_pattern" {
                    let text = self.node_text(child);
                    if !text.is_empty() {
                        params.push(Parameter::new(text));
                    }
                }
            }
        }
        params
    }

    fn extract_doc_comment(&self, node: Node) -> Option<String> {
        if let Some(prev) = node.prev_sibling() {
            if prev.kind() == "comment" {
                let text = self.node_text(prev);
                // Haskell doc comments start with `--` or `{-|`
                if text.starts_with("--") || text.starts_with("{-|") {
                    return Some(text);
                }
            }
        }
        None
    }

    fn visit_body_for_calls(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            // `apply` nodes represent function application
            if child.kind() == "apply" {
                if let Some(func_node) = child.child_by_field_name("function") {
                    let callee = self.node_text(func_node);
                    if !callee.is_empty() {
                        if let Some(ref caller) = self.current_function.clone() {
                            self.calls.push(CallRelation {
                                caller: caller.clone(),
                                callee,
                                call_site_line: child.start_position().row + 1,
                                is_direct: true,
                                struct_type: None,
                                field_name: None,
                            });
                        }
                    }
                }
            }
            self.visit_body_for_calls(child);
        }
    }

    fn calculate_complexity(&self, body: Node) -> ComplexityMetrics {
        let mut builder = ComplexityBuilder::new();
        self.visit_for_complexity(body, &mut builder);
        builder.build()
    }

    fn visit_for_complexity(&self, node: Node, builder: &mut ComplexityBuilder) {
        match node.kind() {
            // case expression — each alternative is a branch
            "case" => {
                builder.add_branch();
                builder.enter_scope();
            }
            "alternative" => {
                builder.add_branch();
            }
            // guards in function definitions
            "guard" | "guard_equation" => {
                builder.add_branch();
            }
            // let/where introduce nested scopes
            "let" | "where" => {
                builder.enter_scope();
            }
            // Logical operators
            "infix" => {
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
            "case" | "let" | "where" => {
                builder.exit_scope();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_visit(source: &[u8]) -> HaskellVisitor<'_> {
        use tree_sitter::Parser;

        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_haskell::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = HaskellVisitor::new(source);
        visitor.visit_node(tree.root_node());
        visitor
    }

    #[test]
    fn test_visitor_function_extraction() {
        let source =
            b"module M where\ngreet :: String -> String\ngreet name = \"Hello, \" ++ name\n";
        let visitor = parse_and_visit(source);
        assert!(!visitor.functions.is_empty());
        let greet = visitor.functions.iter().find(|f| f.name == "greet");
        assert!(greet.is_some());
    }

    #[test]
    fn test_visitor_import_extraction() {
        let source = b"module M where\nimport Data.Text (Text)\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "Data.Text");
    }

    #[test]
    fn test_visitor_data_type() {
        let source = b"module M where\ndata Color = Red | Green | Blue\n";
        let visitor = parse_and_visit(source);
        assert!(!visitor.classes.is_empty());
        let color = visitor.classes.iter().find(|c| c.name == "Color");
        assert!(color.is_some());
    }

    #[test]
    fn test_visitor_class_extraction() {
        let source = b"module M where\nclass Eq a where\n  eq :: a -> a -> Bool\n";
        let visitor = parse_and_visit(source);
        let cls = visitor.classes.iter().find(|c| c.name == "Eq");
        assert!(cls.is_some());
        assert!(cls.unwrap().is_interface);
    }
}
