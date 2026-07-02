// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST extraction for Elixir source code

use codegraph_parser_api::{CodeIR, ModuleEntity, ParserConfig, ParserError};
use std::path::Path;
use tree_sitter::Parser;

use crate::visitor::ElixirVisitor;

/// Extract code entities and relationships from Elixir source code
pub(crate) fn extract(
    source: &str,
    file_path: &Path,
    _config: &ParserConfig,
) -> Result<CodeIR, ParserError> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_elixir::LANGUAGE.into())
        .map_err(|e| ParserError::ParseError(file_path.to_path_buf(), e.to_string()))?;

    let tree = parser.parse(source, None).ok_or_else(|| {
        ParserError::ParseError(file_path.to_path_buf(), "Failed to parse".to_string())
    })?;

    let root_node = tree.root_node();

    let mut ir = CodeIR::new(file_path.to_path_buf());

    let module_name = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();
    ir.module = Some(ModuleEntity {
        name: module_name,
        path: file_path.display().to_string(),
        language: "elixir".to_string(),
        line_count: source.lines().count(),
        doc_comment: None,
        attributes: Vec::new(),
    });

    let mut visitor = ElixirVisitor::new(source.as_bytes());
    visitor.visit_node(root_node);

    ir.functions = visitor.functions;
    ir.imports = visitor.imports;
    ir.calls = visitor.calls;

    Ok(ir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extract_ok(source: &str, path: &str) -> CodeIR {
        let config = ParserConfig::default();
        extract(source, Path::new(path), &config).expect("extract should succeed")
    }

    #[test]
    fn test_module_name_from_file_stem() {
        let ir = extract_ok("defmodule M do\nend\n", "my_app.ex");
        assert_eq!(ir.module.as_ref().unwrap().name, "my_app");
    }

    #[test]
    fn test_module_name_unknown_fallback() {
        let ir = extract_ok("defmodule M do\nend\n", "..");
        assert_eq!(ir.module.as_ref().unwrap().name, "unknown");
    }

    #[test]
    fn test_module_metadata() {
        let source = "defmodule M do\n  def a() do\n    :ok\n  end\nend\n";
        let ir = extract_ok(source, "test.ex");
        let module = ir.module.as_ref().unwrap();
        assert_eq!(module.path, Path::new("test.ex").display().to_string());
        assert_eq!(module.language, "elixir");
        assert_eq!(module.line_count, source.lines().count());
        assert!(module.doc_comment.is_none());
        assert!(module.attributes.is_empty());
    }

    #[test]
    fn test_empty_source_yields_only_module() {
        let ir = extract_ok("", "test.ex");
        assert!(ir.module.is_some());
        assert!(ir.functions.is_empty());
        assert!(ir.imports.is_empty());
        assert!(ir.calls.is_empty());
    }

    #[test]
    fn test_comment_only_source() {
        let ir = extract_ok("# just a comment\n", "test.ex");
        assert!(ir.module.is_some());
        assert!(ir.functions.is_empty());
        assert!(ir.calls.is_empty());
    }

    #[test]
    fn test_calls_populated_via_caller_callee() {
        let source = r#"
defmodule MyApp do
  def run() do
    helper()
  end

  def helper() do
    :ok
  end
end
"#;
        let ir = extract_ok(source, "test.ex");
        assert!(
            ir.calls.iter().any(|c| c.callee == "helper"),
            "expected a call to helper, got {:?}",
            ir.calls
        );
    }

    #[test]
    fn test_multi_function_extraction() {
        let source = r#"
defmodule MyApp do
  def one() do
    :one
  end

  def two() do
    :two
  end

  def three() do
    :three
  end
end
"#;
        let ir = extract_ok(source, "test.ex");
        assert_eq!(ir.functions.len(), 3);
    }

    #[test]
    fn test_extract_simple_function() {
        let source = r#"
defmodule MyApp do
  def hello() do
    IO.puts("Hello, world!")
  end
end
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.ex"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].name, "hello");
    }

    #[test]
    fn test_extract_private_function() {
        let source = r#"
defmodule MyApp do
  defp helper(x) do
    x + 1
  end
end
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.ex"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].visibility, "private");
    }

    #[test]
    fn test_extract_imports() {
        let source = r#"
defmodule MyApp do
  import Ecto.Query
  alias MyApp.Repo
end
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.ex"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.imports.len(), 2);
    }
}
