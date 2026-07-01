// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST extraction for Solidity source code

use codegraph_parser_api::{CodeIR, ModuleEntity, ParserConfig, ParserError};
use std::path::Path;
use tree_sitter::Parser;

use crate::visitor::SolidityVisitor;

/// Extract code entities and relationships from Solidity source code.
pub(crate) fn extract(
    source: &str,
    file_path: &Path,
    _config: &ParserConfig,
) -> Result<CodeIR, ParserError> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_solidity::LANGUAGE.into())
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
        language: "solidity".to_string(),
        line_count: source.lines().count(),
        doc_comment: None,
        attributes: Vec::new(),
    });

    let mut visitor = SolidityVisitor::new(source.as_bytes());
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

    #[test]
    fn test_extract_contract() {
        let source = r#"// SPDX-License-Identifier: MIT
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
        let config = ParserConfig::default();
        let result = extract(source, Path::new("SimpleStorage.sol"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "SimpleStorage");
        assert!(
            !ir.classes[0].methods.is_empty(),
            "Contract should have methods"
        );
    }

    #[test]
    fn test_extract_interface() {
        let source = r#"// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

interface IERC20 {
    function totalSupply() external view returns (uint256);
    function balanceOf(address account) external view returns (uint256);
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("IERC20.sol"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.traits.len(), 1);
        assert_eq!(ir.traits[0].name, "IERC20");
    }

    #[test]
    fn test_extract_import() {
        let source = r#"// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import "./IERC20.sol";
import "@openzeppelin/contracts/access/Ownable.sol";
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("Token.sol"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.imports.len(), 2);
    }

    #[test]
    fn test_extract_library() {
        let source = r#"// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

library SafeMath {
    function add(uint256 a, uint256 b) internal pure returns (uint256) {
        return a + b;
    }
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("SafeMath.sol"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "SafeMath");
    }

    #[test]
    fn test_module_metadata_fields() {
        // The 4 prior tests asserted classes/traits/imports but never the
        // ModuleEntity that extract() assembles directly.
        let source = "// SPDX-License-Identifier: MIT\npragma solidity ^0.8.0;\n";
        let config = ParserConfig::default();
        let ir = extract(source, Path::new("Vault.sol"), &config).unwrap();

        let module = ir.module.expect("module should be set");
        assert_eq!(module.name, "Vault");
        assert_eq!(module.path, "Vault.sol");
        assert_eq!(module.language, "solidity");
        assert_eq!(module.line_count, source.lines().count());
        assert!(module.doc_comment.is_none());
        assert!(module.attributes.is_empty());
    }

    #[test]
    fn test_unknown_module_name_fallback() {
        // An empty path has no file_stem, so the innermost unwrap_or("unknown")
        // arm is the only way module.name resolves - a branch every named-file
        // fixture skips.
        let source = "pragma solidity ^0.8.0;\n";
        let config = ParserConfig::default();
        let ir = extract(source, Path::new(""), &config).unwrap();

        let module = ir.module.expect("module should be set");
        assert_eq!(module.name, "unknown");
    }

    #[test]
    fn test_empty_source_zero_lines() {
        // Empty source parses to a valid empty tree (not a ParseError): the
        // module still assembles with line_count 0 and no entities.
        let config = ParserConfig::default();
        let ir = extract("", Path::new("Empty.sol"), &config).unwrap();

        let module = ir.module.expect("module should be set");
        assert_eq!(module.line_count, 0);
        assert!(ir.classes.is_empty());
        assert!(ir.traits.is_empty());
        assert!(ir.functions.is_empty());
        assert!(ir.imports.is_empty());
    }

    #[test]
    fn test_extract_top_level_free_function() {
        // Solidity 0.7.1+ free functions live outside any contract; they flow
        // into ir.functions, a target none of the class/trait/import tests hit.
        let source = r#"// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

function computeSum(uint256 a, uint256 b) pure returns (uint256) {
    return a + b;
}
"#;
        let config = ParserConfig::default();
        let ir = extract(source, Path::new("Free.sol"), &config).unwrap();

        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].name, "computeSum");
        assert!(ir.classes.is_empty());
    }

    #[test]
    fn test_calls_always_empty_through_extract() {
        // SolidityVisitor has no call-extraction path, so ir.calls (assigned
        // from visitor.calls) stays empty even for a body full of calls.
        let source = r#"// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

contract Caller {
    function run() public {
        require(msg.sender != address(0), "bad");
        set(1);
    }

    function set(uint256 x) public {}
}
"#;
        let config = ParserConfig::default();
        let ir = extract(source, Path::new("Caller.sol"), &config).unwrap();

        assert!(ir.calls.is_empty());
    }
}
