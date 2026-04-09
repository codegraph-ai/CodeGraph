// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting COBOL entities
//!
//! Extracts the following COBOL constructs:
//! - `program_definition` → ClassEntity (COBOL program)
//! - `paragraph_header` → FunctionEntity (COBOL paragraph in PROCEDURE DIVISION)
//! - `copy_statement` → ImportRelation (COPY copybook)
//! - `call_statement` → CallRelation (CALL program-name)

use codegraph_parser_api::{
    CallRelation, ClassEntity, ComplexityBuilder, ComplexityMetrics, FunctionEntity,
    ImportRelation, BODY_PREFIX_MAX_CHARS,
    truncate_body_prefix,
};
use tree_sitter::Node;

pub struct CobolVisitor<'a> {
    pub source: &'a [u8],
    pub programs: Vec<ClassEntity>,
    pub paragraphs: Vec<FunctionEntity>,
    pub imports: Vec<ImportRelation>,
    pub calls: Vec<CallRelation>,
    current_program: Option<String>,
    current_paragraph: Option<String>,
}

impl<'a> CobolVisitor<'a> {
    pub fn new(source: &'a [u8]) -> Self {
        Self {
            source,
            programs: Vec::new(),
            paragraphs: Vec::new(),
            imports: Vec::new(),
            calls: Vec::new(),
            current_program: None,
            current_paragraph: None,
        }
    }

    fn node_text(&self, node: Node) -> String {
        node.utf8_text(self.source).unwrap_or("").to_string()
    }

    /// Recursively search for text of a node with the given kind (BFS up to depth).
    fn find_child_text_recursive(&self, node: Node, kind: &str, depth: usize) -> Option<String> {
        if depth == 0 {
            return None;
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == kind {
                return Some(self.node_text(child));
            }
        }
        let mut cursor2 = node.walk();
        for child in node.children(&mut cursor2) {
            if let Some(text) = self.find_child_text_recursive(child, kind, depth - 1) {
                return Some(text);
            }
        }
        None
    }

    /// Extract the callee name from a string literal node (strip quotes).
    fn strip_string_quotes(s: &str) -> String {
        let s = s.trim();
        if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
            s[1..s.len() - 1].to_string()
        } else {
            s.to_string()
        }
    }

    pub fn visit_node(&mut self, node: Node) {
        match node.kind() {
            "program_definition" => {
                self.visit_program_definition(node);
                return;
            }
            "paragraph_header" => {
                self.visit_paragraph_header(node);
                return;
            }
            "copy_statement" => {
                self.visit_copy_statement(node);
                // fall through to recurse (copy_statement has no interesting children)
            }
            "call_statement" => {
                self.visit_call_statement(node);
                // fall through to recurse
            }
            "perform_statement" | "perform_statement_call_proc" => {
                self.visit_perform_statement(node);
                // fall through to recurse
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child);
        }
    }

    fn visit_program_definition(&mut self, node: Node) {
        let name = self.extract_program_name(node);

        let prev_program = self.current_program.clone();
        let prev_paragraph = self.current_paragraph.clone();
        self.current_program = Some(name.clone());
        self.current_paragraph = None;

        let body_prefix = node
            .utf8_text(self.source)
            .ok()
            .filter(|t| !t.is_empty())
            .map(|t| {
                truncate_body_prefix(t)
            })
            .map(|t| t.to_string());
        let entity = ClassEntity {
            name,
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_abstract: false,
            is_interface: false,
            base_classes: Vec::new(),
            implemented_traits: Vec::new(),
            methods: Vec::new(),
            fields: Vec::new(),
            doc_comment: None,
            attributes: Vec::new(),
            type_parameters: Vec::new(),
            body_prefix,
        };
        self.programs.push(entity);

        // Recurse into children to find paragraphs, calls, copies
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child);
        }

        // Close last paragraph
        if let Some(ref para_name) = self.current_paragraph.clone() {
            let prog_end = node.end_position().row + 1;
            if let Some(para) = self.paragraphs.iter_mut().rfind(|p| p.name == *para_name) {
                if para.line_end == para.line_start {
                    para.line_end = prog_end;
                }
            }
        }

        self.current_program = prev_program;
        self.current_paragraph = prev_paragraph;
    }

    fn extract_program_name(&self, program_node: Node) -> String {
        // program_definition -> identification_division -> program_name (leaf)
        if let Some(text) = self.find_child_text_recursive(program_node, "program_name", 4) {
            let name = text.trim().to_string();
            if !name.is_empty() {
                return name;
            }
        }
        "unknown_program".to_string()
    }

    fn visit_paragraph_header(&mut self, node: Node) {
        // paragraph_header text is like "MAIN-PARA." — strip trailing period
        let full_text = self.node_text(node);
        let name = full_text.trim().trim_end_matches('.').trim().to_string();
        if name.is_empty() {
            return;
        }

        let line_start = node.start_position().row + 1;

        // Close previous paragraph
        if let Some(ref prev_name) = self.current_paragraph.clone() {
            if let Some(para) = self.paragraphs.iter_mut().rfind(|p| p.name == *prev_name) {
                if para.line_end == para.line_start {
                    para.line_end = if line_start > 1 {
                        line_start - 1
                    } else {
                        line_start
                    };
                }
            }
        }

        self.current_paragraph = Some(name.clone());

        let body_prefix = node
            .utf8_text(self.source)
            .ok()
            .filter(|t| !t.is_empty())
            .map(|t| {
                truncate_body_prefix(t)
            })
            .map(|t| t.to_string());
        let func = FunctionEntity {
            name,
            signature: full_text.trim().to_string(),
            visibility: "public".to_string(),
            line_start,
            line_end: line_start, // updated when next paragraph or program end is seen
            is_async: false,
            is_test: false,
            is_static: false,
            is_abstract: false,
            parameters: Vec::new(),
            return_type: None,
            doc_comment: None,
            attributes: Vec::new(),
            parent_class: self.current_program.clone(),
            complexity: Some(ComplexityMetrics::default()),
            body_prefix,
        };
        self.paragraphs.push(func);
    }

    fn visit_copy_statement(&mut self, node: Node) {
        // copy_statement -> WORD (copybook name) or string (quoted copybook name)
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            let book = match child.kind() {
                "WORD" => self.node_text(child).trim().to_string(),
                "string" => Self::strip_string_quotes(&self.node_text(child)),
                _ => continue,
            };
            if !book.is_empty() {
                self.imports.push(ImportRelation {
                    importer: self
                        .current_program
                        .clone()
                        .unwrap_or_else(|| "file".to_string()),
                    imported: book,
                    symbols: Vec::new(),
                    is_wildcard: false,
                    alias: None,
                });
                return;
            }
        }
    }

    /// Extract PERFORM paragraph-name as a call relationship.
    /// PERFORM is the primary control flow mechanism in COBOL.
    ///
    /// AST: perform_statement_call_proc → perform_procedure → label → qualified_word → WORD
    fn visit_perform_statement(&mut self, node: Node) {
        // Find the first WORD in the tree (paragraph name being PERFORMed)
        if let Some(callee) = self.find_first_word(node) {
            let callee = callee.trim().to_string();
            if !callee.is_empty() {
                let caller = self
                    .current_paragraph
                    .clone()
                    .or_else(|| self.current_program.clone())
                    .unwrap_or_else(|| "file".to_string());
                self.calls.push(CallRelation::new(
                    caller,
                    callee,
                    node.start_position().row + 1,
                ));
            }
        }
    }

    /// Recursively find the first WORD node in a subtree.
    fn find_first_word(&self, node: Node) -> Option<String> {
        if node.kind() == "WORD" {
            return Some(self.node_text(node));
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if let Some(word) = self.find_first_word(child) {
                return Some(word);
            }
        }
        None
    }

    fn visit_call_statement(&mut self, node: Node) {
        // call_statement -> (_call_header inlined) -> field 'x' = WORD or string
        // Since _call_header is a private rule, its children appear directly here.
        // The first WORD or string child is the callee.
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            let callee = match child.kind() {
                "WORD" => self.node_text(child).trim().to_string(),
                "string" => Self::strip_string_quotes(&self.node_text(child)),
                _ => continue,
            };
            if !callee.is_empty() {
                let caller = self
                    .current_paragraph
                    .clone()
                    .or_else(|| self.current_program.clone())
                    .unwrap_or_else(|| "file".to_string());
                self.calls.push(CallRelation::new(
                    caller,
                    callee,
                    node.start_position().row + 1,
                ));
                return;
            }
        }
    }

    fn _calculate_complexity(&self, node: Node) -> ComplexityMetrics {
        let mut builder = ComplexityBuilder::new();
        self._visit_for_complexity(node, &mut builder);
        builder.build()
    }

    fn _visit_for_complexity(&self, node: Node, builder: &mut ComplexityBuilder) {
        match node.kind() {
            "if_header" | "else_if_header" | "evaluate_header" => {
                builder.add_branch();
                builder.enter_scope();
            }
            "perform_statement" | "perform_statement_call_proc" => {
                builder.add_loop();
                builder.enter_scope();
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self._visit_for_complexity(child, builder);
        }

        match node.kind() {
            "if_header"
            | "else_if_header"
            | "evaluate_header"
            | "perform_statement"
            | "perform_statement_call_proc" => {
                builder.exit_scope();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_visitor_initial_state() {
        let visitor = CobolVisitor::new(b"");
        assert_eq!(visitor.programs.len(), 0);
        assert_eq!(visitor.paragraphs.len(), 0);
        assert_eq!(visitor.imports.len(), 0);
        assert_eq!(visitor.calls.len(), 0);
    }

    #[test]
    fn test_strip_string_quotes_double() {
        assert_eq!(CobolVisitor::strip_string_quotes("\"MYPROG\""), "MYPROG");
    }

    #[test]
    fn test_strip_string_quotes_single() {
        assert_eq!(CobolVisitor::strip_string_quotes("'MYPROG'"), "MYPROG");
    }

    #[test]
    fn test_strip_string_quotes_unquoted() {
        assert_eq!(CobolVisitor::strip_string_quotes("MYPROG"), "MYPROG");
    }

    #[test]
    fn test_visitor_program_extraction() {
        use tree_sitter::Parser;
        // Minimal COBOL with fixed-format (7 spaces before keywords)
        let source = b"       identification division.\n       program-id. MYPROG.\n       procedure division.\n       stop run.\n";
        let mut parser = Parser::new();
        parser.set_language(&crate::ts_cobol::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = CobolVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.programs.len(), 1);
        assert_eq!(visitor.programs[0].name, "MYPROG");
    }

    #[test]
    fn test_visitor_paragraph_extraction() {
        use tree_sitter::Parser;
        let source = b"       identification division.\n       program-id. TEST.\n       procedure division.\n       MAIN-PARA.\n           stop run.\n";
        let mut parser = Parser::new();
        parser.set_language(&crate::ts_cobol::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = CobolVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.programs.len(), 1);
        assert_eq!(visitor.paragraphs.len(), 1);
        assert_eq!(visitor.paragraphs[0].name, "MAIN-PARA");
        assert_eq!(visitor.paragraphs[0].parent_class, Some("TEST".to_string()));
    }

    #[test]
    fn test_visitor_copy_extraction() {
        use tree_sitter::Parser;
        let source = b"       identification division.\n       program-id. COPYTEST.\n       data division.\n       working-storage section.\n       copy MYBOOK.\n       procedure division.\n       stop run.\n";
        let mut parser = Parser::new();
        parser.set_language(&crate::ts_cobol::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = CobolVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert!(!visitor.imports.is_empty(), "Expected COPY import");
        assert_eq!(visitor.imports[0].imported, "MYBOOK");
    }
}
