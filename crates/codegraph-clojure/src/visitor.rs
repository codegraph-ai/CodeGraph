// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting Clojure entities

use codegraph_parser_api::{
    truncate_body_prefix, CallRelation, ClassEntity, ComplexityBuilder, FunctionEntity,
    ImportRelation, Parameter,
};
use tree_sitter::Node;

pub(crate) struct ClojureVisitor<'a> {
    pub source: &'a [u8],
    pub functions: Vec<FunctionEntity>,
    pub classes: Vec<ClassEntity>,
    pub imports: Vec<ImportRelation>,
    pub calls: Vec<CallRelation>,
    current_function: Option<String>,
    current_namespace: Option<String>,
}

impl<'a> ClojureVisitor<'a> {
    pub fn new(source: &'a [u8]) -> Self {
        Self {
            source,
            functions: Vec::new(),
            classes: Vec::new(),
            imports: Vec::new(),
            calls: Vec::new(),
            current_function: None,
            current_namespace: None,
        }
    }

    fn node_text(&self, node: Node) -> String {
        node.utf8_text(self.source).unwrap_or("").to_string()
    }

    pub fn visit_node(&mut self, node: Node) {
        // Clojure AST is S-expression based.
        // Top-level forms are "list_lit" nodes.
        // We inspect the first symbol child to determine form type.
        match node.kind() {
            "source_file" => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.visit_node(child);
                }
            }
            "list_lit" => {
                self.visit_list(node);
            }
            _ => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.visit_node(child);
                }
            }
        }
    }

    fn visit_list(&mut self, node: Node) {
        // Get the first symbol/keyword child to identify the form
        let first_sym = self.first_sym(node);

        match first_sym.as_deref() {
            Some("defn") | Some("defn-") => {
                self.visit_defn(node, first_sym.as_deref() == Some("defn-"));
            }
            Some("defprotocol") | Some("defrecord") | Some("deftype") => {
                self.visit_deftype(node, first_sym.as_deref().unwrap_or("defprotocol"));
            }
            Some("ns") => {
                self.visit_ns(node);
            }
            Some("require") | Some("use") | Some("import") => {
                self.visit_import_form(node, first_sym.as_deref().unwrap_or("require"));
            }
            _ => {
                // Descend into other lists looking for nested forms
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "list_lit" {
                        self.visit_list(child);
                    }
                }
            }
        }
    }

    /// Return the text of the first sym_lit child of a list node
    fn first_sym(&self, node: Node) -> Option<String> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "sym_lit" {
                return Some(self.node_text(child));
            }
        }
        None
    }

    fn visit_defn(&mut self, node: Node, is_private: bool) {
        // (defn name doc? [params] body...)
        // (defn- name doc? [params] body...)
        // Children (sym_lit): defn/defn- then name
        // Then optionally a str_lit (docstring), then vec_lit (params), then body forms

        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();

        // Find name: second sym_lit
        let mut sym_seen = 0usize;
        let mut name = String::new();
        let mut params_node: Option<Node> = None;
        let mut body_start_idx: Option<usize> = None;
        let mut doc_comment: Option<String> = None;

        for (i, child) in children.iter().enumerate() {
            match child.kind() {
                "sym_lit" => {
                    sym_seen += 1;
                    if sym_seen == 2 {
                        name = self.node_text(*child);
                    }
                }
                "str_lit" if sym_seen >= 2 && doc_comment.is_none() => {
                    doc_comment = Some(self.node_text(*child));
                }
                "vec_lit" if sym_seen >= 2 && params_node.is_none() => {
                    params_node = Some(*child);
                    body_start_idx = Some(i + 1);
                }
                _ => {}
            }
        }

        if name.is_empty() {
            return;
        }

        let parameters = params_node
            .map(|p| self.extract_params_from_vec(p))
            .unwrap_or_default();

        let signature = format!(
            "({} {} [{}])",
            if is_private { "defn-" } else { "defn" },
            name,
            parameters
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        );

        let body_prefix = body_start_idx.and_then(|start| {
            children[start..]
                .iter()
                .find(|c| !matches!(c.kind(), "comment"))
                .and_then(|n| n.utf8_text(self.source).ok())
                .filter(|t| !t.is_empty())
                .map(|t| truncate_body_prefix(t).to_string())
        });

        let complexity = params_node.map(|_| {
            let mut builder = ComplexityBuilder::new();
            // Calculate complexity from body forms
            if let Some(start) = body_start_idx {
                for child in &children[start..] {
                    self.visit_for_complexity(*child, &mut builder);
                }
            }
            builder.build()
        });

        let func = FunctionEntity {
            name: name.clone(),
            signature,
            visibility: if is_private { "private" } else { "public" }.to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_async: false,
            is_test: name.ends_with("-test") || name.starts_with("test-"),
            is_static: false,
            is_abstract: false,
            parameters,
            return_type: None,
            doc_comment,
            attributes: Vec::new(),
            parent_class: self.current_namespace.clone(),
            complexity,
            body_prefix,
        };

        self.functions.push(func);

        // Scan body for calls
        let previous_function = self.current_function.take();
        self.current_function = Some(name);
        if let Some(start) = body_start_idx {
            for child in &children[start..] {
                self.visit_for_calls(*child);
            }
        }
        self.current_function = previous_function;
    }

    fn visit_deftype(&mut self, node: Node, kind: &str) {
        // (defprotocol Name methods...)
        // (defrecord Name [fields] protocols...)
        // (deftype Name [fields] protocols...)
        let mut cursor = node.walk();
        let mut sym_seen = 0usize;
        let mut name = String::new();

        for child in node.children(&mut cursor) {
            if child.kind() == "sym_lit" {
                sym_seen += 1;
                if sym_seen == 2 {
                    name = self.node_text(child);
                    break;
                }
            }
        }

        if name.is_empty() {
            return;
        }

        let class = ClassEntity {
            name: name.clone(),
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_abstract: kind == "defprotocol",
            is_interface: kind == "defprotocol",
            base_classes: Vec::new(),
            implemented_traits: Vec::new(),
            methods: Vec::new(),
            fields: Vec::new(),
            doc_comment: None,
            attributes: vec![kind.to_string()],
            type_parameters: Vec::new(),
            body_prefix: None,
        };

        self.classes.push(class);
    }

    fn visit_ns(&mut self, node: Node) {
        // (ns my.namespace (:require [...]) (:use [...]) (:import [...]))
        let mut cursor = node.walk();
        let mut sym_seen = 0usize;
        let mut ns_name = String::new();

        for child in node.children(&mut cursor) {
            match child.kind() {
                "sym_lit" => {
                    sym_seen += 1;
                    if sym_seen == 2 {
                        ns_name = self.node_text(child);
                    }
                }
                "list_lit" if sym_seen >= 2 => {
                    // (:require ...), (:use ...), (:import ...)
                    self.visit_ns_clause(child);
                }
                _ => {}
            }
        }

        if !ns_name.is_empty() {
            self.current_namespace = Some(ns_name);
        }
    }

    fn visit_ns_clause(&mut self, node: Node) {
        // (:require [clojure.string :as str] clojure.set ...)
        let first = self.first_kw(node);
        match first.as_deref() {
            Some(":require") | Some(":use") | Some(":import") => {
                self.extract_ns_imports(node);
            }
            _ => {}
        }
    }

    fn first_kw(&self, node: Node) -> Option<String> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "kwd_lit" {
                return Some(self.node_text(child));
            }
        }
        None
    }

    fn extract_ns_imports(&mut self, node: Node) {
        // Collect module names from :require/:use/:import clauses
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "vec_lit" => {
                    // [clojure.string :as str] or [some.ns :refer [foo bar]]
                    let module = self.first_sym_in_vec(child);
                    if let Some(m) = module {
                        self.push_import(m, Vec::new());
                    }
                }
                "sym_lit" => {
                    // bare symbol: (require 'clojure.string) or in :import
                    let text = self.node_text(child);
                    if text != "require" && text != "use" && text != "import" {
                        self.push_import(text, Vec::new());
                    }
                }
                "list_lit" => {
                    // Java-style import group: (java.util Date List)
                    self.visit_import_group(child);
                }
                _ => {}
            }
        }
    }

    fn first_sym_in_vec(&self, node: Node) -> Option<String> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "sym_lit" {
                return Some(self.node_text(child));
            }
        }
        None
    }

    fn visit_import_group(&mut self, node: Node) {
        // (java.util Date List Map)
        let mut cursor = node.walk();
        let mut pkg: Option<String> = None;
        for child in node.children(&mut cursor) {
            if child.kind() == "sym_lit" {
                let text = self.node_text(child);
                if pkg.is_none() {
                    pkg = Some(text);
                } else {
                    // class within package
                    let full = format!("{}.{}", pkg.as_deref().unwrap_or(""), text);
                    self.push_import(full, Vec::new());
                }
            }
        }
        // If only package name found (no classes), import the package itself
        if let Some(p) = pkg {
            if self.imports.iter().all(|i| !i.imported.starts_with(&p)) {
                self.push_import(p, Vec::new());
            }
        }
    }

    fn visit_import_form(&mut self, node: Node, _kind: &str) {
        // Standalone (require ...) / (use ...) / (import ...) at top level
        self.extract_ns_imports(node);
    }

    fn push_import(&mut self, module: String, symbols: Vec<String>) {
        // Avoid duplicates
        if self.imports.iter().any(|i| i.imported == module) {
            return;
        }
        self.imports.push(ImportRelation {
            importer: self
                .current_namespace
                .clone()
                .unwrap_or_else(|| "main".to_string()),
            imported: module,
            symbols,
            is_wildcard: false,
            alias: None,
        });
    }

    fn extract_params_from_vec(&self, node: Node) -> Vec<Parameter> {
        let mut params = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "sym_lit" {
                let name = self.node_text(child);
                if name != "&" {
                    params.push(Parameter::new(name));
                }
            }
        }
        params
    }

    fn visit_for_calls(&mut self, node: Node) {
        if node.kind() == "list_lit" {
            // First sym_lit is the callee
            if let Some(callee) = self.first_sym(node) {
                if let Some(ref caller) = self.current_function.clone() {
                    if !callee.is_empty()
                        && callee != "defn"
                        && callee != "defn-"
                        && callee != "let"
                        && callee != "if"
                        && callee != "when"
                        && callee != "cond"
                        && callee != "do"
                        && callee != "fn"
                    {
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

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_for_calls(child);
        }
    }

    fn visit_for_complexity(&self, node: Node, builder: &mut ComplexityBuilder) {
        if node.kind() == "list_lit" {
            if let Some(sym) = self.first_sym(node) {
                match sym.as_str() {
                    "if" | "if-not" | "if-let" | "if-some" | "when" | "when-not" | "when-let"
                    | "when-some" | "cond" | "condp" | "case" => {
                        builder.add_branch();
                        builder.enter_scope();
                    }
                    "loop" | "recur" | "doseq" | "dotimes" | "doall" | "dorun" | "for" => {
                        builder.add_loop();
                        builder.enter_scope();
                    }
                    "and" | "or" => {
                        builder.add_logical_operator();
                    }
                    "try" | "catch" | "finally" => {
                        builder.add_exception_handler();
                    }
                    _ => {}
                }
            }
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_for_complexity(child, builder);
        }

        if node.kind() == "list_lit" {
            if let Some(sym) = self.first_sym(node) {
                match sym.as_str() {
                    "if" | "if-not" | "if-let" | "if-some" | "when" | "when-not" | "when-let"
                    | "when-some" | "cond" | "condp" | "case" => {
                        builder.exit_scope();
                    }
                    "loop" | "recur" | "doseq" | "dotimes" | "doall" | "dorun" | "for" => {
                        builder.exit_scope();
                    }
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::Parser;

    fn parse_and_visit(source: &[u8]) -> ClojureVisitor<'_> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_clojure::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut visitor = ClojureVisitor::new(source);
        visitor.visit_node(tree.root_node());
        visitor
    }

    /// Dump AST node kinds for a given source — run with `cargo test dump_ast -- --nocapture`
    #[test]
    fn dump_ast_node_types() {
        let source = br#"
(ns my.app
  (:require [clojure.string :as str]
            [clojure.set :refer [union]])
  (:import (java.util Date)))

(defn greet
  "Greets someone"
  [name]
  (str "Hello, " name))

(defn- private-helper [x y]
  (+ x y))

(defprotocol Animal
  (speak [this]))

(defrecord Dog [name breed]
  Animal
  (speak [this] (str "Woof! I am " (:name this))))
"#;

        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_clojure::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        fn dump_node(node: tree_sitter::Node, source: &[u8], depth: usize) {
            let indent = "  ".repeat(depth);
            let text = node.utf8_text(source).unwrap_or("").replace('\n', "\\n");
            let preview: String = text.chars().take(60).collect();
            println!(
                "{}{} [{}-{}] {:?}",
                indent,
                node.kind(),
                node.start_position().row + 1,
                node.end_position().row + 1,
                preview
            );
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                dump_node(child, source, depth + 1);
            }
        }

        dump_node(tree.root_node(), source, 0);
    }

    #[test]
    fn test_visitor_function_extraction() {
        let source = b"(defn greet [name] (str \"Hello, \" name))";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "greet");
    }

    #[test]
    fn test_visitor_private_function() {
        let source = b"(defn- helper [x] (* x 2))";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].visibility, "private");
    }

    #[test]
    fn test_visitor_ns_require() {
        let source = b"(ns my.app (:require [clojure.string :as str]))";
        let visitor = parse_and_visit(source);
        assert!(
            visitor
                .imports
                .iter()
                .any(|i| i.imported == "clojure.string"),
            "Expected clojure.string import, found: {:?}",
            visitor
                .imports
                .iter()
                .map(|i| &i.imported)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_visitor_defprotocol() {
        let source = b"(defprotocol Animal (speak [this]))";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "Animal");
    }

    #[test]
    fn test_visitor_defrecord() {
        let source = b"(defrecord Dog [name breed])";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "Dog");
    }
}
