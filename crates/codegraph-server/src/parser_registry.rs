// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Parser Registry - Manages all language parsers implementing the CodeParser trait.

use codegraph::CodeGraph;
use codegraph_c::CParser;
use codegraph_cobol::CobolParser;
use codegraph_cpp::CppParser;
use codegraph_csharp::CSharpParser;
use codegraph_fortran::FortranParser;
use codegraph_go::GoParser;
use codegraph_java::JavaParser;
use codegraph_kotlin::KotlinParser;
use codegraph_parser_api::{CodeParser, FileInfo, ParserConfig, ParserError, ParserMetrics};
use codegraph_php::PhpParser;
use codegraph_python::PythonParser;
use codegraph_ruby::RubyParser;
use codegraph_rust::RustParser;
use codegraph_swift::SwiftParser;
use codegraph_tcl::TclParser;
use codegraph_typescript::TypeScriptParser;
use codegraph_verilog::VerilogParser;
use std::path::Path;
use std::sync::Arc;

/// Registry of all available language parsers.
pub struct ParserRegistry {
    c: Arc<CParser>,
    cobol: Arc<CobolParser>,
    cpp: Arc<CppParser>,
    csharp: Arc<CSharpParser>,
    fortran: Arc<FortranParser>,
    go: Arc<GoParser>,
    java: Arc<JavaParser>,
    kotlin: Arc<KotlinParser>,
    php: Arc<PhpParser>,
    python: Arc<PythonParser>,
    ruby: Arc<RubyParser>,
    rust: Arc<RustParser>,
    swift: Arc<SwiftParser>,
    tcl: Arc<TclParser>,
    typescript: Arc<TypeScriptParser>,
    verilog: Arc<VerilogParser>,
}

impl ParserRegistry {
    /// Create a new parser registry with default configuration.
    pub fn new() -> Self {
        Self::with_config(ParserConfig::default())
    }

    /// Create a new parser registry with custom configuration.
    pub fn with_config(config: ParserConfig) -> Self {
        Self {
            c: Arc::new(CParser::with_config(config.clone())),
            cobol: Arc::new(CobolParser::with_config(config.clone())),
            cpp: Arc::new(CppParser::with_config(config.clone())),
            csharp: Arc::new(CSharpParser::with_config(config.clone())),
            fortran: Arc::new(FortranParser::with_config(config.clone())),
            go: Arc::new(GoParser::with_config(config.clone())),
            java: Arc::new(JavaParser::with_config(config.clone())),
            kotlin: Arc::new(KotlinParser::with_config(config.clone())),
            php: Arc::new(PhpParser::with_config(config.clone())),
            python: Arc::new(PythonParser::with_config(config.clone())),
            ruby: Arc::new(RubyParser::with_config(config.clone())),
            rust: Arc::new(RustParser::with_config(config.clone())),
            swift: Arc::new(SwiftParser::with_config(config.clone())),
            tcl: Arc::new(TclParser::with_config(config.clone())),
            typescript: Arc::new(TypeScriptParser::with_config(config.clone())),
            verilog: Arc::new(VerilogParser::with_config(config)),
        }
    }

    /// Get parser by language identifier.
    pub fn get_parser(&self, language: &str) -> Option<Arc<dyn CodeParser>> {
        match language.to_lowercase().as_str() {
            "c" => Some(self.c.clone()),
            "cobol" => Some(self.cobol.clone()),
            "cpp" | "c++" => Some(self.cpp.clone()),
            "csharp" | "c#" => Some(self.csharp.clone()),
            "fortran" => Some(self.fortran.clone()),
            "go" => Some(self.go.clone()),
            "java" => Some(self.java.clone()),
            "kotlin" => Some(self.kotlin.clone()),
            "php" => Some(self.php.clone()),
            "python" => Some(self.python.clone()),
            "ruby" => Some(self.ruby.clone()),
            "rust" => Some(self.rust.clone()),
            "swift" => Some(self.swift.clone()),
            "tcl" => Some(self.tcl.clone()),
            "typescript" | "javascript" | "typescriptreact" | "javascriptreact" => {
                Some(self.typescript.clone())
            }
            "verilog" | "systemverilog" => Some(self.verilog.clone()),
            _ => None,
        }
    }

    /// Find appropriate parser for a file path.
    ///
    /// Note: C is checked before C++ so `.h` files default to C parsing.
    /// C++-specific extensions (`.hpp`, `.cc`, `.cxx`, `.hh`, `.hxx`) are
    /// only claimed by the C++ parser and resolve correctly.
    pub fn parser_for_path(&self, path: &Path) -> Option<Arc<dyn CodeParser>> {
        let parsers: [Arc<dyn CodeParser>; 16] = [
            self.c.clone(),
            self.cobol.clone(),
            self.cpp.clone(),
            self.csharp.clone(),
            self.fortran.clone(),
            self.go.clone(),
            self.java.clone(),
            self.kotlin.clone(),
            self.php.clone(),
            self.python.clone(),
            self.ruby.clone(),
            self.rust.clone(),
            self.swift.clone(),
            self.tcl.clone(),
            self.typescript.clone(),
            self.verilog.clone(),
        ];

        parsers.into_iter().find(|p| p.can_parse(path))
    }

    /// Get all supported file extensions.
    pub fn supported_extensions(&self) -> Vec<&str> {
        let mut extensions = Vec::new();
        extensions.extend(self.c.file_extensions().iter().copied());
        extensions.extend(self.cobol.file_extensions().iter().copied());
        extensions.extend(self.cpp.file_extensions().iter().copied());
        extensions.extend(self.csharp.file_extensions().iter().copied());
        extensions.extend(self.fortran.file_extensions().iter().copied());
        extensions.extend(self.go.file_extensions().iter().copied());
        extensions.extend(self.java.file_extensions().iter().copied());
        extensions.extend(self.kotlin.file_extensions().iter().copied());
        extensions.extend(self.php.file_extensions().iter().copied());
        extensions.extend(self.python.file_extensions().iter().copied());
        extensions.extend(self.ruby.file_extensions().iter().copied());
        extensions.extend(self.rust.file_extensions().iter().copied());
        extensions.extend(self.swift.file_extensions().iter().copied());
        extensions.extend(self.tcl.file_extensions().iter().copied());
        extensions.extend(self.typescript.file_extensions().iter().copied());
        extensions.extend(self.verilog.file_extensions().iter().copied());
        extensions
    }

    /// Get metrics from all parsers.
    pub fn all_metrics(&self) -> Vec<(&str, ParserMetrics)> {
        vec![
            ("c", self.c.metrics()),
            ("cobol", self.cobol.metrics()),
            ("cpp", self.cpp.metrics()),
            ("csharp", self.csharp.metrics()),
            ("fortran", self.fortran.metrics()),
            ("go", self.go.metrics()),
            ("java", self.java.metrics()),
            ("kotlin", self.kotlin.metrics()),
            ("php", self.php.metrics()),
            ("python", self.python.metrics()),
            ("ruby", self.ruby.metrics()),
            ("rust", self.rust.metrics()),
            ("swift", self.swift.metrics()),
            ("tcl", self.tcl.metrics()),
            ("typescript", self.typescript.metrics()),
            ("verilog", self.verilog.metrics()),
        ]
    }

    /// Check if a file path is supported by any parser.
    pub fn can_parse(&self, path: &Path) -> bool {
        self.parser_for_path(path).is_some()
    }

    /// Parse a file using the appropriate parser.
    pub fn parse_file(&self, path: &Path, graph: &mut CodeGraph) -> Result<FileInfo, ParserError> {
        let parser = self.parser_for_path(path).ok_or_else(|| {
            ParserError::UnsupportedFeature(path.to_path_buf(), "Unsupported file type".to_string())
        })?;

        parser.parse_file(path, graph)
    }

    /// Parse source code string using the appropriate parser for the given path.
    pub fn parse_source(
        &self,
        source: &str,
        path: &Path,
        graph: &mut CodeGraph,
    ) -> Result<FileInfo, ParserError> {
        let parser = self.parser_for_path(path).ok_or_else(|| {
            ParserError::UnsupportedFeature(path.to_path_buf(), "Unsupported file type".to_string())
        })?;

        parser.parse_source(source, path, graph)
    }

    /// Get language name for a file path.
    ///
    /// Note: `.h` files return `"c"` by convention (C-compatible headers).
    /// Use `.hpp`/`.hh`/`.hxx` for C++ headers.
    pub fn language_for_path(&self, path: &Path) -> Option<&'static str> {
        if self.c.can_parse(path) {
            // Check C before C++ so .h defaults to C
            if self.cpp.can_parse(path) {
                // C++-specific extensions
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if matches!(ext, "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx") {
                        return Some("cpp");
                    }
                }
            }
            Some("c")
        } else if self.cobol.can_parse(path) {
            Some("cobol")
        } else if self.cpp.can_parse(path) {
            Some("cpp")
        } else if self.csharp.can_parse(path) {
            Some("csharp")
        } else if self.fortran.can_parse(path) {
            Some("fortran")
        } else if self.go.can_parse(path) {
            Some("go")
        } else if self.java.can_parse(path) {
            Some("java")
        } else if self.kotlin.can_parse(path) {
            Some("kotlin")
        } else if self.php.can_parse(path) {
            Some("php")
        } else if self.python.can_parse(path) {
            Some("python")
        } else if self.ruby.can_parse(path) {
            Some("ruby")
        } else if self.rust.can_parse(path) {
            Some("rust")
        } else if self.swift.can_parse(path) {
            Some("swift")
        } else if self.tcl.can_parse(path) {
            Some("tcl")
        } else if self.typescript.can_parse(path) {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                match ext {
                    "ts" | "tsx" => Some("typescript"),
                    "js" | "jsx" => Some("javascript"),
                    _ => Some("typescript"),
                }
            } else {
                Some("typescript")
            }
        } else if self.verilog.can_parse(path) {
            Some("verilog")
        } else {
            None
        }
    }
}

impl Default for ParserRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

    #[test]
    fn test_parser_registry_new() {
        let registry = ParserRegistry::new();
        assert!(registry.get_parser("c").is_some());
        assert!(registry.get_parser("cobol").is_some());
        assert!(registry.get_parser("cpp").is_some());
        assert!(registry.get_parser("csharp").is_some());
        assert!(registry.get_parser("fortran").is_some());
        assert!(registry.get_parser("go").is_some());
        assert!(registry.get_parser("java").is_some());
        assert!(registry.get_parser("kotlin").is_some());
        assert!(registry.get_parser("php").is_some());
        assert!(registry.get_parser("python").is_some());
        assert!(registry.get_parser("ruby").is_some());
        assert!(registry.get_parser("rust").is_some());
        assert!(registry.get_parser("swift").is_some());
        assert!(registry.get_parser("tcl").is_some());
        assert!(registry.get_parser("typescript").is_some());
        assert!(registry.get_parser("verilog").is_some());
    }

    #[test]
    fn test_parser_registry_default() {
        let registry = ParserRegistry::default();
        assert!(registry.get_parser("python").is_some());
    }

    #[test]
    fn test_parser_registry_with_config() {
        let config = ParserConfig::default();
        let registry = ParserRegistry::with_config(config);
        assert!(registry.get_parser("python").is_some());
    }

    #[test]
    fn test_get_parser_case_insensitive() {
        let registry = ParserRegistry::new();
        assert!(registry.get_parser("C").is_some());
        assert!(registry.get_parser("C++").is_some());
        assert!(registry.get_parser("C#").is_some());
        assert!(registry.get_parser("COBOL").is_some());
        assert!(registry.get_parser("Cpp").is_some());
        assert!(registry.get_parser("CSharp").is_some());
        assert!(registry.get_parser("FORTRAN").is_some());
        assert!(registry.get_parser("Go").is_some());
        assert!(registry.get_parser("JAVA").is_some());
        assert!(registry.get_parser("Java").is_some());
        assert!(registry.get_parser("Kotlin").is_some());
        assert!(registry.get_parser("PHP").is_some());
        assert!(registry.get_parser("PYTHON").is_some());
        assert!(registry.get_parser("Python").is_some());
        assert!(registry.get_parser("RUST").is_some());
        assert!(registry.get_parser("Rust").is_some());
        assert!(registry.get_parser("Ruby").is_some());
        assert!(registry.get_parser("Swift").is_some());
        assert!(registry.get_parser("TCL").is_some());
        assert!(registry.get_parser("TypeScript").is_some());
    }

    #[test]
    fn test_get_parser_javascript_variants() {
        let registry = ParserRegistry::new();
        assert!(registry.get_parser("javascript").is_some());
        assert!(registry.get_parser("typescriptreact").is_some());
        assert!(registry.get_parser("javascriptreact").is_some());
    }

    #[test]
    fn test_get_parser_unknown_language() {
        let registry = ParserRegistry::new();
        assert!(registry.get_parser("unknown").is_none());
        assert!(registry.get_parser("").is_none());
    }

    #[test]
    fn test_parser_for_path() {
        let registry = ParserRegistry::new();
        assert!(registry.parser_for_path(&PathBuf::from("test.c")).is_some());
        assert!(registry
            .parser_for_path(&PathBuf::from("test.cob"))
            .is_some());
        assert!(registry
            .parser_for_path(&PathBuf::from("test.cpp"))
            .is_some());
        assert!(registry
            .parser_for_path(&PathBuf::from("test.cs"))
            .is_some());
        assert!(registry
            .parser_for_path(&PathBuf::from("test.f90"))
            .is_some());
        assert!(registry
            .parser_for_path(&PathBuf::from("test.go"))
            .is_some());
        assert!(registry.parser_for_path(&PathBuf::from("test.h")).is_some());
        assert!(registry
            .parser_for_path(&PathBuf::from("Test.java"))
            .is_some());
        assert!(registry
            .parser_for_path(&PathBuf::from("test.js"))
            .is_some());
        assert!(registry
            .parser_for_path(&PathBuf::from("test.kt"))
            .is_some());
        assert!(registry
            .parser_for_path(&PathBuf::from("test.php"))
            .is_some());
        assert!(registry
            .parser_for_path(&PathBuf::from("test.py"))
            .is_some());
        assert!(registry
            .parser_for_path(&PathBuf::from("test.rb"))
            .is_some());
        assert!(registry
            .parser_for_path(&PathBuf::from("test.rs"))
            .is_some());
        assert!(registry
            .parser_for_path(&PathBuf::from("test.swift"))
            .is_some());
        assert!(registry
            .parser_for_path(&PathBuf::from("test.tcl"))
            .is_some());
        assert!(registry
            .parser_for_path(&PathBuf::from("test.ts"))
            .is_some());
        assert!(registry
            .parser_for_path(&PathBuf::from("test.sv"))
            .is_some());
        assert!(registry
            .parser_for_path(&PathBuf::from("test.txt"))
            .is_none());
    }

    #[test]
    fn test_parser_for_path_cpp_variants() {
        let registry = ParserRegistry::new();
        assert!(registry
            .parser_for_path(&PathBuf::from("test.cc"))
            .is_some());
        assert!(registry
            .parser_for_path(&PathBuf::from("test.cxx"))
            .is_some());
        assert!(registry
            .parser_for_path(&PathBuf::from("test.hpp"))
            .is_some());
        assert!(registry
            .parser_for_path(&PathBuf::from("test.hh"))
            .is_some());
        assert!(registry
            .parser_for_path(&PathBuf::from("test.hxx"))
            .is_some());
    }

    #[test]
    fn test_supported_extensions() {
        let registry = ParserRegistry::new();
        let extensions = registry.supported_extensions();
        assert!(!extensions.is_empty());
        assert!(extensions.len() >= 16);
    }

    #[test]
    fn test_can_parse() {
        let registry = ParserRegistry::new();
        assert!(registry.can_parse(Path::new("test.c")));
        assert!(registry.can_parse(Path::new("test.cob")));
        assert!(registry.can_parse(Path::new("test.cpp")));
        assert!(registry.can_parse(Path::new("test.cs")));
        assert!(registry.can_parse(Path::new("test.f90")));
        assert!(registry.can_parse(Path::new("test.go")));
        assert!(registry.can_parse(Path::new("test.h")));
        assert!(registry.can_parse(Path::new("test.java")));
        assert!(registry.can_parse(Path::new("test.js")));
        assert!(registry.can_parse(Path::new("test.kt")));
        assert!(registry.can_parse(Path::new("test.php")));
        assert!(registry.can_parse(Path::new("test.py")));
        assert!(registry.can_parse(Path::new("test.rb")));
        assert!(registry.can_parse(Path::new("test.rs")));
        assert!(registry.can_parse(Path::new("test.sv")));
        assert!(registry.can_parse(Path::new("test.swift")));
        assert!(registry.can_parse(Path::new("test.tcl")));
        assert!(registry.can_parse(Path::new("test.ts")));
        assert!(!registry.can_parse(Path::new("test.txt")));
        assert!(!registry.can_parse(Path::new("test.md")));
    }

    #[test]
    fn test_all_metrics() {
        let registry = ParserRegistry::new();
        let metrics = registry.all_metrics();
        assert_eq!(metrics.len(), 16);
        assert_eq!(metrics[0].0, "c");
        assert_eq!(metrics[1].0, "cobol");
        assert_eq!(metrics[2].0, "cpp");
        assert_eq!(metrics[3].0, "csharp");
        assert_eq!(metrics[4].0, "fortran");
        assert_eq!(metrics[5].0, "go");
        assert_eq!(metrics[6].0, "java");
        assert_eq!(metrics[7].0, "kotlin");
        assert_eq!(metrics[8].0, "php");
        assert_eq!(metrics[9].0, "python");
        assert_eq!(metrics[10].0, "ruby");
        assert_eq!(metrics[11].0, "rust");
        assert_eq!(metrics[12].0, "swift");
        assert_eq!(metrics[13].0, "tcl");
        assert_eq!(metrics[14].0, "typescript");
        assert_eq!(metrics[15].0, "verilog");
    }

    #[test]
    fn test_language_for_path() {
        let registry = ParserRegistry::new();
        assert_eq!(
            registry.language_for_path(&PathBuf::from("test.c")),
            Some("c")
        );
        assert_eq!(
            registry.language_for_path(&PathBuf::from("test.h")),
            Some("c")
        );
        assert_eq!(
            registry.language_for_path(&PathBuf::from("test.cob")),
            Some("cobol")
        );
        assert_eq!(
            registry.language_for_path(&PathBuf::from("test.cpp")),
            Some("cpp")
        );
        assert_eq!(
            registry.language_for_path(&PathBuf::from("test.cc")),
            Some("cpp")
        );
        assert_eq!(
            registry.language_for_path(&PathBuf::from("test.hpp")),
            Some("cpp")
        );
        assert_eq!(
            registry.language_for_path(&PathBuf::from("test.cs")),
            Some("csharp")
        );
        assert_eq!(
            registry.language_for_path(&PathBuf::from("test.f90")),
            Some("fortran")
        );
        assert_eq!(
            registry.language_for_path(&PathBuf::from("test.go")),
            Some("go")
        );
        assert_eq!(
            registry.language_for_path(&PathBuf::from("Test.java")),
            Some("java")
        );
        assert_eq!(
            registry.language_for_path(&PathBuf::from("test.kt")),
            Some("kotlin")
        );
        assert_eq!(
            registry.language_for_path(&PathBuf::from("index.php")),
            Some("php")
        );
        assert_eq!(
            registry.language_for_path(&PathBuf::from("test.py")),
            Some("python")
        );
        assert_eq!(
            registry.language_for_path(&PathBuf::from("app.rb")),
            Some("ruby")
        );
        assert_eq!(
            registry.language_for_path(&PathBuf::from("test.rs")),
            Some("rust")
        );
        assert_eq!(
            registry.language_for_path(&PathBuf::from("test.swift")),
            Some("swift")
        );
        assert_eq!(
            registry.language_for_path(&PathBuf::from("script.tcl")),
            Some("tcl")
        );
        assert_eq!(
            registry.language_for_path(&PathBuf::from("test.ts")),
            Some("typescript")
        );
        assert_eq!(
            registry.language_for_path(&PathBuf::from("test.js")),
            Some("javascript")
        );
        assert_eq!(
            registry.language_for_path(&PathBuf::from("test.tsx")),
            Some("typescript")
        );
        assert_eq!(
            registry.language_for_path(&PathBuf::from("test.jsx")),
            Some("javascript")
        );
        assert_eq!(
            registry.language_for_path(&PathBuf::from("test.sv")),
            Some("verilog")
        );
        assert_eq!(registry.language_for_path(&PathBuf::from("test.txt")), None);
    }

    #[test]
    fn test_parse_source_unsupported() {
        let registry = ParserRegistry::new();
        let mut graph = CodeGraph::in_memory().unwrap();
        let result = registry.parse_source("some content", Path::new("test.txt"), &mut graph);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_file() {
        let registry = ParserRegistry::new();
        let mut graph = CodeGraph::in_memory().unwrap();
        let mut temp_file = NamedTempFile::with_suffix(".py").unwrap();
        writeln!(temp_file, "def test_function():\n    pass").unwrap();
        let result = registry.parse_file(temp_file.path(), &mut graph);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_file_unsupported() {
        let registry = ParserRegistry::new();
        let mut graph = CodeGraph::in_memory().unwrap();
        let mut temp_file = NamedTempFile::with_suffix(".txt").unwrap();
        writeln!(temp_file, "some text content").unwrap();
        let result = registry.parse_file(temp_file.path(), &mut graph);
        assert!(result.is_err());
    }
}
