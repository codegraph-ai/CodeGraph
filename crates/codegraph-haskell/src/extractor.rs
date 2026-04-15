// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST extraction for Haskell source code

use codegraph_parser_api::{CodeIR, ModuleEntity, ParserConfig, ParserError};
use std::path::Path;
use tree_sitter::Parser;

use crate::visitor::HaskellVisitor;

/// Extract code entities and relationships from Haskell source code
pub(crate) fn extract(
    source: &str,
    file_path: &Path,
    _config: &ParserConfig,
) -> Result<CodeIR, ParserError> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_haskell::LANGUAGE.into())
        .map_err(|e| ParserError::ParseError(file_path.to_path_buf(), e.to_string()))?;

    let tree = parser.parse(source, None).ok_or_else(|| {
        ParserError::ParseError(file_path.to_path_buf(), "Failed to parse".to_string())
    })?;

    let root_node = tree.root_node();

    let mut ir = CodeIR::new(file_path.to_path_buf());

    // Attempt to extract the module name from the `header` node
    let module_name = extract_module_name(root_node, source.as_bytes())
        .or_else(|| {
            file_path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "unknown".to_string());

    ir.module = Some(ModuleEntity {
        name: module_name,
        path: file_path.display().to_string(),
        language: "haskell".to_string(),
        line_count: source.lines().count(),
        doc_comment: None,
        attributes: Vec::new(),
    });

    let mut visitor = HaskellVisitor::new(source.as_bytes());
    visitor.visit_node(root_node);

    ir.functions = visitor.functions;
    ir.classes = visitor.classes;
    ir.imports = visitor.imports;
    ir.calls = visitor.calls;

    Ok(ir)
}

/// Walk the AST looking for the `header` → `module` child to get the module name.
fn extract_module_name(root: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if child.kind() == "header" {
            if let Some(module_node) = child.child_by_field_name("module") {
                let text = module_node.utf8_text(source).ok()?;
                return Some(text.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_simple_function() {
        let source =
            "module M where\ngreet :: String -> String\ngreet name = \"Hello, \" ++ name\n";
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.hs"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        let greet = ir.functions.iter().find(|f| f.name == "greet");
        assert!(greet.is_some(), "greet function not found");
    }

    #[test]
    fn test_extract_import() {
        let source = "module M where\nimport Data.Text (Text)\n";
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.hs"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.imports.len(), 1);
        assert_eq!(ir.imports[0].imported, "Data.Text");
    }

    #[test]
    fn test_extract_data_type() {
        let source = "module M where\ndata Color = Red | Green | Blue\n";
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.hs"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        let color = ir.classes.iter().find(|c| c.name == "Color");
        assert!(color.is_some(), "Color data type not found");
    }
}
