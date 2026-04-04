// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST extraction for Go source code

use codegraph_parser_api::{CodeIR, ModuleEntity, ParserConfig, ParserError};
use std::path::Path;
use tree_sitter::Parser;

use crate::visitor::GoVisitor;

/// Extract code entities and relationships from Go source code
pub fn extract(
    source: &str,
    file_path: &Path,
    _config: &ParserConfig,
) -> Result<CodeIR, ParserError> {
    let mut parser = Parser::new();
    let language = tree_sitter_go::language();
    parser
        .set_language(&language)
        .map_err(|e| ParserError::ParseError(file_path.to_path_buf(), e.to_string()))?;

    let tree = parser.parse(source, None).ok_or_else(|| {
        ParserError::ParseError(file_path.to_path_buf(), "Failed to parse".to_string())
    })?;

    let root_node = tree.root_node();
    if root_node.has_error() {
        return Err(ParserError::SyntaxError(
            file_path.to_path_buf(),
            0,
            0,
            "Syntax error".to_string(),
        ));
    }

    let mut ir = CodeIR::new(file_path.to_path_buf());

    let module_name = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();
    ir.module = Some(ModuleEntity {
        name: module_name,
        path: file_path.display().to_string(),
        language: "go".to_string(),
        line_count: source.lines().count(),
        doc_comment: None,
        attributes: Vec::new(),
    });

    let mut visitor = GoVisitor::new(source.as_bytes());
    visitor.visit_node(root_node);

    ir.functions = visitor.functions;
    ir.classes = visitor.structs;
    ir.traits = visitor.interfaces;
    ir.imports = visitor.imports;
    ir.calls = visitor.calls;
    ir.type_references = visitor.type_references;

    Ok(ir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_simple_function() {
        let source = r#"
package main

func hello() {
    println("Hello, world!")
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.go"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].name, "hello");
    }

    #[test]
    fn test_extract_struct() {
        let source = r#"
package main

type Person struct {
    Name string
    Age  int
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.go"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "Person");
    }

    #[test]
    fn test_extract_interface() {
        let source = r#"
package main

type Reader interface {
    Read(p []byte) (n int, err error)
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.go"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.traits.len(), 1);
        assert_eq!(ir.traits[0].name, "Reader");
    }

    #[test]
    fn test_extract_method() {
        let source = r#"
package main

type Calculator struct{}

func (c Calculator) Add(a, b int) int {
    return a + b
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.go"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.functions.len(), 1);
    }

    #[test]
    fn test_extract_import() {
        let source = r#"
package main

import "fmt"
import "os"
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.go"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.imports.len(), 2);
    }

    #[test]
    fn test_extract_multiple_entities() {
        let source = r#"
package main

import "fmt"

type Shape interface {
    Area() float64
}

type Circle struct {
    Radius float64
}

func (c Circle) Area() float64 {
    return 3.14 * c.Radius * c.Radius
}

func main() {
    fmt.Println("Hello")
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.go"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.traits.len(), 1);
        assert_eq!(ir.traits[0].name, "Shape");
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "Circle");
        assert!(ir.functions.len() >= 2); // main and Area method
        assert_eq!(ir.imports.len(), 1);
    }

    #[test]
    fn test_extract_with_syntax_error() {
        let source = r#"
package main

func broken( {
    // Missing closing brace
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.go"), &config);

        assert!(result.is_err());
        match result {
            Err(ParserError::SyntaxError(..)) => (),
            _ => panic!("Expected SyntaxError"),
        }
    }

    #[test]
    fn test_extract_module_info() {
        let source = r#"
package main

func test() {
    println("test")
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("module.go"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert!(ir.module.is_some());
        let module = ir.module.unwrap();
        assert_eq!(module.name, "module");
        assert_eq!(module.language, "go");
        assert!(module.line_count > 0);
    }

    #[test]
    fn test_extract_function_with_return_type() {
        let source = r#"
package main

func add(a int, b int) int {
    return a + b
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.go"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].name, "add");
    }

    #[test]
    fn test_extract_multiple_type_declarations() {
        let source = r#"
package main

type (
    User struct {
        ID   int
        Name string
    }
    Admin struct {
        User
        Level int
    }
)
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.go"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert!(!ir.classes.is_empty()); // Should extract at least User
    }

    #[test]
    fn test_extract_variadic_function() {
        let source = r#"
package main

func sum(nums ...int) int {
    total := 0
    for _, num := range nums {
        total += num
    }
    return total
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.go"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].name, "sum");
    }
}
