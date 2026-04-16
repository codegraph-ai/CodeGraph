// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting Solidity entities

use codegraph_parser_api::{
    truncate_body_prefix, CallRelation, ClassEntity, ComplexityBuilder, ComplexityMetrics,
    FunctionEntity, ImportRelation, Parameter, TraitEntity,
};
use tree_sitter::Node;

pub(crate) struct SolidityVisitor<'a> {
    pub source: &'a [u8],
    pub functions: Vec<FunctionEntity>,
    pub classes: Vec<ClassEntity>,
    pub traits: Vec<TraitEntity>,
    pub imports: Vec<ImportRelation>,
    pub calls: Vec<CallRelation>,
    current_class: Option<String>,
}

impl<'a> SolidityVisitor<'a> {
    pub fn new(source: &'a [u8]) -> Self {
        Self {
            source,
            functions: Vec::new(),
            classes: Vec::new(),
            traits: Vec::new(),
            imports: Vec::new(),
            calls: Vec::new(),
            current_class: None,
        }
    }

    fn node_text(&self, node: Node) -> String {
        node.utf8_text(self.source).unwrap_or("").to_string()
    }

    pub fn visit_node(&mut self, node: Node) {
        let should_recurse = match node.kind() {
            // Import directive: import "path" or import {X} from "path"
            "import_directive" => {
                self.visit_import(node);
                false
            }
            // Top-level contract / interface / library
            "contract_declaration" => {
                self.visit_contract(node);
                false
            }
            "interface_declaration" => {
                self.visit_interface(node);
                false
            }
            "library_declaration" => {
                self.visit_library(node);
                false
            }
            // Top-level free functions (Solidity 0.7.1+)
            "function_definition" => {
                if self.current_class.is_none() {
                    self.visit_function(node);
                }
                false
            }
            _ => true,
        };

        if should_recurse {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                self.visit_node(child);
            }
        }
    }

    // -------------------------------------------------------------------------
    // Imports
    // -------------------------------------------------------------------------

    fn visit_import(&mut self, node: Node) {
        // Possible forms:
        //   import "path";
        //   import "path" as Alias;
        //   import { Sym } from "path";
        //   import * as Alias from "path";
        let mut path = String::new();
        let mut alias: Option<String> = None;
        let mut symbols: Vec<String> = Vec::new();
        let mut is_wildcard = false;

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "string" | "string_literal" => {
                    // Strip surrounding quotes
                    let raw = self.node_text(child);
                    path = raw.trim_matches('"').trim_matches('\'').to_string();
                }
                "import_wildcard" => {
                    is_wildcard = true;
                }
                "import_clause" | "named_imports" => {
                    // import { A, B } from "path"
                    let mut inner = child.walk();
                    for sym in child.children(&mut inner) {
                        if sym.kind() == "identifier" || sym.kind() == "import_specifier" {
                            symbols.push(self.node_text(sym));
                        }
                    }
                }
                "identifier" => {
                    // Could be an alias after `as`
                    alias = Some(self.node_text(child));
                }
                _ => {}
            }
        }

        if path.is_empty() {
            // Fall back: try getting the raw path from the node text
            let text = self.node_text(node);
            if let Some(start) = text.find('"') {
                if let Some(end) = text[start + 1..].find('"') {
                    path = text[start + 1..start + 1 + end].to_string();
                }
            } else if let Some(start) = text.find('\'') {
                if let Some(end) = text[start + 1..].find('\'') {
                    path = text[start + 1..start + 1 + end].to_string();
                }
            }
        }

        if path.is_empty() {
            return;
        }

        self.imports.push(ImportRelation {
            importer: "file".to_string(),
            imported: path,
            symbols,
            is_wildcard,
            alias,
        });
    }

    // -------------------------------------------------------------------------
    // Contract / interface / library
    // -------------------------------------------------------------------------

    fn visit_contract(&mut self, node: Node) {
        let name = self.get_name(node);
        if name.is_empty() {
            return;
        }

        let visibility = self.extract_contract_visibility(node);
        let doc_comment = self.extract_natspec(node);
        let body_prefix = self.get_body_prefix(node);

        let mut class = ClassEntity {
            name: name.clone(),
            visibility,
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_abstract: self.is_abstract(node),
            is_interface: false,
            base_classes: Vec::new(),
            implemented_traits: Vec::new(),
            doc_comment,
            attributes: Vec::new(),
            type_parameters: Vec::new(),
            body_prefix,
            methods: Vec::new(),
            fields: Vec::new(),
        };

        let prev_class = self.current_class.replace(name.clone());

        // Visit body for methods
        if let Some(body) = self.find_body(node) {
            self.visit_contract_body(body, &mut class);
        }

        self.current_class = prev_class;
        self.classes.push(class);
    }

    fn visit_interface(&mut self, node: Node) {
        let name = self.get_name(node);
        if name.is_empty() {
            return;
        }

        let doc_comment = self.extract_natspec(node);

        let mut trait_entity = TraitEntity {
            name: name.clone(),
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            doc_comment,
            required_methods: Vec::new(),
            attributes: Vec::new(),
            parent_traits: Vec::new(),
        };

        let prev_class = self.current_class.replace(name.clone());

        // Visit body for methods
        if let Some(body) = self.find_body(node) {
            self.visit_interface_body(body, &mut trait_entity);
        }

        self.current_class = prev_class;
        self.traits.push(trait_entity);
    }

    fn visit_library(&mut self, node: Node) {
        // Libraries are like contracts in CodeGraph terms — map to ClassEntity
        let name = self.get_name(node);
        if name.is_empty() {
            return;
        }

        let doc_comment = self.extract_natspec(node);
        let body_prefix = self.get_body_prefix(node);

        let mut class = ClassEntity {
            name: name.clone(),
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_abstract: false,
            is_interface: false,
            base_classes: Vec::new(),
            implemented_traits: Vec::new(),
            doc_comment,
            attributes: vec!["library".to_string()],
            type_parameters: Vec::new(),
            body_prefix,
            methods: Vec::new(),
            fields: Vec::new(),
        };

        let prev_class = self.current_class.replace(name.clone());

        if let Some(body) = self.find_body(node) {
            self.visit_contract_body(body, &mut class);
        }

        self.current_class = prev_class;
        self.classes.push(class);
    }

    // -------------------------------------------------------------------------
    // Contract body
    // -------------------------------------------------------------------------

    fn visit_contract_body(&mut self, body: Node, class: &mut ClassEntity) {
        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            match child.kind() {
                "function_definition" => {
                    if let Some(func) = self.extract_function(child) {
                        class.methods.push(func);
                    }
                }
                "constructor_definition" => {
                    if let Some(func) = self.extract_constructor(child) {
                        class.methods.push(func);
                    }
                }
                "modifier_definition" => {
                    if let Some(func) = self.extract_modifier(child) {
                        class.methods.push(func);
                    }
                }
                "fallback_receive_definition" | "receive_function_definition"
                | "fallback_function_definition" => {
                    if let Some(func) = self.extract_special_fn(child) {
                        class.methods.push(func);
                    }
                }
                _ => {}
            }
        }
    }

    fn visit_interface_body(&mut self, body: Node, trait_entity: &mut TraitEntity) {
        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            if child.kind() == "function_definition" {
                if let Some(func) = self.extract_function(child) {
                    trait_entity.required_methods.push(func);
                }
            }
        }
    }

    // -------------------------------------------------------------------------
    // Function extraction helpers
    // -------------------------------------------------------------------------

    fn extract_function(&self, node: Node) -> Option<FunctionEntity> {
        let name = self.get_name(node);
        if name.is_empty() {
            return None;
        }

        let visibility = self.extract_visibility(node);
        let parameters = self.extract_parameters(node);
        let return_type = self.extract_return_type(node);
        let doc_comment = self.extract_natspec(node);
        let body_prefix = self.get_body_prefix(node);
        let complexity = self.get_body_node(node).map(|b| self.calculate_complexity(b));

        // A function is abstract if it has no body (ends with `;`) or is marked `virtual`
        let has_body = self.get_body_node(node).is_some();
        let is_abstract = !has_body || self.has_keyword(node, "virtual");

        Some(FunctionEntity {
            name,
            signature: self.build_signature(node),
            visibility,
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_async: false,
            is_test: false,
            is_static: false,
            is_abstract,
            parameters,
            return_type,
            doc_comment,
            attributes: Vec::new(),
            parent_class: self.current_class.clone(),
            complexity,
            body_prefix,
        })
    }

    fn extract_constructor(&self, node: Node) -> Option<FunctionEntity> {
        let parameters = self.extract_parameters(node);
        let visibility = self.extract_visibility(node);
        let doc_comment = self.extract_natspec(node);
        let body_prefix = self.get_body_prefix(node);
        let complexity = self.get_body_node(node).map(|b| self.calculate_complexity(b));

        let class_name = self.current_class.clone().unwrap_or_else(|| "unknown".to_string());

        Some(FunctionEntity {
            name: "constructor".to_string(),
            signature: format!("constructor({})", self.params_signature(&parameters)),
            visibility,
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_async: false,
            is_test: false,
            is_static: false,
            is_abstract: false,
            parameters,
            return_type: Some(class_name.clone()),
            doc_comment,
            attributes: Vec::new(),
            parent_class: Some(class_name),
            complexity,
            body_prefix,
        })
    }

    fn extract_modifier(&self, node: Node) -> Option<FunctionEntity> {
        let name = self.get_name(node);
        if name.is_empty() {
            return None;
        }

        let parameters = self.extract_parameters(node);
        let doc_comment = self.extract_natspec(node);
        let body_prefix = self.get_body_prefix(node);
        let complexity = self.get_body_node(node).map(|b| self.calculate_complexity(b));

        Some(FunctionEntity {
            name: name.clone(),
            signature: format!("modifier {}({})", name, self.params_signature(&parameters)),
            visibility: "internal".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_async: false,
            is_test: false,
            is_static: false,
            is_abstract: false,
            parameters,
            return_type: None,
            doc_comment,
            attributes: vec!["modifier".to_string()],
            parent_class: self.current_class.clone(),
            complexity,
            body_prefix,
        })
    }

    fn extract_special_fn(&self, node: Node) -> Option<FunctionEntity> {
        // Both receive() and fallback() parse as fallback_receive_definition.
        // Determine which by the first keyword child.
        let fn_name = {
            let mut cursor = node.walk();
            let mut name = "fallback";
            for child in node.children(&mut cursor) {
                if child.kind() == "receive" {
                    name = "receive";
                    break;
                }
                if child.kind() == "fallback" {
                    name = "fallback";
                    break;
                }
            }
            name
        };

        let visibility = self.extract_visibility(node);
        let body_prefix = self.get_body_prefix(node);

        Some(FunctionEntity {
            name: fn_name.to_string(),
            signature: format!("{fn_name}() external"),
            visibility,
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_async: false,
            is_test: false,
            is_static: false,
            is_abstract: false,
            parameters: Vec::new(),
            return_type: None,
            doc_comment: None,
            attributes: Vec::new(),
            parent_class: self.current_class.clone(),
            complexity: None,
            body_prefix,
        })
    }

    fn visit_function(&mut self, node: Node) {
        if let Some(func) = self.extract_function(node) {
            self.functions.push(func);
        }
    }

    // -------------------------------------------------------------------------
    // Helpers
    // -------------------------------------------------------------------------

    fn get_name(&self, node: Node) -> String {
        // Try field name "name" first
        if let Some(name_node) = node.child_by_field_name("name") {
            return self.node_text(name_node);
        }
        // For some grammars, name is just an identifier child
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "identifier" {
                return self.node_text(child);
            }
        }
        String::new()
    }

    fn find_body<'b>(&self, node: Node<'b>) -> Option<Node<'b>> {
        if let Some(body) = node.child_by_field_name("body") {
            return Some(body);
        }
        // tree-sitter cursor borrow makes .find() unusable — imperative loop required
        self.find_child_by_kind(node, "contract_body")
    }

    fn get_body_node<'b>(&self, node: Node<'b>) -> Option<Node<'b>> {
        // In tree-sitter-solidity, the function body is a `function_body` child node.
        // Interface/abstract functions end with `;` — no function_body present.
        self.find_child_by_kind(node, "function_body")
    }

    // tree-sitter's cursor borrow makes `.find()` unusable here — the cursor must
    // outlive the iterator but `.find()` consumes the iterator while the cursor is held.
    #[allow(clippy::manual_find)]
    fn find_child_by_kind<'b>(&self, node: Node<'b>, kind: &str) -> Option<Node<'b>> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == kind {
                return Some(child);
            }
        }
        None
    }

    fn get_body_prefix(&self, node: Node) -> Option<String> {
        self.get_body_node(node)
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| truncate_body_prefix(t).to_string())
    }

    fn extract_visibility(&self, node: Node) -> String {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            let text = self.node_text(child);
            match text.as_str() {
                "public" | "private" | "internal" | "external" => return text,
                _ => {}
            }
            // visibility_modifier or function_attributes node
            if child.kind() == "visibility" || child.kind() == "function_attributes" {
                let mut inner = child.walk();
                for attr in child.children(&mut inner) {
                    let t = self.node_text(attr);
                    match t.as_str() {
                        "public" | "private" | "internal" | "external" => return t,
                        _ => {}
                    }
                }
            }
        }
        "internal".to_string()
    }

    fn extract_contract_visibility(&self, _node: Node) -> String {
        // Contracts don't have visibility — they're always public at the EVM level
        "public".to_string()
    }

    fn is_abstract(&self, node: Node) -> bool {
        self.has_keyword(node, "abstract")
    }

    fn has_keyword(&self, node: Node, keyword: &str) -> bool {
        let text = self.node_text(node);
        text.contains(keyword)
    }

    fn extract_parameters(&self, node: Node) -> Vec<Parameter> {
        let mut params = Vec::new();
        // Parameters are direct children of the function node with kind "parameter".
        // They appear between '(' and ')' — just scan all direct children.
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "parameter" {
                self.extract_one_param(child, &mut params);
            }
        }
        params
    }

    fn extract_one_param(&self, param_node: Node, params: &mut Vec<Parameter>) {
        // Parameter node: type_name [memory/storage/calldata] identifier
        // The last identifier is the name; the type_name is the type.
        let type_name = param_node
            .child_by_field_name("type")
            .map(|n| self.node_text(n))
            .or_else(|| {
                // Fallback: find type_name child
                let mut found = None;
                let mut c = param_node.walk();
                for n in param_node.children(&mut c) {
                    if n.kind() == "type_name" {
                        found = Some(self.node_text(n));
                        break;
                    }
                }
                found
            });

        // Name is the last identifier child
        let name = {
            let mut last_id = String::new();
            let mut c = param_node.walk();
            for n in param_node.children(&mut c) {
                if n.kind() == "identifier" {
                    last_id = self.node_text(n);
                }
            }
            last_id
        };

        if name.is_empty() && type_name.is_none() {
            return;
        }

        let display_name = if name.is_empty() {
            // unnamed parameter (e.g., `uint256` with no name in returns)
            type_name.clone().unwrap_or_default()
        } else {
            name
        };

        let mut p = Parameter::new(&display_name);
        if let Some(t) = type_name {
            p.type_annotation = Some(t);
        }
        params.push(p);
    }

    fn extract_return_type(&self, node: Node) -> Option<String> {
        // In tree-sitter-solidity, returns clause is a `return_type_definition` child.
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "return_type_definition" {
                // Extract the parameter types from inside returns(...)
                let mut params = Vec::new();
                let mut inner = child.walk();
                for ret_param in child.children(&mut inner) {
                    if ret_param.kind() == "parameter" {
                        let mut type_text: Option<String> = None;
                        let mut c2 = ret_param.walk();
                        for n in ret_param.children(&mut c2) {
                            if n.kind() == "type_name" {
                                type_text = Some(self.node_text(n));
                                break;
                            }
                        }
                        if let Some(t) = type_text {
                            params.push(t);
                        }
                    }
                }
                if !params.is_empty() {
                    return Some(params.join(", "));
                }
                // Fallback: use the text between parens
                let text = self.node_text(child);
                if let Some(start) = text.find('(') {
                    if let Some(end) = text.rfind(')') {
                        return Some(text[start + 1..end].trim().to_string());
                    }
                }
            }
        }
        None
    }

    fn build_signature(&self, node: Node) -> String {
        // Take the first line of the function
        self.node_text(node)
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .to_string()
    }

    fn params_signature(&self, params: &[Parameter]) -> String {
        params
            .iter()
            .map(|p| {
                if let Some(ref t) = p.type_annotation {
                    format!("{} {}", t, p.name)
                } else {
                    p.name.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn extract_natspec(&self, node: Node) -> Option<String> {
        if let Some(prev) = node.prev_sibling() {
            let text = self.node_text(prev);
            if text.starts_with("///") || text.starts_with("/**") {
                return Some(text);
            }
        }
        None
    }

    fn calculate_complexity(&self, body: Node) -> ComplexityMetrics {
        let mut builder = ComplexityBuilder::new();
        self.visit_for_complexity(body, &mut builder);
        builder.build()
    }

    fn visit_for_complexity(&self, node: Node, builder: &mut ComplexityBuilder) {
        match node.kind() {
            "if_statement" => {
                builder.add_branch();
                builder.enter_scope();
            }
            "else_clause" => {
                builder.add_branch();
            }
            "for_statement" | "while_statement" | "do_while_statement" => {
                builder.add_loop();
                builder.enter_scope();
            }
            "try_statement" => {
                builder.add_exception_handler();
                builder.enter_scope();
            }
            "binary_expression" => {
                let text = self.node_text(node);
                if text.contains("&&") || text.contains("||") {
                    builder.add_logical_operator();
                }
            }
            "return_statement" => {
                builder.add_early_return();
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_for_complexity(child, builder);
        }

        match node.kind() {
            "if_statement"
            | "for_statement"
            | "while_statement"
            | "do_while_statement"
            | "try_statement" => {
                builder.exit_scope();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_visit(source: &[u8]) -> SolidityVisitor<'_> {
        use tree_sitter::Parser;

        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_solidity::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = SolidityVisitor::new(source);
        visitor.visit_node(tree.root_node());
        visitor
    }

    /// Dump AST for debugging. Run with: cargo test dump_ast -- --nocapture
    #[test]
    fn dump_ast() {
        let source = r#"// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import "./IERC20.sol";

abstract contract BaseToken {
    address internal owner;

    modifier onlyOwner() {
        require(msg.sender == owner);
        _;
    }
}

contract Token is BaseToken {
    string public name;
    uint256 public totalSupply;

    constructor(string memory _name, uint256 _supply) {
        name = _name;
        totalSupply = _supply;
        owner = msg.sender;
    }

    function transfer(address to, uint256 amount) public returns (bool) {
        return true;
    }

    function _burn(uint256 amount) internal {
        totalSupply -= amount;
    }

    receive() external payable {}

    fallback() external payable {}
}

interface IERC20 {
    function totalSupply() external view returns (uint256);
    function balanceOf(address account) external view returns (uint256);
}

library SafeMath {
    function add(uint256 a, uint256 b) internal pure returns (uint256) {
        return a + b;
    }
}
"#;
        let source_bytes = source.as_bytes();

        use tree_sitter::Parser;
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_solidity::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source_bytes, None).unwrap();

        fn dump(node: tree_sitter::Node, source: &[u8], indent: usize) {
            let text = node.utf8_text(source).unwrap_or("").chars().take(40).collect::<String>();
            let text = text.replace('\n', "\\n");
            println!(
                "{}{} [{}-{}] {:?}",
                " ".repeat(indent * 2),
                node.kind(),
                node.start_position().row + 1,
                node.end_position().row + 1,
                text
            );
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                dump(child, source, indent + 1);
            }
        }

        dump(tree.root_node(), source_bytes, 0);

        let visitor = parse_and_visit(source_bytes);
        println!("\n=== Extracted ===");
        println!("Classes: {:?}", visitor.classes.iter().map(|c| &c.name).collect::<Vec<_>>());
        println!("Traits: {:?}", visitor.traits.iter().map(|t| &t.name).collect::<Vec<_>>());
        println!("Imports: {:?}", visitor.imports.iter().map(|i| &i.imported).collect::<Vec<_>>());
        println!("Functions: {:?}", visitor.functions.iter().map(|f| &f.name).collect::<Vec<_>>());
        for c in &visitor.classes {
            println!("  {} methods: {:?}", c.name, c.methods.iter().map(|m| &m.name).collect::<Vec<_>>());
        }
        for t in &visitor.traits {
            println!("  {} methods: {:?}", t.name, t.required_methods.iter().map(|m| &m.name).collect::<Vec<_>>());
        }
    }

    #[test]
    fn test_visitor_contract_extraction() {
        let source = b"// SPDX-License-Identifier: MIT\npragma solidity ^0.8.0;\n\ncontract MyContract {\n    function foo() public {}\n}\n";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "MyContract");
        assert!(!visitor.classes[0].methods.is_empty());
    }

    #[test]
    fn test_visitor_interface_extraction() {
        let source = b"// SPDX-License-Identifier: MIT\npragma solidity ^0.8.0;\n\ninterface IToken {\n    function totalSupply() external view returns (uint256);\n}\n";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.traits.len(), 1);
        assert_eq!(visitor.traits[0].name, "IToken");
    }

    #[test]
    fn test_visitor_import_extraction() {
        let source = b"// SPDX-License-Identifier: MIT\npragma solidity ^0.8.0;\n\nimport \"./IERC20.sol\";\n";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "./IERC20.sol");
    }
}
