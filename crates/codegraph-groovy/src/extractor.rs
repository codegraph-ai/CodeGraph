// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST extraction for Groovy source code

use codegraph_parser_api::{CodeIR, ModuleEntity, ParserConfig, ParserError};
use std::path::Path;
use tree_sitter::Parser;

use crate::visitor::GroovyVisitor;

/// Extract code entities and relationships from Groovy source code
pub(crate) fn extract(
    source: &str,
    file_path: &Path,
    _config: &ParserConfig,
) -> Result<CodeIR, ParserError> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_groovy::LANGUAGE.into())
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
        language: "groovy".to_string(),
        line_count: source.lines().count(),
        doc_comment: None,
        attributes: Vec::new(),
    });

    let mut visitor = GroovyVisitor::new(source.as_bytes());
    visitor.visit_node(root_node);

    ir.functions = visitor.functions;
    ir.classes = visitor.classes;
    ir.imports = visitor.imports;
    ir.calls = visitor.calls;

    Ok(ir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extract_ok(source: &str, path: &str) -> CodeIR {
        extract(source, Path::new(path), &ParserConfig::default()).expect("extract should succeed")
    }

    #[test]
    fn test_extract_class() {
        let source = r#"
class UserService {
    def greet(String name) {
        println "Hello, ${name}"
    }
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("UserService.groovy"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "UserService");
    }

    #[test]
    fn test_extract_import() {
        let source = "import groovy.json.JsonSlurper\n";
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.groovy"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.imports.len(), 1);
        assert_eq!(ir.imports[0].imported, "groovy.json.JsonSlurper");
    }

    #[test]
    fn test_extract_top_level_function() {
        let ir = extract_ok("def standalone(String a) { println a }", "test.groovy");
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].name, "standalone");
        assert!(ir.functions[0].parent_class.is_none());
    }

    #[test]
    fn test_module_name_from_file_stem() {
        let ir = extract_ok("def f() {}", "src/Core.groovy");
        let module = ir.module.expect("module should be set");
        assert_eq!(module.name, "Core");
        assert_eq!(module.language, "groovy");
    }

    #[test]
    fn test_module_path_and_line_count() {
        let source = "def a() {}\ndef b() {}\ndef c() {}";
        let ir = extract_ok(source, "app/Util.groovy");
        let module = ir.module.expect("module should be set");
        assert_eq!(module.path, "app/Util.groovy");
        assert_eq!(module.line_count, source.lines().count());
        assert!(module.doc_comment.is_none());
        assert!(module.attributes.is_empty());
    }

    #[test]
    fn test_module_name_unknown_without_stem() {
        // A path with no file stem falls back to "unknown".
        let ir = extract_ok("def f() {}", "..");
        assert_eq!(ir.module.expect("module").name, "unknown");
    }

    #[test]
    fn test_empty_source_yields_only_module() {
        let ir = extract_ok("", "empty.groovy");
        assert!(ir.functions.is_empty());
        assert!(ir.classes.is_empty());
        assert!(ir.imports.is_empty());
        assert!(ir.module.is_some());
    }

    #[test]
    fn test_comment_only_source_yields_no_entities() {
        let ir = extract_ok("// just a comment\n/* another one */", "c.groovy");
        assert!(ir.functions.is_empty());
        assert!(ir.classes.is_empty());
        assert!(ir.imports.is_empty());
    }

    #[test]
    fn test_calls_empty_by_default() {
        // The Groovy visitor does not populate call relations, so ir.calls
        // stays empty even for a body that invokes another method.
        let ir = extract_ok("def f() { helper() }", "test.groovy");
        assert!(ir.calls.is_empty());
    }

    #[test]
    fn test_extract_mixed_entities() {
        let source = "import a.B\n\
             class Shape {\n    def area() {}\n}\n\
             def square(int s) { s * s }";
        let ir = extract_ok(source, "shapes.groovy");
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "Shape");
        assert!(ir.imports.iter().any(|i| i.imported == "a.B"));
    }

    #[test]
    fn test_multiple_functions_extracted() {
        let ir = extract_ok("def a() {}\ndef b() {}", "m.groovy");
        let names: Vec<&str> = ir.functions.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b"]);
    }
}
