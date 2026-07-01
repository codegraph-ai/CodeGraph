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

    #[test]
    fn test_function_signature_and_return_type() {
        // A preceding `signature` declaration is attached to the function and
        // the return type is derived from the segment after the last `->`.
        let source =
            b"module M where\ngreet :: String -> String\ngreet name = \"Hello, \" ++ name\n";
        let visitor = parse_and_visit(source);
        let greet = visitor
            .functions
            .iter()
            .find(|f| f.name == "greet")
            .expect("greet function extracted");
        assert!(greet.signature.contains("String -> String"));
        assert_eq!(greet.return_type.as_deref(), Some("String"));
        assert_eq!(greet.visibility, "public");
        assert!(!greet.is_abstract);
    }

    #[test]
    fn test_function_parameters_extracted() {
        // The `patterns` field holds simple variable argument patterns.
        let source = b"module M where\nadd :: Int -> Int -> Int\nadd a b = a + b\n";
        let visitor = parse_and_visit(source);
        let add = visitor
            .functions
            .iter()
            .find(|f| f.name == "add")
            .expect("add function extracted");
        let names: Vec<&str> = add.parameters.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b"]);
    }

    #[test]
    fn test_import_with_alias() {
        // `import qualified Data.Map as Map` records the alias and is not wildcard.
        let source = b"module M where\nimport qualified Data.Map as Map\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 1);
        let imp = &visitor.imports[0];
        assert_eq!(imp.imported, "Data.Map");
        assert_eq!(imp.alias.as_deref(), Some("Map"));
        assert!(!imp.is_wildcard);
        assert!(imp.symbols.is_empty());
    }

    #[test]
    fn test_import_with_symbols() {
        // `import Data.Text (Text, pack)` collects the named symbols.
        let source = b"module M where\nimport Data.Text (Text, pack)\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 1);
        let imp = &visitor.imports[0];
        assert_eq!(imp.imported, "Data.Text");
        assert!(imp.symbols.contains(&"Text".to_string()));
        assert!(imp.symbols.contains(&"pack".to_string()));
        assert!(!imp.is_wildcard);
        assert!(imp.alias.is_none());
    }

    #[test]
    fn test_import_wildcard() {
        // A bare `import Data.List` with no names and no alias is a wildcard import.
        let source = b"module M where\nimport Data.List\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 1);
        let imp = &visitor.imports[0];
        assert_eq!(imp.imported, "Data.List");
        assert!(imp.is_wildcard);
        assert!(imp.symbols.is_empty());
        assert!(imp.alias.is_none());
    }

    #[test]
    fn test_newtype_maps_to_class() {
        let source = b"module M where\nnewtype Age = Age Int\n";
        let visitor = parse_and_visit(source);
        let age = visitor
            .classes
            .iter()
            .find(|c| c.name == "Age")
            .expect("newtype maps to a class");
        assert!(!age.is_interface);
        assert!(!age.is_abstract);
    }

    #[test]
    fn test_class_method_not_registered_without_toplevel_signature() {
        // A class-body method is only promoted to an abstract FunctionEntity when
        // a matching name exists in seen_signatures (populated by top-level
        // signatures). An in-class-only signature is never inserted there, so the
        // method is not registered as a function - only the class is emitted.
        let source = b"module M where\nclass Shape a where\n  area :: a -> Double\n";
        let visitor = parse_and_visit(source);
        assert!(visitor.classes.iter().any(|c| c.name == "Shape"));
        assert!(
            !visitor.functions.iter().any(|f| f.name == "area"),
            "in-class-only signature is not promoted to a function"
        );
    }

    #[test]
    fn test_class_method_registered_with_toplevel_signature() {
        // With a matching top-level signature seen first, the class method is
        // promoted to an abstract function whose parent_class is the class name.
        let source = concat!(
            "module M where\n",
            "area :: Double\n",
            "class Shape a where\n",
            "  area :: a -> Double\n",
        )
        .as_bytes();
        let visitor = parse_and_visit(source);
        let area = visitor
            .functions
            .iter()
            .find(|f| f.name == "area" && f.is_abstract)
            .expect("class method promoted with matching top-level signature");
        assert_eq!(area.parent_class.as_deref(), Some("Shape"));
    }

    #[test]
    fn test_instance_method_qualified_name() {
        // instance methods get a qualified name and parent_class of the type class.
        let source = concat!(
            "module M where\n",
            "class Greet a where\n",
            "  hello :: a -> String\n",
            "data Person = Person\n",
            "instance Greet Person where\n",
            "  hello p = \"hi\"\n",
        )
        .as_bytes();
        let visitor = parse_and_visit(source);
        let inst = visitor
            .functions
            .iter()
            .find(|f| f.parent_class.as_deref() == Some("Greet") && f.name.contains("hello"));
        assert!(
            inst.is_some(),
            "instance method extracted with parent_class"
        );
        assert!(inst.unwrap().name.starts_with("Greet."));
    }

    #[test]
    fn test_complexity_counts_case_branches() {
        let source = concat!(
            "module M where\n",
            "classify :: Int -> String\n",
            "classify n = case n of\n",
            "  0 -> \"zero\"\n",
            "  _ -> \"other\"\n",
        )
        .as_bytes();
        let visitor = parse_and_visit(source);
        let classify = visitor
            .functions
            .iter()
            .find(|f| f.name == "classify")
            .expect("classify extracted");
        let complexity = classify.complexity.as_ref().expect("complexity computed");
        assert!(
            complexity.cyclomatic_complexity > 1,
            "case with alternatives should raise cyclomatic complexity, got {}",
            complexity.cyclomatic_complexity
        );
    }

    #[test]
    fn test_doc_comment_extraction() {
        let source =
            b"module M where\n-- | Doubles its input\ndouble :: Int -> Int\ndouble x = x * 2\n";
        let visitor = parse_and_visit(source);
        let sig = visitor.seen_signatures.get("double");
        assert!(sig.is_some(), "signature seen for double");
        // The doc comment precedes the signature node, so it attaches to the
        // signature rather than the function definition; assert extraction runs.
        let double = visitor.functions.iter().find(|f| f.name == "double");
        assert!(double.is_some());
    }

    #[test]
    fn test_call_relations_from_apply() {
        let source = concat!(
            "module M where\n",
            "helper :: Int -> Int\n",
            "helper y = y\n",
            "compute :: Int -> Int\n",
            "compute x = helper x\n",
        )
        .as_bytes();
        let visitor = parse_and_visit(source);
        let call = visitor
            .calls
            .iter()
            .find(|c| c.caller == "compute" && c.callee == "helper");
        assert!(call.is_some(), "apply of helper produces a call relation");
        assert!(call.unwrap().is_direct);
    }

    #[test]
    fn test_multiple_imports() {
        let source =
            b"module M where\nimport Data.List\nimport Data.Map (Map)\nimport Data.Set as S\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 3);
        let modules: Vec<&str> = visitor
            .imports
            .iter()
            .map(|i| i.imported.as_str())
            .collect();
        assert!(modules.contains(&"Data.List"));
        assert!(modules.contains(&"Data.Map"));
        assert!(modules.contains(&"Data.Set"));
    }

    #[test]
    fn test_empty_module_yields_nothing() {
        let source = b"module M where\n";
        let visitor = parse_and_visit(source);
        assert!(visitor.functions.is_empty());
        assert!(visitor.classes.is_empty());
        assert!(visitor.imports.is_empty());
        assert!(visitor.calls.is_empty());
    }

    #[test]
    fn test_function_line_numbers_are_one_indexed() {
        // greet's definition sits on line 3 (module=1, signature=2, def=3).
        let source =
            b"module M where\ngreet :: String -> String\ngreet name = \"Hello, \" ++ name\n";
        let visitor = parse_and_visit(source);
        let greet = visitor
            .functions
            .iter()
            .find(|f| f.name == "greet")
            .expect("greet extracted");
        assert_eq!(greet.line_start, 3);
        assert!(greet.line_end >= greet.line_start);
    }

    #[test]
    fn test_function_default_flags() {
        // A plain top-level function is not async/test/static/abstract.
        let source = b"module M where\nfoo :: Int -> Int\nfoo n = n\n";
        let visitor = parse_and_visit(source);
        let foo = visitor
            .functions
            .iter()
            .find(|f| f.name == "foo")
            .expect("foo extracted");
        assert!(!foo.is_async);
        assert!(!foo.is_test);
        assert!(!foo.is_static);
        assert!(!foo.is_abstract);
        assert!(foo.parent_class.is_none());
        assert!(foo.attributes.is_empty());
    }

    #[test]
    fn test_function_without_signature_uses_first_line_fallback() {
        // With no preceding `signature`, the signature falls back to the first
        // line of the definition and there is no return type.
        let source = b"module M where\nidentity x = x\n";
        let visitor = parse_and_visit(source);
        let identity = visitor
            .functions
            .iter()
            .find(|f| f.name == "identity")
            .expect("identity extracted");
        assert!(identity.signature.contains("identity x = x"));
        assert!(identity.return_type.is_none());
    }

    #[test]
    fn test_multi_arrow_return_type_is_last_segment() {
        // The return type is the segment after the LAST `->`.
        let source = b"module M where\ncmp :: Int -> String -> Bool\ncmp a b = True\n";
        let visitor = parse_and_visit(source);
        let cmp = visitor
            .functions
            .iter()
            .find(|f| f.name == "cmp")
            .expect("cmp extracted");
        assert_eq!(cmp.return_type.as_deref(), Some("Bool"));
    }

    #[test]
    fn test_body_prefix_populated() {
        let source = concat!(
            "module M where\n",
            "helper :: Int -> Int\n",
            "helper y = y\n",
            "compute :: Int -> Int\n",
            "compute x = helper x\n",
        )
        .as_bytes();
        let visitor = parse_and_visit(source);
        let compute = visitor
            .functions
            .iter()
            .find(|f| f.name == "compute")
            .expect("compute extracted");
        assert!(compute.body_prefix.is_some());
        assert!(compute.body_prefix.as_ref().unwrap().contains("helper"));
    }

    #[test]
    fn test_logical_operator_raises_complexity() {
        // `&&` inside an infix expression is counted as a logical operator.
        let source = concat!(
            "module M where\n",
            "inRange :: Int -> Bool\n",
            "inRange x = x > 0 && x < 10\n",
        )
        .as_bytes();
        let visitor = parse_and_visit(source);
        let f = visitor
            .functions
            .iter()
            .find(|f| f.name == "inRange")
            .expect("inRange extracted");
        assert!(f.complexity.is_some());
    }

    #[test]
    fn test_where_clause_computes_complexity() {
        // A `where` binding introduces a nested scope; complexity is computed.
        let source = concat!(
            "module M where\n",
            "area :: Int -> Int\n",
            "area r = sq where sq = r * r\n",
        )
        .as_bytes();
        let visitor = parse_and_visit(source);
        let area = visitor
            .functions
            .iter()
            .find(|f| f.name == "area")
            .expect("area extracted");
        assert!(area.complexity.is_some());
    }

    #[test]
    fn test_instance_method_tracks_calls() {
        // A call inside an instance method body is attributed to the qualified
        // instance-method name as caller.
        let source = concat!(
            "module M where\n",
            "shout :: String -> String\n",
            "shout s = s\n",
            "class Greet a where\n",
            "  hello :: a -> String\n",
            "data Person = Person\n",
            "instance Greet Person where\n",
            "  hello p = shout \"hi\"\n",
        )
        .as_bytes();
        let visitor = parse_and_visit(source);
        let call = visitor.calls.iter().find(|c| c.callee == "shout");
        assert!(call.is_some(), "instance method call to shout tracked");
        assert!(call.unwrap().caller.contains("hello"));
    }

    #[test]
    fn test_import_alias_only_is_not_wildcard() {
        // `import Data.Set as S` records the alias S and is not a wildcard.
        let source = b"module M where\nimport Data.Set as S\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 1);
        let imp = &visitor.imports[0];
        assert_eq!(imp.imported, "Data.Set");
        assert_eq!(imp.alias.as_deref(), Some("S"));
        assert!(!imp.is_wildcard);
    }

    #[test]
    fn test_data_type_with_multiple_constructors_is_single_class() {
        // A sum type with several constructors still yields exactly one class.
        let source = b"module M where\ndata Dir = North | South | East | West\n";
        let visitor = parse_and_visit(source);
        let dirs: Vec<_> = visitor.classes.iter().filter(|c| c.name == "Dir").collect();
        assert_eq!(dirs.len(), 1);
        assert!(!dirs[0].is_interface);
    }

    #[test]
    fn test_import_importer_defaults_to_main() {
        // Every ImportRelation is attributed to the synthetic "main" importer.
        let source = b"module M where\nimport Data.List\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].importer, "main");
    }

    #[test]
    fn test_or_operator_raises_complexity() {
        // `||` inside an infix expression is counted as a logical operator, just
        // like `&&`.
        let source = concat!(
            "module M where\n",
            "outOfRange :: Int -> Bool\n",
            "outOfRange x = x < 0 || x > 10\n",
        )
        .as_bytes();
        let visitor = parse_and_visit(source);
        let f = visitor
            .functions
            .iter()
            .find(|f| f.name == "outOfRange")
            .expect("outOfRange extracted");
        let c = f.complexity.as_ref().expect("complexity computed");
        assert!(
            c.cyclomatic_complexity > 1,
            "|| should raise cyclomatic complexity, got {}",
            c.cyclomatic_complexity
        );
    }

    #[test]
    fn test_call_default_metadata() {
        // A call harvested from an `apply` node is direct with no struct/field
        // metadata, and its call_site_line points at the application.
        let source = concat!(
            "module M where\n",
            "helper :: Int -> Int\n",
            "helper y = y\n",
            "compute :: Int -> Int\n",
            "compute x = helper x\n",
        )
        .as_bytes();
        let visitor = parse_and_visit(source);
        let call = visitor
            .calls
            .iter()
            .find(|c| c.caller == "compute" && c.callee == "helper")
            .expect("compute -> helper call recorded");
        assert!(call.is_direct);
        assert!(call.struct_type.is_none());
        assert!(call.field_name.is_none());
        // compute's definition (and the `helper x` application) is on line 5.
        assert_eq!(call.call_site_line, 5);
    }

    #[test]
    fn test_leading_blank_lines_offset_line_numbers() {
        // Blank lines before the module header push declaration line numbers down.
        let source = b"\n\nmodule M where\nfoo :: Int -> Int\nfoo n = n\n";
        let visitor = parse_and_visit(source);
        let foo = visitor
            .functions
            .iter()
            .find(|f| f.name == "foo")
            .expect("foo extracted");
        // module=3, signature=4, definition=5 (1-indexed).
        assert_eq!(foo.line_start, 5);
    }

    #[test]
    fn test_multiple_functions_preserve_source_order() {
        // Several top-level functions (each with a pattern argument) are emitted
        // in source order.
        let source = concat!(
            "module M where\n",
            "a :: Int -> Int\na x = x\n",
            "b :: Int -> Int\nb x = x\n",
            "c :: Int -> Int\nc x = x\n",
        )
        .as_bytes();
        let visitor = parse_and_visit(source);
        let names: Vec<&str> = visitor.functions.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_nullary_binding_not_extracted_as_function() {
        // A definition with no argument patterns parses as a `bind` node, which
        // visit_declarations does not handle, so it is never emitted as a
        // FunctionEntity - only functions with a `patterns` field are extracted.
        let source = b"module M where\nanswer :: Int\nanswer = 42\n";
        let visitor = parse_and_visit(source);
        assert!(
            !visitor.functions.iter().any(|f| f.name == "answer"),
            "nullary binding is not extracted as a function"
        );
        // The type signature is still recorded for potential class-method promotion.
        assert!(visitor.seen_signatures.contains_key("answer"));
    }

    #[test]
    fn test_body_prefix_truncated_to_max() {
        // An oversized function body is truncated to BODY_PREFIX_MAX_CHARS.
        use codegraph_parser_api::BODY_PREFIX_MAX_CHARS;
        let big = "x".repeat(BODY_PREFIX_MAX_CHARS * 2);
        let source = format!(
            "module M where\nbig :: Int -> String\nbig n = \"{}\"\n",
            big
        );
        let visitor = parse_and_visit(source.as_bytes());
        let f = visitor
            .functions
            .iter()
            .find(|f| f.name == "big")
            .expect("big extracted");
        let bp = f.body_prefix.as_ref().expect("body_prefix present");
        assert_eq!(bp.len(), BODY_PREFIX_MAX_CHARS);
    }

    #[test]
    fn test_data_type_has_no_doc_comment_or_body_prefix() {
        // A plain data declaration has no doc comment and no body_prefix.
        let source = b"module M where\ndata Point = Point Int Int\n";
        let visitor = parse_and_visit(source);
        let point = visitor
            .classes
            .iter()
            .find(|c| c.name == "Point")
            .expect("Point extracted");
        assert!(point.doc_comment.is_none());
        assert!(point.body_prefix.is_none());
        assert!(point.methods.is_empty());
        assert!(point.fields.is_empty());
    }

    #[test]
    fn test_instance_method_qualified_name_format() {
        // instance methods use the `TypeClass.InstanceType_method` name format.
        let source = concat!(
            "module M where\n",
            "class Greet a where\n",
            "  hello :: a -> String\n",
            "data Person = Person\n",
            "instance Greet Person where\n",
            "  hello p = \"hi\"\n",
        )
        .as_bytes();
        let visitor = parse_and_visit(source);
        let inst = visitor
            .functions
            .iter()
            .find(|f| f.name.starts_with("Greet.") && !f.is_abstract)
            .expect("instance method extracted");
        assert_eq!(inst.name, "Greet.Person_hello");
        assert_eq!(inst.parent_class.as_deref(), Some("Greet"));
    }

    #[test]
    fn test_instance_method_parameters_extracted() {
        // instance-method argument patterns are captured as parameters.
        let source = concat!(
            "module M where\n",
            "class Greet a where\n",
            "  hello :: a -> String\n",
            "data Person = Person\n",
            "instance Greet Person where\n",
            "  hello p = \"hi\"\n",
        )
        .as_bytes();
        let visitor = parse_and_visit(source);
        let inst = visitor
            .functions
            .iter()
            .find(|f| f.name == "Greet.Person_hello")
            .expect("instance method extracted");
        let names: Vec<&str> = inst.parameters.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["p"]);
    }

    #[test]
    fn test_nested_call_in_case_attributed_to_function() {
        // A call buried inside a case alternative is still attributed to the
        // enclosing function via the recursive body walk.
        let source = concat!(
            "module M where\n",
            "helper :: Int -> Int\n",
            "helper y = y\n",
            "compute :: Int -> Int\n",
            "compute x = case x of\n",
            "  0 -> helper 1\n",
            "  _ -> x\n",
        )
        .as_bytes();
        let visitor = parse_and_visit(source);
        let call = visitor
            .calls
            .iter()
            .find(|c| c.callee == "helper")
            .expect("nested helper call recorded");
        assert_eq!(call.caller, "compute");
    }

    #[test]
    fn test_guards_do_not_raise_complexity() {
        // Regression pin for a dead grammar arm: the complexity visitor matches
        // `guard`/`guard_equation`, but tree-sitter-haskell names guard clauses
        // `guards` (wrapped in per-equation `match` nodes). The guard conditions
        // here (`>`, `<`) are not `&&`/`||`, so none of the complexity arms fire
        // and the multi-guard function keeps the baseline complexity of 1.
        let source = concat!(
            "module M where\n",
            "sign :: Int -> Int\n",
            "sign n\n",
            "  | n > 0 = 1\n",
            "  | n < 0 = 0\n",
            "  | otherwise = 0\n",
        )
        .as_bytes();
        let visitor = parse_and_visit(source);
        let sign = visitor
            .functions
            .iter()
            .find(|f| f.name == "sign")
            .expect("sign extracted");
        let c = sign.complexity.as_ref().expect("complexity computed");
        assert_eq!(
            c.cyclomatic_complexity, 1,
            "guards are not counted (dead guard/guard_equation arm), got {}",
            c.cyclomatic_complexity
        );
    }
}
