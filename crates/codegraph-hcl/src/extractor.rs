// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST extraction for HCL/Terraform source code

use codegraph_parser_api::{CodeIR, ModuleEntity, ParserConfig, ParserError};
use std::path::Path;
use tree_sitter::Parser;

use crate::visitor::HclVisitor;

/// Extract code entities and relationships from HCL/Terraform source code
pub(crate) fn extract(
    source: &str,
    file_path: &Path,
    _config: &ParserConfig,
) -> Result<CodeIR, ParserError> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_hcl::LANGUAGE.into())
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
        language: "hcl".to_string(),
        line_count: source.lines().count(),
        doc_comment: None,
        attributes: Vec::new(),
    });

    let mut visitor = HclVisitor::new(source.as_bytes());
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
        extract(source, Path::new(path), &ParserConfig::default())
            .expect("extract should succeed on valid HCL")
    }

    #[test]
    fn test_module_metadata_from_file_stem() {
        let ir = extract_ok("", "infra/main.tf");
        let module = ir.module.expect("module metadata should be set");
        assert_eq!(module.name, "main");
        assert_eq!(module.language, "hcl");
        assert_eq!(module.path, "infra/main.tf");
        assert_eq!(module.doc_comment, None);
        assert!(module.attributes.is_empty());
    }

    #[test]
    fn test_module_name_unknown_fallback() {
        // ".." has no file stem, so the name falls back to "unknown".
        let ir = extract_ok("", "..");
        assert_eq!(ir.module.expect("module set").name, "unknown");
    }

    #[test]
    fn test_line_count_matches_source_lines() {
        let source = "resource \"aws_instance\" \"web\" {\n  ami = \"x\"\n}\n";
        let ir = extract_ok(source, "main.tf");
        assert_eq!(
            ir.module.expect("module set").line_count,
            source.lines().count()
        );
    }

    #[test]
    fn test_empty_source_yields_no_entities() {
        let ir = extract_ok("", "main.tf");
        assert_eq!(ir.module.expect("module set").line_count, 0);
        assert!(ir.functions.is_empty());
        assert!(ir.imports.is_empty());
        assert!(ir.calls.is_empty());
    }

    #[test]
    fn test_resource_flows_into_ir_functions() {
        let source = r#"
resource "aws_instance" "web" {
  ami = "ami-12345"
}
"#;
        let ir = extract_ok(source, "main.tf");
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].name, "aws_instance.web");
    }

    #[test]
    fn test_module_block_flows_into_ir_imports() {
        let source = r#"
module "vpc" {
  source = "./modules/vpc"
}
"#;
        let ir = extract_ok(source, "main.tf");
        assert!(ir.functions.is_empty());
        assert_eq!(ir.imports.len(), 1);
        assert_eq!(ir.imports[0].imported, "./modules/vpc");
        assert_eq!(ir.imports[0].alias, Some("vpc".to_string()));
    }

    #[test]
    fn test_mixed_blocks_partition_functions_and_imports() {
        let source = r#"
provider "aws" {
  region = "us-east-1"
}

variable "name" {
  default = "web"
}

module "vpc" {
  source = "./modules/vpc"
}
"#;
        let ir = extract_ok(source, "main.tf");
        // provider + variable = 2 functions; module = 1 import.
        assert_eq!(ir.functions.len(), 2);
        assert_eq!(ir.imports.len(), 1);
        let names: Vec<&str> = ir.functions.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"provider.aws"));
        assert!(names.contains(&"var.name"));
    }
}
