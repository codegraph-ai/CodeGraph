// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST extraction for Lua source code

use codegraph_parser_api::{CodeIR, ModuleEntity, ParserConfig, ParserError};
use std::path::Path;
use tree_sitter::Parser;

use crate::visitor::LuaVisitor;

/// Extract code entities and relationships from Lua source code
pub(crate) fn extract(
    source: &str,
    file_path: &Path,
    _config: &ParserConfig,
) -> Result<CodeIR, ParserError> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_lua::LANGUAGE.into())
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
        language: "lua".to_string(),
        line_count: source.lines().count(),
        doc_comment: None,
        attributes: Vec::new(),
    });

    let mut visitor = LuaVisitor::new(source.as_bytes());
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
        extract(source, Path::new(path), &ParserConfig::default()).expect("extract should succeed")
    }

    #[test]
    fn test_module_name_from_file_stem() {
        let ir = extract_ok("local x = 1\n", "widget.lua");
        let module = ir.module.expect("module should be set");
        assert_eq!(module.name, "widget");
    }

    #[test]
    fn test_module_name_unknown_fallback() {
        let ir = extract_ok("local x = 1\n", "..");
        let module = ir.module.expect("module should be set");
        assert_eq!(module.name, "unknown");
    }

    #[test]
    fn test_module_metadata() {
        let source = "local x = 1\nlocal y = 2\n";
        let ir = extract_ok(source, "path/to/mod.lua");
        let module = ir.module.expect("module should be set");
        assert_eq!(module.path, "path/to/mod.lua");
        assert_eq!(module.language, "lua");
        assert_eq!(module.line_count, 2);
        assert!(module.doc_comment.is_none());
        assert!(module.attributes.is_empty());
    }

    #[test]
    fn test_empty_source_yields_only_module() {
        let ir = extract_ok("", "empty.lua");
        assert!(ir.module.is_some());
        assert!(ir.functions.is_empty());
        assert!(ir.imports.is_empty());
        assert!(ir.calls.is_empty());
    }

    #[test]
    fn test_comment_only_source() {
        let ir = extract_ok("-- just a comment\n", "comment.lua");
        assert!(ir.module.is_some());
        assert!(ir.functions.is_empty());
        assert!(ir.imports.is_empty());
        assert!(ir.calls.is_empty());
    }

    #[test]
    fn test_calls_populated() {
        let source = r#"
function callee()
    return 1
end

function caller()
    callee()
end
"#;
        let ir = extract_ok(source, "calls.lua");
        assert!(ir
            .calls
            .iter()
            .any(|c| c.caller == "caller" && c.callee == "callee"));
    }

    #[test]
    fn test_no_class_or_trait_concept() {
        let source = r#"
function f()
    return 1
end
"#;
        let ir = extract_ok(source, "noclass.lua");
        assert!(ir.classes.is_empty());
        assert!(ir.traits.is_empty());
    }

    #[test]
    fn test_multi_function_extraction() {
        let source = r#"
function one() end
function two() end
function three() end
"#;
        let ir = extract_ok(source, "multi.lua");
        assert_eq!(ir.functions.len(), 3);
    }

    #[test]
    fn test_extract_simple_function() {
        let source = r#"
function hello()
    print("Hello, world!")
end
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.lua"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].name, "hello");
    }

    #[test]
    fn test_extract_local_function() {
        let source = r#"
local function helper()
    return 42
end
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.lua"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].name, "helper");
    }

    #[test]
    fn test_extract_require() {
        let source = r#"
local json = require("json")
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.lua"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.imports.len(), 1);
    }
}
