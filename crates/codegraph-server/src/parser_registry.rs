// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Parser Registry - Manages all language parsers implementing the CodeParser trait.

use codegraph::CodeGraph;
use codegraph_bash::BashParser;
use codegraph_c::CParser;
use codegraph_cobol::CobolParser;
use codegraph_cpp::CppParser;
use codegraph_csharp::CSharpParser;
use codegraph_dart::DartParser;
use codegraph_elixir::ElixirParser;
use codegraph_fortran::FortranParser;
use codegraph_go::GoParser;
use codegraph_groovy::GroovyParser;
use codegraph_haskell::HaskellParser;
use codegraph_hcl::HclParser;
use codegraph_java::JavaParser;
use codegraph_julia::JuliaParser;
use codegraph_kotlin::KotlinParser;
use codegraph_lua::LuaParser;
use codegraph_ocaml::OcamlParser;
use codegraph_parser_api::{CodeParser, FileInfo, ParserConfig, ParserError, ParserMetrics};
use codegraph_perl::PerlParser;
use codegraph_php::PhpParser;
use codegraph_python::PythonParser;
use codegraph_r::RParser;
use codegraph_ruby::RubyParser;
use codegraph_rust::RustParser;
use codegraph_scala::ScalaParser;
use codegraph_swift::SwiftParser;
use codegraph_tcl::TclParser;
use codegraph_toml::TomlParser;
use codegraph_typescript::TypeScriptParser;
use codegraph_verilog::VerilogParser;
use codegraph_yaml::YamlParser;
use codegraph_zig::ZigParser;
use std::path::Path;
use std::sync::Arc;

/// Registry of all available language parsers.
pub struct ParserRegistry {
    bash: Arc<BashParser>,
    c: Arc<CParser>,
    cobol: Arc<CobolParser>,
    cpp: Arc<CppParser>,
    csharp: Arc<CSharpParser>,
    dart: Arc<DartParser>,
    elixir: Arc<ElixirParser>,
    fortran: Arc<FortranParser>,
    go: Arc<GoParser>,
    groovy: Arc<GroovyParser>,
    haskell: Arc<HaskellParser>,
    hcl: Arc<HclParser>,
    java: Arc<JavaParser>,
    julia: Arc<JuliaParser>,
    kotlin: Arc<KotlinParser>,
    lua: Arc<LuaParser>,
    ocaml: Arc<OcamlParser>,
    perl: Arc<PerlParser>,
    php: Arc<PhpParser>,
    python: Arc<PythonParser>,
    r: Arc<RParser>,
    ruby: Arc<RubyParser>,
    rust: Arc<RustParser>,
    scala: Arc<ScalaParser>,
    swift: Arc<SwiftParser>,
    tcl: Arc<TclParser>,
    toml: Arc<TomlParser>,
    typescript: Arc<TypeScriptParser>,
    verilog: Arc<VerilogParser>,
    yaml: Arc<YamlParser>,
    zig: Arc<ZigParser>,
}

impl ParserRegistry {
    /// Create a new parser registry with default configuration.
    pub fn new() -> Self {
        Self::with_config(ParserConfig::default())
    }

    /// Create a new parser registry with custom configuration.
    pub fn with_config(config: ParserConfig) -> Self {
        Self {
            bash: Arc::new(BashParser::with_config(config.clone())),
            c: Arc::new(CParser::with_config(config.clone())),
            cobol: Arc::new(CobolParser::with_config(config.clone())),
            cpp: Arc::new(CppParser::with_config(config.clone())),
            csharp: Arc::new(CSharpParser::with_config(config.clone())),
            dart: Arc::new(DartParser::with_config(config.clone())),
            elixir: Arc::new(ElixirParser::with_config(config.clone())),
            fortran: Arc::new(FortranParser::with_config(config.clone())),
            go: Arc::new(GoParser::with_config(config.clone())),
            groovy: Arc::new(GroovyParser::with_config(config.clone())),
            haskell: Arc::new(HaskellParser::with_config(config.clone())),
            hcl: Arc::new(HclParser::with_config(config.clone())),
            java: Arc::new(JavaParser::with_config(config.clone())),
            julia: Arc::new(JuliaParser::with_config(config.clone())),
            kotlin: Arc::new(KotlinParser::with_config(config.clone())),
            lua: Arc::new(LuaParser::with_config(config.clone())),
            ocaml: Arc::new(OcamlParser::with_config(config.clone())),
            perl: Arc::new(PerlParser::with_config(config.clone())),
            php: Arc::new(PhpParser::with_config(config.clone())),
            python: Arc::new(PythonParser::with_config(config.clone())),
            r: Arc::new(RParser::with_config(config.clone())),
            ruby: Arc::new(RubyParser::with_config(config.clone())),
            rust: Arc::new(RustParser::with_config(config.clone())),
            scala: Arc::new(ScalaParser::with_config(config.clone())),
            swift: Arc::new(SwiftParser::with_config(config.clone())),
            tcl: Arc::new(TclParser::with_config(config.clone())),
            toml: Arc::new(TomlParser::with_config(config.clone())),
            typescript: Arc::new(TypeScriptParser::with_config(config.clone())),
            verilog: Arc::new(VerilogParser::with_config(config.clone())),
            yaml: Arc::new(YamlParser::with_config(config.clone())),
            zig: Arc::new(ZigParser::with_config(config)),
        }
    }

    /// Get parser by language identifier.
    pub fn get_parser(&self, language: &str) -> Option<Arc<dyn CodeParser>> {
        match language.to_lowercase().as_str() {
            "bash" | "shell" | "sh" => Some(self.bash.clone()),
            "c" => Some(self.c.clone()),
            "cobol" => Some(self.cobol.clone()),
            "cpp" | "c++" => Some(self.cpp.clone()),
            "csharp" | "c#" => Some(self.csharp.clone()),
            "dart" => Some(self.dart.clone()),
            "elixir" => Some(self.elixir.clone()),
            "fortran" => Some(self.fortran.clone()),
            "go" => Some(self.go.clone()),
            "groovy" => Some(self.groovy.clone()),
            "haskell" => Some(self.haskell.clone()),
            "hcl" | "terraform" => Some(self.hcl.clone()),
            "java" => Some(self.java.clone()),
            "julia" => Some(self.julia.clone()),
            "kotlin" => Some(self.kotlin.clone()),
            "lua" => Some(self.lua.clone()),
            "ocaml" => Some(self.ocaml.clone()),
            "perl" => Some(self.perl.clone()),
            "php" => Some(self.php.clone()),
            "python" => Some(self.python.clone()),
            "r" => Some(self.r.clone()),
            "ruby" => Some(self.ruby.clone()),
            "rust" => Some(self.rust.clone()),
            "scala" => Some(self.scala.clone()),
            "swift" => Some(self.swift.clone()),
            "tcl" => Some(self.tcl.clone()),
            "toml" => Some(self.toml.clone()),
            "typescript" | "javascript" | "typescriptreact" | "javascriptreact" => {
                Some(self.typescript.clone())
            }
            "verilog" | "systemverilog" => Some(self.verilog.clone()),
            "yaml" => Some(self.yaml.clone()),
            "zig" => Some(self.zig.clone()),
            _ => None,
        }
    }

    /// Find appropriate parser for a file path.
    ///
    /// Note: C is checked before C++ so `.h` files default to C parsing.
    /// C++-specific extensions (`.hpp`, `.cc`, `.cxx`, `.hh`, `.hxx`) are
    /// only claimed by the C++ parser and resolve correctly.
    pub fn parser_for_path(&self, path: &Path) -> Option<Arc<dyn CodeParser>> {
        let parsers: [Arc<dyn CodeParser>; 31] = [
            self.bash.clone(),
            self.c.clone(),
            self.cobol.clone(),
            self.cpp.clone(),
            self.csharp.clone(),
            self.dart.clone(),
            self.elixir.clone(),
            self.fortran.clone(),
            self.go.clone(),
            self.groovy.clone(),
            self.haskell.clone(),
            self.hcl.clone(),
            self.java.clone(),
            self.julia.clone(),
            self.kotlin.clone(),
            self.lua.clone(),
            self.ocaml.clone(),
            self.perl.clone(),
            self.php.clone(),
            self.python.clone(),
            self.r.clone(),
            self.ruby.clone(),
            self.rust.clone(),
            self.scala.clone(),
            self.swift.clone(),
            self.tcl.clone(),
            self.toml.clone(),
            self.typescript.clone(),
            self.verilog.clone(),
            self.yaml.clone(),
            self.zig.clone(),
        ];

        parsers.into_iter().find(|p| p.can_parse(path))
    }

    /// Get all supported file extensions.
    pub fn supported_extensions(&self) -> Vec<&str> {
        let mut extensions = Vec::new();
        extensions.extend(self.bash.file_extensions().iter().copied());
        extensions.extend(self.c.file_extensions().iter().copied());
        extensions.extend(self.cobol.file_extensions().iter().copied());
        extensions.extend(self.cpp.file_extensions().iter().copied());
        extensions.extend(self.csharp.file_extensions().iter().copied());
        extensions.extend(self.dart.file_extensions().iter().copied());
        extensions.extend(self.elixir.file_extensions().iter().copied());
        extensions.extend(self.fortran.file_extensions().iter().copied());
        extensions.extend(self.go.file_extensions().iter().copied());
        extensions.extend(self.groovy.file_extensions().iter().copied());
        extensions.extend(self.haskell.file_extensions().iter().copied());
        extensions.extend(self.hcl.file_extensions().iter().copied());
        extensions.extend(self.java.file_extensions().iter().copied());
        extensions.extend(self.julia.file_extensions().iter().copied());
        extensions.extend(self.kotlin.file_extensions().iter().copied());
        extensions.extend(self.lua.file_extensions().iter().copied());
        extensions.extend(self.ocaml.file_extensions().iter().copied());
        extensions.extend(self.perl.file_extensions().iter().copied());
        extensions.extend(self.php.file_extensions().iter().copied());
        extensions.extend(self.python.file_extensions().iter().copied());
        extensions.extend(self.r.file_extensions().iter().copied());
        extensions.extend(self.ruby.file_extensions().iter().copied());
        extensions.extend(self.rust.file_extensions().iter().copied());
        extensions.extend(self.scala.file_extensions().iter().copied());
        extensions.extend(self.swift.file_extensions().iter().copied());
        extensions.extend(self.tcl.file_extensions().iter().copied());
        extensions.extend(self.toml.file_extensions().iter().copied());
        extensions.extend(self.typescript.file_extensions().iter().copied());
        extensions.extend(self.verilog.file_extensions().iter().copied());
        extensions.extend(self.yaml.file_extensions().iter().copied());
        extensions.extend(self.zig.file_extensions().iter().copied());
        extensions
    }

    /// Get metrics from all parsers.
    pub fn all_metrics(&self) -> Vec<(&str, ParserMetrics)> {
        vec![
            ("bash", self.bash.metrics()),
            ("c", self.c.metrics()),
            ("cobol", self.cobol.metrics()),
            ("cpp", self.cpp.metrics()),
            ("csharp", self.csharp.metrics()),
            ("dart", self.dart.metrics()),
            ("elixir", self.elixir.metrics()),
            ("fortran", self.fortran.metrics()),
            ("go", self.go.metrics()),
            ("groovy", self.groovy.metrics()),
            ("haskell", self.haskell.metrics()),
            ("hcl", self.hcl.metrics()),
            ("java", self.java.metrics()),
            ("julia", self.julia.metrics()),
            ("kotlin", self.kotlin.metrics()),
            ("lua", self.lua.metrics()),
            ("ocaml", self.ocaml.metrics()),
            ("perl", self.perl.metrics()),
            ("php", self.php.metrics()),
            ("python", self.python.metrics()),
            ("r", self.r.metrics()),
            ("ruby", self.ruby.metrics()),
            ("rust", self.rust.metrics()),
            ("scala", self.scala.metrics()),
            ("swift", self.swift.metrics()),
            ("tcl", self.tcl.metrics()),
            ("toml", self.toml.metrics()),
            ("typescript", self.typescript.metrics()),
            ("verilog", self.verilog.metrics()),
            ("yaml", self.yaml.metrics()),
            ("zig", self.zig.metrics()),
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
        // Note: C is checked before C++ so `.h` files default to C parsing.
        if self.c.can_parse(path) {
            if self.cpp.can_parse(path) {
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if matches!(ext, "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx") {
                        return Some("cpp");
                    }
                }
            }
            Some("c")
        } else if self.bash.can_parse(path) {
            Some("bash")
        } else if self.cobol.can_parse(path) {
            Some("cobol")
        } else if self.cpp.can_parse(path) {
            Some("cpp")
        } else if self.csharp.can_parse(path) {
            Some("csharp")
        } else if self.dart.can_parse(path) {
            Some("dart")
        } else if self.elixir.can_parse(path) {
            Some("elixir")
        } else if self.fortran.can_parse(path) {
            Some("fortran")
        } else if self.go.can_parse(path) {
            Some("go")
        } else if self.groovy.can_parse(path) {
            Some("groovy")
        } else if self.haskell.can_parse(path) {
            Some("haskell")
        } else if self.hcl.can_parse(path) {
            Some("hcl")
        } else if self.java.can_parse(path) {
            Some("java")
        } else if self.julia.can_parse(path) {
            Some("julia")
        } else if self.kotlin.can_parse(path) {
            Some("kotlin")
        } else if self.lua.can_parse(path) {
            Some("lua")
        } else if self.ocaml.can_parse(path) {
            Some("ocaml")
        } else if self.perl.can_parse(path) {
            Some("perl")
        } else if self.php.can_parse(path) {
            Some("php")
        } else if self.python.can_parse(path) {
            Some("python")
        } else if self.r.can_parse(path) {
            Some("r")
        } else if self.ruby.can_parse(path) {
            Some("ruby")
        } else if self.rust.can_parse(path) {
            Some("rust")
        } else if self.scala.can_parse(path) {
            Some("scala")
        } else if self.swift.can_parse(path) {
            Some("swift")
        } else if self.tcl.can_parse(path) {
            Some("tcl")
        } else if self.toml.can_parse(path) {
            Some("toml")
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
        } else if self.yaml.can_parse(path) {
            Some("yaml")
        } else if self.zig.can_parse(path) {
            Some("zig")
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
        assert!(registry.get_parser("dart").is_some());
        assert!(registry.get_parser("fortran").is_some());
        assert!(registry.get_parser("go").is_some());
        assert!(registry.get_parser("java").is_some());
        assert!(registry.get_parser("kotlin").is_some());
        assert!(registry.get_parser("lua").is_some());
        assert!(registry.get_parser("groovy").is_some());
        assert!(registry.get_parser("php").is_some());
        assert!(registry.get_parser("python").is_some());
        assert!(registry.get_parser("r").is_some());
        assert!(registry.get_parser("ruby").is_some());
        assert!(registry.get_parser("rust").is_some());
        assert!(registry.get_parser("scala").is_some());
        assert!(registry.get_parser("swift").is_some());
        assert!(registry.get_parser("tcl").is_some());
        assert!(registry.get_parser("typescript").is_some());
        assert!(registry.get_parser("verilog").is_some());
        assert!(registry.get_parser("zig").is_some());
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
        assert!(registry.get_parser("Dart").is_some());
        assert!(registry.get_parser("Lua").is_some());
        assert!(registry.get_parser("Groovy").is_some());
        assert!(registry.get_parser("Scala").is_some());
        assert!(registry.get_parser("Zig").is_some());
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
            .parser_for_path(&PathBuf::from("test.dart"))
            .is_some());
        assert!(registry
            .parser_for_path(&PathBuf::from("test.zig"))
            .is_some());
        assert!(registry
            .parser_for_path(&PathBuf::from("test.lua"))
            .is_some());
        assert!(registry
            .parser_for_path(&PathBuf::from("Service.groovy"))
            .is_some());
        assert!(registry
            .parser_for_path(&PathBuf::from("build.gradle"))
            .is_some());
        assert!(registry.parser_for_path(&PathBuf::from("test.R")).is_some());
        assert!(registry
            .parser_for_path(&PathBuf::from("test.scala"))
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
        assert!(extensions.len() >= 22);
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
        assert!(registry.can_parse(Path::new("test.dart")));
        assert!(registry.can_parse(Path::new("test.zig")));
        assert!(registry.can_parse(Path::new("test.lua")));
        assert!(registry.can_parse(Path::new("Service.groovy")));
        assert!(registry.can_parse(Path::new("build.gradle")));
        assert!(registry.can_parse(Path::new("test.R")));
        assert!(registry.can_parse(Path::new("test.scala")));
        assert!(!registry.can_parse(Path::new("test.txt")));
        assert!(!registry.can_parse(Path::new("test.md")));
    }

    #[test]
    fn test_all_metrics() {
        let registry = ParserRegistry::new();
        let metrics = registry.all_metrics();
        assert_eq!(metrics.len(), 31);
        let names: Vec<&str> = metrics.iter().map(|(n, _)| *n).collect();
        assert_eq!(
            names,
            vec![
                "bash", "c", "cobol", "cpp", "csharp", "dart", "elixir",
                "fortran", "go", "groovy", "haskell", "hcl", "java", "julia",
                "kotlin", "lua", "ocaml", "perl", "php", "python", "r", "ruby",
                "rust", "scala", "swift", "tcl", "toml", "typescript", "verilog",
                "yaml", "zig",
            ]
        );
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
        assert_eq!(
            registry.language_for_path(&PathBuf::from("test.dart")),
            Some("dart")
        );
        assert_eq!(
            registry.language_for_path(&PathBuf::from("test.zig")),
            Some("zig")
        );
        assert_eq!(
            registry.language_for_path(&PathBuf::from("test.lua")),
            Some("lua")
        );
        assert_eq!(
            registry.language_for_path(&PathBuf::from("Service.groovy")),
            Some("groovy")
        );
        assert_eq!(
            registry.language_for_path(&PathBuf::from("build.gradle")),
            Some("groovy")
        );
        assert_eq!(
            registry.language_for_path(&PathBuf::from("test.R")),
            Some("r")
        );
        assert_eq!(
            registry.language_for_path(&PathBuf::from("Main.scala")),
            Some("scala")
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
