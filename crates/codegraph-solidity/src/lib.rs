// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Solidity parser for CodeGraph

mod extractor;
mod mapper;
mod parser_impl;
mod visitor;

pub use parser_impl::SolidityParser;

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph::CodeGraph;
    use codegraph_parser_api::CodeParser;
    use std::path::Path;

    #[test]
    fn test_parser_creation() {
        let parser = SolidityParser::new();
        assert_eq!(parser.language(), "solidity");
    }

    #[test]
    fn test_parse_simple_source() {
        let parser = SolidityParser::new();
        let mut graph = CodeGraph::in_memory().unwrap();

        let source = r#"
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

contract SimpleStorage {
    uint256 private storedData;

    function set(uint256 x) public {
        storedData = x;
    }

    function get() public view returns (uint256) {
        return storedData;
    }
}
"#;

        let result = parser.parse_source(source, Path::new("SimpleStorage.sol"), &mut graph);
        assert!(result.is_ok());

        let file_info = result.unwrap();
        assert_eq!(file_info.classes.len(), 1);
        assert!(file_info.functions.len() >= 2);
    }
}
