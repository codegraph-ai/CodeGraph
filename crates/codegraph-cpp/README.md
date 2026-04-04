# codegraph-cpp

C++ parser for CodeGraph - extracts code entities and relationships from C++ source files.

## Features

- Parses C++ source files using tree-sitter
- Extracts classes, structs, enums, and functions
- Supports namespaces and nested namespaces
- Handles templates and template parameters
- Extracts inheritance relationships
- Detects function calls and method invocations
- Supports various C++ file extensions: `.cpp`, `.cc`, `.cxx`, `.hpp`, `.hh`, `.hxx`, `.h`

## Usage

```rust
use codegraph::CodeGraph;
use codegraph_cpp::CppParser;
use codegraph_parser_api::CodeParser;
use std::path::Path;

let parser = CppParser::new();
let mut graph = CodeGraph::in_memory().unwrap();

let source = r#"
    namespace myns {
        class MyClass {
        public:
            void myMethod() {}
        };
    }
"#;

let file_info = parser
    .parse_source(source, Path::new("example.cpp"), &mut graph)
    .unwrap();

println!("Found {} classes", file_info.classes.len());
println!("Found {} functions", file_info.functions.len());
```

## Supported Constructs

- **Classes**: `class`, `struct`
- **Enums**: `enum`, `enum class`
- **Functions**: Free functions, methods, constructors, destructors
- **Namespaces**: Named namespaces, nested namespaces
- **Templates**: Class templates, function templates
- **Inheritance**: Single and multiple inheritance
- **Access specifiers**: `public`, `private`, `protected`
- **Includes**: `#include` directives

## License

MIT
