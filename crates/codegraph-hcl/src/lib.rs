// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! HCL/Terraform parser for CodeGraph

mod extractor;
mod mapper;
mod parser_impl;
mod visitor;

pub use parser_impl::HclParser;

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph::CodeGraph;
    use codegraph_parser_api::CodeParser;
    use std::path::Path;

    #[test]
    fn test_parser_creation() {
        let parser = HclParser::new();
        assert_eq!(parser.language(), "hcl");
    }

    #[test]
    fn test_parse_simple_source() {
        let parser = HclParser::new();
        let mut graph = CodeGraph::in_memory().unwrap();

        let source = r#"
resource "aws_instance" "web" {
  ami           = "ami-12345"
  instance_type = "t3.micro"
}
"#;

        let result = parser.parse_source(source, Path::new("main.tf"), &mut graph);
        assert!(result.is_ok());

        let file_info = result.unwrap();
        assert_eq!(file_info.functions.len(), 1);
    }
}
