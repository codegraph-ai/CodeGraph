// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Integration tests for Solidity parser

use codegraph::CodeGraph;
use codegraph_parser_api::CodeParser;
use codegraph_solidity::SolidityParser;
use std::path::Path;

const SAMPLE_APP: &str = include_str!("fixtures/sample_app.sol");

#[test]
fn test_parse_sample_app_contracts() {
    let parser = SolidityParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.sol"), &mut graph)
        .unwrap();

    // Should find: SafeMath (library), Ownable (abstract contract), ERC20Token (contract)
    assert!(
        file_info.classes.len() >= 2,
        "Expected at least 2 classes (contracts/libraries), found {}",
        file_info.classes.len()
    );

    let mut class_names = Vec::new();
    for class_id in &file_info.classes {
        let node = graph.get_node(*class_id).unwrap();
        if let Some(codegraph::PropertyValue::String(name)) = node.properties.get("name") {
            class_names.push(name.clone());
        }
    }

    println!("Classes found: {:?}", class_names);
    assert!(
        class_names.iter().any(|n| n == "ERC20Token"),
        "Should find ERC20Token contract, got: {:?}",
        class_names
    );
    assert!(
        class_names.iter().any(|n| n == "SafeMath"),
        "Should find SafeMath library, got: {:?}",
        class_names
    );
}

#[test]
fn test_parse_sample_app_interface() {
    let parser = SolidityParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.sol"), &mut graph)
        .unwrap();

    // Should find IERC20 interface
    assert!(
        !file_info.traits.is_empty(),
        "Expected at least 1 interface (trait), found 0"
    );

    let mut interface_names = Vec::new();
    for trait_id in &file_info.traits {
        let node = graph.get_node(*trait_id).unwrap();
        if let Some(codegraph::PropertyValue::String(name)) = node.properties.get("name") {
            interface_names.push(name.clone());
        }
    }

    println!("Interfaces found: {:?}", interface_names);
    assert!(
        interface_names.iter().any(|n| n == "IERC20"),
        "Should find IERC20 interface, got: {:?}",
        interface_names
    );
}

#[test]
fn test_parse_sample_app_functions() {
    let parser = SolidityParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.sol"), &mut graph)
        .unwrap();

    // Should find many functions (methods in contracts + interface methods)
    // ERC20Token: constructor, totalSupply, balanceOf, transfer, allowance, approve,
    //   transferFrom, mint, burn, _transfer, _mint, _burn, _approve, receive, fallback = 15
    // Ownable: constructor, owner, transferOwnership, renounceOwnership = 4
    // SafeMath: add, sub, mul = 3
    // IERC20 interface methods: 6
    assert!(
        file_info.functions.len() >= 15,
        "Expected at least 15 functions, found {}",
        file_info.functions.len()
    );

    let mut func_names = Vec::new();
    for func_id in &file_info.functions {
        let node = graph.get_node(*func_id).unwrap();
        if let Some(codegraph::PropertyValue::String(name)) = node.properties.get("name") {
            func_names.push(name.clone());
        }
    }

    println!("Functions found: {} total", func_names.len());
    println!("Sample: {:?}", &func_names[..func_names.len().min(20)]);

    // Check for key functions
    assert!(
        func_names.iter().any(|n| n.contains("transfer")),
        "Should find transfer function, got: {:?}",
        func_names
    );
    assert!(
        func_names.iter().any(|n| n.contains("balanceOf")),
        "Should find balanceOf function, got: {:?}",
        func_names
    );
    assert!(
        func_names.iter().any(|n| n.contains("constructor")),
        "Should find constructor, got: {:?}",
        func_names
    );
}

#[test]
fn test_parse_sample_app_imports() {
    let parser = SolidityParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.sol"), &mut graph)
        .unwrap();

    // Should find 2 imports: "./IERC20.sol" and "@openzeppelin/contracts/access/Ownable.sol"
    assert!(
        file_info.imports.len() >= 2,
        "Expected at least 2 imports, found {}",
        file_info.imports.len()
    );

    let mut import_names = Vec::new();
    for import_id in &file_info.imports {
        let node = graph.get_node(*import_id).unwrap();
        if let Some(codegraph::PropertyValue::String(name)) = node.properties.get("name") {
            import_names.push(name.clone());
        }
    }

    println!("Imports found: {:?}", import_names);
    assert!(
        import_names.iter().any(|n| n.contains("IERC20")),
        "Should find IERC20 import, got: {:?}",
        import_names
    );
}

#[test]
fn test_parse_sample_app_edges() {
    let parser = SolidityParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let _file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.sol"), &mut graph)
        .unwrap();

    let edge_count = graph.edge_count();
    assert!(
        edge_count >= 5,
        "Expected at least 5 edges (Contains relationships), found {}",
        edge_count
    );

    println!("Total edges: {}", edge_count);
}

#[test]
fn test_parse_sample_app_visibility() {
    let parser = SolidityParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.sol"), &mut graph)
        .unwrap();

    // Check that functions have visibility set
    let mut has_public = false;
    let mut has_internal = false;

    for func_id in &file_info.functions {
        let node = graph.get_node(*func_id).unwrap();
        if let Some(codegraph::PropertyValue::String(vis)) = node.properties.get("visibility") {
            match vis.as_str() {
                "public" => has_public = true,
                "internal" => has_internal = true,
                _ => {}
            }
        }
    }

    assert!(has_public, "Should have public functions");
    assert!(has_internal, "Should have internal functions");
}

#[test]
fn test_parse_sample_app_complexity() {
    let parser = SolidityParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.sol"), &mut graph)
        .unwrap();

    // At least one function should have complexity > 1 (e.g., _transfer has multiple requires)
    let mut found_complex = false;
    for func_id in &file_info.functions {
        let node = graph.get_node(*func_id).unwrap();
        if let Some(codegraph::PropertyValue::Int(complexity)) =
            node.properties.get("complexity")
        {
            if *complexity > 1 {
                found_complex = true;
                let name = node
                    .properties
                    .get("name")
                    .and_then(|v| {
                        if let codegraph::PropertyValue::String(s) = v {
                            Some(s.as_str())
                        } else {
                            None
                        }
                    })
                    .unwrap_or("?");
                println!("Complex function: {} (complexity={})", name, complexity);
            }
        }
    }

    assert!(
        found_complex,
        "Expected at least one function with complexity > 1"
    );
}

#[test]
fn test_parse_abstract_contract() {
    let source = r#"// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

abstract contract Base {
    function doSomething() public virtual;
    function helper() internal pure returns (uint256) {
        return 42;
    }
}
"#;

    let parser = SolidityParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(source, Path::new("Base.sol"), &mut graph)
        .unwrap();

    assert_eq!(file_info.classes.len(), 1);

    let class_node = graph.get_node(file_info.classes[0]).unwrap();
    if let Some(codegraph::PropertyValue::Bool(is_abstract)) =
        class_node.properties.get("is_abstract")
    {
        assert!(*is_abstract, "Contract should be abstract");
    }
}

#[test]
fn test_parse_modifier() {
    let source = r#"// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

contract Guarded {
    address public owner;

    modifier onlyOwner() {
        require(msg.sender == owner, "Not owner");
        _;
    }

    modifier validAmount(uint256 amount) {
        require(amount > 0, "Amount must be positive");
        _;
    }

    function withdraw(uint256 amount) public onlyOwner validAmount(amount) {
        payable(msg.sender).transfer(amount);
    }
}
"#;

    let parser = SolidityParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(source, Path::new("Guarded.sol"), &mut graph)
        .unwrap();

    assert_eq!(file_info.classes.len(), 1);

    let mut method_names = Vec::new();
    for func_id in &file_info.functions {
        let node = graph.get_node(*func_id).unwrap();
        if let Some(codegraph::PropertyValue::String(name)) = node.properties.get("name") {
            method_names.push(name.clone());
        }
    }

    println!("Methods in Guarded: {:?}", method_names);
    assert!(
        method_names.iter().any(|n| n.contains("onlyOwner")),
        "Should find onlyOwner modifier, got: {:?}",
        method_names
    );
    assert!(
        method_names.iter().any(|n| n.contains("withdraw")),
        "Should find withdraw function, got: {:?}",
        method_names
    );
}

#[test]
fn test_parse_receive_fallback() {
    let source = r#"// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

contract Wallet {
    event Received(address sender, uint256 amount);

    receive() external payable {
        emit Received(msg.sender, msg.value);
    }

    fallback() external payable {
        revert("Not supported");
    }
}
"#;

    let parser = SolidityParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(source, Path::new("Wallet.sol"), &mut graph)
        .unwrap();

    assert_eq!(file_info.classes.len(), 1);

    let mut method_names = Vec::new();
    for func_id in &file_info.functions {
        let node = graph.get_node(*func_id).unwrap();
        if let Some(codegraph::PropertyValue::String(name)) = node.properties.get("name") {
            method_names.push(name.clone());
        }
    }

    println!("Methods in Wallet: {:?}", method_names);
    assert!(
        method_names.iter().any(|n| n == "Wallet.receive"),
        "Should find receive, got: {:?}",
        method_names
    );
    assert!(
        method_names.iter().any(|n| n == "Wallet.fallback"),
        "Should find fallback, got: {:?}",
        method_names
    );
}

#[test]
fn test_parse_sample_app_summary() {
    let parser = SolidityParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.sol"), &mut graph)
        .unwrap();

    println!("\n=== Solidity Parser Sample App Summary ===");
    println!("File: sample_app.sol");
    println!("Lines: {}", file_info.line_count);
    println!("Classes (contracts/libraries): {}", file_info.classes.len());
    println!("Traits (interfaces): {}", file_info.traits.len());
    println!("Functions (methods): {}", file_info.functions.len());
    println!("Imports: {}", file_info.imports.len());
    println!("Parse time: {:?}", file_info.parse_time);
    println!("==========================================\n");

    assert!(file_info.line_count > 50);
    assert!(!file_info.classes.is_empty());
    assert!(!file_info.functions.is_empty());
}
