// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST extraction for CSS source code

use codegraph_parser_api::{CodeIR, ModuleEntity, ParserConfig, ParserError};
use std::path::Path;
use tree_sitter::Parser;

use crate::visitor::CssVisitor;

/// Extract code entities and relationships from CSS source code
pub(crate) fn extract(
    source: &str,
    file_path: &Path,
    _config: &ParserConfig,
) -> Result<CodeIR, ParserError> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_css::LANGUAGE.into())
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
        language: "css".to_string(),
        line_count: source.lines().count(),
        doc_comment: None,
        attributes: Vec::new(),
    });

    let mut visitor = CssVisitor::new(source.as_bytes());
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
    fn test_extract_simple_rule() {
        let source = r#"
.container {
    max-width: 1200px;
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.css"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.functions.len(), 1);
        assert!(
            ir.functions[0].name.contains("container"),
            "expected selector containing 'container', got: {:?}",
            ir.functions[0].name
        );
    }

    #[test]
    fn test_extract_import() {
        let source = r#"
@import "reset.css";
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.css"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.imports.len(), 1);
        assert_eq!(ir.imports[0].imported, "reset.css");
    }

    #[test]
    fn test_extract_multiple_rules() {
        let source = r#"
body {
    margin: 0;
}

.container {
    max-width: 1200px;
}

h1, h2 {
    color: red;
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.css"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.functions.len(), 3);
    }

    #[test]
    fn test_module_name_from_file_stem() {
        let ir = extract_ok("body { margin: 0; }\n", "theme.css");
        let module = ir.module.expect("module should be set");
        assert_eq!(module.name, "theme");
    }

    #[test]
    fn test_module_name_unknown_fallback() {
        // A path of ".." has no file_stem, exercising the "unknown" fallback.
        let ir = extract_ok("body { margin: 0; }\n", "..");
        let module = ir.module.expect("module should be set");
        assert_eq!(module.name, "unknown");
    }

    #[test]
    fn test_module_path_and_language() {
        let ir = extract_ok("body { margin: 0; }\n", "styles/main.css");
        let module = ir.module.expect("module should be set");
        assert_eq!(
            module.path,
            Path::new("styles/main.css").display().to_string()
        );
        assert_eq!(module.language, "css");
    }

    #[test]
    fn test_module_line_count() {
        let source = "a {\n  color: red;\n}\n";
        let ir = extract_ok(source, "test.css");
        let module = ir.module.expect("module should be set");
        assert_eq!(module.line_count, source.lines().count());
    }

    #[test]
    fn test_module_doc_comment_and_attributes_empty() {
        let ir = extract_ok("body { margin: 0; }\n", "test.css");
        let module = ir.module.expect("module should be set");
        assert_eq!(module.doc_comment, None);
        assert!(module.attributes.is_empty());
    }

    #[test]
    fn test_empty_source_yields_only_module() {
        let ir = extract_ok("", "empty.css");
        assert!(ir.module.is_some());
        assert!(ir.functions.is_empty());
        assert!(ir.classes.is_empty());
        assert!(ir.imports.is_empty());
        assert!(ir.calls.is_empty());
    }

    #[test]
    fn test_comment_only_source_yields_no_entities() {
        let ir = extract_ok("/* just a comment */\n", "test.css");
        assert!(ir.module.is_some());
        assert!(ir.functions.is_empty());
        assert!(ir.classes.is_empty());
        assert!(ir.imports.is_empty());
        assert!(ir.calls.is_empty());
    }

    #[test]
    fn test_calls_always_empty() {
        // CSS has no call concept; a mixed source never populates ir.calls.
        let source = r#"
@import "reset.css";
body {
    margin: 0;
}
.btn:hover {
    background: blue;
}
"#;
        let ir = extract_ok(source, "test.css");
        assert!(ir.calls.is_empty());
    }

    #[test]
    fn test_classes_always_empty() {
        // CSS has no class concept; rule_sets flow into ir.functions, not classes.
        let ir = extract_ok(".container { max-width: 1200px; }\n", "test.css");
        assert!(ir.classes.is_empty());
        assert_eq!(ir.functions.len(), 1);
    }

    #[test]
    fn test_mixed_imports_and_rules() {
        let source = r#"
@import "reset.css";
@import url("vars.css");
body {
    margin: 0;
}
.container {
    max-width: 1200px;
}
"#;
        let ir = extract_ok(source, "test.css");
        assert_eq!(ir.imports.len(), 2);
        assert_eq!(ir.imports[0].imported, "reset.css");
        assert_eq!(ir.imports[1].imported, "vars.css");
        assert_eq!(ir.functions.len(), 2);
    }
}
