// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting C entities
//!
//! This visitor traverses the tree-sitter AST and extracts:
//! - Functions (with complexity metrics)
//! - Structs, unions, enums
//! - Include directives
//! - Function calls (for call graph building)

use codegraph_parser_api::{
    ClassEntity, ComplexityBuilder, ComplexityMetrics, Field, FunctionEntity, ImportRelation,
    Parameter, BODY_PREFIX_MAX_CHARS,
    truncate_body_prefix,
};
use tree_sitter::Node;

/// Represents a function call found in the source
#[derive(Debug, Clone)]
pub struct FunctionCall {
    /// Name of the called function
    pub callee: String,
    /// Line number where the call occurs
    pub line: usize,
    /// Name of the calling function (if inside a function)
    pub caller: Option<String>,
    /// For ops struct assignments: the struct type name (e.g., "net_device_ops")
    pub struct_type: Option<String>,
    /// For ops struct assignments: the field name (e.g., "ndo_open")
    pub field_name: Option<String>,
}

pub struct CVisitor<'a> {
    pub source: &'a [u8],
    pub functions: Vec<FunctionEntity>,
    pub structs: Vec<ClassEntity>,
    pub imports: Vec<ImportRelation>,
    /// Function calls extracted from the source
    pub calls: Vec<FunctionCall>,
    /// Functions registered via module_init/module_exit (entry points)
    pub entry_points: Vec<String>,
    /// Functions registered via EXPORT_SYMBOL/EXPORT_SYMBOL_GPL (public API)
    pub exported_symbols: Vec<String>,
    /// Whether to extract function calls
    extract_calls: bool,
    /// Current function being visited (for tracking caller)
    current_function: Option<String>,
}

impl<'a> CVisitor<'a> {
    pub fn new(source: &'a [u8]) -> Self {
        Self {
            source,
            functions: Vec::new(),
            structs: Vec::new(),
            imports: Vec::new(),
            calls: Vec::new(),
            entry_points: Vec::new(),
            exported_symbols: Vec::new(),
            extract_calls: false,
            current_function: None,
        }
    }

    /// Enable or disable call extraction
    pub fn set_extract_calls(&mut self, extract: bool) {
        self.extract_calls = extract;
    }

    fn node_text(&self, node: Node) -> String {
        node.utf8_text(self.source).unwrap_or("").to_string()
    }

    pub fn visit_node(&mut self, node: Node) {
        // Skip ERROR nodes - tree-sitter marks unparseable sections as ERROR
        // We continue visiting children to extract what we can
        if node.is_error() {
            // Still visit children of ERROR nodes to extract valid nested content
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                self.visit_node(child);
            }
            return;
        }

        match node.kind() {
            "function_definition" => self.visit_function(node),
            "struct_specifier" => self.visit_struct(node),
            "union_specifier" => self.visit_union(node),
            "enum_specifier" => self.visit_enum(node),
            "preproc_include" => self.visit_include(node),
            "call_expression" if self.extract_calls => self.visit_call(node),
            "initializer_pair" if self.extract_calls => {
                self.visit_initializer_pair(node);
            }
            _ => {}
        }

        // Don't recurse into function bodies for top-level visits
        // (we handle them specially in visit_function)
        if node.kind() != "function_definition" {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                self.visit_node(child);
            }
        } else {
            // For function definitions, only recurse into the non-body children
            // (call extraction is handled inside visit_function while current_function is set)
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() != "compound_statement" {
                    self.visit_node(child);
                }
            }
        }
    }

    /// Visit nodes specifically for call extraction (doesn't extract entities)
    fn visit_node_for_calls(&mut self, node: Node) {
        // For ERROR nodes, still recurse into children to extract valid calls
        // within partially-parsed regions (common in macro-heavy kernel code)
        if !node.is_error() && node.kind() == "call_expression" {
            self.visit_call(node);
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node_for_calls(child);
        }
    }

    /// Extract a function call
    fn visit_call(&mut self, node: Node) {
        // call_expression has a "function" field that contains the callee
        if let Some(function_node) = node.child_by_field_name("function") {
            let callee = match function_node.kind() {
                "identifier" => self.node_text(function_node),
                "field_expression" => {
                    // For method calls like obj->method or obj.method
                    if let Some(field) = function_node.child_by_field_name("field") {
                        self.node_text(field)
                    } else {
                        self.node_text(function_node)
                    }
                }
                "parenthesized_expression" => {
                    // Function pointer call: (*func_ptr)(args)
                    "(*indirect)".to_string()
                }
                _ => self.node_text(function_node),
            };

            if !callee.is_empty() {
                // Detect kernel registration macros at top level
                if self.current_function.is_none() {
                    match callee.as_str() {
                        "module_init" | "module_exit" | "late_initcall" | "subsys_initcall"
                        | "device_initcall" => {
                            // Extract the argument (the registered function name)
                            if let Some(args) = node.child_by_field_name("arguments") {
                                if let Some(arg) = args.named_child(0) {
                                    let func_name = self.node_text(arg);
                                    if !func_name.is_empty() {
                                        self.entry_points.push(func_name);
                                    }
                                }
                            }
                            return;
                        }
                        "EXPORT_SYMBOL"
                        | "EXPORT_SYMBOL_GPL"
                        | "EXPORT_SYMBOL_NS"
                        | "EXPORT_SYMBOL_NS_GPL" => {
                            if let Some(args) = node.child_by_field_name("arguments") {
                                if let Some(arg) = args.named_child(0) {
                                    let func_name = self.node_text(arg);
                                    if !func_name.is_empty() {
                                        self.exported_symbols.push(func_name);
                                    }
                                }
                            }
                            return;
                        }
                        _ => {}
                    }
                }

                self.calls.push(FunctionCall {
                    callee: callee.clone(),
                    line: node.start_position().row + 1,
                    caller: self.current_function.clone(),
                    struct_type: None,
                    field_name: None,
                });

                // Extract function pointer arguments — bare identifiers passed
                // as arguments that look like function names (callbacks).
                // e.g., request_irq(irq, ice_misc_intr, ...) → ice_misc_intr is a call target
                if self.current_function.is_some() {
                    if let Some(args) = node.child_by_field_name("arguments") {
                        let mut cursor = args.walk();
                        for arg in args.children(&mut cursor) {
                            if arg.kind() == "identifier" {
                                let arg_name = self.node_text(arg);
                                // Filter: must look like a function name, not a variable
                                // Heuristic: starts with letter/underscore, contains underscore
                                // (most C function names do), not a common keyword/type
                                if !arg_name.is_empty()
                                    && arg_name.contains('_')
                                    && !Self::is_common_identifier(&arg_name)
                                    && arg_name != callee
                                {
                                    self.calls.push(FunctionCall {
                                        callee: arg_name,
                                        line: node.start_position().row + 1,
                                        caller: self.current_function.clone(),
                                        struct_type: None,
                                        field_name: None,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Check if an identifier is a common variable/type name (not a function pointer).
    fn is_common_identifier(name: &str) -> bool {
        matches!(
            name,
            "NULL"
                | "null"
                | "true"
                | "false"
                | "TRUE"
                | "FALSE"
                | "GFP_KERNEL"
                | "GFP_ATOMIC"
                | "IRQF_SHARED"
                | "THIS_MODULE"
                | "ARRAY_SIZE"
        )
    }

    fn visit_function(&mut self, node: Node) {
        let mut name = String::new();
        let mut return_type = String::new();
        let mut parameters = Vec::new();
        let mut is_static = false;

        // Check for storage class specifier (static)
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "storage_class_specifier" {
                let text = self.node_text(child);
                if text == "static" {
                    is_static = true;
                }
            }
        }

        // Extract return type
        if let Some(type_node) = node.child_by_field_name("type") {
            return_type = self.extract_type_string(type_node);
        }

        // Extract function name and parameters from declarator
        if let Some(declarator) = node.child_by_field_name("declarator") {
            self.extract_function_declarator(declarator, &mut name, &mut parameters);
        }

        // Set current function for call tracking
        let prev_function = self.current_function.take();
        if !name.is_empty() {
            self.current_function = Some(name.clone());
        }

        // Calculate complexity from function body
        let complexity = node
            .child_by_field_name("body")
            .map(|body| self.calculate_complexity(body));

        let visibility = if is_static { "private" } else { "public" };

        let signature = self
            .node_text(node)
            .lines()
            .next()
            .unwrap_or("")
            .to_string();

        let body_prefix = node
            .child_by_field_name("body")
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| {
                truncate_body_prefix(t)
            })
            .map(|t| t.to_string());

        let func = FunctionEntity {
            name,
            signature,
            visibility: visibility.to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_async: false,
            is_test: false,
            is_static,
            is_abstract: false,
            parameters,
            return_type: if return_type.is_empty() {
                None
            } else {
                Some(return_type)
            },
            doc_comment: None,
            attributes: Vec::new(),
            parent_class: None,
            complexity,
            body_prefix,
        };
        self.functions.push(func);

        // Extract calls from function body while current_function is still set
        if self.extract_calls {
            if let Some(body) = node.child_by_field_name("body") {
                self.visit_node_for_calls(body);
            }
        }

        // Restore previous function context
        self.current_function = prev_function;
    }

    fn extract_function_declarator(
        &self,
        node: Node,
        name: &mut String,
        parameters: &mut Vec<Parameter>,
    ) {
        match node.kind() {
            "function_declarator" => {
                // Get function name from nested declarator
                if let Some(decl) = node.child_by_field_name("declarator") {
                    *name = self.extract_identifier(decl);
                }
                // Get parameters
                if let Some(params) = node.child_by_field_name("parameters") {
                    self.extract_parameters(params, parameters);
                }
            }
            "pointer_declarator" => {
                // Handle pointer return type: int *func()
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "function_declarator" {
                        self.extract_function_declarator(child, name, parameters);
                        return;
                    }
                }
            }
            "identifier" => {
                *name = self.node_text(node);
            }
            _ => {}
        }
    }

    fn extract_identifier(&self, node: Node) -> String {
        match node.kind() {
            "identifier" => self.node_text(node),
            "pointer_declarator" => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    let id = self.extract_identifier(child);
                    if !id.is_empty() {
                        return id;
                    }
                }
                String::new()
            }
            _ => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    let id = self.extract_identifier(child);
                    if !id.is_empty() {
                        return id;
                    }
                }
                String::new()
            }
        }
    }

    fn extract_parameters(&self, node: Node, parameters: &mut Vec<Parameter>) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "parameter_declaration" {
                if let Some(param) = self.extract_parameter(child) {
                    parameters.push(param);
                }
            } else if child.kind() == "variadic_parameter" {
                parameters.push(Parameter {
                    name: "...".to_string(),
                    type_annotation: Some("...".to_string()),
                    default_value: None,
                    is_variadic: true,
                });
            }
        }
    }

    fn extract_parameter(&self, node: Node) -> Option<Parameter> {
        let mut type_str = String::new();
        let mut name = String::new();

        // Extract type
        if let Some(type_node) = node.child_by_field_name("type") {
            type_str = self.extract_type_string(type_node);
        }

        // Extract name from declarator
        if let Some(declarator) = node.child_by_field_name("declarator") {
            let (decl_name, pointer_prefix) = self.extract_declarator_info(declarator);
            name = decl_name;
            if !pointer_prefix.is_empty() {
                type_str = format!("{type_str}{pointer_prefix}");
            }
        }

        // Handle case where there's no declarator (just type)
        if name.is_empty() {
            name = "param".to_string();
        }

        Some(Parameter {
            name,
            type_annotation: if type_str.is_empty() {
                None
            } else {
                Some(type_str)
            },
            default_value: None,
            is_variadic: false,
        })
    }

    fn extract_declarator_info(&self, node: Node) -> (String, String) {
        match node.kind() {
            "identifier" | "field_identifier" => (self.node_text(node), String::new()),
            "pointer_declarator" => {
                let mut pointer_count = 0;
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "*" {
                        pointer_count += 1;
                    } else {
                        let (name, extra_ptrs) = self.extract_declarator_info(child);
                        if !name.is_empty() {
                            return (name, "*".repeat(pointer_count) + &extra_ptrs);
                        }
                    }
                }
                (String::new(), "*".repeat(pointer_count))
            }
            "array_declarator" => {
                if let Some(decl) = node.child_by_field_name("declarator") {
                    let (name, _) = self.extract_declarator_info(decl);
                    return (name, "[]".to_string());
                }
                (String::new(), "[]".to_string())
            }
            _ => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    let (name, ptrs) = self.extract_declarator_info(child);
                    if !name.is_empty() {
                        return (name, ptrs);
                    }
                }
                (String::new(), String::new())
            }
        }
    }

    fn extract_type_string(&self, node: Node) -> String {
        let text = self.node_text(node);
        text.trim().to_string()
    }

    fn visit_struct(&mut self, node: Node) {
        // Only extract structs that have a body (not forward declarations)
        let has_body = node.child_by_field_name("body").is_some();
        if !has_body {
            return;
        }

        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_else(|| format!("__anon_struct_{}", node.start_position().row + 1));

        let fields = self.extract_struct_fields(node);

        let body_prefix = node
            .child_by_field_name("body")
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| {
                truncate_body_prefix(t)
            })
            .map(|t| t.to_string());

        let struct_entity = ClassEntity {
            name,
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_abstract: false,
            is_interface: false,
            base_classes: Vec::new(),
            implemented_traits: Vec::new(),
            methods: Vec::new(),
            fields,
            doc_comment: None,
            attributes: vec!["struct".to_string()],
            type_parameters: Vec::new(),
            body_prefix,
        };
        self.structs.push(struct_entity);
    }

    fn visit_union(&mut self, node: Node) {
        // Only extract unions that have a body
        let has_body = node.child_by_field_name("body").is_some();
        if !has_body {
            return;
        }

        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_else(|| format!("__anon_union_{}", node.start_position().row + 1));

        let fields = self.extract_struct_fields(node);

        let body_prefix = node
            .child_by_field_name("body")
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| {
                truncate_body_prefix(t)
            })
            .map(|t| t.to_string());

        let union_entity = ClassEntity {
            name,
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_abstract: false,
            is_interface: false,
            base_classes: Vec::new(),
            implemented_traits: Vec::new(),
            methods: Vec::new(),
            fields,
            doc_comment: None,
            attributes: vec!["union".to_string()],
            type_parameters: Vec::new(),
            body_prefix,
        };
        self.structs.push(union_entity);
    }

    fn visit_enum(&mut self, node: Node) {
        // Only extract enums that have a body
        let has_body = node.child_by_field_name("body").is_some();
        if !has_body {
            return;
        }

        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_else(|| format!("__anon_enum_{}", node.start_position().row + 1));

        // Extract enum constants as fields
        let mut fields = Vec::new();
        if let Some(body) = node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for child in body.children(&mut cursor) {
                if child.kind() == "enumerator" {
                    let enumerator_name = child
                        .child_by_field_name("name")
                        .map(|n| self.node_text(n))
                        .unwrap_or_default();

                    let default_value = child
                        .child_by_field_name("value")
                        .map(|n| self.node_text(n));

                    if !enumerator_name.is_empty() {
                        fields.push(Field {
                            name: enumerator_name,
                            type_annotation: Some("int".to_string()),
                            visibility: "public".to_string(),
                            is_static: true,
                            is_constant: true,
                            default_value,
                        });
                    }
                }
            }
        }

        let body_prefix = node
            .child_by_field_name("body")
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| {
                truncate_body_prefix(t)
            })
            .map(|t| t.to_string());

        let enum_entity = ClassEntity {
            name,
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_abstract: false,
            is_interface: false,
            base_classes: Vec::new(),
            implemented_traits: Vec::new(),
            methods: Vec::new(),
            fields,
            doc_comment: None,
            attributes: vec!["enum".to_string()],
            type_parameters: Vec::new(),
            body_prefix,
        };
        self.structs.push(enum_entity);
    }

    fn extract_struct_fields(&self, node: Node) -> Vec<Field> {
        let mut fields = Vec::new();

        if let Some(body) = node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for child in body.children(&mut cursor) {
                if child.kind() == "field_declaration" {
                    let field_type = child
                        .child_by_field_name("type")
                        .map(|n| self.extract_type_string(n))
                        .unwrap_or_default();

                    // Try to get declarator - field can have multiple declarators
                    // e.g., "int x, y;" has two declarators
                    let mut field_cursor = child.walk();
                    for field_child in child.children(&mut field_cursor) {
                        // Handle different declarator types
                        match field_child.kind() {
                            "field_identifier" => {
                                // Simple field: int x;
                                let field_name = self.node_text(field_child);
                                if !field_name.is_empty() {
                                    fields.push(Field {
                                        name: field_name,
                                        type_annotation: Some(field_type.clone()),
                                        visibility: "public".to_string(),
                                        is_static: false,
                                        is_constant: false,
                                        default_value: None,
                                    });
                                }
                            }
                            "pointer_declarator" | "array_declarator" => {
                                // Pointer/array field: char *name; int arr[10];
                                let (field_name, pointer_suffix) =
                                    self.extract_declarator_info(field_child);
                                let full_type = if pointer_suffix.is_empty() {
                                    field_type.clone()
                                } else {
                                    format!("{field_type}{pointer_suffix}")
                                };

                                if !field_name.is_empty() {
                                    fields.push(Field {
                                        name: field_name,
                                        type_annotation: Some(full_type),
                                        visibility: "public".to_string(),
                                        is_static: false,
                                        is_constant: false,
                                        default_value: None,
                                    });
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        fields
    }

    fn visit_include(&mut self, node: Node) {
        let mut path = String::new();
        let mut is_system = false;

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "system_lib_string" => {
                    // Remove < and >
                    path = self.node_text(child);
                    path = path
                        .trim_start_matches('<')
                        .trim_end_matches('>')
                        .to_string();
                    is_system = true;
                }
                "string_literal" => {
                    // Remove quotes
                    path = self.node_text(child);
                    path = path.trim_matches('"').to_string();
                    is_system = false;
                }
                _ => {}
            }
        }

        if !path.is_empty() {
            let import = ImportRelation {
                importer: "current_file".to_string(),
                imported: path,
                symbols: Vec::new(),
                is_wildcard: true, // C includes always import everything
                alias: if is_system {
                    Some("system".to_string())
                } else {
                    None
                },
            };
            self.imports.push(import);
        }
    }

    /// Extract function pointer assignments from struct initializers.
    ///
    /// Detects patterns like:
    /// ```c
    /// static DeviceOps ops = {
    ///     .getStats = my_get_stats,
    ///     .attach = my_attach,
    /// };
    /// ```
    /// Records each `.field = func_name` as a call relationship, making
    /// vtable-registered functions visible to callers/find_unused_code.
    fn visit_initializer_pair(&mut self, node: Node) {
        // initializer_pair: field_designator "=" value
        // We want: value is an identifier (function pointer, not a literal)
        let mut field_name = String::new();
        let mut value_name = String::new();

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "field_designator" => {
                    if let Some(field) = child.child(1) {
                        // child(0) is ".", child(1) is field_identifier
                        if field.kind() == "field_identifier" {
                            field_name = self.node_text(field);
                        }
                    }
                }
                "identifier" => {
                    value_name = self.node_text(child);
                }
                _ => {}
            }
        }

        // Only record if the value looks like a function name (not NULL, not a number)
        if !value_name.is_empty()
            && value_name != "NULL"
            && value_name != "null"
            && !value_name.starts_with(|c: char| c.is_ascii_digit())
        {
            // Walk up the tree to find the struct type from the parent declaration:
            //   declaration → type_identifier (e.g., "net_device_ops")
            //   or: declaration → struct_specifier → type_identifier
            let struct_type = Self::find_parent_struct_type(node, self.source);

            let caller = self
                .current_function
                .clone()
                .unwrap_or_else(|| format!("vtable_{}", field_name));

            self.calls.push(FunctionCall {
                callee: value_name,
                line: node.start_position().row + 1,
                caller: Some(caller),
                struct_type,
                field_name: if field_name.is_empty() {
                    None
                } else {
                    Some(field_name)
                },
            });
        }
    }

    /// Walk up the AST from an initializer_pair to find the struct type name
    /// from the enclosing declaration.
    ///
    /// Handles patterns like:
    /// - `static const struct net_device_ops ops = { .field = fn };`
    /// - `DeviceOps ops = { .field = fn };`
    /// - `static struct fuse_operations vmblockOps = { .field = fn };`
    fn find_parent_struct_type(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
        let mut current = node;
        // Walk up at most 5 levels to find the declaration
        for _ in 0..5 {
            current = current.parent()?;
            if current.kind() == "declaration" {
                // Look for type identifier or struct specifier in the declaration
                let mut cursor = current.walk();
                for child in current.children(&mut cursor) {
                    match child.kind() {
                        // Direct type name: `DeviceOps ops = { ... };`
                        "type_identifier" => {
                            let text = child.utf8_text(source).unwrap_or("").to_string();
                            if !text.is_empty() {
                                return Some(text);
                            }
                        }
                        // Struct specifier: `struct net_device_ops ops = { ... };`
                        "struct_specifier" => {
                            let mut inner = child.walk();
                            for sc in child.children(&mut inner) {
                                if sc.kind() == "type_identifier" {
                                    let text = sc.utf8_text(source).unwrap_or("").to_string();
                                    if !text.is_empty() {
                                        return Some(text);
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
                return None;
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
        // Recurse through ERROR nodes to count complexity in partially-parsed regions
        if !node.is_error() {
            match node.kind() {
                "if_statement" => {
                    builder.add_branch();
                    builder.enter_scope();
                }
                "else_clause" => {
                    builder.add_branch();
                }
                "for_statement" => {
                    builder.add_loop();
                    builder.enter_scope();
                }
                "while_statement" => {
                    builder.add_loop();
                    builder.enter_scope();
                }
                "do_statement" => {
                    builder.add_loop();
                    builder.enter_scope();
                }
                "switch_statement" => {
                    builder.enter_scope();
                }
                "case_statement" => {
                    builder.add_branch();
                }
                "default_statement" => {
                    builder.add_branch();
                }
                "conditional_expression" => {
                    builder.add_branch();
                }
                "goto_statement" => {
                    builder.add_branch();
                }
                "binary_expression" => {
                    if let Some(op) = node.child_by_field_name("operator") {
                        let op_text = self.node_text(op);
                        if op_text == "&&" || op_text == "||" {
                            builder.add_logical_operator();
                        }
                    }
                }
                "return_statement" => {}
                _ => {}
            }
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_for_complexity(child, builder);
        }

        // Exit scope for control structures (only for non-error nodes)
        if !node.is_error() {
            match node.kind() {
                "if_statement" | "for_statement" | "while_statement" | "do_statement"
                | "switch_statement" => {
                    builder.exit_scope();
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::Parser;

    fn parse_and_visit(source: &[u8]) -> CVisitor<'_> {
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_c::LANGUAGE.into()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = CVisitor::new(source);
        visitor.visit_node(tree.root_node());
        visitor
    }

    #[test]
    fn test_visitor_basics() {
        let visitor = CVisitor::new(b"int main() {}");
        assert_eq!(visitor.functions.len(), 0);
        assert_eq!(visitor.structs.len(), 0);
        assert_eq!(visitor.imports.len(), 0);
    }

    #[test]
    fn test_visitor_function_extraction() {
        let source = b"int greet(char *name) { return 0; }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "greet");
        assert_eq!(visitor.functions[0].return_type, Some("int".to_string()));
    }

    #[test]
    fn test_visitor_static_function() {
        let source = b"static void helper() {}";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].visibility, "private");
        assert!(visitor.functions[0].is_static);
    }

    #[test]
    fn test_visitor_struct_extraction() {
        let source = b"struct Person { char *name; int age; };";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.structs.len(), 1);
        assert_eq!(visitor.structs[0].name, "Person");
        assert_eq!(visitor.structs[0].fields.len(), 2);
        assert!(visitor.structs[0]
            .attributes
            .contains(&"struct".to_string()));
    }

    #[test]
    fn test_visitor_union_extraction() {
        let source = b"union Data { int i; float f; };";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.structs.len(), 1);
        assert_eq!(visitor.structs[0].name, "Data");
        assert!(visitor.structs[0].attributes.contains(&"union".to_string()));
    }

    #[test]
    fn test_visitor_enum_extraction() {
        let source = b"enum Color { RED, GREEN, BLUE };";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.structs.len(), 1);
        assert_eq!(visitor.structs[0].name, "Color");
        assert!(visitor.structs[0].attributes.contains(&"enum".to_string()));
        assert_eq!(visitor.structs[0].fields.len(), 3);
    }

    #[test]
    fn test_visitor_system_include() {
        let source = b"#include <stdio.h>";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "stdio.h");
        assert_eq!(visitor.imports[0].alias, Some("system".to_string()));
    }

    #[test]
    fn test_visitor_local_include() {
        let source = b"#include \"myheader.h\"";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "myheader.h");
        assert_eq!(visitor.imports[0].alias, None);
    }

    #[test]
    fn test_visitor_multiple_includes() {
        let source = b"#include <stdio.h>\n#include <stdlib.h>\n#include \"myheader.h\"";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 3);
    }

    #[test]
    fn test_visitor_function_with_params() {
        let source = b"int add(int a, int b) { return a + b; }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].parameters.len(), 2);
        assert_eq!(visitor.functions[0].parameters[0].name, "a");
        assert_eq!(visitor.functions[0].parameters[1].name, "b");
    }

    #[test]
    fn test_visitor_pointer_params() {
        let source = b"void process(int *arr, char **argv) {}";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].parameters.len(), 2);
        // Pointer types should be captured
        let param1 = &visitor.functions[0].parameters[0];
        assert!(param1
            .type_annotation
            .as_ref()
            .map(|t| t.contains("*"))
            .unwrap_or(false));
    }

    #[test]
    fn test_visitor_variadic_function() {
        let source = b"int printf(const char *fmt, ...) { return 0; }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let params = &visitor.functions[0].parameters;
        assert!(params.iter().any(|p| p.is_variadic));
    }

    #[test]
    fn test_visitor_complexity_if() {
        let source = b"void test() { if (1) {} }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(complexity.branches >= 1);
    }

    #[test]
    fn test_visitor_complexity_loop() {
        let source = b"void test() { for (int i = 0; i < 10; i++) {} while(1) {} }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(complexity.loops >= 2);
    }

    #[test]
    fn test_visitor_complexity_switch() {
        let source =
            b"void test(int x) { switch(x) { case 1: break; case 2: break; default: break; } }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(complexity.branches >= 3);
    }

    #[test]
    fn test_visitor_complexity_logical_operators() {
        let source = b"void test(int a, int b) { if (a && b || a) {} }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        assert!(complexity.logical_operators >= 2);
    }

    #[test]
    fn test_visitor_complexity_goto() {
        let source = b"void test() { label: goto label; }";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let complexity = visitor.functions[0].complexity.as_ref().unwrap();
        // goto should add to branches
        assert!(complexity.branches >= 1);
    }

    #[test]
    fn test_visitor_forward_declaration_ignored() {
        let source = b"struct Forward;";
        let visitor = parse_and_visit(source);

        // Forward declarations should not be extracted
        assert_eq!(visitor.structs.len(), 0);
    }

    #[test]
    fn test_visitor_anonymous_struct() {
        let source = b"struct { int x; int y; } point;";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.structs.len(), 1);
        // Anonymous struct should get a generated name
        assert!(visitor.structs[0].name.starts_with("__anon_struct_"));
    }

    #[test]
    fn test_visitor_enum_with_values() {
        let source = b"enum Size { SMALL = 1, MEDIUM = 5, LARGE = 10 };";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.structs.len(), 1);
        let enum_entity = &visitor.structs[0];
        assert_eq!(enum_entity.fields.len(), 3);

        // Check that values are captured
        let small = enum_entity.fields.iter().find(|f| f.name == "SMALL");
        assert!(small.is_some());
        assert_eq!(small.unwrap().default_value, Some("1".to_string()));
    }

    #[test]
    fn test_vmk_complexity_computed_correctly() {
        // Test the EXACT path: parse → visitor → complexity
        let source = b"VMK_ReturnStatus\nirndrv_RDMAOpGetPrivStats(vmk_AddrCookie driverData, char *statBuf,\n                          vmk_ByteCount length)\n{\n   irndrv_Pf *pf = (irndrv_Pf *)driverData.ptr;\n   vmk_ByteCount outLen;\n   VMK_ReturnStatus status;\n\n   if (length < 100) {\n      return VMK_BAD_PARAM;\n   }\n\n   for (int i = 0; i < 10; i++) {\n      vmk_Memset(statBuf, 0, length);\n   }\n\n   while (status != 0) {\n      status = vmk_AtomicRead16(&flag);\n   }\n\n   return VMK_OK;\n}\n";

        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 1);
        let func = &visitor.functions[0];
        println!("name={} complexity={:?}", func.name, func.complexity);
        let cx = func.complexity.as_ref().expect("should have complexity");
        println!(
            "  cyclomatic={} branches={} loops={}",
            cx.cyclomatic_complexity, cx.branches, cx.loops
        );
        assert!(
            cx.cyclomatic_complexity > 1,
            "Expected complexity > 1, got {}",
            cx.cyclomatic_complexity
        );
    }
}
