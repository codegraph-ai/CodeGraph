// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST extraction for Objective-C source code

use codegraph_parser_api::{CodeIR, ModuleEntity, ParserConfig, ParserError};
use std::path::Path;
use tree_sitter::Parser;

use crate::visitor::ObjcVisitor;

/// Extract code entities and relationships from Objective-C source code
pub(crate) fn extract(
    source: &str,
    file_path: &Path,
    _config: &ParserConfig,
) -> Result<CodeIR, ParserError> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_objc::LANGUAGE.into())
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
        language: "objc".to_string(),
        line_count: source.lines().count(),
        doc_comment: None,
        attributes: Vec::new(),
    });

    let mut visitor = ObjcVisitor::new(source.as_bytes());
    visitor.visit_node(root_node);

    ir.functions = visitor.functions;
    ir.classes = visitor.classes;
    ir.traits = visitor.traits;
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
        let ir = extract_ok("", "Widget.m");
        let module = ir.module.expect("module should be set");
        assert_eq!(module.name, "Widget");
    }

    #[test]
    fn test_module_name_unknown_fallback() {
        let ir = extract_ok("", "..");
        let module = ir.module.expect("module should be set");
        assert_eq!(module.name, "unknown");
    }

    #[test]
    fn test_module_path_recorded() {
        let ir = extract_ok("", "src/Widget.m");
        let module = ir.module.expect("module should be set");
        assert_eq!(module.path, "src/Widget.m");
    }

    #[test]
    fn test_module_language_is_objc() {
        let ir = extract_ok("", "Widget.m");
        let module = ir.module.expect("module should be set");
        assert_eq!(module.language, "objc");
    }

    #[test]
    fn test_module_line_count() {
        let source = "// a\n// b\n// c\n";
        let ir = extract_ok(source, "Widget.m");
        let module = ir.module.expect("module should be set");
        assert_eq!(module.line_count, 3);
    }

    #[test]
    fn test_module_doc_comment_and_attributes_empty() {
        let ir = extract_ok("", "Widget.m");
        let module = ir.module.expect("module should be set");
        assert!(module.doc_comment.is_none());
        assert!(module.attributes.is_empty());
    }

    #[test]
    fn test_empty_source_yields_only_module() {
        let ir = extract_ok("", "Widget.m");
        assert!(ir.module.is_some());
        assert!(ir.functions.is_empty());
        assert!(ir.classes.is_empty());
        assert!(ir.traits.is_empty());
        assert!(ir.imports.is_empty());
        assert!(ir.calls.is_empty());
    }

    #[test]
    fn test_comment_only_source() {
        let ir = extract_ok("// just a comment\n", "Widget.m");
        assert!(ir.module.is_some());
        assert!(ir.functions.is_empty());
        assert!(ir.classes.is_empty());
        assert!(ir.calls.is_empty());
    }

    #[test]
    fn test_calls_populated() {
        let source = r#"
@implementation MyClass
- (void)greet {
    NSLog(@"Hello");
}
@end
"#;
        let ir = extract_ok(source, "MyClass.m");
        assert!(
            ir.calls.iter().any(|c| c.callee == "NSLog"),
            "expected an NSLog call relation"
        );
    }

    #[test]
    fn test_mixed_source_populates_entities() {
        let source = r#"
#import <Foundation/Foundation.h>

@protocol MyProtocol
- (void)doSomething;
@end

@interface MyClass : NSObject
- (void)greet;
@end

@implementation MyClass
- (void)greet {
    NSLog(@"Hi");
}
@end
"#;
        let ir = extract_ok(source, "MyClass.m");
        assert!(!ir.imports.is_empty());
        assert!(!ir.classes.is_empty());
        assert!(!ir.traits.is_empty());
        assert!(!ir.functions.is_empty());
    }

    #[test]
    fn test_extract_class_interface() {
        let source = r#"
@interface MyClass : NSObject
- (void)greet;
@end

@implementation MyClass
- (void)greet {
    NSLog(@"Hello");
}
@end
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("MyClass.m"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "MyClass");
    }

    #[test]
    fn test_extract_import() {
        let source = r#"
#import <Foundation/Foundation.h>
#import "MyHelper.h"
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.m"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.imports.len(), 2);
    }

    #[test]
    fn test_extract_protocol() {
        let source = r#"
@protocol MyProtocol
- (void)doSomething;
@end
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.m"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.traits.len(), 1);
        assert_eq!(ir.traits[0].name, "MyProtocol");
    }
}
