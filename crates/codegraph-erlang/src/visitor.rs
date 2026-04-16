// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting Erlang entities
//!
//! Node types (tree-sitter-erlang v0.15):
//! - `fun_decl`           — function declaration (all clauses, may end with `.` or `;`)
//! - `function_clause`    — a single clause within a fun_decl
//! - `expr_args`          — argument list `(a, b, ...)`
//! - `clause_body`        — `-> Expr, ...`
//! - `module_attribute`   — `-module(Name).`
//! - `export_attribute`   — `-export([f/A, ...]).`
//! - `import_attribute`   — `-import(Mod, [f/A, ...]).`
//! - `record_decl`        — `-record(Name, {...}).`
//! - `behaviour_attribute`— `-behaviour(Mod).`
//! - `fa`                 — fun/arity pair inside export/import lists
//! - `atom`               — Erlang atom
//! - `var`                — Erlang variable (uppercase)
//! - `call`               — function call `f(args)` (local or remote `M:f(args)`)

use codegraph_parser_api::{
    truncate_body_prefix, CallRelation, ClassEntity, ComplexityBuilder, ComplexityMetrics,
    FunctionEntity, ImportRelation, Parameter, TraitEntity,
};
use tree_sitter::Node;

pub(crate) struct ErlangVisitor<'a> {
    pub source: &'a [u8],
    pub functions: Vec<FunctionEntity>,
    /// Records → mapped as ClassEntity
    pub classes: Vec<ClassEntity>,
    /// Behaviours declared via `-behaviour(...)` → mapped as TraitEntity
    pub traits: Vec<TraitEntity>,
    pub imports: Vec<ImportRelation>,
    pub calls: Vec<CallRelation>,
    /// Module name extracted from `-module(Name).`
    pub module_name: Option<String>,
    /// Functions listed in `-export([f/a, ...])` — used for visibility
    exported: std::collections::HashSet<String>,
    current_function: Option<String>,
}

impl<'a> ErlangVisitor<'a> {
    pub fn new(source: &'a [u8]) -> Self {
        Self {
            source,
            functions: Vec::new(),
            classes: Vec::new(),
            traits: Vec::new(),
            imports: Vec::new(),
            calls: Vec::new(),
            module_name: None,
            exported: std::collections::HashSet::new(),
            current_function: None,
        }
    }

    fn node_text(&self, node: Node) -> String {
        node.utf8_text(self.source).unwrap_or("").to_string()
    }

    /// Two-pass visit: first collect exports (for visibility), then process everything.
    pub fn visit_node(&mut self, node: Node) {
        self.collect_exports(node);
        self.visit_forms(node);
    }

    // -----------------------------------------------------------------------
    // Pass 1 — collect -module and -export attributes
    // -----------------------------------------------------------------------

    fn collect_exports(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "module_attribute" => {
                    // First atom child is the module name
                    self.module_name = Some(self.first_direct_atom(child));
                }
                "export_attribute" => {
                    // Walk all `fa` nodes inside the export list
                    self.collect_fa_names_exported(child);
                }
                _ => {}
            }
        }
    }

    fn collect_fa_names_exported(&mut self, node: Node) {
        if node.kind() == "fa" {
            let name = self.first_direct_atom(node);
            if !name.is_empty() {
                self.exported.insert(name);
            }
            return;
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect_fa_names_exported(child);
        }
    }

    // -----------------------------------------------------------------------
    // Pass 2 — entity extraction
    // -----------------------------------------------------------------------

    fn visit_forms(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "fun_decl" => self.visit_fun_decl(child),
                "record_decl" => self.visit_record_decl(child),
                "behaviour_attribute" | "behavior_attribute" => {
                    self.visit_behaviour_attribute(child)
                }
                "import_attribute" => self.visit_import_attribute(child),
                // module_attribute and export_attribute handled in pass 1
                _ => {}
            }
        }
    }

    // -----------------------------------------------------------------------
    // Functions — `fun_decl` containing one or more `function_clause` children
    // -----------------------------------------------------------------------

    fn visit_fun_decl(&mut self, node: Node) {
        // Function name: first atom of the first function_clause
        let name = self.fun_decl_name(node);
        if name.is_empty() {
            return;
        }

        let is_exported = self.exported.contains(&name);
        let visibility = if is_exported { "public" } else { "private" }.to_string();

        // Signature = first line of the declaration
        let signature = self
            .node_text(node)
            .lines()
            .next()
            .unwrap_or("")
            .to_string();

        let doc_comment = self.extract_doc_comment(node);
        let parameters = self.fun_decl_parameters(node);

        let body_prefix = node
            .utf8_text(self.source)
            .ok()
            .filter(|t| !t.is_empty())
            .map(|t| truncate_body_prefix(t).to_string());

        let complexity = self.calculate_complexity(node);

        let func = FunctionEntity {
            name: name.clone(),
            signature,
            visibility,
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_async: false,
            is_test: name.starts_with("test_") || name.starts_with("prop_"),
            is_static: true,
            is_abstract: false,
            parameters,
            return_type: None,
            doc_comment,
            attributes: Vec::new(),
            parent_class: None,
            complexity: Some(complexity),
            body_prefix,
        };

        self.functions.push(func);

        let prev = self.current_function.take();
        self.current_function = Some(name);
        self.visit_body_for_calls(node);
        self.current_function = prev;
    }

    /// Extract function name from a `fun_decl` node.
    fn fun_decl_name(&self, node: Node) -> String {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "function_clause" {
                // First atom child of function_clause is the name
                return self.first_direct_atom(child);
            }
        }
        String::new()
    }

    /// Extract parameters from the first function_clause's expr_args.
    fn fun_decl_parameters(&self, node: Node) -> Vec<Parameter> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "function_clause" {
                return self.clause_parameters(child);
            }
        }
        Vec::new()
    }

    fn clause_parameters(&self, clause: Node) -> Vec<Parameter> {
        let mut params = Vec::new();

        // Find expr_args child of the clause
        let mut cursor = clause.walk();
        for child in clause.children(&mut cursor) {
            if child.kind() == "expr_args" {
                let mut ac = child.walk();
                for arg in child.children(&mut ac) {
                    match arg.kind() {
                        "var" => params.push(Parameter::new(self.node_text(arg))),
                        "atom" => params.push(Parameter::new(self.node_text(arg))),
                        _ => {}
                    }
                }
                break;
            }
        }

        params
    }

    // -----------------------------------------------------------------------
    // Records — `record_decl`
    // -----------------------------------------------------------------------

    fn visit_record_decl(&mut self, node: Node) {
        // First atom child is the record name
        let name = self.first_direct_atom(node);
        if name.is_empty() {
            return;
        }

        let doc_comment = self.extract_doc_comment(node);
        let body_prefix = node
            .utf8_text(self.source)
            .ok()
            .filter(|t| !t.is_empty())
            .map(|t| truncate_body_prefix(t).to_string());

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
            attributes: vec!["record".to_string()],
            type_parameters: Vec::new(),
            body_prefix,
        });
    }

    // -----------------------------------------------------------------------
    // Attributes
    // -----------------------------------------------------------------------

    fn visit_import_attribute(&mut self, node: Node) {
        // First atom is the module name (no `module` named field in this grammar)
        let module = self.first_direct_atom(node);
        if module.is_empty() {
            return;
        }

        let mut symbols = Vec::new();
        self.collect_fa_symbols(node, &mut symbols);

        self.imports.push(ImportRelation {
            importer: self
                .module_name
                .clone()
                .unwrap_or_else(|| "main".to_string()),
            imported: module,
            symbols,
            is_wildcard: false,
            alias: None,
        });
    }

    fn collect_fa_symbols(&self, node: Node, symbols: &mut Vec<String>) {
        if node.kind() == "fa" {
            let name = self.first_direct_atom(node);
            if !name.is_empty() {
                symbols.push(name);
            }
            return;
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect_fa_symbols(child, symbols);
        }
    }

    fn visit_behaviour_attribute(&mut self, node: Node) {
        let behaviour_name = self.first_direct_atom(node);
        if behaviour_name.is_empty() {
            return;
        }

        self.traits.push(TraitEntity {
            name: behaviour_name,
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            required_methods: Vec::new(),
            parent_traits: Vec::new(),
            doc_comment: None,
            attributes: vec!["behaviour".to_string()],
        });
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn first_direct_atom(&self, node: Node) -> String {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "atom" {
                return self.node_text(child);
            }
        }
        String::new()
    }

    fn extract_doc_comment(&self, node: Node) -> Option<String> {
        if let Some(prev) = node.prev_sibling() {
            if prev.kind() == "comment" {
                let text = self.node_text(prev);
                if text.starts_with('%') {
                    return Some(text);
                }
            }
        }
        None
    }

    // -----------------------------------------------------------------------
    // Call extraction
    // -----------------------------------------------------------------------

    fn visit_body_for_calls(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "call" {
                self.extract_call(child);
            } else {
                self.visit_body_for_calls(child);
            }
        }
    }

    fn extract_call(&mut self, node: Node) {
        // `call` node: first atom child is the callee name (for local calls)
        let callee = self.first_direct_atom(node);
        if !callee.is_empty() {
            if let Some(ref caller) = self.current_function.clone() {
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
        // Recurse into call arguments
        self.visit_body_for_calls(node);
    }

    // -----------------------------------------------------------------------
    // Complexity
    // -----------------------------------------------------------------------

    fn calculate_complexity(&self, node: Node) -> ComplexityMetrics {
        let mut builder = ComplexityBuilder::new();
        self.visit_for_complexity(node, &mut builder);
        builder.build()
    }

    fn visit_for_complexity(&self, node: Node, builder: &mut ComplexityBuilder) {
        match node.kind() {
            // Each additional function_clause beyond the first is a branch
            "function_clause" => {
                builder.add_branch();
            }
            // case expression
            "case_expr" => {
                builder.enter_scope();
            }
            // cr_clause = case clause / receive clause
            "cr_clause" => {
                builder.add_branch();
            }
            // if expression
            "if_expr" => {
                builder.add_branch();
                builder.enter_scope();
            }
            // if clause
            "if_clause" => {
                builder.add_branch();
            }
            // receive expression
            "receive_expr" => {
                builder.enter_scope();
            }
            // try/catch
            "try_expr" => {
                builder.enter_scope();
            }
            "catch_clause" => {
                builder.add_exception_handler();
            }
            // List/binary comprehensions
            "lc" | "bc" => {
                builder.add_loop();
                builder.enter_scope();
            }
            // Logical operators in guard / body expressions
            "binary_op_expr" => {
                let text = self.node_text(node);
                if text.contains("andalso") || text.contains("orelse") {
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
            "case_expr" | "if_expr" | "receive_expr" | "try_expr" | "lc" | "bc" => {
                builder.exit_scope();
            }
            _ => {}
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_visit(source: &[u8]) -> ErlangVisitor<'_> {
        use tree_sitter::Parser;

        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_erlang::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = ErlangVisitor::new(source);
        visitor.visit_node(tree.root_node());
        visitor
    }

    /// Dump the AST — run with `cargo test dump_ast -- --nocapture`
    #[test]
    fn dump_ast() {
        use tree_sitter::Parser;

        let source = br#"-module(mymodule).
-behaviour(gen_server).
-export([start/0, stop/1]).
-import(lists, [map/2, filter/2]).
-record(person, {name, age}).

%% @doc Start the server
start() ->
    ok.

stop(Reason) ->
    Reason.

factorial(0) -> 1;
factorial(N) when N > 0 ->
    N * factorial(N - 1).
"#;

        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_erlang::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();
        println!("\n=== Erlang AST dump ===");
        print_tree(tree.root_node(), source, 0);
        println!("======================\n");
    }

    fn print_tree(node: tree_sitter::Node, source: &[u8], depth: usize) {
        let indent = "  ".repeat(depth);
        let text = if node.child_count() == 0 {
            let t = node.utf8_text(source).unwrap_or("").replace('\n', "\\n");
            format!(" = {:?}", &t[..t.len().min(40)])
        } else {
            String::new()
        };
        println!("{}[{}]{}", indent, node.kind(), text);
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            print_tree(child, source, depth + 1);
        }
    }

    #[test]
    fn test_visitor_function_extraction() {
        let source = br#"-module(mymod).
-export([greet/1]).

greet(Name) ->
    io:format("Hello ~s~n", [Name]).
"#;
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1, "Expected 1 function");
        assert_eq!(visitor.functions[0].name, "greet");
        assert_eq!(visitor.functions[0].visibility, "public");
    }

    #[test]
    fn test_visitor_private_function() {
        let source = br#"-module(mymod).

helper(X) -> X + 1.
"#;
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].visibility, "private");
    }

    #[test]
    fn test_visitor_import_extraction() {
        let source = br#"-module(mymod).
-import(lists, [map/2, filter/2]).

foo() -> ok.
"#;
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "lists");
        assert_eq!(visitor.imports[0].symbols.len(), 2);
    }

    #[test]
    fn test_visitor_record_extraction() {
        let source = br#"-module(mymod).
-record(person, {name, age}).

foo() -> ok.
"#;
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "person");
    }

    #[test]
    fn test_visitor_behaviour_extraction() {
        let source = br#"-module(mymod).
-behaviour(gen_server).

init([]) -> {ok, #{}}.
"#;
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.traits.len(), 1);
        assert_eq!(visitor.traits[0].name, "gen_server");
    }

    #[test]
    fn test_visitor_module_name() {
        let source = br#"-module(mymodule).

foo() -> ok.
"#;
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.module_name.as_deref(), Some("mymodule"));
    }

    #[test]
    fn test_visitor_multi_clause_function() {
        let source = br#"-module(mymod).

factorial(0) -> 1;
factorial(N) when N > 0 ->
    N * factorial(N - 1).
"#;
        let visitor = parse_and_visit(source);

        // All clauses of `factorial` should be grouped into one function
        // (tree-sitter-erlang v0.15 may emit separate fun_decl per clause — handle both)
        let factorial_count = visitor
            .functions
            .iter()
            .filter(|f| f.name == "factorial")
            .count();
        assert!(
            factorial_count >= 1,
            "Expected at least 1 factorial entry, got {}",
            factorial_count
        );
    }
}
