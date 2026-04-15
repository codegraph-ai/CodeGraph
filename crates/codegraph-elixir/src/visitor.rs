// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting Elixir entities

use codegraph_parser_api::{
    truncate_body_prefix, CallRelation, ComplexityBuilder, ComplexityMetrics, FunctionEntity,
    ImportRelation, Parameter,
};
use tree_sitter::Node;

pub(crate) struct ElixirVisitor<'a> {
    pub source: &'a [u8],
    pub functions: Vec<FunctionEntity>,
    pub imports: Vec<ImportRelation>,
    pub calls: Vec<CallRelation>,
    current_function: Option<String>,
}

impl<'a> ElixirVisitor<'a> {
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

    /// Return the first line of a node's text (for signatures)
    fn first_line_text(&self, node: Node) -> String {
        self.node_text(node)
            .lines()
            .next()
            .unwrap_or("")
            .to_string()
    }

    pub fn visit_node(&mut self, node: Node) {
        if node.kind() == "call" {
            // Do NOT recurse further — visit_call_node handles its own recursion
            self.visit_call_node(node);
            return;
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child);
        }
    }

    /// Dispatch on the call's target identifier name.
    ///
    /// In tree-sitter-elixir every macro/keyword (`def`, `defmodule`, `import`, ...)
    /// is represented as a `call` node with:
    ///   - `target` field: an `identifier` (the macro name)
    ///   - `arguments` child: wraps the actual arguments
    ///   - `do_block` child (optional): the `do … end` body
    fn visit_call_node(&mut self, node: Node) {
        let func_name = node
            .child_by_field_name("target")
            .map(|n| self.node_text(n))
            .unwrap_or_default();

        match func_name.as_str() {
            "def" | "defp" => self.visit_def(node, func_name == "defp"),
            "defmodule" => self.visit_defmodule(node),
            "import" => self.visit_import_directive(node, false),
            "alias" => self.visit_import_directive(node, false),
            "use" => self.visit_import_directive(node, false),
            "require" => self.visit_import_directive(node, false),
            _ => {
                // Recurse into children so nested defs / calls are found
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.visit_node(child);
                }
            }
        }
    }

    fn visit_defmodule(&mut self, node: Node) {
        // Recurse into the do_block to find function definitions
        if let Some(body) = self.find_do_block(node) {
            let mut cursor = body.walk();
            for child in body.children(&mut cursor) {
                self.visit_node(child);
            }
        }
    }

    /// Handle `def` / `defp` calls.
    ///
    /// AST shape:
    /// ```text
    /// (call
    ///   target: (identifier)          -- "def" or "defp"
    ///   (arguments
    ///     (call                        -- function head: name(params...)
    ///       target: (identifier)       -- function name
    ///       (arguments ...)))          -- parameters
    ///   (do_block ...))               -- body
    /// ```
    /// Zero-arg functions have the head as a bare `identifier` inside `arguments`.
    fn visit_def(&mut self, node: Node, is_private: bool) {
        let args_node = match self.find_arguments(node) {
            Some(n) => n,
            None => return,
        };

        // The function head is the first child of the arguments node
        let head_node = match args_node.child(0) {
            Some(n) => n,
            None => return,
        };

        let (func_name, parameters) = self.parse_function_head(head_node);
        if func_name.is_empty() {
            return;
        }

        let signature = self.first_line_text(node);
        let doc_comment = self.extract_doc_comment(node);
        let body_node = self.find_do_block(node);

        let body_prefix = body_node
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| truncate_body_prefix(t).to_string());

        let complexity = body_node.map(|b| self.calculate_complexity(b));

        let func = FunctionEntity {
            name: func_name.clone(),
            signature,
            visibility: if is_private { "private" } else { "public" }.to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_async: false,
            is_test: false,
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

        let previous_function = self.current_function.take();
        self.current_function = Some(func_name);

        if let Some(body) = body_node {
            self.visit_body_for_calls(body);
        }

        self.current_function = previous_function;
    }

    /// Find the first `arguments` child of a node.
    // tree-sitter's children() iterator borrows the cursor for its lifetime,
    // making the clippy::manual_find refactor unsound here — suppress it.
    #[allow(clippy::manual_find)]
    fn find_arguments<'b>(&self, node: Node<'b>) -> Option<Node<'b>> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "arguments" {
                return Some(child);
            }
        }
        None
    }

    /// Find the first `do_block` child of a node.
    // Same cursor-lifetime constraint as find_arguments.
    #[allow(clippy::manual_find)]
    fn find_do_block<'b>(&self, node: Node<'b>) -> Option<Node<'b>> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "do_block" {
                return Some(child);
            }
        }
        None
    }

    /// Parse a function head — returns (name, parameters).
    ///
    /// The head may be:
    ///   - `identifier`       — zero-arg function: `def foo do`
    ///   - `call`             — normal: `def foo(a, b) do`
    ///   - `binary_operator`  — guard: `def foo(x) when is_binary(x) do`
    fn parse_function_head<'b>(&self, head: Node<'b>) -> (String, Vec<Parameter>) {
        match head.kind() {
            "identifier" => (self.node_text(head), Vec::new()),
            "call" => {
                let name = head
                    .child_by_field_name("target")
                    .map(|n| self.node_text(n))
                    .unwrap_or_default();
                let params = self.extract_params_from_call(head);
                (name, params)
            }
            "binary_operator" => {
                // `foo(a, b) when guard` — left operand is the actual head call
                if let Some(left) = head.child_by_field_name("left") {
                    self.parse_function_head(left)
                } else {
                    (String::new(), Vec::new())
                }
            }
            _ => (String::new(), Vec::new()),
        }
    }

    fn extract_params_from_call<'b>(&self, call_node: Node<'b>) -> Vec<Parameter> {
        let mut params = Vec::new();
        if let Some(args) = self.find_arguments(call_node) {
            let mut cursor = args.walk();
            for child in args.children(&mut cursor) {
                match child.kind() {
                    "identifier" => {
                        params.push(Parameter::new(self.node_text(child)));
                    }
                    "binary_operator" => {
                        // Default arg: `x \\ default` — take the left side
                        if let Some(left) = child.child_by_field_name("left") {
                            if left.kind() == "identifier" {
                                params.push(Parameter::new(self.node_text(left)));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        params
    }

    /// Handle `import`, `alias`, `use`, `require` directives.
    ///
    /// The module name is the first argument, which is an `alias` node
    /// (dotted module name like `MyApp.Repo`).
    fn visit_import_directive<'b>(&mut self, node: Node<'b>, is_wildcard: bool) {
        if let Some(args) = self.find_arguments(node) {
            if let Some(first_arg) = args.child(0) {
                let module_name = self.node_text(first_arg);
                let module_name = module_name.trim().to_string();
                if !module_name.is_empty() {
                    self.imports.push(ImportRelation {
                        importer: "main".to_string(),
                        imported: module_name,
                        symbols: Vec::new(),
                        is_wildcard,
                        alias: None,
                    });
                }
            }
        }
    }

    fn visit_body_for_calls(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "call" {
                let name = child
                    .child_by_field_name("target")
                    .map(|n| self.node_text(n))
                    .unwrap_or_default();

                // Skip known definition keywords
                if !name.is_empty()
                    && !matches!(
                        name.as_str(),
                        "def" | "defp" | "defmodule" | "import" | "alias" | "use" | "require"
                    )
                {
                    if let Some(ref caller) = self.current_function.clone() {
                        self.calls.push(CallRelation {
                            caller: caller.clone(),
                            callee: name,
                            call_site_line: child.start_position().row + 1,
                            is_direct: true,
                            struct_type: None,
                            field_name: None,
                        });
                    }
                }
            }
            self.visit_body_for_calls(child);
        }
    }

    /// Look for a preceding `unary_operator` sibling that carries `@doc` or `@moduledoc`.
    ///
    /// In the Elixir grammar attributes like `@doc "..."` are represented as:
    /// `(unary_operator operand: (call target: (identifier["doc"]) ...))`
    fn extract_doc_comment(&self, node: Node) -> Option<String> {
        let mut current = node.prev_sibling();
        while let Some(prev) = current {
            match prev.kind() {
                "unary_operator" => {
                    let text = self.node_text(prev);
                    if text.starts_with("@doc") || text.starts_with("@moduledoc") {
                        return Some(text);
                    }
                    // Stop if it's some other unary operator
                    break;
                }
                // Skip through blank lines / comments
                "comment" => {
                    current = prev.prev_sibling();
                    continue;
                }
                _ => break,
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
        // In Elixir, control flow constructs appear as `call` nodes with specific
        // target identifiers. The do_block / else_block structure adds branches.
        if node.kind() == "call" {
            let name = node
                .child_by_field_name("target")
                .map(|n| self.node_text(n))
                .unwrap_or_default();

            match name.as_str() {
                "if" | "unless" | "case" | "cond" => {
                    builder.add_branch();
                    builder.enter_scope();
                    let mut cursor = node.walk();
                    for child in node.children(&mut cursor) {
                        self.visit_for_complexity(child, builder);
                    }
                    builder.exit_scope();
                    return;
                }
                "for" => {
                    builder.add_loop();
                    builder.enter_scope();
                    let mut cursor = node.walk();
                    for child in node.children(&mut cursor) {
                        self.visit_for_complexity(child, builder);
                    }
                    builder.exit_scope();
                    return;
                }
                _ => {}
            }
        }

        match node.kind() {
            "else_block" => {
                builder.add_branch();
            }
            "binary_operator" => {
                // && / || / and / or
                let op = node
                    .child_by_field_name("operator")
                    .map(|n| self.node_text(n))
                    .unwrap_or_default();
                if matches!(op.as_str(), "&&" | "||" | "and" | "or") {
                    builder.add_logical_operator();
                }
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_for_complexity(child, builder);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_visit(source: &[u8]) -> ElixirVisitor<'_> {
        use tree_sitter::Parser;

        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_elixir::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = ElixirVisitor::new(source);
        visitor.visit_node(tree.root_node());
        visitor
    }

    #[test]
    fn test_visitor_function_extraction() {
        let source = br#"
defmodule MyApp do
  def greet(name) do
    "Hello, #{name}"
  end
end
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "greet");
    }

    #[test]
    fn test_visitor_private_function() {
        let source = br#"
defmodule MyApp do
  defp helper(x) do
    x + 1
  end
end
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].visibility, "private");
    }

    #[test]
    fn test_visitor_import_extraction() {
        let source = br#"
defmodule MyApp do
  import Ecto.Query
  alias MyApp.Repo
end
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 2);
    }

    #[test]
    fn test_visitor_zero_arg_function() {
        let source = br#"
defmodule MyApp do
  def init do
    :ok
  end
end
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "init");
    }
}
