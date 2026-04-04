// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting Fortran entities
//!
//! Handles Fortran program units: program, module, submodule, subroutine, function,
//! and block_data. Extracts USE statements (imports) and CALL/function-call
//! relationships.

use codegraph_parser_api::{
    CallRelation, ClassEntity, ComplexityBuilder, ComplexityMetrics, FunctionEntity,
    ImportRelation, Parameter, BODY_PREFIX_MAX_CHARS,
};
use tree_sitter::Node;

pub struct FortranVisitor<'a> {
    pub source: &'a [u8],
    /// Modules/programs/submodules stored as ClassEntity (top-level program units)
    pub program_units: Vec<ClassEntity>,
    pub functions: Vec<FunctionEntity>,
    pub imports: Vec<ImportRelation>,
    pub calls: Vec<CallRelation>,
    current_unit: Option<String>,
    current_function: Option<String>,
}

impl<'a> FortranVisitor<'a> {
    pub fn new(source: &'a [u8]) -> Self {
        Self {
            source,
            program_units: Vec::new(),
            functions: Vec::new(),
            imports: Vec::new(),
            calls: Vec::new(),
            current_unit: None,
            current_function: None,
        }
    }

    fn node_text(&self, node: Node) -> String {
        node.utf8_text(self.source).unwrap_or("").to_string()
    }

    /// Find the first `name` child node and return its text.
    fn find_name_child(&self, node: Node) -> Option<String> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "name" {
                return Some(self.node_text(child));
            }
        }
        None
    }

    /// Find the first `identifier` child node and return its text.
    fn find_identifier_child(&self, node: Node) -> Option<String> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "identifier" {
                return Some(self.node_text(child));
            }
        }
        None
    }

    /// Extract parameter names from a subroutine_statement or function_statement node.
    /// The statement has `parameters: (parameters (identifier) ...)`.
    fn extract_parameters(&self, stmt_node: Node) -> Vec<Parameter> {
        let mut cursor = stmt_node.walk();
        for child in stmt_node.children(&mut cursor) {
            if child.kind() == "parameters" {
                let mut params = Vec::new();
                let mut inner = child.walk();
                for param in child.children(&mut inner) {
                    if param.kind() == "identifier" {
                        params.push(Parameter {
                            name: self.node_text(param),
                            type_annotation: None,
                            default_value: None,
                            is_variadic: false,
                        });
                    }
                }
                return params;
            }
        }
        Vec::new()
    }

    /// Extract name from the first statement child of a program unit.
    /// For `program`, `module`, `subroutine`, `function` the first child is
    /// a `*_statement` that has a `name` child.
    fn extract_unit_name(&self, node: Node, stmt_kind: &str) -> String {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == stmt_kind {
                if let Some(name) = self.find_name_child(child) {
                    return name;
                }
            }
        }
        // fallback: any name child at this level
        self.find_name_child(node)
            .unwrap_or_else(|| "unknown".to_string())
    }

    pub fn visit_node(&mut self, node: Node) {
        // Skip anonymous tokens (e.g. the `program` keyword token inside
        // program_statement shares the same kind string as the named program
        // program-unit node, so we must filter by is_named()).
        if !node.is_named() {
            return;
        }
        match node.kind() {
            "program" => {
                self.visit_program_unit(node, "program_statement", false, false);
                return;
            }
            "module" => {
                self.visit_program_unit(node, "module_statement", false, false);
                return;
            }
            "submodule" => {
                self.visit_submodule(node);
                return;
            }
            "block_data" => {
                self.visit_program_unit(node, "block_data_statement", false, false);
                return;
            }
            "subroutine" => {
                self.visit_subroutine(node);
                return;
            }
            "function" => {
                self.visit_function(node);
                return;
            }
            "use_statement" => {
                self.visit_use_statement(node);
            }
            "subroutine_call" => {
                self.visit_subroutine_call(node);
            }
            "call_expression" => {
                self.visit_call_expression(node);
            }
            "include_statement" => {
                self.visit_include_statement(node);
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child);
        }
    }

    fn visit_program_unit(
        &mut self,
        node: Node,
        stmt_kind: &str,
        is_abstract: bool,
        is_interface: bool,
    ) {
        let name = self.extract_unit_name(node, stmt_kind);

        let prev_unit = self.current_unit.clone();
        self.current_unit = Some(name.clone());

        let body_prefix = node
            .child_by_field_name("body")
            .or(Some(node))
            .and_then(|n| n.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| {
                if t.len() > BODY_PREFIX_MAX_CHARS {
                    &t[..BODY_PREFIX_MAX_CHARS]
                } else {
                    t
                }
            })
            .map(|t| t.to_string());
        let entity = ClassEntity {
            name,
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_abstract,
            is_interface,
            base_classes: Vec::new(),
            implemented_traits: Vec::new(),
            methods: Vec::new(),
            fields: Vec::new(),
            doc_comment: None,
            attributes: Vec::new(),
            type_parameters: Vec::new(),
            body_prefix,
        };
        self.program_units.push(entity);

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child);
        }

        self.current_unit = prev_unit;
    }

    fn visit_submodule(&mut self, node: Node) {
        // submodule_statement has a `name` child (submodule name)
        let name = self.extract_unit_name(node, "submodule_statement");
        let prev_unit = self.current_unit.clone();
        self.current_unit = Some(name.clone());

        let body_prefix = node
            .child_by_field_name("body")
            .or(Some(node))
            .and_then(|n| n.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| {
                if t.len() > BODY_PREFIX_MAX_CHARS {
                    &t[..BODY_PREFIX_MAX_CHARS]
                } else {
                    t
                }
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
        self.program_units.push(entity);

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child);
        }

        self.current_unit = prev_unit;
    }

    fn visit_subroutine(&mut self, node: Node) {
        let name = self.extract_unit_name(node, "subroutine_statement");

        // Extract parameters from subroutine_statement
        let parameters = {
            let mut cursor = node.walk();
            let stmt = node
                .children(&mut cursor)
                .find(|c| c.kind() == "subroutine_statement");
            stmt.map(|s| self.extract_parameters(s)).unwrap_or_default()
        };

        let prev_function = self.current_function.clone();
        self.current_function = Some(name.clone());

        let complexity = self.calculate_complexity(node);

        let body_prefix = node
            .child_by_field_name("body")
            .or(Some(node))
            .and_then(|n| n.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| {
                if t.len() > BODY_PREFIX_MAX_CHARS {
                    &t[..BODY_PREFIX_MAX_CHARS]
                } else {
                    t
                }
            })
            .map(|t| t.to_string());
        let func = FunctionEntity {
            name,
            signature: self
                .node_text(node)
                .lines()
                .next()
                .unwrap_or("")
                .to_string(),
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_async: false,
            is_test: false,
            is_static: false,
            is_abstract: false,
            parameters,
            return_type: None,
            doc_comment: None,
            attributes: Vec::new(),
            parent_class: self.current_unit.clone(),
            complexity: Some(complexity),
            body_prefix,
        };
        self.functions.push(func);

        // Visit children for nested calls/imports
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child);
        }

        self.current_function = prev_function;
    }

    fn visit_function(&mut self, node: Node) {
        let name = self.extract_unit_name(node, "function_statement");

        // Extract parameters from function_statement
        let parameters = {
            let mut cursor = node.walk();
            let stmt = node
                .children(&mut cursor)
                .find(|c| c.kind() == "function_statement");
            stmt.map(|s| self.extract_parameters(s)).unwrap_or_default()
        };

        let prev_function = self.current_function.clone();
        self.current_function = Some(name.clone());

        let complexity = self.calculate_complexity(node);

        let body_prefix = node
            .child_by_field_name("body")
            .or(Some(node))
            .and_then(|n| n.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| {
                if t.len() > BODY_PREFIX_MAX_CHARS {
                    &t[..BODY_PREFIX_MAX_CHARS]
                } else {
                    t
                }
            })
            .map(|t| t.to_string());
        let func = FunctionEntity {
            name,
            signature: self
                .node_text(node)
                .lines()
                .next()
                .unwrap_or("")
                .to_string(),
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_async: false,
            is_test: false,
            is_static: false,
            is_abstract: false,
            parameters,
            return_type: None,
            doc_comment: None,
            attributes: Vec::new(),
            parent_class: self.current_unit.clone(),
            complexity: Some(complexity),
            body_prefix,
        };
        self.functions.push(func);

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child);
        }

        self.current_function = prev_function;
    }

    fn visit_use_statement(&mut self, node: Node) {
        // use_statement -> module_name -> identifier OR name
        let module_name: Option<String> = {
            let mut cursor = node.walk();
            let found = node
                .children(&mut cursor)
                .find(|c| c.kind() == "module_name");
            found.and_then(|mn| {
                self.find_identifier_child(mn)
                    .or_else(|| self.find_name_child(mn))
                    .or_else(|| Some(self.node_text(mn)))
            })
        };

        if let Some(name) = module_name {
            if !name.is_empty() {
                // Check for only/rename list to detect wildcard vs specific
                let is_wildcard = {
                    let text = self.node_text(node);
                    !text.contains("ONLY") && !text.contains("only")
                };

                self.imports.push(ImportRelation {
                    importer: self
                        .current_unit
                        .clone()
                        .or_else(|| self.current_function.clone())
                        .unwrap_or_else(|| "file".to_string()),
                    imported: name,
                    symbols: Vec::new(),
                    is_wildcard,
                    alias: None,
                });
            }
        }
    }

    fn visit_subroutine_call(&mut self, node: Node) {
        // subroutine_call -> subroutine field -> identifier/call_expression
        let callee = {
            let mut cursor = node.walk();
            let sub_field = node
                .children(&mut cursor)
                .find(|c| c.kind() == "identifier" || c.kind() == "name");
            sub_field
                .map(|n| self.node_text(n))
                .filter(|s| !s.is_empty())
        };

        if let Some(name) = callee {
            let caller = self
                .current_function
                .clone()
                .or_else(|| self.current_unit.clone())
                .unwrap_or_else(|| "file".to_string());
            self.calls.push(CallRelation::new(
                caller,
                name,
                node.start_position().row + 1,
            ));
        }
    }

    fn visit_call_expression(&mut self, node: Node) {
        // call_expression has a `function` field of type `identifier`
        // Note: In Fortran, array indexing `x(i)` is syntactically identical to
        // a function call `f(x)`. We filter out intrinsic functions and likely
        // array accesses to reduce noise.
        let callee = {
            let mut cursor = node.walk();
            let func_child = node
                .children(&mut cursor)
                .find(|c| c.kind() == "identifier");
            func_child
                .map(|n| self.node_text(n))
                .filter(|s| !s.is_empty())
        };

        if let Some(name) = callee {
            let lower = name.to_lowercase();
            // Skip Fortran intrinsic functions and likely array accesses
            if is_fortran_intrinsic(&lower) {
                return;
            }
            let caller = self
                .current_function
                .clone()
                .or_else(|| self.current_unit.clone())
                .unwrap_or_else(|| "file".to_string());
            self.calls.push(CallRelation::new(
                caller,
                name,
                node.start_position().row + 1,
            ));
        }
    }

    /// Extract INCLUDE 'filename' as an import relationship.
    /// Fortran's INCLUDE is similar to C's #include — it textually includes
    /// the contents of another file.
    fn visit_include_statement(&mut self, node: Node) {
        let text = self.node_text(node);
        // INCLUDE 'filename' or INCLUDE "filename"
        let name = text
            .trim()
            .strip_prefix("include")
            .or_else(|| text.trim().strip_prefix("INCLUDE"))
            .and_then(|rest| {
                let rest = rest.trim();
                if (rest.starts_with('\'') && rest.ends_with('\''))
                    || (rest.starts_with('"') && rest.ends_with('"'))
                {
                    Some(rest[1..rest.len() - 1].to_string())
                } else {
                    None
                }
            });

        if let Some(name) = name {
            if !name.is_empty() {
                self.imports.push(ImportRelation {
                    importer: self
                        .current_unit
                        .clone()
                        .or_else(|| self.current_function.clone())
                        .unwrap_or_else(|| "file".to_string()),
                    imported: name,
                    symbols: Vec::new(),
                    is_wildcard: true,
                    alias: None,
                });
            }
        }
    }

    fn calculate_complexity(&self, node: Node) -> ComplexityMetrics {
        let mut builder = ComplexityBuilder::new();
        self.visit_for_complexity(node, &mut builder);
        builder.build()
    }

    fn visit_for_complexity(&self, node: Node, builder: &mut ComplexityBuilder) {
        match node.kind() {
            "if_statement"
            | "arithmetic_if_statement"
            | "select_case_statement"
            | "select_rank_statement"
            | "select_type_statement" => {
                builder.add_branch();
                builder.enter_scope();
            }
            "elseif_clause" | "case_statement" => {
                builder.add_branch();
            }
            "do_loop_statement" | "do_label_statement" | "while_statement" => {
                builder.add_loop();
                builder.enter_scope();
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_for_complexity(child, builder);
        }

        match node.kind() {
            "if_statement"
            | "arithmetic_if_statement"
            | "select_case_statement"
            | "select_rank_statement"
            | "select_type_statement"
            | "do_loop_statement"
            | "do_label_statement"
            | "while_statement" => {
                builder.exit_scope();
            }
            _ => {}
        }
    }
}

/// Check if a lowercase name is a Fortran intrinsic function or a likely
/// array access (single letter). These produce `call_expression` nodes in
/// tree-sitter but are not user-defined procedures.
fn is_fortran_intrinsic(name: &str) -> bool {
    // Single-letter names are almost always array/variable accesses, not calls
    if name.len() == 1 {
        return true;
    }
    matches!(
        name,
        // Numeric / math
        "abs" | "acos" | "acosh" | "aimag" | "aint" | "anint" | "asin" | "asinh"
        | "atan" | "atan2" | "atanh" | "ceiling" | "cmplx" | "conjg" | "cos"
        | "cosh" | "dble" | "dim" | "dprod" | "exp" | "floor" | "fraction"
        | "huge" | "hypot" | "int" | "log" | "log10" | "log_gamma" | "max"
        | "min" | "mod" | "modulo" | "nearest" | "nint" | "real" | "rrspacing"
        | "scale" | "set_exponent" | "sign" | "sin" | "sinh" | "spacing"
        | "sqrt" | "tan" | "tanh" | "tiny"
        // Array / matrix
        | "all" | "any" | "count" | "cshift" | "dot_product" | "eoshift"
        | "lbound" | "matmul" | "maxloc" | "maxval" | "merge" | "minloc"
        | "minval" | "norm2" | "pack" | "product" | "reshape" | "shape"
        | "size" | "spread" | "sum" | "transpose" | "ubound" | "unpack"
        // String
        | "achar" | "adjustl" | "adjustr" | "char" | "iachar" | "ichar"
        | "index" | "len" | "len_trim" | "lge" | "lgt" | "lle" | "llt"
        | "repeat" | "scan" | "trim" | "verify"
        // Type / conversion
        | "allocated" | "associated" | "bit_size" | "digits" | "epsilon"
        | "exponent" | "kind" | "logical" | "maxexponent" | "minexponent"
        | "precision" | "present" | "radix" | "range" | "selected_int_kind"
        | "selected_real_kind" | "storage_size" | "transfer"
        // Bit manipulation
        | "bge" | "bgt" | "ble" | "blt" | "dshiftl" | "dshiftr" | "iand"
        | "ibclr" | "ibits" | "ibset" | "ieor" | "ior" | "ishft" | "ishftc"
        | "leadz" | "maskl" | "maskr" | "merge_bits" | "mvbits" | "not"
        | "popcnt" | "poppar" | "shifta" | "shiftl" | "shiftr" | "trailz"
        // System / misc
        | "command_argument_count" | "cpu_time" | "date_and_time"
        | "get_command" | "get_command_argument" | "get_environment_variable"
        | "is_iostat_end" | "is_iostat_eor" | "move_alloc" | "new_line"
        | "null" | "system_clock"
        // Statements sometimes parsed as call_expression
        | "allocate" | "deallocate" | "c_f_pointer" | "c_loc" | "c_funloc"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_visitor_basics() {
        let visitor = FortranVisitor::new(b"program hello\nend program hello");
        assert_eq!(visitor.program_units.len(), 0);
        assert_eq!(visitor.functions.len(), 0);
        assert_eq!(visitor.imports.len(), 0);
    }

    #[test]
    fn test_visitor_program_extraction() {
        use tree_sitter::Parser;
        let source = b"program hello\n  implicit none\nend program hello\n";
        let mut parser = Parser::new();
        parser.set_language(&crate::ts_fortran::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = FortranVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.program_units.len(), 1);
        assert_eq!(visitor.program_units[0].name.to_lowercase(), "hello");
    }

    #[test]
    fn test_visitor_module_extraction() {
        use tree_sitter::Parser;
        let source = b"module mymod\n  implicit none\nend module mymod\n";
        let mut parser = Parser::new();
        parser.set_language(&crate::ts_fortran::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = FortranVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.program_units.len(), 1);
        assert_eq!(visitor.program_units[0].name.to_lowercase(), "mymod");
    }

    #[test]
    fn test_visitor_subroutine_extraction() {
        use tree_sitter::Parser;
        let source =
            b"subroutine greet(name)\n  character(*), intent(in) :: name\nend subroutine greet\n";
        let mut parser = Parser::new();
        parser.set_language(&crate::ts_fortran::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = FortranVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert!(
            !visitor.functions.is_empty(),
            "Expected at least one subroutine"
        );
        assert_eq!(visitor.functions[0].name.to_lowercase(), "greet");
        // Verify parameter extraction in basic test too
        let params: Vec<String> = visitor.functions[0]
            .parameters
            .iter()
            .map(|p| p.name.to_lowercase())
            .collect();
        assert_eq!(params, vec!["name"]);
    }

    #[test]
    fn test_visitor_function_extraction() {
        use tree_sitter::Parser;
        let source =
            b"function add(a, b)\n  integer :: a, b, add\n  add = a + b\nend function add\n";
        let mut parser = Parser::new();
        parser.set_language(&crate::ts_fortran::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = FortranVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert!(
            !visitor.functions.is_empty(),
            "Expected at least one function"
        );
        assert_eq!(visitor.functions[0].name.to_lowercase(), "add");
        // Verify parameter extraction in basic test too
        let params: Vec<String> = visitor.functions[0]
            .parameters
            .iter()
            .map(|p| p.name.to_lowercase())
            .collect();
        assert_eq!(params, vec!["a", "b"]);
    }

    #[test]
    fn test_visitor_use_statement() {
        use tree_sitter::Parser;
        let source = b"program main\n  use iso_fortran_env\n  implicit none\nend program main\n";
        let mut parser = Parser::new();
        parser.set_language(&crate::ts_fortran::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = FortranVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert!(
            !visitor.imports.is_empty(),
            "Expected import from use statement"
        );
        assert_eq!(
            visitor.imports[0].imported.to_lowercase(),
            "iso_fortran_env"
        );
    }

    #[test]
    fn test_visitor_subroutine_call() {
        use tree_sitter::Parser;
        let source = b"program main\n  implicit none\n  call greet('world')\nend program main\n";
        let mut parser = Parser::new();
        parser.set_language(&crate::ts_fortran::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = FortranVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert!(!visitor.calls.is_empty(), "Expected call relation");
        assert_eq!(visitor.calls[0].callee.to_lowercase(), "greet");
    }

    #[test]
    fn test_subroutine_parameter_extraction() {
        use tree_sitter::Parser;
        let source =
            b"subroutine greet(name, age)\n  character(*), intent(in) :: name\n  integer, intent(in) :: age\nend subroutine greet\n";
        let mut parser = Parser::new();
        parser.set_language(&crate::ts_fortran::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = FortranVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.functions.len(), 1);
        let params: Vec<String> = visitor.functions[0]
            .parameters
            .iter()
            .map(|p| p.name.to_lowercase())
            .collect();
        assert_eq!(params, vec!["name", "age"]);
    }

    #[test]
    fn test_function_parameter_extraction() {
        use tree_sitter::Parser;
        let source =
            b"function add(a, b) result(res)\n  integer :: a, b, res\n  res = a + b\nend function add\n";
        let mut parser = Parser::new();
        parser.set_language(&crate::ts_fortran::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = FortranVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.functions.len(), 1);
        let params: Vec<String> = visitor.functions[0]
            .parameters
            .iter()
            .map(|p| p.name.to_lowercase())
            .collect();
        assert_eq!(params, vec!["a", "b"]);
    }

    #[test]
    fn test_subroutine_no_parameters() {
        use tree_sitter::Parser;
        let source = b"subroutine init()\nend subroutine init\n";
        let mut parser = Parser::new();
        parser.set_language(&crate::ts_fortran::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = FortranVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.functions.len(), 1);
        assert!(
            visitor.functions[0].parameters.is_empty(),
            "Expected no parameters for parameterless subroutine"
        );
    }

    #[test]
    fn test_function_single_parameter() {
        use tree_sitter::Parser;
        let source =
            b"function square(x)\n  real :: x, square\n  square = x * x\nend function square\n";
        let mut parser = Parser::new();
        parser.set_language(&crate::ts_fortran::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = FortranVisitor::new(source);
        visitor.visit_node(tree.root_node());

        assert_eq!(visitor.functions.len(), 1);
        let params: Vec<String> = visitor.functions[0]
            .parameters
            .iter()
            .map(|p| p.name.to_lowercase())
            .collect();
        assert_eq!(params, vec!["x"]);
    }
}
