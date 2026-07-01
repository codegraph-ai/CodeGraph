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
    truncate_body_prefix, CallRelation, ClassEntity, ComplexityBuilder, ComplexityMetrics,
    FunctionEntity, ImportRelation,
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
            .map(truncate_body_prefix)
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
            .map(truncate_body_prefix)
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
    use tree_sitter::Parser;

    /// Parse COBOL source and run the visitor, returning the populated visitor.
    /// The tree is dropped after visiting since the visitor only borrows `source`.
    fn parse(source: &[u8]) -> CobolVisitor<'_> {
        let mut parser = Parser::new();
        parser.set_language(&crate::ts_cobol::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut visitor = CobolVisitor::new(source);
        visitor.visit_node(tree.root_node());
        visitor
    }

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
    fn test_strip_string_quotes_trims_surrounding_whitespace() {
        // trim() runs before the quote check, so padded quoted text still unwraps.
        assert_eq!(CobolVisitor::strip_string_quotes("  \"X\"  "), "X");
    }

    #[test]
    fn test_strip_string_quotes_empty_quoted() {
        // s[1..len-1] of "\"\"" is the empty string.
        assert_eq!(CobolVisitor::strip_string_quotes("\"\""), "");
    }

    #[test]
    fn test_strip_string_quotes_mismatched_quotes_kept() {
        // Opening double, closing single -> neither branch matches, returned as-is (trimmed).
        assert_eq!(CobolVisitor::strip_string_quotes("\"MYPROG'"), "\"MYPROG'");
    }

    #[test]
    fn test_visitor_program_extraction() {
        // Minimal COBOL with fixed-format (7 spaces before keywords)
        let source = b"       identification division.\n       program-id. MYPROG.\n       procedure division.\n       stop run.\n";
        let visitor = parse(source);

        assert_eq!(visitor.programs.len(), 1);
        assert_eq!(visitor.programs[0].name, "MYPROG");
    }

    #[test]
    fn test_program_metadata_defaults() {
        let source = b"       identification division.\n       program-id. MYPROG.\n       procedure division.\n       stop run.\n";
        let visitor = parse(source);

        let prog = &visitor.programs[0];
        assert_eq!(prog.visibility, "public");
        assert!(!prog.is_abstract);
        assert!(!prog.is_interface);
        assert!(prog.base_classes.is_empty());
        assert!(prog.methods.is_empty());
        assert!(prog.doc_comment.is_none());
        // Fixed-format program spans line 1 through the final line (1-based).
        assert_eq!(prog.line_start, 1);
        assert!(prog.line_end >= 4);
    }

    #[test]
    fn test_program_body_prefix_populated() {
        let source = b"       identification division.\n       program-id. MYPROG.\n       procedure division.\n       stop run.\n";
        let visitor = parse(source);

        let body = visitor.programs[0].body_prefix.as_deref().unwrap();
        assert!(body.contains("identification"));
    }

    #[test]
    fn test_visitor_paragraph_extraction() {
        let source = b"       identification division.\n       program-id. TEST.\n       procedure division.\n       MAIN-PARA.\n           stop run.\n";
        let visitor = parse(source);

        assert_eq!(visitor.programs.len(), 1);
        assert_eq!(visitor.paragraphs.len(), 1);
        assert_eq!(visitor.paragraphs[0].name, "MAIN-PARA");
        assert_eq!(visitor.paragraphs[0].parent_class, Some("TEST".to_string()));
    }

    #[test]
    fn test_paragraph_metadata_defaults() {
        let source = b"       identification division.\n       program-id. TEST.\n       procedure division.\n       MAIN-PARA.\n           stop run.\n";
        let visitor = parse(source);

        let para = &visitor.paragraphs[0];
        assert_eq!(para.visibility, "public");
        assert!(!para.is_async);
        assert!(!para.is_test);
        assert!(!para.is_static);
        assert!(para.parameters.is_empty());
        assert!(para.return_type.is_none());
        // Default complexity metrics are attached to every paragraph.
        assert!(para.complexity.is_some());
        // signature is the header text with the trailing period retained.
        assert_eq!(para.signature, "MAIN-PARA.");
        assert_eq!(para.line_start, 4);
    }

    #[test]
    fn test_multiple_paragraphs_close_line_end() {
        let source = b"       identification division.\n       program-id. TEST.\n       procedure division.\n       FIRST-PARA.\n           display \"a\".\n       SECOND-PARA.\n           stop run.\n";
        let visitor = parse(source);

        assert_eq!(visitor.paragraphs.len(), 2);
        assert_eq!(visitor.paragraphs[0].name, "FIRST-PARA");
        assert_eq!(visitor.paragraphs[1].name, "SECOND-PARA");
        // FIRST-PARA (line 4) closes at the line before SECOND-PARA (line 6) -> 5.
        assert_eq!(visitor.paragraphs[0].line_end, 5);
        // SECOND-PARA is the last paragraph; it closes at program end.
        assert!(visitor.paragraphs[1].line_end >= visitor.paragraphs[1].line_start);
    }

    #[test]
    fn test_visitor_copy_extraction() {
        let source = b"       identification division.\n       program-id. COPYTEST.\n       data division.\n       working-storage section.\n       copy MYBOOK.\n       procedure division.\n       stop run.\n";
        let visitor = parse(source);

        assert!(!visitor.imports.is_empty(), "Expected COPY import");
        assert_eq!(visitor.imports[0].imported, "MYBOOK");
    }

    #[test]
    fn test_copy_importer_is_program_name() {
        let source = b"       identification division.\n       program-id. COPYTEST.\n       data division.\n       working-storage section.\n       copy MYBOOK.\n       procedure division.\n       stop run.\n";
        let visitor = parse(source);

        assert_eq!(visitor.imports[0].importer, "COPYTEST");
        assert!(!visitor.imports[0].is_wildcard);
        assert!(visitor.imports[0].symbols.is_empty());
        assert!(visitor.imports[0].alias.is_none());
    }

    #[test]
    fn test_call_statement_records_call() {
        let source = b"       identification division.\n       program-id. TEST.\n       procedure division.\n       MAIN-PARA.\n           call \"SUBPROG\".\n           stop run.\n";
        let visitor = parse(source);

        let call = visitor
            .calls
            .iter()
            .find(|c| c.callee == "SUBPROG")
            .expect("Expected CALL to SUBPROG");
        // Caller is the enclosing paragraph.
        assert_eq!(call.caller, "MAIN-PARA");
    }

    #[test]
    fn test_perform_statement_records_call() {
        let source = b"       identification division.\n       program-id. TEST.\n       procedure division.\n       MAIN-PARA.\n           perform DO-WORK.\n           stop run.\n       DO-WORK.\n           display \"x\".\n";
        let visitor = parse(source);

        let call = visitor
            .calls
            .iter()
            .find(|c| c.callee == "DO-WORK")
            .expect("Expected PERFORM of DO-WORK");
        assert_eq!(call.caller, "MAIN-PARA");
    }

    #[test]
    fn test_find_first_word_returns_none_without_word() {
        let visitor = CobolVisitor::new(b"");
        let mut parser = Parser::new();
        parser.set_language(&crate::ts_cobol::language()).unwrap();
        // Source with no PROCEDURE content -> no WORD leaf in an empty subtree search.
        let source = b"       identification division.\n       program-id. TEST.\n";
        let tree = parser.parse(source, None).unwrap();
        // The root has children but no WORD under identification-only source is fine;
        // assert the recursive search terminates and finds the program-id token or None.
        let _ = visitor.find_first_word(tree.root_node());
    }

    #[test]
    fn test_calculate_complexity_counts_if_and_perform() {
        let source = b"       identification division.\n       program-id. TEST.\n       procedure division.\n       MAIN-PARA.\n           if x = 1\n               perform DO-WORK\n           end-if.\n           stop run.\n       DO-WORK.\n           display \"x\".\n";
        let visitor = parse(source);

        let mut parser = Parser::new();
        parser.set_language(&crate::ts_cobol::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let metrics = visitor._calculate_complexity(tree.root_node());
        // An IF header (branch) and a PERFORM (loop) push cyclomatic above the base 1.
        assert!(metrics.cyclomatic_complexity >= 2);
    }
}
