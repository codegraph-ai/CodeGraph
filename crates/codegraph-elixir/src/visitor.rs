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

    #[test]
    fn test_function_metadata_defaults() {
        let source = br#"
defmodule MyApp do
  def greet(name) do
    name
  end
end
"#;
        let visitor = parse_and_visit(source);
        let f = &visitor.functions[0];
        assert_eq!(f.visibility, "public");
        assert!(!f.is_async);
        assert!(!f.is_test);
        assert!(!f.is_static);
        assert!(!f.is_abstract);
        assert!(f.return_type.is_none());
        assert!(f.parent_class.is_none());
        assert!(f.attributes.is_empty());
        assert!(f.line_start >= 1);
        assert!(f.line_end >= f.line_start);
    }

    #[test]
    fn test_signature_is_first_line() {
        let source = br#"
defmodule MyApp do
  def greet(name) do
    name
  end
end
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions[0].signature, "def greet(name) do");
    }

    #[test]
    fn test_single_parameter_extraction() {
        let source = br#"
defmodule MyApp do
  def greet(name) do
    name
  end
end
"#;
        let visitor = parse_and_visit(source);
        let params = &visitor.functions[0].parameters;
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "name");
    }

    #[test]
    fn test_multiple_parameters_extraction() {
        let source = br#"
defmodule MyApp do
  def add(a, b) do
    a + b
  end
end
"#;
        let visitor = parse_and_visit(source);
        let params = &visitor.functions[0].parameters;
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].name, "a");
        assert_eq!(params[1].name, "b");
    }

    #[test]
    fn test_default_argument_parameter() {
        let source = br#"
defmodule MyApp do
  def greet(name \\ "world") do
    name
  end
end
"#;
        let visitor = parse_and_visit(source);
        let params = &visitor.functions[0].parameters;
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "name");
    }

    #[test]
    fn test_guard_function_head() {
        let source = br#"
defmodule MyApp do
  def check(x) when is_binary(x) do
    x
  end
end
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "check");
        assert_eq!(visitor.functions[0].parameters.len(), 1);
        assert_eq!(visitor.functions[0].parameters[0].name, "x");
    }

    #[test]
    fn test_use_and_require_imports() {
        let source = br#"
defmodule MyApp do
  use GenServer
  require Logger
end
"#;
        let visitor = parse_and_visit(source);
        let names: Vec<&str> = visitor
            .imports
            .iter()
            .map(|i| i.imported.as_str())
            .collect();
        assert!(names.contains(&"GenServer"));
        assert!(names.contains(&"Logger"));
    }

    #[test]
    fn test_import_defaults() {
        let source = br#"
defmodule MyApp do
  import Ecto.Query
end
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 1);
        let imp = &visitor.imports[0];
        assert_eq!(imp.imported, "Ecto.Query");
        assert_eq!(imp.importer, "main");
        assert!(imp.symbols.is_empty());
        assert!(!imp.is_wildcard);
        assert!(imp.alias.is_none());
    }

    #[test]
    fn test_doc_comment_extraction() {
        let source = br#"
defmodule MyApp do
  @doc "Greets a person"
  def greet(name) do
    name
  end
end
"#;
        let visitor = parse_and_visit(source);
        let doc = visitor.functions[0].doc_comment.as_deref().unwrap_or("");
        assert!(doc.starts_with("@doc"));
    }

    #[test]
    fn test_doc_comment_absent() {
        let source = br#"
defmodule MyApp do
  def greet(name) do
    name
  end
end
"#;
        let visitor = parse_and_visit(source);
        assert!(visitor.functions[0].doc_comment.is_none());
    }

    #[test]
    fn test_body_prefix_present() {
        let source = br#"
defmodule MyApp do
  def greet(name) do
    "Hello, #{name}"
  end
end
"#;
        let visitor = parse_and_visit(source);
        assert!(visitor.functions[0].body_prefix.is_some());
    }

    #[test]
    fn test_baseline_complexity() {
        let source = br#"
defmodule MyApp do
  def greet(name) do
    name
  end
end
"#;
        let visitor = parse_and_visit(source);
        let c = visitor.functions[0].complexity.as_ref().unwrap();
        assert_eq!(c.cyclomatic_complexity, 1);
    }

    #[test]
    fn test_if_raises_complexity() {
        let source = br#"
defmodule MyApp do
  def check(x) do
    if x > 0 do
      :pos
    else
      :neg
    end
  end
end
"#;
        let visitor = parse_and_visit(source);
        let c = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(c.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_case_raises_complexity() {
        let source = br#"
defmodule MyApp do
  def classify(x) do
    case x do
      0 -> :zero
      _ -> :other
    end
  end
end
"#;
        let visitor = parse_and_visit(source);
        let c = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(c.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_for_loop_raises_complexity() {
        let source = br#"
defmodule MyApp do
  def loop(list) do
    for x <- list do
      x
    end
  end
end
"#;
        let visitor = parse_and_visit(source);
        let c = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(c.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_in_function_call_tracking() {
        let source = br#"
defmodule MyApp do
  def caller do
    do_work()
  end
end
"#;
        let visitor = parse_and_visit(source);
        assert!(visitor
            .calls
            .iter()
            .any(|c| c.caller == "caller" && c.callee == "do_work"));
    }

    #[test]
    fn test_definition_keywords_not_tracked_as_calls() {
        let source = br#"
defmodule MyApp do
  def caller do
    do_work()
  end
end
"#;
        let visitor = parse_and_visit(source);
        // Only the do_work() call is tracked, not def/defmodule.
        assert!(visitor
            .calls
            .iter()
            .all(|c| c.callee != "def" && c.callee != "defmodule"));
    }

    #[test]
    fn test_top_level_call_not_tracked() {
        let source = br#"
defmodule MyApp do
  IO.puts("hi")
end
"#;
        let visitor = parse_and_visit(source);
        assert!(visitor.calls.is_empty());
    }

    #[test]
    fn test_multiple_functions() {
        let source = br#"
defmodule MyApp do
  def a do
    1
  end

  def b do
    2
  end
end
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 2);
        assert_eq!(visitor.functions[0].name, "a");
        assert_eq!(visitor.functions[1].name, "b");
    }

    #[test]
    fn test_empty_source() {
        let source = br#""#;
        let visitor = parse_and_visit(source);
        assert!(visitor.functions.is_empty());
        assert!(visitor.imports.is_empty());
        assert!(visitor.calls.is_empty());
    }
}
