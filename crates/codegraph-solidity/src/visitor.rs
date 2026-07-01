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

        // Track whether we are inside `{ ... }`: in this grammar the named-import
        // identifiers are direct children of `import_directive` between the braces,
        // while a bare identifier outside the braces is an `as` alias.
        let mut in_braces = false;
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "{" => in_braces = true,
                "}" => in_braces = false,
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
                "identifier" | "import_specifier" => {
                    if in_braces {
                        // import { A, B } from "path" — a named symbol
                        symbols.push(self.node_text(child));
                    } else {
                        // A bare identifier outside braces is an `as` alias
                        alias = Some(self.node_text(child));
                    }
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
                "fallback_receive_definition"
                | "receive_function_definition"
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
        let complexity = self
            .get_body_node(node)
            .map(|b| self.calculate_complexity(b));

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
        let complexity = self
            .get_body_node(node)
            .map(|b| self.calculate_complexity(b));

        let class_name = self
            .current_class
            .clone()
            .unwrap_or_else(|| "unknown".to_string());

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
        let complexity = self
            .get_body_node(node)
            .map(|b| self.calculate_complexity(b));

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
            "if_statement" | "for_statement" | "while_statement" | "do_while_statement"
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
            let text = node
                .utf8_text(source)
                .unwrap_or("")
                .chars()
                .take(40)
                .collect::<String>();
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
        println!(
            "Classes: {:?}",
            visitor.classes.iter().map(|c| &c.name).collect::<Vec<_>>()
        );
        println!(
            "Traits: {:?}",
            visitor.traits.iter().map(|t| &t.name).collect::<Vec<_>>()
        );
        println!(
            "Imports: {:?}",
            visitor
                .imports
                .iter()
                .map(|i| &i.imported)
                .collect::<Vec<_>>()
        );
        println!(
            "Functions: {:?}",
            visitor
                .functions
                .iter()
                .map(|f| &f.name)
                .collect::<Vec<_>>()
        );
        for c in &visitor.classes {
            println!(
                "  {} methods: {:?}",
                c.name,
                c.methods.iter().map(|m| &m.name).collect::<Vec<_>>()
            );
        }
        for t in &visitor.traits {
            println!(
                "  {} methods: {:?}",
                t.name,
                t.required_methods
                    .iter()
                    .map(|m| &m.name)
                    .collect::<Vec<_>>()
            );
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

    #[test]
    fn test_library_extraction() {
        let source = b"pragma solidity ^0.8.0;\n\nlibrary SafeMath {\n    function add(uint256 a, uint256 b) internal pure returns (uint256) {\n        return a + b;\n    }\n}\n";
        let visitor = parse_and_visit(source);

        // Library maps to a ClassEntity tagged with the "library" attribute.
        assert_eq!(visitor.classes.len(), 1);
        let lib = &visitor.classes[0];
        assert_eq!(lib.name, "SafeMath");
        assert_eq!(lib.visibility, "public");
        assert!(lib.attributes.contains(&"library".to_string()));
        assert!(!lib.is_abstract);
        // Its function becomes a method, not a top-level free function.
        assert!(visitor.functions.is_empty());
        assert_eq!(lib.methods.len(), 1);
        assert_eq!(lib.methods[0].name, "add");
    }

    #[test]
    fn test_abstract_contract_flag() {
        let source = b"pragma solidity ^0.8.0;\n\nabstract contract Base {\n    function foo() public virtual;\n}\n";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert!(visitor.classes[0].is_abstract);
        // A body-less virtual function is flagged abstract.
        let foo = &visitor.classes[0].methods[0];
        assert_eq!(foo.name, "foo");
        assert!(foo.is_abstract);
        assert!(foo.complexity.is_none());
    }

    #[test]
    fn test_constructor_extraction() {
        let source = b"pragma solidity ^0.8.0;\n\ncontract Token {\n    constructor(uint256 supply) {\n        _supply = supply;\n    }\n}\n";
        let visitor = parse_and_visit(source);

        let ctor = visitor.classes[0]
            .methods
            .iter()
            .find(|m| m.name == "constructor")
            .expect("constructor method present");
        // Constructor's return type and parent_class are the enclosing contract name.
        assert_eq!(ctor.return_type.as_deref(), Some("Token"));
        assert_eq!(ctor.parent_class.as_deref(), Some("Token"));
        assert_eq!(ctor.parameters.len(), 1);
        assert_eq!(ctor.parameters[0].name, "supply");
    }

    #[test]
    fn test_modifier_extraction() {
        let source = b"pragma solidity ^0.8.0;\n\ncontract Owned {\n    modifier onlyOwner() {\n        require(msg.sender == owner);\n        _;\n    }\n}\n";
        let visitor = parse_and_visit(source);

        let modifier = visitor.classes[0]
            .methods
            .iter()
            .find(|m| m.name == "onlyOwner")
            .expect("modifier method present");
        assert!(modifier.attributes.contains(&"modifier".to_string()));
        assert_eq!(modifier.visibility, "internal");
        assert!(modifier.signature.starts_with("modifier onlyOwner("));
    }

    #[test]
    fn test_special_receive_and_fallback() {
        let source = b"pragma solidity ^0.8.0;\n\ncontract Wallet {\n    receive() external payable {}\n    fallback() external payable {}\n}\n";
        let visitor = parse_and_visit(source);

        let names: Vec<&str> = visitor.classes[0]
            .methods
            .iter()
            .map(|m| m.name.as_str())
            .collect();
        assert!(names.contains(&"receive"));
        assert!(names.contains(&"fallback"));
    }

    #[test]
    fn test_top_level_free_function() {
        let source = b"pragma solidity ^0.8.0;\n\nfunction helper(uint256 x) pure returns (uint256) {\n    return x + 1;\n}\n";
        let visitor = parse_and_visit(source);

        // Top-level (no enclosing contract) functions land in functions, not classes.
        assert_eq!(visitor.functions.len(), 1);
        let f = &visitor.functions[0];
        assert_eq!(f.name, "helper");
        assert!(f.parent_class.is_none());
        assert_eq!(f.parameters.len(), 1);
        assert_eq!(f.parameters[0].name, "x");
        assert_eq!(f.return_type.as_deref(), Some("uint256"));
    }

    #[test]
    fn test_import_named_symbols() {
        let source =
            b"pragma solidity ^0.8.0;\n\nimport { IERC20, IERC721 } from \"./tokens.sol\";\n";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 1);
        let imp = &visitor.imports[0];
        assert_eq!(imp.imported, "./tokens.sol");
        assert!(imp.symbols.contains(&"IERC20".to_string()));
        assert!(imp.symbols.contains(&"IERC721".to_string()));
        // Brace-delimited names are symbols, not an alias.
        assert!(imp.alias.is_none());
    }

    #[test]
    fn test_import_alias() {
        let source = b"pragma solidity ^0.8.0;\n\nimport \"./token.sol\" as Tok;\n";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 1);
        let imp = &visitor.imports[0];
        assert_eq!(imp.imported, "./token.sol");
        // An identifier outside braces is captured as the `as` alias, not a symbol.
        assert_eq!(imp.alias.as_deref(), Some("Tok"));
        assert!(imp.symbols.is_empty());
    }

    #[test]
    fn test_function_visibility_public() {
        let source = b"pragma solidity ^0.8.0;\n\ncontract C {\n    function pub() public {}\n    function priv() private {}\n}\n";
        let visitor = parse_and_visit(source);

        let methods = &visitor.classes[0].methods;
        let pub_fn = methods.iter().find(|m| m.name == "pub").unwrap();
        let priv_fn = methods.iter().find(|m| m.name == "priv").unwrap();
        assert_eq!(pub_fn.visibility, "public");
        assert_eq!(priv_fn.visibility, "private");
    }

    #[test]
    fn test_function_complexity_branches() {
        let source = b"pragma solidity ^0.8.0;\n\ncontract C {\n    function branchy(uint256 x) public returns (uint256) {\n        if (x > 0) {\n            return 1;\n        } else {\n            return 2;\n        }\n    }\n}\n";
        let visitor = parse_and_visit(source);

        let f = &visitor.classes[0].methods[0];
        let complexity = f.complexity.as_ref().expect("body yields complexity");
        // An if/else branch raises cyclomatic complexity above the base of 1.
        assert!(complexity.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_natspec_doc_comment() {
        let source = b"pragma solidity ^0.8.0;\n\ncontract C {\n    /// @notice does a thing\n    function documented() public {}\n}\n";
        let visitor = parse_and_visit(source);

        let f = &visitor.classes[0].methods[0];
        assert_eq!(f.name, "documented");
        assert!(f
            .doc_comment
            .as_deref()
            .map(|d| d.contains("@notice"))
            .unwrap_or(false));
    }

    #[test]
    fn test_interface_required_methods() {
        let source = b"pragma solidity ^0.8.0;\n\ninterface IToken {\n    function totalSupply() external view returns (uint256);\n    function balanceOf(address a) external view returns (uint256);\n}\n";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.traits.len(), 1);
        let names: Vec<&str> = visitor.traits[0]
            .required_methods
            .iter()
            .map(|m| m.name.as_str())
            .collect();
        assert!(names.contains(&"totalSupply"));
        assert!(names.contains(&"balanceOf"));
    }

    #[test]
    fn test_empty_source_yields_nothing() {
        let visitor =
            parse_and_visit(b"// SPDX-License-Identifier: MIT\npragma solidity ^0.8.0;\n");
        assert!(visitor.classes.is_empty());
        assert!(visitor.traits.is_empty());
        assert!(visitor.functions.is_empty());
        assert!(visitor.imports.is_empty());
    }

    #[test]
    fn test_multiple_contracts_extracted() {
        let source = b"pragma solidity ^0.8.0;\n\ncontract A {\n    function a() public {}\n}\n\ncontract B {\n    function b() public {}\n}\n";
        let visitor = parse_and_visit(source);

        let names: Vec<&str> = visitor.classes.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["A", "B"]);
        // Each method is routed to its own enclosing contract via parent_class.
        assert_eq!(
            visitor.classes[0].methods[0].parent_class.as_deref(),
            Some("A")
        );
        assert_eq!(
            visitor.classes[1].methods[0].parent_class.as_deref(),
            Some("B")
        );
    }

    #[test]
    fn test_contract_line_bounds_are_one_based() {
        // Contract spans source lines 3..5 (1-based).
        let source = b"pragma solidity ^0.8.0;\n\ncontract C {\n    uint256 x;\n}\n";
        let visitor = parse_and_visit(source);

        let c = &visitor.classes[0];
        assert_eq!(c.line_start, 3);
        assert_eq!(c.line_end, 5);
        // Contracts have no visibility keyword — always reported public.
        assert_eq!(c.visibility, "public");
        // Fields are not extracted by this visitor.
        assert!(c.fields.is_empty());
    }

    #[test]
    fn test_wildcard_star_import_not_flagged_wildcard() {
        // `import * as X from "path"` has no `import_wildcard` node in this grammar:
        // the `*` is a bare token and `X` parses as an outside-braces `as` alias.
        let source = b"pragma solidity ^0.8.0;\n\nimport * as Utils from \"./utils.sol\";\n";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 1);
        let imp = &visitor.imports[0];
        assert_eq!(imp.imported, "./utils.sol");
        assert_eq!(imp.alias.as_deref(), Some("Utils"));
        assert!(!imp.is_wildcard);
        assert!(imp.symbols.is_empty());
    }

    #[test]
    fn test_import_importer_is_file() {
        let source = b"pragma solidity ^0.8.0;\n\nimport \"./a.sol\";\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports[0].importer, "file");
    }

    #[test]
    fn test_function_external_and_default_visibility() {
        let source = b"pragma solidity ^0.8.0;\n\ncontract C {\n    function ext() external {}\n    function plain() {}\n}\n";
        let visitor = parse_and_visit(source);

        let methods = &visitor.classes[0].methods;
        let ext = methods.iter().find(|m| m.name == "ext").unwrap();
        let plain = methods.iter().find(|m| m.name == "plain").unwrap();
        assert_eq!(ext.visibility, "external");
        // No visibility keyword defaults to "internal".
        assert_eq!(plain.visibility, "internal");
    }

    #[test]
    fn test_multiple_return_types_joined() {
        let source = b"pragma solidity ^0.8.0;\n\ncontract C {\n    function pair() public returns (uint256, bool) {\n        return (1, true);\n    }\n}\n";
        let visitor = parse_and_visit(source);

        let f = &visitor.classes[0].methods[0];
        // Multiple return params are joined with ", ".
        assert_eq!(f.return_type.as_deref(), Some("uint256, bool"));
    }

    #[test]
    fn test_parameter_type_annotation_captured() {
        let source = b"pragma solidity ^0.8.0;\n\ncontract C {\n    function f(address to, uint256 amount) public {}\n}\n";
        let visitor = parse_and_visit(source);

        let params = &visitor.classes[0].methods[0].parameters;
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].name, "to");
        assert_eq!(params[0].type_annotation.as_deref(), Some("address"));
        assert_eq!(params[1].name, "amount");
        assert_eq!(params[1].type_annotation.as_deref(), Some("uint256"));
    }

    #[test]
    fn test_function_body_prefix_present() {
        let source = b"pragma solidity ^0.8.0;\n\ncontract C {\n    function f() public {\n        uint256 x = 1;\n    }\n}\n";
        let visitor = parse_and_visit(source);

        let f = &visitor.classes[0].methods[0];
        let prefix = f.body_prefix.as_deref().expect("body prefix present");
        assert!(prefix.contains("uint256 x"));
    }

    #[test]
    fn test_virtual_function_with_body_is_abstract() {
        // `virtual` marks a function abstract even when it has a body.
        let source = b"pragma solidity ^0.8.0;\n\ncontract C {\n    function f() public virtual {\n        return;\n    }\n}\n";
        let visitor = parse_and_visit(source);

        let f = &visitor.classes[0].methods[0];
        assert!(f.is_abstract);
    }

    #[test]
    fn test_complexity_loop_counted() {
        let source = b"pragma solidity ^0.8.0;\n\ncontract C {\n    function f(uint256 n) public {\n        for (uint256 i = 0; i < n; i++) {\n            n += i;\n        }\n    }\n}\n";
        let visitor = parse_and_visit(source);

        let complexity = visitor.classes[0].methods[0]
            .complexity
            .as_ref()
            .expect("body yields complexity");
        assert!(complexity.loops >= 1);
    }

    #[test]
    fn test_complexity_logical_operator_counted() {
        let source = b"pragma solidity ^0.8.0;\n\ncontract C {\n    function f(uint256 x) public {\n        if (x > 0 && x < 10) {\n            x = 1;\n        }\n    }\n}\n";
        let visitor = parse_and_visit(source);

        let complexity = visitor.classes[0].methods[0]
            .complexity
            .as_ref()
            .expect("body yields complexity");
        assert!(complexity.logical_operators >= 1);
    }

    #[test]
    fn test_complexity_try_and_early_return_counted() {
        let source = b"pragma solidity ^0.8.0;\n\ncontract C {\n    function f() public returns (uint256) {\n        try this.f() returns (uint256 r) {\n            return r;\n        } catch {\n            return 0;\n        }\n    }\n}\n";
        let visitor = parse_and_visit(source);

        let complexity = visitor.classes[0].methods[0]
            .complexity
            .as_ref()
            .expect("body yields complexity");
        assert!(complexity.exception_handlers >= 1);
        assert!(complexity.early_returns >= 1);
    }

    #[test]
    fn test_constructor_signature_has_typed_params() {
        let source = b"pragma solidity ^0.8.0;\n\ncontract C {\n    constructor(uint256 supply, address owner) {}\n}\n";
        let visitor = parse_and_visit(source);

        let ctor = visitor.classes[0]
            .methods
            .iter()
            .find(|m| m.name == "constructor")
            .unwrap();
        assert_eq!(ctor.signature, "constructor(uint256 supply, address owner)");
    }

    #[test]
    fn test_modifier_with_parameters() {
        let source = b"pragma solidity ^0.8.0;\n\ncontract C {\n    modifier only(address who) {\n        require(msg.sender == who);\n        _;\n    }\n}\n";
        let visitor = parse_and_visit(source);

        let m = visitor.classes[0]
            .methods
            .iter()
            .find(|m| m.name == "only")
            .unwrap();
        assert_eq!(m.parameters.len(), 1);
        assert_eq!(m.parameters[0].name, "who");
        assert_eq!(m.signature, "modifier only(address who)");
    }

    #[test]
    fn test_natspec_block_comment() {
        let source = b"pragma solidity ^0.8.0;\n\ncontract C {\n    /** @dev block doc */\n    function f() public {}\n}\n";
        let visitor = parse_and_visit(source);

        let f = &visitor.classes[0].methods[0];
        assert!(f
            .doc_comment
            .as_deref()
            .map(|d| d.contains("@dev"))
            .unwrap_or(false));
    }

    #[test]
    fn test_special_fn_has_no_complexity() {
        let source =
            b"pragma solidity ^0.8.0;\n\ncontract C {\n    receive() external payable {}\n}\n";
        let visitor = parse_and_visit(source);

        let recv = visitor.classes[0]
            .methods
            .iter()
            .find(|m| m.name == "receive")
            .unwrap();
        // extract_special_fn hardcodes complexity to None regardless of body.
        assert!(recv.complexity.is_none());
        assert_eq!(recv.visibility, "external");
        assert_eq!(recv.signature, "receive() external");
    }

    #[test]
    fn test_library_function_pure_visibility_internal() {
        let source = b"pragma solidity ^0.8.0;\n\nlibrary L {\n    function helper() internal pure returns (uint256) {\n        return 1;\n    }\n}\n";
        let visitor = parse_and_visit(source);

        let f = &visitor.classes[0].methods[0];
        assert_eq!(f.visibility, "internal");
        assert_eq!(f.return_type.as_deref(), Some("uint256"));
        assert!(!f.is_abstract);
    }
}
