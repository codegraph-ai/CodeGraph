// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting Julia entities
//!
//! Node kinds observed via tree-sitter-julia 0.23.1:
//!   function_definition → (signature (call_expression name args) | (typed_expression (call_expression name args) return_type))
//!   struct_definition   → (type_head (identifier)) fields...
//!   abstract_definition → (type_head (identifier)) ...
//!   module_definition   → name:(identifier) body...
//!   using_statement     → (identifier)
//!   import_statement    → (selected_import (identifier module) (import_alias (identifier) (identifier)))
//!   export_statement    → (identifier)...
//!   call_expression     → (identifier) (argument_list ...)

use codegraph_parser_api::{
    truncate_body_prefix, CallRelation, ClassEntity, ComplexityBuilder, ComplexityMetrics,
    FunctionEntity, ImportRelation, Parameter, TraitEntity,
};
use tree_sitter::Node;

pub(crate) struct JuliaVisitor<'a> {
    pub source: &'a [u8],
    pub functions: Vec<FunctionEntity>,
    pub imports: Vec<ImportRelation>,
    pub calls: Vec<CallRelation>,
    /// Structs / mutable structs → ClassEntity
    pub classes: Vec<ClassEntity>,
    /// Abstract types → TraitEntity
    pub traits: Vec<TraitEntity>,
    current_function: Option<String>,
    /// Names exported via `export` — determines visibility
    exported_names: std::collections::HashSet<String>,
}

impl<'a> JuliaVisitor<'a> {
    pub fn new(source: &'a [u8]) -> Self {
        Self {
            source,
            functions: Vec::new(),
            imports: Vec::new(),
            calls: Vec::new(),
            classes: Vec::new(),
            traits: Vec::new(),
            current_function: None,
            exported_names: std::collections::HashSet::new(),
        }
    }

    fn node_text(&self, node: Node) -> String {
        node.utf8_text(self.source).unwrap_or("").to_string()
    }

    pub fn visit_node(&mut self, node: Node) {
        match node.kind() {
            "function_definition" | "short_function_definition" => {
                self.visit_function_definition(node);
                return;
            }
            "struct_definition" => {
                self.visit_struct_definition(node);
                return;
            }
            "abstract_definition" => {
                self.visit_abstract_definition(node);
                return;
            }
            "module_definition" => {
                self.visit_module(node);
                return;
            }
            "using_statement" => {
                self.visit_using(node);
            }
            "import_statement" => {
                self.visit_import(node);
            }
            "export_statement" => {
                self.visit_export(node);
            }
            "call_expression" => {
                self.visit_call_expression(node);
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child);
        }
    }

    fn visit_module(&mut self, node: Node) {
        // Recurse into module body without tracking the module as a separate entity
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child);
        }
    }

    // ─── Functions ────────────────────────────────────────────────────────────

    fn visit_function_definition(&mut self, node: Node) {
        // The function name and parameters live inside a `signature` child.
        // signature contains either:
        //   (call_expression (identifier name) (argument_list params...))
        //   (typed_expression (call_expression (identifier name) (argument_list params...)) (identifier return_type))
        let sig_node = match self.find_child_by_kind(node, "signature") {
            Some(n) => n,
            None => return,
        };

        let (name, params, return_type) = self.parse_signature(sig_node);
        if name.is_empty() {
            return;
        }

        let full_text = self.node_text(node);
        let signature = full_text.lines().next().unwrap_or("").trim().to_string();

        let doc_comment = self.extract_doc_comment(node);

        // Body: everything after the signature
        let body_prefix = self.extract_body_prefix_from_function(node);
        let complexity = self.extract_complexity_from_function(node);

        let visibility = if self.exported_names.contains(&name) {
            "public"
        } else {
            "private"
        }
        .to_string();

        let func = FunctionEntity {
            name: name.clone(),
            signature,
            visibility,
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_async: false,
            is_test: false,
            is_static: false,
            is_abstract: false,
            parameters: params,
            return_type,
            doc_comment,
            attributes: Vec::new(),
            parent_class: None,
            complexity,
            body_prefix,
        };

        self.functions.push(func);

        let previous_function = self.current_function.take();
        self.current_function = Some(name);

        // Visit body children for call tracking
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() != "signature" {
                self.visit_body_for_calls(child);
            }
        }

        self.current_function = previous_function;
    }

    /// Parse a `signature` node → (function_name, parameters, return_type)
    fn parse_signature(&self, sig: Node) -> (String, Vec<Parameter>, Option<String>) {
        // Check for typed signature (with return type annotation):
        //   (typed_expression (call_expression ...) (identifier return_type))
        // or plain:
        //   (call_expression (identifier name) (argument_list ...))
        let mut cursor = sig.walk();
        for child in sig.children(&mut cursor) {
            match child.kind() {
                "typed_expression" => {
                    // First child is the call, second is the return type
                    let (name, params) = self.parse_call_signature(child);
                    let return_type = self.extract_return_type_from_typed_expr(child);
                    return (name, params, return_type);
                }
                "call_expression" => {
                    let (name, params) = self.parse_call_node(child);
                    return (name, params, None);
                }
                "identifier" => {
                    // Simple `function f end` with no params
                    return (self.node_text(child), Vec::new(), None);
                }
                _ => {}
            }
        }
        (String::new(), Vec::new(), None)
    }

    /// From a `typed_expression` in a signature, get the call_expression part
    fn parse_call_signature(&self, typed: Node) -> (String, Vec<Parameter>) {
        let mut cursor = typed.walk();
        for child in typed.children(&mut cursor) {
            if child.kind() == "call_expression" {
                return self.parse_call_node(child);
            }
        }
        (String::new(), Vec::new())
    }

    /// From a `typed_expression`, extract the type after `::` (last identifier sibling)
    fn extract_return_type_from_typed_expr(&self, typed: Node) -> Option<String> {
        // typed_expression = (call_expression ...) (identifier)
        // The identifier at the end is the return type
        let mut last_id: Option<String> = None;
        let mut cursor = typed.walk();
        for child in typed.children(&mut cursor) {
            if child.kind() == "identifier" {
                last_id = Some(self.node_text(child));
            }
        }
        last_id
    }

    /// From a `call_expression`, extract (name, parameters)
    fn parse_call_node(&self, call: Node) -> (String, Vec<Parameter>) {
        let mut cursor = call.walk();
        let mut name = String::new();
        let mut params = Vec::new();

        for child in call.children(&mut cursor) {
            match child.kind() {
                "identifier" => {
                    if name.is_empty() {
                        name = self.node_text(child);
                    }
                }
                "argument_list" => {
                    params = self.extract_params_from_arg_list(child);
                }
                _ => {}
            }
        }
        (name, params)
    }

    fn extract_params_from_arg_list(&self, args: Node) -> Vec<Parameter> {
        let mut params = Vec::new();
        let mut cursor = args.walk();
        for child in args.children(&mut cursor) {
            match child.kind() {
                "identifier" => {
                    params.push(Parameter::new(self.node_text(child)));
                }
                "typed_expression" => {
                    // `name::Type` — first identifier is the name
                    let inner_name = self.first_identifier(child);
                    if !inner_name.is_empty() {
                        params.push(Parameter::new(inner_name));
                    }
                }
                "optional_parameter" | "default_parameter" => {
                    // `name=default` — first identifier is the name
                    let inner_name = self.first_identifier(child);
                    if !inner_name.is_empty() {
                        params.push(Parameter::new(inner_name));
                    }
                }
                "splat_expression" => {
                    // `args...`
                    let text = self.node_text(child);
                    let name = text.trim_end_matches("...").to_string();
                    params.push(Parameter::new(name).variadic());
                }
                _ => {}
            }
        }
        params
    }

    fn first_identifier(&self, node: Node) -> String {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "identifier" {
                return self.node_text(child);
            }
        }
        String::new()
    }

    fn extract_body_prefix_from_function(&self, node: Node) -> Option<String> {
        // Everything that's not signature/keyword — collect body text
        let mut cursor = node.walk();
        let body_parts: Vec<&str> = node
            .children(&mut cursor)
            .filter(|c| !matches!(c.kind(), "signature" | "function" | "end"))
            .filter_map(|c| c.utf8_text(self.source).ok())
            .collect();

        let body = body_parts.join("\n");
        if body.trim().is_empty() {
            None
        } else {
            Some(truncate_body_prefix(body.trim()).to_string())
        }
    }

    fn extract_complexity_from_function(&self, node: Node) -> Option<ComplexityMetrics> {
        let mut builder = ComplexityBuilder::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if !matches!(child.kind(), "signature" | "function" | "end") {
                self.visit_for_complexity(child, &mut builder);
            }
        }
        Some(builder.build())
    }

    // ─── Structs ──────────────────────────────────────────────────────────────

    fn visit_struct_definition(&mut self, node: Node) {
        // struct_definition → (type_head (identifier)) typed_expression...
        // mutable struct starts with "mutable struct" in source text
        let name = self.extract_type_name(node);
        if name.is_empty() {
            return;
        }

        let text = self.node_text(node);
        let is_mutable = text.trim_start().starts_with("mutable");

        let visibility = if self.exported_names.contains(&name) {
            "public"
        } else {
            "private"
        }
        .to_string();

        let doc_comment = self.extract_doc_comment(node);

        let mut class = ClassEntity::new(
            name,
            node.start_position().row + 1,
            node.end_position().row + 1,
        )
        .with_visibility(visibility);

        if is_mutable {
            class.attributes.push("mutable".to_string());
        }
        if let Some(doc) = doc_comment {
            class = class.with_doc(doc);
        }

        self.classes.push(class);
    }

    // ─── Abstract types ───────────────────────────────────────────────────────

    fn visit_abstract_definition(&mut self, node: Node) {
        let name = self.extract_type_name(node);
        if name.is_empty() {
            return;
        }

        let visibility = if self.exported_names.contains(&name) {
            "public"
        } else {
            "private"
        }
        .to_string();

        let doc_comment = self.extract_doc_comment(node);

        let mut trait_entity = TraitEntity::new(
            name,
            node.start_position().row + 1,
            node.end_position().row + 1,
        )
        .with_visibility(visibility);

        if let Some(doc) = doc_comment {
            trait_entity = trait_entity.with_doc(doc);
        }

        self.traits.push(trait_entity);
    }

    /// Extract the type name from struct_definition or abstract_definition.
    /// AST: (type_head (identifier name)) or children with identifier
    fn extract_type_name(&self, node: Node) -> String {
        // Try type_head child first
        if let Some(type_head) = self.find_child_by_kind(node, "type_head") {
            return self.first_identifier(type_head);
        }
        // Fallback: first identifier child
        self.first_identifier(node)
    }

    // ─── Imports ──────────────────────────────────────────────────────────────

    fn visit_using(&mut self, node: Node) {
        // `using DataFrames`  → (using_statement (identifier))
        // `using A, B, C`     → (using_statement (identifier) (identifier) ...)
        // `using A: x, y`     → (using_statement (selected_import (identifier) (identifier)...))
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "identifier" => {
                    let name = self.node_text(child);
                    if !name.is_empty() {
                        self.imports.push(ImportRelation {
                            importer: "main".to_string(),
                            imported: name,
                            symbols: Vec::new(),
                            is_wildcard: false,
                            alias: None,
                        });
                    }
                }
                "selected_import" => {
                    self.extract_selected_import(child);
                }
                _ => {}
            }
        }
    }

    fn visit_import(&mut self, node: Node) {
        // `import JSON`                → (import_statement (identifier))
        // `import JSON: parse`         → (import_statement (selected_import (identifier) (identifier)))
        // `import JSON: parse as alias`→ (import_statement (selected_import (identifier) (import_alias ...)))
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "identifier" => {
                    let name = self.node_text(child);
                    if !name.is_empty() {
                        self.imports.push(ImportRelation {
                            importer: "main".to_string(),
                            imported: name,
                            symbols: Vec::new(),
                            is_wildcard: false,
                            alias: None,
                        });
                    }
                }
                "selected_import" => {
                    self.extract_selected_import(child);
                }
                _ => {}
            }
        }
    }

    fn extract_selected_import(&mut self, node: Node) {
        // (selected_import (identifier module) (identifier sym1) (import_alias (identifier sym) (identifier alias)) ...)
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();

        let module = children
            .first()
            .filter(|n| n.kind() == "identifier")
            .map(|n| self.node_text(*n))
            .unwrap_or_default();

        if module.is_empty() {
            return;
        }

        let mut symbols = Vec::new();
        for child in &children[1..] {
            match child.kind() {
                "identifier" => {
                    symbols.push(self.node_text(*child));
                }
                "import_alias" => {
                    // (import_alias (identifier original) (identifier alias))
                    let original = self.first_identifier(*child);
                    if !original.is_empty() {
                        symbols.push(original);
                    }
                }
                _ => {}
            }
        }

        self.imports.push(ImportRelation {
            importer: "main".to_string(),
            imported: module,
            symbols,
            is_wildcard: false,
            alias: None,
        });
    }

    // ─── Exports ──────────────────────────────────────────────────────────────

    fn visit_export(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "identifier" {
                self.exported_names.insert(self.node_text(child));
            }
        }
    }

    // ─── Calls ────────────────────────────────────────────────────────────────

    fn visit_call_expression(&mut self, node: Node) {
        if let Some(ref caller) = self.current_function.clone() {
            if let Some(func_node) = node.child(0) {
                let callee = self.node_text(func_node);
                if !callee.is_empty() {
                    self.calls.push(CallRelation {
                        caller: caller.clone(),
                        callee,
                        call_site_line: node.start_position().row + 1,
                        is_direct: true,
                        struct_type: None,
                        field_name: None,
                    });
                }
            }
        }
    }

    fn visit_body_for_calls(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "call_expression" {
                self.visit_call_expression(child);
            }
            self.visit_body_for_calls(child);
        }
    }

    // ─── Helpers ──────────────────────────────────────────────────────────────

    fn find_child_by_kind<'b>(&self, node: Node<'b>, kind: &str) -> Option<Node<'b>> {
        let mut cursor = node.walk();
        let mut result = None;
        for child in node.children(&mut cursor) {
            if child.kind() == kind {
                result = Some(child);
                break;
            }
        }
        result
    }

    fn extract_doc_comment(&self, node: Node) -> Option<String> {
        if let Some(prev) = node.prev_sibling() {
            match prev.kind() {
                "line_comment" | "comment" => {
                    return Some(self.node_text(prev));
                }
                // Julia triple-quoted docstrings appear as string literals
                "string_literal" | "string" => {
                    let text = self.node_text(prev);
                    if text.starts_with('"') {
                        return Some(text);
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn visit_for_complexity(&self, node: Node, builder: &mut ComplexityBuilder) {
        match node.kind() {
            "if_statement" => {
                builder.add_branch();
                builder.enter_scope();
            }
            "elseif_clause" => {
                builder.add_branch();
            }
            "else_clause" => {
                builder.add_branch();
            }
            "for_statement" | "while_statement" => {
                builder.add_loop();
                builder.enter_scope();
            }
            "ternary_expression" => {
                builder.add_branch();
            }
            "binary_expression" => {
                let text = self.node_text(node);
                if text.contains(" && ") || text.contains(" || ") {
                    builder.add_logical_operator();
                }
            }
            "try_statement" => {
                builder.add_exception_handler();
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_for_complexity(child, builder);
        }

        match node.kind() {
            "if_statement" | "for_statement" | "while_statement" => {
                builder.exit_scope();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_visit(source: &[u8]) -> JuliaVisitor<'_> {
        use tree_sitter::Parser;

        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_julia::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = JuliaVisitor::new(source);
        visitor.visit_node(tree.root_node());
        visitor
    }

    #[test]
    fn test_visitor_function_extraction() {
        let source = b"function greet(name)\n    println(\"Hello, $name\")\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "greet");
    }

    #[test]
    fn test_visitor_function_with_typed_params() {
        let source =
            b"function create_user(name::String, email::String)::User\n    return User(name, email)\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "create_user");
        assert_eq!(visitor.functions[0].parameters.len(), 2);
        assert_eq!(visitor.functions[0].parameters[0].name, "name");
        assert_eq!(visitor.functions[0].parameters[1].name, "email");
        assert_eq!(
            visitor.functions[0].return_type,
            Some("User".to_string())
        );
    }

    #[test]
    fn test_visitor_struct_extraction() {
        let source = b"struct User\n    name::String\n    email::String\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "User");
    }

    #[test]
    fn test_visitor_import_extraction() {
        let source = b"using DataFrames\n";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "DataFrames");
    }

    #[test]
    fn test_visitor_import_selected() {
        let source = b"import JSON: parse as json_parse\n";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "JSON");
        assert!(!visitor.imports[0].symbols.is_empty());
    }

    #[test]
    fn test_visitor_full_module() {
        let source = br#"
module MyApp

using DataFrames
import JSON: parse as json_parse

struct User
    name::String
    email::String
end

function create_user(name::String, email::String)::User
    return User(name, email)
end

function greet(user::User)
    if isempty(user.name)
        println("Hello, stranger")
    else
        println("Hello, $(user.name)")
    end
end

end # module
"#;
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 2, "expected 2 functions");
        assert_eq!(visitor.classes.len(), 1, "expected 1 struct");
        assert_eq!(visitor.imports.len(), 2, "expected 2 imports");
    }
}
