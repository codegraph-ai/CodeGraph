// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST extraction for Perl source code

use codegraph_parser_api::{CodeIR, ModuleEntity, ParserConfig, ParserError};
use std::path::Path;
use tree_sitter::Parser;

use crate::visitor::PerlVisitor;

/// Extract code entities and relationships from Perl source code
pub(crate) fn extract(
    source: &str,
    file_path: &Path,
    _config: &ParserConfig,
) -> Result<CodeIR, ParserError> {
    let mut parser = Parser::new();
    let language = crate::ts_perl::language();
    parser
        .set_language(&language)
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
        language: "perl".to_string(),
        line_count: source.lines().count(),
        doc_comment: None,
        attributes: Vec::new(),
    });

    let mut visitor = PerlVisitor::new(source.as_bytes());
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
        let config = ParserConfig::default();
        extract(source, Path::new(path), &config).expect("extract should succeed")
    }

    #[test]
    fn test_module_name_from_file_stem() {
        let ir = extract_ok("sub f {}\n", "widgets.pl");
        let module = ir.module.expect("module should be set");
        assert_eq!(module.name, "widgets");
    }

    #[test]
    fn test_module_name_unknown_fallback() {
        // ".." has no file_stem, so the extractor falls back to "unknown".
        let ir = extract_ok("sub f {}\n", "..");
        let module = ir.module.expect("module should be set");
        assert_eq!(module.name, "unknown");
    }

    #[test]
    fn test_module_metadata() {
        let source = "sub a {}\nsub b {}\n";
        let ir = extract_ok(source, "meta.pl");
        let module = ir.module.expect("module should be set");
        assert_eq!(module.path, "meta.pl");
        assert_eq!(module.language, "perl");
        assert_eq!(module.line_count, source.lines().count());
        assert!(module.doc_comment.is_none());
        assert!(module.attributes.is_empty());
    }

    #[test]
    fn test_empty_source_yields_only_module() {
        let ir = extract_ok("", "empty.pl");
        assert!(ir.module.is_some());
        assert!(ir.functions.is_empty());
        assert!(ir.classes.is_empty());
        assert!(ir.imports.is_empty());
        assert!(ir.calls.is_empty());
    }

    #[test]
    fn test_comment_only_source() {
        let ir = extract_ok("# just a comment\n", "comment.pl");
        assert!(ir.module.is_some());
        assert!(ir.functions.is_empty());
        assert!(ir.calls.is_empty());
    }

    #[test]
    fn test_calls_populated_via_caller_callee() {
        let source = r#"
sub helper {
    return 1;
}

sub run {
    return helper();
}
"#;
        let ir = extract_ok(source, "calls.pl");
        assert!(
            ir.calls.iter().any(|c| c.callee == "helper"),
            "expected a call to helper, got: {:?}",
            ir.calls
        );
    }

    #[test]
    fn test_multiple_functions_extracted() {
        let source = "sub a {}\nsub b {}\nsub c {}\n";
        let ir = extract_ok(source, "multi.pl");
        assert_eq!(ir.functions.len(), 3);
    }

    #[test]
    fn test_package_flows_into_classes() {
        let source = "package My::Thing;\nsub new {}\n";
        let ir = extract_ok(source, "Thing.pm");
        assert!(ir.classes.iter().any(|c| c.name == "My::Thing"));
    }

    #[test]
    fn test_extract_simple_sub() {
        let source = r#"
sub hello {
    print "Hello, world!\n";
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.pl"), &config);
        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].name, "hello");
    }

    #[test]
    fn test_extract_package() {
        let source = r#"
package MyApp::User;
use strict;

sub new {
    my ($class) = @_;
    return bless {}, $class;
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("User.pm"), &config);
        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "MyApp::User");
    }

    #[test]
    fn test_extract_use_import() {
        let source = r#"
use POSIX qw(floor ceil);
use Scalar::Util qw(looks_like_number);
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.pl"), &config);
        assert!(result.is_ok());
        let ir = result.unwrap();
        // POSIX and Scalar::Util should be extracted (strict/warnings are filtered)
        assert!(ir.imports.iter().any(|i| i.imported == "POSIX"));
        assert!(ir.imports.iter().any(|i| i.imported == "Scalar::Util"));
    }
}
