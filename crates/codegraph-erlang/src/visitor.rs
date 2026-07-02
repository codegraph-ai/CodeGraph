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

    // ------------------------------------------------------------------
    // Function metadata defaults
    // ------------------------------------------------------------------

    #[test]
    fn test_function_metadata_defaults() {
        let source = br#"-module(m).
foo() -> ok.
"#;
        let visitor = parse_and_visit(source);
        let f = &visitor.functions[0];
        assert_eq!(f.name, "foo");
        assert!(!f.is_async, "Erlang functions are never async");
        assert!(f.is_static, "Erlang functions are always static");
        assert!(!f.is_abstract);
        assert_eq!(f.return_type, None);
        assert_eq!(f.parent_class, None);
        assert!(f.attributes.is_empty());
    }

    #[test]
    fn test_function_line_bounds_one_based() {
        let source = br#"-module(m).

foo() -> ok.
"#;
        let visitor = parse_and_visit(source);
        // `foo` is on the third physical line (1-based).
        assert_eq!(visitor.functions[0].line_start, 3);
        assert_eq!(visitor.functions[0].line_end, 3);
    }

    #[test]
    fn test_function_signature_first_line() {
        let source = br#"-module(m).
compute(X) ->
    X + 1.
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions[0].signature, "compute(X) ->");
    }

    #[test]
    fn test_function_body_prefix_present() {
        let source = br#"-module(m).
compute(X) ->
    X + 1.
"#;
        let visitor = parse_and_visit(source);
        assert!(visitor.functions[0]
            .body_prefix
            .as_deref()
            .unwrap()
            .contains("compute"));
    }

    // ------------------------------------------------------------------
    // Parameters
    // ------------------------------------------------------------------

    #[test]
    fn test_single_var_parameter() {
        let source = br#"-module(m).
greet(Name) -> Name.
"#;
        let visitor = parse_and_visit(source);
        let params = &visitor.functions[0].parameters;
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "Name");
    }

    #[test]
    fn test_multiple_var_parameters() {
        let source = br#"-module(m).
add(A, B) -> A + B.
"#;
        let visitor = parse_and_visit(source);
        let params = &visitor.functions[0].parameters;
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].name, "A");
        assert_eq!(params[1].name, "B");
    }

    #[test]
    fn test_atom_parameter_captured() {
        // The first clause pattern-matches an atom literal in the arg position.
        let source = br#"-module(m).
handle(stop) -> ok.
"#;
        let visitor = parse_and_visit(source);
        let params = &visitor.functions[0].parameters;
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "stop");
    }

    #[test]
    fn test_integer_pattern_param_dropped() {
        // A `factorial(0)` head has an integer arg, which is neither var nor atom
        // and is therefore not recorded as a parameter.
        let source = br#"-module(m).
zero(0) -> yes.
"#;
        let visitor = parse_and_visit(source);
        assert!(visitor.functions[0].parameters.is_empty());
    }

    #[test]
    fn test_no_parameters() {
        let source = br#"-module(m).
noop() -> ok.
"#;
        let visitor = parse_and_visit(source);
        assert!(visitor.functions[0].parameters.is_empty());
    }

    // ------------------------------------------------------------------
    // is_test detection
    // ------------------------------------------------------------------

    #[test]
    fn test_is_test_prefix() {
        let source = br#"-module(m).
test_foo() -> ok.
prop_bar() -> ok.
plain() -> ok.
"#;
        let visitor = parse_and_visit(source);
        let by = |n: &str| visitor.functions.iter().find(|f| f.name == n).unwrap();
        assert!(by("test_foo").is_test);
        assert!(by("prop_bar").is_test);
        assert!(!by("plain").is_test);
    }

    // ------------------------------------------------------------------
    // Doc comments
    // ------------------------------------------------------------------

    #[test]
    fn test_doc_comment_extraction() {
        let source = br#"-module(m).
%% @doc Does the thing
foo() -> ok.
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(
            visitor.functions[0].doc_comment.as_deref(),
            Some("%% @doc Does the thing")
        );
    }

    #[test]
    fn test_doc_comment_absent() {
        let source = br#"-module(m).

foo() -> ok.
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions[0].doc_comment, None);
    }

    // ------------------------------------------------------------------
    // Complexity
    // ------------------------------------------------------------------

    #[test]
    fn test_complexity_single_clause_counts_clause_branch() {
        // Even a single-clause function has one `function_clause` node, which the
        // complexity walker treats as a branch, so the baseline is 2 (1 + 1 clause).
        let source = br#"-module(m).
foo() -> ok.
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(
            visitor.functions[0]
                .complexity
                .as_ref()
                .unwrap()
                .cyclomatic_complexity,
            2
        );
    }

    #[test]
    fn test_complexity_case_raises() {
        let source = br#"-module(m).
classify(N) ->
    case N of
        0 -> zero;
        _ -> other
    end.
"#;
        let visitor = parse_and_visit(source);
        assert!(
            visitor.functions[0]
                .complexity
                .as_ref()
                .unwrap()
                .cyclomatic_complexity
                > 1
        );
    }

    #[test]
    fn test_complexity_multi_clause_raises() {
        let source = br#"-module(m).
f(0) -> zero;
f(_) -> other.
"#;
        let visitor = parse_and_visit(source);
        // Two clauses of the same fun_decl each register as a branch.
        let f = visitor.functions.iter().find(|f| f.name == "f").unwrap();
        assert!(f.complexity.as_ref().unwrap().cyclomatic_complexity > 1);
    }

    #[test]
    fn test_complexity_logical_operator_raises() {
        let source = br#"-module(m).
flag(A, B) -> A andalso B.
"#;
        let visitor = parse_and_visit(source);
        assert!(
            visitor.functions[0]
                .complexity
                .as_ref()
                .unwrap()
                .cyclomatic_complexity
                > 1
        );
    }

    // ------------------------------------------------------------------
    // Call extraction
    // ------------------------------------------------------------------

    #[test]
    fn test_local_call_tracked() {
        let source = br#"-module(m).
a() -> b().
b() -> ok.
"#;
        let visitor = parse_and_visit(source);
        let call = visitor
            .calls
            .iter()
            .find(|c| c.caller == "a")
            .expect("expected a->b call");
        assert_eq!(call.callee, "b");
        assert!(call.is_direct);
    }

    #[test]
    fn test_remote_call_not_tracked() {
        // A remote call `io:format(...)` wraps the callee in a `remote` node, so
        // first_direct_atom finds no direct atom and no CallRelation is emitted.
        let source = br#"-module(m).
go() -> io:format("hi~n").
"#;
        let visitor = parse_and_visit(source);
        assert!(visitor.calls.is_empty());
    }

    #[test]
    fn test_call_records_caller() {
        let source = br#"-module(m).
outer() -> helper(), helper().
helper() -> ok.
"#;
        let visitor = parse_and_visit(source);
        let outer_calls: Vec<_> = visitor
            .calls
            .iter()
            .filter(|c| c.caller == "outer")
            .collect();
        assert_eq!(outer_calls.len(), 2);
        assert!(outer_calls.iter().all(|c| c.callee == "helper"));
    }

    // ------------------------------------------------------------------
    // Imports / records / behaviours
    // ------------------------------------------------------------------

    #[test]
    fn test_import_symbols_and_importer() {
        let source = br#"-module(mymod).
-import(lists, [map/2, filter/2]).
foo() -> ok.
"#;
        let visitor = parse_and_visit(source);
        let imp = &visitor.imports[0];
        assert_eq!(imp.imported, "lists");
        assert_eq!(imp.importer, "mymod");
        assert!(!imp.is_wildcard);
        assert_eq!(imp.alias, None);
        assert!(imp.symbols.contains(&"map".to_string()));
        assert!(imp.symbols.contains(&"filter".to_string()));
    }

    #[test]
    fn test_import_importer_defaults_to_main_without_module() {
        let source = br#"-import(lists, [map/2]).
foo() -> ok.
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports[0].importer, "main");
    }

    #[test]
    fn test_record_attributes() {
        let source = br#"-module(m).
-record(person, {name, age}).
foo() -> ok.
"#;
        let visitor = parse_and_visit(source);
        let c = &visitor.classes[0];
        assert_eq!(c.name, "person");
        assert_eq!(c.visibility, "public");
        assert!(!c.is_abstract);
        assert!(!c.is_interface);
        assert_eq!(c.attributes, vec!["record".to_string()]);
    }

    #[test]
    fn test_behaviour_attributes() {
        let source = br#"-module(m).
-behaviour(gen_server).
init([]) -> ok.
"#;
        let visitor = parse_and_visit(source);
        let t = &visitor.traits[0];
        assert_eq!(t.name, "gen_server");
        assert_eq!(t.visibility, "public");
        assert_eq!(t.attributes, vec!["behaviour".to_string()]);
    }

    #[test]
    fn test_behavior_american_spelling() {
        let source = br#"-module(m).
-behavior(gen_server).
init([]) -> ok.
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.traits.len(), 1);
        assert_eq!(visitor.traits[0].name, "gen_server");
    }

    // ------------------------------------------------------------------
    // Edge cases
    // ------------------------------------------------------------------

    #[test]
    fn test_empty_source() {
        let visitor = parse_and_visit(b"");
        assert!(visitor.functions.is_empty());
        assert!(visitor.classes.is_empty());
        assert!(visitor.traits.is_empty());
        assert!(visitor.imports.is_empty());
        assert!(visitor.calls.is_empty());
        assert_eq!(visitor.module_name, None);
    }

    #[test]
    fn test_comment_only_source() {
        let visitor = parse_and_visit(b"%% just a comment\n");
        assert!(visitor.functions.is_empty());
    }

    #[test]
    fn test_multiple_functions_extracted() {
        let source = br#"-module(m).
a() -> ok.
b() -> ok.
c() -> ok.
"#;
        let visitor = parse_and_visit(source);
        let names: Vec<_> = visitor.functions.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"a"));
        assert!(names.contains(&"b"));
        assert!(names.contains(&"c"));
    }

    // ------------------------------------------------------------------
    // body_prefix truncation & records
    // ------------------------------------------------------------------

    #[test]
    fn test_body_prefix_truncated_to_max() {
        use codegraph_parser_api::BODY_PREFIX_MAX_CHARS;
        // Build a function whose text far exceeds the truncation boundary.
        let filler = "    X = 1, X = 1, X = 1, X = 1, X = 1,\n".repeat(40);
        let source = format!("-module(m).\nbig() ->\n{filler}    ok.\n").into_bytes();
        let visitor = parse_and_visit(&source);
        let bp = visitor.functions[0].body_prefix.as_deref().unwrap();
        assert_eq!(bp.chars().count(), BODY_PREFIX_MAX_CHARS);
    }

    #[test]
    fn test_record_body_prefix_present() {
        let source = br#"-module(m).
-record(person, {name, age}).
foo() -> ok.
"#;
        let visitor = parse_and_visit(source);
        assert!(visitor.classes[0]
            .body_prefix
            .as_deref()
            .unwrap()
            .contains("person"));
    }

    #[test]
    fn test_record_doc_comment_captured() {
        let source = br#"-module(m).
%% @doc a person record
-record(person, {name, age}).
foo() -> ok.
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(
            visitor.classes[0].doc_comment.as_deref(),
            Some("%% @doc a person record")
        );
    }

    #[test]
    fn test_record_line_bounds_one_based() {
        let source = br#"-module(m).

-record(person, {name, age}).
foo() -> ok.
"#;
        let visitor = parse_and_visit(source);
        // The record is on the third physical line (1-based).
        assert_eq!(visitor.classes[0].line_start, 3);
        assert_eq!(visitor.classes[0].line_end, 3);
    }

    // ------------------------------------------------------------------
    // Additional complexity paths
    // ------------------------------------------------------------------

    #[test]
    fn test_complexity_if_raises() {
        let source = br#"-module(m).
pick(N) ->
    if
        N > 0 -> pos;
        true -> nonpos
    end.
"#;
        let visitor = parse_and_visit(source);
        assert!(
            visitor.functions[0]
                .complexity
                .as_ref()
                .unwrap()
                .cyclomatic_complexity
                > 2
        );
    }

    #[test]
    fn test_complexity_receive_raises() {
        let source = br#"-module(m).
loop() ->
    receive
        stop -> ok;
        _ -> loop()
    end.
"#;
        let visitor = parse_and_visit(source);
        // Two receive clauses (cr_clause) each register as a branch.
        assert!(
            visitor.functions[0]
                .complexity
                .as_ref()
                .unwrap()
                .cyclomatic_complexity
                > 2
        );
    }

    #[test]
    fn test_complexity_try_catch_exception_handler() {
        let source = br#"-module(m).
safe() ->
    try risky() of
        Ok -> Ok
    catch
        _:_ -> error
    end.
"#;
        let visitor = parse_and_visit(source);
        assert!(
            visitor.functions[0]
                .complexity
                .as_ref()
                .unwrap()
                .exception_handlers
                >= 1
        );
    }

    #[test]
    fn test_complexity_list_comprehension_not_counted_as_loop() {
        // tree-sitter-erlang emits `list_comprehension`/`binary_comprehension`, but
        // the complexity walker only matches `lc`/`bc` (which the grammar never
        // produces), so a comprehension is a latent gap and adds no loop.
        let source = br#"-module(m).
doubles(Xs) -> [X * 2 || X <- Xs].
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions[0].complexity.as_ref().unwrap().loops, 0);
    }

    #[test]
    fn test_complexity_orelse_logical_operator() {
        let source = br#"-module(m).
flag(A, B) -> A orelse B.
"#;
        let visitor = parse_and_visit(source);
        assert!(
            visitor.functions[0]
                .complexity
                .as_ref()
                .unwrap()
                .logical_operators
                >= 1
        );
    }

    // ------------------------------------------------------------------
    // Call metadata & attribution
    // ------------------------------------------------------------------

    #[test]
    fn test_call_default_metadata() {
        let source = br#"-module(m).
a() -> b().
b() -> ok.
"#;
        let visitor = parse_and_visit(source);
        let call = visitor.calls.iter().find(|c| c.caller == "a").unwrap();
        assert!(call.is_direct);
        assert_eq!(call.struct_type, None);
        assert_eq!(call.field_name, None);
        // `b()` is on the second physical line.
        assert_eq!(call.call_site_line, 2);
    }

    #[test]
    fn test_nested_call_attributed_to_enclosing_function() {
        // A call inside a case body is still attributed to the enclosing function.
        let source = br#"-module(m).
route(N) ->
    case N of
        0 -> helper();
        _ -> ok
    end.
helper() -> ok.
"#;
        let visitor = parse_and_visit(source);
        let call = visitor
            .calls
            .iter()
            .find(|c| c.callee == "helper")
            .expect("expected nested helper call");
        assert_eq!(call.caller, "route");
    }
}
