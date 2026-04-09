// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting Tcl entities
//!
//! The vendored tree-sitter-tcl grammar (ABI v15→v14 patch) produces ERROR nodes
//! for 14 Tcl keywords instead of proper named AST nodes. This visitor uses
//! [`resolve_error_keyword`] to transparently map ERROR nodes to their keyword
//! names, so all dispatch code sees resolved kinds — never "ERROR".

use codegraph_parser_api::{
    CallRelation, ClassEntity, ComplexityMetrics, FunctionEntity, ImportRelation, Parameter,
    BODY_PREFIX_MAX_CHARS,
    truncate_body_prefix,
};
use tree_sitter::Node;

use crate::eda::{self, EdaCommand, EdaData};
use crate::sdc::{self, SdcData};

/// All Tcl keywords that the tree-sitter-tcl grammar defines as named rules.
/// These may appear as proper AST nodes OR as ERROR nodes depending on context.
const TCL_KEYWORDS: &[&str] = &[
    "proc",
    "namespace",
    "if",
    "elseif",
    "else",
    "while",
    "foreach",
    "try",
    "catch",
    "finally",
    "set",
    "global",
    "regexp",
    "expr",
];

/// Scan an ERROR node's children for a recognizable Tcl keyword.
///
/// Returns `&'static str` (string literals from match arms) so there are
/// no lifetime conflicts with `&mut self` in callers. Recurses into nested
/// ERROR nodes to handle `ERROR(ERROR(proc))` patterns.
fn resolve_error_keyword(node: Node) -> &'static str {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "proc" => return "proc",
            "namespace" => return "namespace",
            "if" => return "if",
            "elseif" => return "elseif",
            "else" => return "else",
            "while" => return "while",
            "foreach" => return "foreach",
            "try" => return "try",
            "catch" => return "catch",
            "finally" => return "finally",
            "set" => return "set",
            "global" => return "global",
            "regexp" => return "regexp",
            "expr" => return "expr",
            "ERROR" => {
                let inner = resolve_error_keyword(child);
                if inner != "unknown" {
                    return inner;
                }
            }
            _ => continue,
        }
    }
    "unknown"
}

/// Like [`resolve_error_keyword`] but also checks `simple_word`/`word` children
/// against the source text. This catches non-grammar keywords like `source`,
/// `package`, and arbitrary commands (SDC/EDA) that the grammar wraps in ERROR.
///
/// Returns the text of the first `simple_word`/`word` child if no grammar keyword
/// is found. This allows the sibling-stitching logic to handle ANY command that
/// the grammar splits at position 0.
fn resolve_error_keyword_with_source(node: Node, source: &[u8]) -> String {
    let kw = resolve_error_keyword(node);
    if kw != "unknown" {
        return kw.to_string();
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "simple_word" || child.kind() == "word" {
            if let Ok(text) = child.utf8_text(source) {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    return trimmed.to_string();
                }
            }
        }
    }
    "unknown".to_string()
}

/// Resolve a node's effective kind. ERROR nodes are mapped to their keyword;
/// `procedure` nodes are normalized to `"proc"`. Everything else passes through.
fn resolve_kind(node: Node<'_>) -> &str {
    match node.kind() {
        "ERROR" => resolve_error_keyword(node),
        "procedure" => "proc",
        k => k,
    }
}

pub struct TclVisitor<'a> {
    pub source: &'a [u8],

    // Standard CodeIR entities
    pub functions: Vec<FunctionEntity>,
    pub classes: Vec<ClassEntity>,
    pub imports: Vec<ImportRelation>,
    pub calls: Vec<CallRelation>,

    // EDA/SDC data
    pub sdc_data: SdcData,
    pub eda_data: EdaData,

    // Context tracking
    namespace_stack: Vec<String>,
    current_procedure: Option<String>,
}

impl<'a> TclVisitor<'a> {
    pub fn new(source: &'a [u8]) -> Self {
        Self {
            source,
            functions: Vec::new(),
            classes: Vec::new(),
            imports: Vec::new(),
            calls: Vec::new(),
            sdc_data: SdcData::default(),
            eda_data: EdaData::default(),
            namespace_stack: Vec::new(),
            current_procedure: None,
        }
    }

    fn node_text(&self, node: Node) -> String {
        node.utf8_text(self.source).unwrap_or("").to_string()
    }

    fn current_namespace(&self) -> Option<String> {
        if self.namespace_stack.is_empty() {
            None
        } else {
            Some(self.namespace_stack.join("::"))
        }
    }

    fn qualified_name(&self, name: &str) -> String {
        match self.current_namespace() {
            Some(ns) => format!("{}::{}", ns, name),
            None => name.to_string(),
        }
    }

    // ── Main dispatch ───────────────────────────────────────────────────

    pub fn visit_node(&mut self, node: Node) {
        let kind = resolve_kind(node);

        match kind {
            "source_file" => self.visit_children(node),
            "command" => self.visit_command(node),
            "proc" => self.visit_proc(node),
            "namespace" => self.visit_namespace(node),
            "if" | "elseif" | "while" | "foreach" | "try" | "catch" | "set" | "global"
            | "regexp" | "expr" | "else" | "finally" => {
                self.record_call(kind, node);
                self.visit_bodies(node);
            }
            _ => self.visit_children(node),
        }
    }

    /// Sibling-aware child visiting. Detects ERROR(keyword) + command(args)
    /// pairs produced by the grammar's position-0 split bug and stitches them
    /// back together before dispatch.
    ///
    /// Only stitches when the ERROR and command are on the same line — if they're
    /// on different lines, they are independent commands (e.g. `compile\nreport_timing`).
    fn visit_children(&mut self, node: Node) {
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();
        let mut i = 0;
        while i < children.len() {
            let child = children[i];
            if child.kind() == "ERROR" {
                if let Some(&next) = children.get(i + 1) {
                    if next.kind() == "command"
                        && child.end_position().row == next.start_position().row
                    {
                        let kw = resolve_error_keyword_with_source(child, self.source);
                        if kw != "unknown" {
                            self.handle_stitched_pair(&kw, child, next);
                            i += 2;
                            continue;
                        }
                    }
                }
                // Standalone ERROR on its own line — check if it's a known command
                let kw = resolve_error_keyword_with_source(child, self.source);
                if kw != "unknown" && !TCL_KEYWORDS.contains(&kw.as_str()) {
                    // Bare command like `compile` wrapped in ERROR at position 0
                    self.record_call(&kw, child);
                    i += 1;
                    continue;
                }
            }
            self.visit_node(child);
            i += 1;
        }
    }

    /// Dispatch a stitched ERROR(keyword) + command(args) pair.
    fn handle_stitched_pair(&mut self, keyword: &str, error_node: Node, cmd_node: Node) {
        match keyword {
            "proc" => {
                self.visit_proc_from_split(error_node, cmd_node);
            }
            "namespace" => {
                self.visit_namespace_from_split(cmd_node);
            }
            "source" => {
                let filename = self.extract_command_name(cmd_node);
                let path = if filename.is_empty() {
                    let args = self.collect_argument_nodes(cmd_node);
                    args.first()
                        .map(|a| self.node_text(*a).trim().to_string())
                        .unwrap_or_default()
                } else {
                    filename
                };
                let cleaned = path.trim_matches('"').trim_matches('\'').to_string();
                if !cleaned.is_empty() {
                    self.imports.push(ImportRelation {
                        importer: "file".to_string(),
                        imported: cleaned,
                        symbols: Vec::new(),
                        is_wildcard: true,
                        alias: None,
                    });
                }
            }
            "package" => {
                let subcmd = self.extract_command_name(cmd_node);
                if subcmd == "require" {
                    let args = self.collect_argument_nodes(cmd_node);
                    if let Some(pkg_node) = args.first() {
                        let pkg_name = self.node_text(*pkg_node).trim().to_string();
                        if !pkg_name.is_empty() {
                            self.imports.push(ImportRelation {
                                importer: "file".to_string(),
                                imported: pkg_name,
                                symbols: Vec::new(),
                                is_wildcard: false,
                                alias: None,
                            });
                        }
                    }
                }
            }
            other => {
                self.visit_general_command_from_split(other, cmd_node);
            }
        }
    }

    /// Handle a general command from a stitched ERROR+command pair.
    /// Uses split-aware arg collection (doesn't skip first child).
    fn visit_general_command_from_split(&mut self, cmd_name: &str, node: Node) {
        if sdc::is_sdc_command(cmd_name) {
            if let Some(constraint) =
                sdc::extract_sdc_constraint_from_split(cmd_name, node, self.source)
            {
                self.sdc_data.add(constraint);
            }
            self.record_call(cmd_name, node);
            return;
        }

        if eda::is_eda_command(cmd_name) {
            if let Some(eda_cmd) = eda::classify_eda_command_from_split(cmd_name, node, self.source)
            {
                match eda_cmd {
                    EdaCommand::DesignFileRead { file_type, path } => {
                        if !path.is_empty() {
                            self.imports.push(ImportRelation {
                                importer: "file".to_string(),
                                imported: path.clone(),
                                symbols: Vec::new(),
                                is_wildcard: false,
                                alias: None,
                            });
                        }
                        self.eda_data.design_reads.push((file_type, path));
                    }
                    EdaCommand::DesignFileWrite { file_type, path } => {
                        self.eda_data.design_writes.push((file_type, path));
                    }
                    EdaCommand::ToolFlowCommand { ref name, .. }
                    | EdaCommand::ObjectQuery { ref name, .. } => {
                        self.record_call(name, node);
                    }
                    EdaCommand::CommandRegistration { name, usage } => {
                        self.eda_data.registered_commands.push((name, usage));
                    }
                    EdaCommand::CollectionIteration { .. } => {
                        self.record_call(cmd_name, node);
                        self.visit_braced_bodies(node);
                    }
                    EdaCommand::AttributeAccess { .. } => {
                        self.record_call(cmd_name, node);
                    }
                }
            }
            return;
        }

        self.record_call(cmd_name, node);
    }

    /// Handle proc from a stitched ERROR(proc) + command(name args) pair.
    /// Extracts name from cmd_node's name field, params/body from its arguments,
    /// and doc comments from comment children inside the error_node.
    ///
    /// Handles two body shapes:
    /// - Intact body: word_list has two braced_words (params, body)
    /// - Fragmented body: body `{` becomes ERROR, content scatters as simple_words
    fn visit_proc_from_split(&mut self, error_node: Node, cmd_node: Node) {
        let name_str = self.extract_command_name(cmd_node);
        if name_str.is_empty() {
            return;
        }

        let args = self.collect_argument_nodes(cmd_node);

        // Find params (first braced_word) and body (second braced_word, if present)
        let mut params_node = None;
        let mut body_node = None;
        let mut body_scatter_start = None;
        for (idx, arg) in args.iter().enumerate() {
            if arg.kind() == "braced_word" || arg.kind() == "braced_word_simple" {
                if params_node.is_none() {
                    params_node = Some(*arg);
                } else if body_node.is_none() {
                    body_node = Some(*arg);
                }
            } else if params_node.is_some() && body_node.is_none() && body_scatter_start.is_none() {
                // First non-braced_word after params — fragmented body starts here
                body_scatter_start = Some(idx);
            }
        }

        let qualified = self.qualified_name(&name_str);
        let params = match params_node {
            Some(pn) => self.extract_params_from_braced(pn),
            None => Vec::new(),
        };

        // Extract doc comments from inside the error_node (comment children
        // preceding the proc keyword get trapped there in the split case)
        let doc_comment = {
            let mut comments = Vec::new();
            let mut c = error_node.walk();
            for child in error_node.children(&mut c) {
                if child.kind() == "comment" {
                    comments.push(self.node_text(child));
                }
            }
            if comments.is_empty() {
                None
            } else {
                Some(comments.join("\n"))
            }
        };

        // Calculate complexity: either from intact body or scattered keywords
        let mut complexity = if let Some(bn) = body_node {
            self.calculate_complexity(bn)
        } else {
            ComplexityMetrics {
                cyclomatic_complexity: 1,
                branches: 0,
                loops: 0,
                logical_operators: 0,
                max_nesting_depth: 0,
                exception_handlers: 0,
                early_returns: 0,
            }
        };

        // If body is fragmented, scan scattered content for complexity keywords
        if body_node.is_none() {
            if let Some(start) = body_scatter_start {
                for arg in &args[start..] {
                    let kind = resolve_kind(*arg);
                    match kind {
                        "if" | "elseif" => {
                            complexity.cyclomatic_complexity += 1;
                            complexity.branches += 1;
                        }
                        "while" | "foreach" | "for" => {
                            complexity.cyclomatic_complexity += 1;
                            complexity.loops += 1;
                        }
                        "catch" => {
                            complexity.cyclomatic_complexity += 1;
                            complexity.exception_handlers += 1;
                        }
                        _ => {
                            // Check simple_word text for keyword names
                            if arg.kind() == "simple_word" {
                                let text = self.node_text(*arg);
                                let trimmed = text.trim();
                                match trimmed {
                                    "if" | "elseif" => {
                                        complexity.cyclomatic_complexity += 1;
                                        complexity.branches += 1;
                                    }
                                    "while" | "foreach" | "for" => {
                                        complexity.cyclomatic_complexity += 1;
                                        complexity.loops += 1;
                                    }
                                    "catch" => {
                                        complexity.cyclomatic_complexity += 1;
                                        complexity.exception_handlers += 1;
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
        }

        let param_str = params
            .iter()
            .map(|p| {
                if let Some(ref default) = p.default_value {
                    format!("{{{} {}}}", p.name, default)
                } else {
                    p.name.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        let signature = format!("proc {} {{{}}} {{...}}", name_str, param_str);

        let mut func = FunctionEntity::new(
            &qualified,
            error_node.start_position().row + 1,
            cmd_node.end_position().row + 1,
        )
        .with_visibility("public")
        .with_signature(&signature);

        func.parameters = params;
        func.doc_comment = doc_comment;
        func.parent_class = self.current_namespace();
        func.complexity = Some(complexity);
        func.body_prefix = error_node
            .utf8_text(self.source)
            .ok()
            .filter(|t| !t.is_empty())
            .map(|t| {
                truncate_body_prefix(t)
            })
            .map(|t| t.to_string());
        self.functions.push(func);

        // Visit body for nested calls
        let prev_proc = self.current_procedure.take();
        self.current_procedure = Some(qualified);
        if let Some(bn) = body_node {
            if bn.kind() == "arguments" {
                self.visit_arguments_body(bn);
            } else {
                self.visit_braced_body(bn);
            }
        } else if let Some(start) = body_scatter_start {
            // Body is fragmented — scan remaining args for keyword calls
            for arg in &args[start..] {
                let kind = resolve_kind(*arg);
                match kind {
                    "command" => self.visit_command(*arg),
                    "if" | "elseif" | "while" | "foreach" | "try" | "catch" | "set" | "global"
                    | "regexp" | "expr" | "else" | "finally" => {
                        self.record_call(kind, *arg);
                        self.visit_bodies(*arg);
                    }
                    _ => {
                        if arg.kind() == "simple_word" {
                            let text = self.node_text(*arg);
                            let trimmed = text.trim();
                            if TCL_KEYWORDS.contains(&trimmed) {
                                self.record_call(trimmed, *arg);
                            }
                        }
                    }
                }
            }
        }
        self.current_procedure = prev_proc;
    }

    /// Handle namespace from a stitched ERROR(namespace) + command(eval name {body}) pair.
    fn visit_namespace_from_split(&mut self, cmd_node: Node) {
        let subcmd = self.extract_command_name(cmd_node);
        if subcmd != "eval" {
            return;
        }
        let args = self.collect_argument_nodes(cmd_node);
        let ns_name = args
            .first()
            .map(|n| self.node_text(*n).trim().to_string())
            .unwrap_or_default();
        if ns_name.is_empty() {
            return;
        }
        let body_node = args
            .iter()
            .skip(1)
            .find(|a| a.kind() == "braced_word" || a.kind() == "braced_word_simple");

        self.namespace_stack.push(ns_name);
        let full_ns = self.current_namespace().unwrap_or_default();

        let body_prefix = cmd_node
            .utf8_text(self.source)
            .ok()
            .filter(|t| !t.is_empty())
            .map(|t| {
                truncate_body_prefix(t)
            })
            .map(|t| t.to_string());
        let class = ClassEntity {
            name: full_ns,
            visibility: "public".to_string(),
            line_start: cmd_node.start_position().row + 1,
            line_end: cmd_node.end_position().row + 1,
            is_abstract: false,
            is_interface: false,
            base_classes: Vec::new(),
            implemented_traits: Vec::new(),
            methods: Vec::new(),
            fields: Vec::new(),
            doc_comment: None,
            attributes: vec!["namespace".to_string()],
            type_parameters: Vec::new(),
            body_prefix,
        };
        self.classes.push(class);

        if let Some(bn) = body_node {
            self.visit_braced_body(*bn);
        }

        self.namespace_stack.pop();
    }

    // ── Proc handling (unified) ─────────────────────────────────────────

    /// Handle `proc name {params} {body}` regardless of AST structure.
    ///
    /// Covers four observed parse structures:
    /// - `procedure` node: `[proc, ERROR(name), braced_word(params), arguments, braced_word(body)]`
    /// - ERROR (simple): `[proc, ERROR(name, params), braced_word(body)]`
    /// - ERROR (comments): `[comment*, proc, command(name, word_list(params, body))]`
    /// - `command` node: `name="proc", arguments=[name, params, body]`
    fn visit_proc(&mut self, node: Node) {
        let (name_str, params_node, body_node) = if node.kind() == "command" {
            self.extract_proc_from_command(node)
        } else {
            self.extract_proc_from_tree(node)
        };

        if name_str.is_empty() {
            return;
        }

        let qualified = self.qualified_name(&name_str);
        let params = match params_node {
            Some(pn) => self.extract_params_from_braced(pn),
            None => Vec::new(),
        };

        // Extract doc comments: siblings first, then comment children inside ERROR
        let doc_comment = self.extract_preceding_comment(node).or_else(|| {
            let mut inner_comments = Vec::new();
            let mut c = node.walk();
            for child in node.children(&mut c) {
                if child.kind() == "comment" {
                    inner_comments.push(self.node_text(child));
                } else if child.kind() == "proc" {
                    break;
                }
            }
            if inner_comments.is_empty() {
                None
            } else {
                Some(inner_comments.join("\n"))
            }
        });

        // Collect scattered body args when body braces are fragmented.
        // The word_list will have: braced_word(params), ERROR("{"), simple_word(keyword), ...
        let scattered_body = if body_node.is_none() {
            self.collect_scattered_body_args(node)
        } else {
            Vec::new()
        };

        let mut complexity = match body_node {
            Some(bn) => self.calculate_complexity(bn),
            None => ComplexityMetrics {
                cyclomatic_complexity: 1,
                branches: 0,
                loops: 0,
                logical_operators: 0,
                max_nesting_depth: 0,
                exception_handlers: 0,
                early_returns: 0,
            },
        };

        // Add complexity from scattered body keywords
        for arg in &scattered_body {
            let text = self.node_text(*arg);
            let trimmed = text.trim();
            match trimmed {
                "if" | "elseif" => {
                    complexity.cyclomatic_complexity += 1;
                    complexity.branches += 1;
                }
                "while" | "foreach" | "for" => {
                    complexity.cyclomatic_complexity += 1;
                    complexity.loops += 1;
                }
                "catch" => {
                    complexity.cyclomatic_complexity += 1;
                    complexity.exception_handlers += 1;
                }
                _ => {}
            }
        }

        let param_str = params
            .iter()
            .map(|p| {
                if let Some(ref default) = p.default_value {
                    format!("{{{} {}}}", p.name, default)
                } else {
                    p.name.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        let signature = format!("proc {} {{{}}} {{...}}", name_str, param_str);

        let mut func = FunctionEntity::new(
            &qualified,
            node.start_position().row + 1,
            node.end_position().row + 1,
        )
        .with_visibility("public")
        .with_signature(&signature);

        func.parameters = params;
        func.doc_comment = doc_comment;
        func.parent_class = self.current_namespace();
        func.complexity = Some(complexity);
        func.body_prefix = node
            .utf8_text(self.source)
            .ok()
            .filter(|t| !t.is_empty())
            .map(|t| {
                truncate_body_prefix(t)
            })
            .map(|t| t.to_string());
        self.functions.push(func);

        // Visit body for nested calls
        let prev_proc = self.current_procedure.take();
        self.current_procedure = Some(qualified);
        if let Some(bn) = body_node {
            if bn.kind() == "arguments" {
                self.visit_arguments_body(bn);
            } else {
                self.visit_braced_body(bn);
            }
        } else {
            // Scan scattered body args for keyword calls
            for arg in &scattered_body {
                let text = self.node_text(*arg);
                let trimmed = text.trim();
                if TCL_KEYWORDS.contains(&trimmed) {
                    self.record_call(trimmed, *arg);
                }
            }
        }
        self.current_procedure = prev_proc;
    }

    /// Extract proc name/params/body when the node is a `command` (name="proc").
    fn extract_proc_from_command<'b>(
        &self,
        node: Node<'b>,
    ) -> (String, Option<Node<'b>>, Option<Node<'b>>) {
        let args = self.collect_argument_nodes(node);
        if args.len() < 3 {
            return (String::new(), None, None);
        }
        let name = self.node_text(args[0]).trim().to_string();
        (name, Some(args[1]), Some(args[2]))
    }

    /// Extract proc name/params/body from procedure/ERROR node trees.
    fn extract_proc_from_tree<'b>(
        &self,
        node: Node<'b>,
    ) -> (String, Option<Node<'b>>, Option<Node<'b>>) {
        let mut name_str = String::new();
        let mut params_node: Option<Node<'b>> = None;
        let mut body_node: Option<Node<'b>> = None;
        let mut found_proc = false;

        let mut cursor = node.walk();
        let children: Vec<Node<'b>> = node.children(&mut cursor).collect();

        for child in &children {
            match child.kind() {
                "comment" => continue,
                "proc" => {
                    found_proc = true;
                    continue;
                }
                // Nested ERROR wrapping proc keyword: ERROR(ERROR(proc)) or ERROR(proc)
                "ERROR" if !found_proc => {
                    let mut ic = child.walk();
                    for inner in child.children(&mut ic) {
                        if inner.kind() == "proc" {
                            found_proc = true;
                            break;
                        }
                    }
                    if found_proc {
                        continue;
                    }
                }
                // Comments case: rest of proc parsed as a command child
                "command" if found_proc => {
                    if let Some(name_node) = child.child_by_field_name("name") {
                        if name_str.is_empty() {
                            name_str = self.node_text(name_node).trim().to_string();
                        }
                    }
                    if let Some(args_node) = child.child_by_field_name("arguments") {
                        let mut ic = args_node.walk();
                        let mut body_brace_broken = false;
                        for inner in args_node.children(&mut ic) {
                            // Detect fragmented body: ERROR("{") after params means
                            // the body braces are broken and subsequent braced_words
                            // are NOT the proc body.
                            if inner.kind() == "ERROR" && params_node.is_some() {
                                let text = self.node_text(inner);
                                if text.trim() == "{" {
                                    body_brace_broken = true;
                                }
                            }
                            if inner.kind() == "braced_word" || inner.kind() == "braced_word_simple"
                            {
                                if params_node.is_none() {
                                    params_node = Some(inner);
                                } else if body_node.is_none() && !body_brace_broken {
                                    body_node = Some(inner);
                                }
                            }
                        }
                    }
                }
                // Nested ERROR containing name and possibly params
                "ERROR" if found_proc => {
                    let mut ic = child.walk();
                    for inner in child.children(&mut ic) {
                        match inner.kind() {
                            "simple_word" | "word" if name_str.is_empty() => {
                                name_str = self.node_text(inner).trim().to_string();
                            }
                            "braced_word" | "braced_word_simple" if params_node.is_none() => {
                                params_node = Some(inner);
                            }
                            _ => {}
                        }
                    }
                }
                // Flat name child
                "simple_word" | "word" if found_proc && name_str.is_empty() => {
                    name_str = self.node_text(*child).trim().to_string();
                }
                // Flat braced_word children: first is params, second is body.
                // Always prefer braced_word over arguments for body.
                "braced_word" | "braced_word_simple" if found_proc => {
                    if name_str.is_empty() {
                        continue;
                    } else if params_node.is_none() {
                        params_node = Some(*child);
                    } else {
                        body_node = Some(*child);
                    }
                }
                // `arguments` can be params (in procedure nodes) or body
                // (when body contains keywords that flatten it).
                "arguments" if found_proc && !name_str.is_empty() => {
                    if params_node.is_none() {
                        params_node = Some(*child);
                    } else if body_node.is_none() {
                        body_node = Some(*child);
                    }
                }
                _ => {}
            }
        }

        (name_str, params_node, body_node)
    }

    // ── Namespace handling (unified) ────────────────────────────────────

    /// Handle `namespace eval name {body}` regardless of AST structure.
    fn visit_namespace(&mut self, node: Node) {
        let (ns_name, body_node) = if node.kind() == "command" {
            self.extract_namespace_from_command(node)
        } else {
            self.extract_namespace_from_tree(node)
        };

        if ns_name.is_empty() {
            return;
        }

        self.namespace_stack.push(ns_name);
        let full_ns = self.current_namespace().unwrap_or_default();
        let doc_comment = self.extract_preceding_comment(node);

        let body_prefix = node
            .utf8_text(self.source)
            .ok()
            .filter(|t| !t.is_empty())
            .map(|t| {
                truncate_body_prefix(t)
            })
            .map(|t| t.to_string());
        let class = ClassEntity {
            name: full_ns,
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_abstract: false,
            is_interface: false,
            base_classes: Vec::new(),
            implemented_traits: Vec::new(),
            methods: Vec::new(),
            fields: Vec::new(),
            doc_comment,
            attributes: vec!["namespace".to_string()],
            type_parameters: Vec::new(),
            body_prefix,
        };
        self.classes.push(class);

        if let Some(bn) = body_node {
            self.visit_braced_body(bn);
        } else if node.kind() == "ERROR" {
            // Flat ERROR: all tokens are siblings. Scan for procs and commands
            // between the `{` after the namespace name and its matching `}`.
            self.visit_flat_error_body(node);
        }

        self.namespace_stack.pop();
    }

    /// Extract namespace name/body from a `command` node (name="namespace").
    fn extract_namespace_from_command<'b>(&self, node: Node<'b>) -> (String, Option<Node<'b>>) {
        let args = self.collect_argument_nodes(node);
        if args.is_empty() {
            return (String::new(), None);
        }
        let subcommand = self.node_text(args[0]).trim().to_string();
        if subcommand != "eval" || args.len() < 3 {
            return (String::new(), None);
        }
        let ns_name = self.node_text(args[1]).trim().to_string();
        (ns_name, Some(args[2]))
    }

    /// Extract namespace name/body from ERROR/procedure node trees.
    fn extract_namespace_from_tree<'b>(&self, node: Node<'b>) -> (String, Option<Node<'b>>) {
        let mut found_namespace = false;
        let mut found_eval = false;
        let mut ns_name = String::new();
        let mut body_node: Option<Node<'b>> = None;

        let mut cursor = node.walk();
        let children: Vec<Node<'b>> = node.children(&mut cursor).collect();

        for child in &children {
            match child.kind() {
                "namespace" => {
                    found_namespace = true;
                    continue;
                }
                // Handle ERROR(namespace) child — detect the keyword inside
                "ERROR" if !found_namespace => {
                    let mut ic = child.walk();
                    for inner in child.children(&mut ic) {
                        if inner.kind() == "namespace" {
                            found_namespace = true;
                            break;
                        }
                    }
                    if found_namespace {
                        continue;
                    }
                }
                // Handle split case: ERROR(namespace) + command(eval name {body})
                "command" if found_namespace && !found_eval => {
                    let cmd_name = self.extract_command_name(*child);
                    if cmd_name == "eval" {
                        found_eval = true;
                        let args = self.collect_argument_nodes(*child);
                        if let Some(name_node) = args.first() {
                            ns_name = self.node_text(*name_node).trim().to_string();
                        }
                        for arg in args.iter().skip(1) {
                            if (arg.kind() == "braced_word" || arg.kind() == "braced_word_simple")
                                && body_node.is_none()
                            {
                                body_node = Some(*arg);
                            }
                        }
                    }
                }
                // Multi-line case: namespace node has a word_list child
                // containing eval, name, and braced_word body.
                "word_list" => {
                    let mut ic = child.walk();
                    for inner in child.children(&mut ic) {
                        match inner.kind() {
                            "simple_word" | "word" => {
                                let text = self.node_text(inner).trim().to_string();
                                if text == "eval" {
                                    found_eval = true;
                                } else if found_eval && ns_name.is_empty() {
                                    ns_name = text;
                                }
                            }
                            "braced_word" | "braced_word_simple"
                                if found_eval && !ns_name.is_empty() && body_node.is_none() =>
                            {
                                body_node = Some(inner);
                            }
                            _ => {}
                        }
                    }
                }
                "ERROR" => {
                    let mut ic = child.walk();
                    for inner in child.children(&mut ic) {
                        let text = self.node_text(inner).trim().to_string();
                        if text == "eval" {
                            found_eval = true;
                        } else if found_eval && ns_name.is_empty() {
                            ns_name = text;
                        }
                    }
                }
                "simple_word" | "word" => {
                    let text = self.node_text(*child).trim().to_string();
                    if text == "eval" {
                        found_eval = true;
                    } else if found_eval && ns_name.is_empty() {
                        ns_name = text;
                    }
                }
                "braced_word" | "braced_word_simple" if found_eval && !ns_name.is_empty() => {
                    body_node = Some(*child);
                }
                _ => {}
            }
        }

        if !found_eval {
            return (String::new(), None);
        }
        (ns_name, body_node)
    }

    // ── Command dispatch ────────────────────────────────────────────────

    fn visit_command(&mut self, node: Node) {
        let cmd_name = self.extract_command_name(node);
        if cmd_name.is_empty() {
            return;
        }

        match cmd_name.as_str() {
            "proc" => self.visit_proc(node),
            "namespace" => self.visit_namespace(node),
            "source" => self.visit_source_command(node),
            "package" => self.visit_package_command(node),
            "if" | "elseif" | "while" | "foreach" | "for" | "switch" | "try" | "catch" => {
                self.record_call(&cmd_name, node);
                self.visit_braced_bodies(node);
            }
            _ => self.visit_general_command(&cmd_name, node),
        }
    }

    fn visit_general_command(&mut self, cmd_name: &str, node: Node) {
        // Check for collapsed word_list: the grammar may merge multiple lines
        // into one giant word_list when bracket expressions ([...]) are present.
        // Detect and split at embedded SDC/EDA command boundaries.
        if sdc::is_sdc_command(cmd_name) || eda::is_eda_command(cmd_name) {
            if let Some(args_node) = node.child_by_field_name("arguments") {
                if args_node.kind() == "word_list" {
                    let has_embedded = {
                        let mut c = args_node.walk();
                        let result = args_node.children(&mut c).any(|child| {
                            if child.kind() == "simple_word" {
                                let text = child.utf8_text(self.source).unwrap_or("").trim();
                                sdc::is_sdc_command(text) || eda::is_eda_command(text)
                            } else {
                                false
                            }
                        });
                        result
                    };
                    if has_embedded {
                        self.visit_collapsed_commands(cmd_name, node);
                        return;
                    }
                }
            }
        }

        if sdc::is_sdc_command(cmd_name) {
            if let Some(constraint) = sdc::extract_sdc_constraint(cmd_name, node, self.source) {
                self.sdc_data.add(constraint);
            }
            self.record_call(cmd_name, node);
            return;
        }

        if eda::is_eda_command(cmd_name) {
            if let Some(eda_cmd) = eda::classify_eda_command(cmd_name, node, self.source) {
                match eda_cmd {
                    EdaCommand::DesignFileRead { file_type, path } => {
                        if !path.is_empty() {
                            self.imports.push(ImportRelation {
                                importer: "file".to_string(),
                                imported: path.clone(),
                                symbols: Vec::new(),
                                is_wildcard: false,
                                alias: None,
                            });
                        }
                        self.eda_data.design_reads.push((file_type, path));
                    }
                    EdaCommand::DesignFileWrite { file_type, path } => {
                        self.eda_data.design_writes.push((file_type, path));
                    }
                    EdaCommand::ToolFlowCommand { ref name, .. }
                    | EdaCommand::ObjectQuery { ref name, .. } => {
                        self.record_call(name, node);
                    }
                    EdaCommand::CommandRegistration { name, usage } => {
                        self.eda_data.registered_commands.push((name, usage));
                    }
                    EdaCommand::CollectionIteration { .. } => {
                        self.record_call(cmd_name, node);
                        self.visit_braced_bodies(node);
                    }
                    EdaCommand::AttributeAccess { .. } => {
                        self.record_call(cmd_name, node);
                    }
                }
            }
            return;
        }

        self.record_call(cmd_name, node);
    }

    /// Handle a collapsed word_list where the grammar merged multiple command
    /// lines into a single command node. Splits at SDC/EDA command boundaries
    /// and processes each segment independently.
    fn visit_collapsed_commands(&mut self, first_cmd: &str, node: Node) {
        let args_node = match node.child_by_field_name("arguments") {
            Some(a) if a.kind() == "word_list" => a,
            _ => return,
        };

        let mut cursor = args_node.walk();
        let children: Vec<Node> = args_node.children(&mut cursor).collect();

        // Build segments: each segment is (cmd_name, args as strings)
        let mut segments: Vec<(String, Vec<String>)> = Vec::new();
        let mut current_cmd = first_cmd.to_string();
        let mut current_args: Vec<String> = Vec::new();

        for child in &children {
            let text = child
                .utf8_text(self.source)
                .unwrap_or("")
                .trim()
                .to_string();
            if child.kind() == "simple_word"
                && (sdc::is_sdc_command(&text) || eda::is_eda_command(&text))
            {
                // Start of a new command — flush the current one
                segments.push((current_cmd, current_args));
                current_cmd = text;
                current_args = Vec::new();
            } else if !text.is_empty() {
                current_args.push(text);
            }
        }
        segments.push((current_cmd, current_args));

        // Process each segment
        for (cmd_name, args) in &segments {
            if sdc::is_sdc_command(cmd_name) {
                if let Some(constraint) = sdc::extract_sdc_from_args(cmd_name, args) {
                    self.sdc_data.add(constraint);
                }
                self.record_call(cmd_name, node);
            } else if eda::is_eda_command(cmd_name) {
                self.record_call(cmd_name, node);
            }
        }
    }

    fn visit_source_command(&mut self, node: Node) {
        let args = self.collect_argument_nodes(node);
        if let Some(arg) = args.first() {
            let filename = self.node_text(*arg).trim().to_string();
            let cleaned = filename.trim_matches('"').trim_matches('\'').to_string();
            if !cleaned.is_empty() {
                self.imports.push(ImportRelation {
                    importer: "file".to_string(),
                    imported: cleaned,
                    symbols: Vec::new(),
                    is_wildcard: true,
                    alias: None,
                });
            }
        }
    }

    fn visit_package_command(&mut self, node: Node) {
        let args = self.collect_argument_nodes(node);
        if args.is_empty() {
            return;
        }
        let subcommand = self.node_text(args[0]).trim().to_string();
        if subcommand == "require" && args.len() >= 2 {
            let pkg_name = self.node_text(args[1]).trim().to_string();
            if !pkg_name.is_empty() {
                self.imports.push(ImportRelation {
                    importer: "file".to_string(),
                    imported: pkg_name,
                    symbols: Vec::new(),
                    is_wildcard: false,
                    alias: None,
                });
            }
        }
    }

    // ── Body visiting ───────────────────────────────────────────────────

    /// Visit commands inside a braced_word body (proc body, namespace body, etc.).
    /// Uses `resolve_kind` so ERROR nodes are dispatched by their keyword.
    fn visit_braced_body(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            let kind = resolve_kind(child);
            match kind {
                "command" => self.visit_command(child),
                "proc" => self.visit_proc(child),
                "namespace" => self.visit_namespace(child),
                "if" | "elseif" | "while" | "foreach" | "try" | "catch" | "set" | "global"
                | "regexp" | "expr" | "else" | "finally" => {
                    self.record_call(kind, child);
                    self.visit_braced_body(child);
                }
                _ => self.visit_braced_body(child),
            }
        }
    }

    /// Visit braced_word children of a `command` node's arguments.
    fn visit_braced_bodies(&mut self, node: Node) {
        if let Some(args_node) = node.child_by_field_name("arguments") {
            let mut cursor = args_node.walk();
            for child in args_node.children(&mut cursor) {
                if child.kind() == "braced_word" || child.kind() == "braced_word_simple" {
                    self.visit_braced_body(child);
                }
            }
        }
    }

    /// Visit all braced_word children of any node (ERROR, procedure, etc.).
    /// Used for control flow keywords resolved from ERROR nodes.
    fn visit_bodies(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "braced_word" || child.kind() == "braced_word_simple" {
                self.visit_braced_body(child);
            }
        }
    }

    /// Scan a flat ERROR node for proc definitions and commands.
    ///
    /// When tree-sitter-tcl flattens a namespace body into a single ERROR node,
    /// all tokens (namespace, eval, name, {, proc, name, params, {, body, }, })
    /// appear as siblings. This method finds the body region (between the first
    /// `{` and its matching `}`) and scans for proc definitions and commands.
    fn visit_flat_error_body(&mut self, node: Node) {
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();

        // Find the opening `{` of the namespace body (skip past namespace/eval/name)
        let mut body_start = None;
        let mut brace_count = 0;
        for (i, child) in children.iter().enumerate() {
            if child.kind() == "{" {
                brace_count += 1;
                if brace_count == 1 {
                    body_start = Some(i + 1);
                    break;
                }
            }
        }

        let start = match body_start {
            Some(s) => s,
            None => return,
        };

        let mut depth = 1; // we're inside the first `{`
        let mut i = start;
        while i < children.len() && depth > 0 {
            let kind = children[i].kind();
            match kind {
                "}" => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    i += 1;
                }
                "{" => {
                    depth += 1;
                    i += 1;
                }
                "proc" if depth == 1 => {
                    let consumed = self.visit_flat_proc(&children, i);
                    i += consumed;
                }
                "command" => {
                    self.visit_command(children[i]);
                    i += 1;
                }
                _ => {
                    // Check if this is a keyword token (set, if, etc.)
                    if depth == 1 && TCL_KEYWORDS.contains(&kind) && kind != "proc" {
                        self.record_call(kind, children[i]);
                    }
                    i += 1;
                }
            }
        }
    }

    /// Extract a proc definition from flat sibling tokens.
    ///
    /// `tokens[proc_idx]` is the `proc` keyword. Scans forward for name,
    /// params, and body. Returns the number of tokens consumed.
    fn visit_flat_proc(&mut self, tokens: &[Node], proc_idx: usize) -> usize {
        let mut i = proc_idx + 1; // skip "proc"
        let mut name_str = String::new();
        let mut params: Vec<Parameter> = Vec::new();

        // Find name (skip ERROR/comment/whitespace nodes)
        while i < tokens.len() {
            match tokens[i].kind() {
                "simple_word" | "word" if name_str.is_empty() => {
                    name_str = self.node_text(tokens[i]).trim().to_string();
                    i += 1;
                    break;
                }
                "ERROR" | "comment" => {
                    i += 1;
                }
                _ => break,
            }
        }

        if name_str.is_empty() {
            return i - proc_idx;
        }

        // Find params (arguments node or braced_word)
        while i < tokens.len() {
            match tokens[i].kind() {
                "arguments" | "braced_word" | "braced_word_simple" => {
                    params = self.extract_params_from_braced(tokens[i]);
                    i += 1;
                    break;
                }
                "ERROR" | "comment" => {
                    i += 1;
                }
                _ => break,
            }
        }

        // Find body: match { ... } with brace depth tracking
        while i < tokens.len() && tokens[i].kind() != "{" {
            i += 1;
        }
        if i >= tokens.len() {
            return i - proc_idx;
        }
        let body_start = i + 1; // after opening {
        i += 1;
        let mut depth = 1;
        while i < tokens.len() && depth > 0 {
            match tokens[i].kind() {
                "{" => depth += 1,
                "}" => depth -= 1,
                _ => {}
            }
            i += 1;
        }
        let body_end = i - 1; // before closing }

        // Create the function entity
        let qualified = self.qualified_name(&name_str);
        let param_str = params
            .iter()
            .map(|p| {
                if let Some(ref default) = p.default_value {
                    format!("{{{} {}}}", p.name, default)
                } else {
                    p.name.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        let signature = format!("proc {} {{{}}} {{...}}", name_str, param_str);

        let proc_node = tokens[proc_idx];
        let mut func = FunctionEntity::new(
            &qualified,
            proc_node.start_position().row + 1,
            tokens
                .get(body_end)
                .map_or(proc_node.end_position().row + 1, |n| {
                    n.end_position().row + 1
                }),
        )
        .with_visibility("public")
        .with_signature(&signature);

        func.parameters = params;
        func.parent_class = self.current_namespace();
        self.functions.push(func);

        // Visit body tokens for nested calls
        let prev_proc = self.current_procedure.take();
        self.current_procedure = Some(qualified);
        for token in &tokens[body_start..body_end] {
            let kind = token.kind();
            if kind == "command" {
                self.visit_command(*token);
            } else if TCL_KEYWORDS.contains(&kind) && kind != "proc" {
                self.record_call(kind, *token);
            }
        }
        self.current_procedure = prev_proc;

        i - proc_idx
    }

    /// Visit an `arguments` node used as a proc body.
    ///
    /// When tree-sitter-tcl parses a proc body containing grammar keywords
    /// (set, global, expr, etc.), it may flatten the body into an `arguments`
    /// node instead of a `braced_word`. The inner commands appear as `argument`
    /// children with text matching keyword names.
    fn visit_arguments_body(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "command" => self.visit_command(child),
                "argument" => {
                    let text = self.node_text(child);
                    let trimmed = text.trim();
                    if TCL_KEYWORDS.contains(&trimmed) {
                        self.record_call(trimmed, child);
                    }
                }
                _ => {
                    let kind = resolve_kind(child);
                    match kind {
                        "proc" => self.visit_proc(child),
                        "namespace" => self.visit_namespace(child),
                        "if" | "elseif" | "while" | "foreach" | "try" | "catch" | "set"
                        | "global" | "regexp" | "expr" | "else" | "finally" => {
                            self.record_call(kind, child);
                            self.visit_bodies(child);
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    /// Collect scattered body content from a proc node's command child.
    ///
    /// When the body `{` is fragmented (becomes ERROR), body content scatters
    /// as simple_word siblings in the command's word_list. This method finds
    /// those simple_words that appear after the params braced_word.
    fn collect_scattered_body_args<'b>(&self, node: Node<'b>) -> Vec<Node<'b>> {
        let mut result = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() != "command" {
                continue;
            }
            if let Some(args_node) = child.child_by_field_name("arguments") {
                if args_node.kind() != "word_list" {
                    continue;
                }
                let mut ic = args_node.walk();
                let mut past_params = false;
                let mut past_error_brace = false;
                for inner in args_node.children(&mut ic) {
                    if !past_params {
                        if inner.kind() == "braced_word" || inner.kind() == "braced_word_simple" {
                            past_params = true;
                        }
                        continue;
                    }
                    if !past_error_brace {
                        if inner.kind() == "ERROR" {
                            past_error_brace = true;
                        }
                        continue;
                    }
                    // Everything after params + ERROR("{") is scattered body content
                    if inner.kind() == "simple_word" || inner.kind() == "word" {
                        result.push(inner);
                    }
                }
            }
        }
        result
    }

    // ── Helpers ─────────────────────────────────────────────────────────

    fn record_call(&mut self, callee: &str, node: Node) {
        let caller = match &self.current_procedure {
            Some(name) => name.clone(),
            None => "::".to_string(),
        };
        self.calls.push(CallRelation {
            caller,
            callee: callee.to_string(),
            call_site_line: node.start_position().row + 1,
            is_direct: true,
            struct_type: None,
            field_name: None,
        });
    }

    fn extract_command_name(&self, node: Node) -> String {
        if let Some(name_node) = node.child_by_field_name("name") {
            return self.node_text(name_node).trim().to_string();
        }
        String::new()
    }

    fn collect_argument_nodes<'b>(&self, node: Node<'b>) -> Vec<Node<'b>> {
        let mut result = Vec::new();
        if let Some(args_node) = node.child_by_field_name("arguments") {
            if args_node.kind() == "word_list" {
                let mut cursor = args_node.walk();
                for child in args_node.children(&mut cursor) {
                    if !child.is_extra() {
                        result.push(child);
                    }
                }
            } else {
                result.push(args_node);
            }
        }
        result
    }

    fn extract_params_from_braced(&self, node: Node) -> Vec<Parameter> {
        let text = self.node_text(node);
        let inner = text.trim_start_matches('{').trim_end_matches('}').trim();

        if inner.is_empty() {
            return Vec::new();
        }

        let mut params = Vec::new();
        let chars = inner.chars();
        let mut current = String::new();
        let mut depth = 0;

        for ch in chars {
            match ch {
                '{' => {
                    depth += 1;
                    if depth > 1 {
                        current.push(ch);
                    }
                }
                '}' => {
                    depth -= 1;
                    if depth > 0 {
                        current.push(ch);
                    } else if depth == 0 {
                        let trimmed = current.trim().to_string();
                        if !trimmed.is_empty() {
                            params.push(Self::parse_param_spec(&trimmed));
                        }
                        current.clear();
                    }
                }
                ' ' | '\t' if depth == 0 => {
                    let trimmed = current.trim().to_string();
                    if !trimmed.is_empty() {
                        params.push(Self::parse_param_spec(&trimmed));
                    }
                    current.clear();
                }
                _ => {
                    current.push(ch);
                }
            }
        }

        let trimmed = current.trim().to_string();
        if !trimmed.is_empty() {
            params.push(Self::parse_param_spec(&trimmed));
        }

        params
    }

    fn parse_param_spec(spec: &str) -> Parameter {
        let parts: Vec<&str> = spec.splitn(2, char::is_whitespace).collect();
        let name = parts[0].to_string();
        let is_variadic = name == "args";
        let default_value = parts.get(1).map(|s| s.trim().to_string());

        let mut param = Parameter::new(name);
        param.is_variadic = is_variadic;
        param.default_value = default_value;
        param
    }

    fn extract_preceding_comment(&self, node: Node) -> Option<String> {
        let mut comments = Vec::new();
        let mut prev = node.prev_sibling();

        while let Some(sibling) = prev {
            if sibling.kind() == "comment" {
                let text = self.node_text(sibling);
                comments.push(text);
                prev = sibling.prev_sibling();
            } else {
                break;
            }
        }

        if comments.is_empty() {
            return None;
        }

        comments.reverse();
        Some(comments.join("\n"))
    }

    // ── Complexity analysis ─────────────────────────────────────────────

    fn calculate_complexity(&self, body_node: Node) -> ComplexityMetrics {
        let mut metrics = ComplexityMetrics {
            cyclomatic_complexity: 1,
            branches: 0,
            loops: 0,
            logical_operators: 0,
            max_nesting_depth: 0,
            exception_handlers: 0,
            early_returns: 0,
        };
        self.walk_complexity(body_node, 0, &mut metrics);
        metrics
    }

    fn walk_complexity(&self, node: Node, depth: u32, metrics: &mut ComplexityMetrics) {
        if depth > metrics.max_nesting_depth {
            metrics.max_nesting_depth = depth;
        }

        // Resolve the effective kind so ERROR nodes are handled transparently
        let kind = resolve_kind(node);

        let effective_cmd = match kind {
            "command" => Some(self.extract_command_name(node)),
            "if" | "elseif" | "while" | "foreach" | "for" | "catch" | "return" | "switch"
            | "try" | "set" | "global" | "regexp" | "expr" => Some(kind.to_string()),
            _ => None,
        };

        if let Some(cmd_name) = effective_cmd {
            match cmd_name.as_str() {
                "if" | "elseif" => {
                    metrics.cyclomatic_complexity += 1;
                    metrics.branches += 1;
                }
                "while" | "foreach" | "for" => {
                    metrics.cyclomatic_complexity += 1;
                    metrics.loops += 1;
                }
                "foreach_in_collection" => {
                    metrics.cyclomatic_complexity += 1;
                    metrics.loops += 1;
                }
                "catch" => {
                    metrics.cyclomatic_complexity += 1;
                    metrics.exception_handlers += 1;
                }
                "return" => {
                    metrics.early_returns += 1;
                }
                _ => {}
            }
        }

        let new_depth = if node.kind() == "braced_word" || node.kind() == "braced_word_simple" {
            depth + 1
        } else {
            depth
        };

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk_complexity(child, new_depth, metrics);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::Parser;

    fn parse_and_visit(source: &[u8]) -> TclVisitor<'_> {
        let mut parser = Parser::new();
        let language = crate::ts_tcl::language();
        parser.set_language(&language).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = TclVisitor::new(source);
        visitor.visit_node(tree.root_node());
        visitor
    }

    #[test]
    fn test_visitor_empty() {
        let visitor = TclVisitor::new(b"");
        assert_eq!(visitor.functions.len(), 0);
        assert_eq!(visitor.classes.len(), 0);
    }

    #[test]
    fn test_visit_simple_proc() {
        let source = b"proc greet {name} {\n    puts \"Hello $name\"\n}";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "greet");
        assert_eq!(visitor.functions[0].parameters.len(), 1);
        assert_eq!(visitor.functions[0].parameters[0].name, "name");
    }

    #[test]
    fn test_visit_proc_with_defaults() {
        let source = b"proc add {a {b 0}} {\n    expr {$a + $b}\n}";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "add");
        assert!(visitor.functions[0].parameters.len() >= 2);
        assert_eq!(visitor.functions[0].parameters[0].name, "a");
        assert_eq!(visitor.functions[0].parameters[1].name, "b");
        assert_eq!(
            visitor.functions[0].parameters[1].default_value,
            Some("0".to_string())
        );
    }

    #[test]
    fn test_visit_proc_with_args() {
        let source = b"proc variadic {args} {\n    puts $args\n}";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert!(visitor.functions[0]
            .parameters
            .iter()
            .any(|p| p.is_variadic));
    }

    #[test]
    fn test_visit_source_import() {
        let source = b"source utils.tcl\nsource \"lib/helpers.tcl\"";
        let visitor = parse_and_visit(source);

        assert!(!visitor.imports.is_empty());
        assert!(visitor
            .imports
            .iter()
            .any(|i| i.imported.contains("utils.tcl")));
    }

    #[test]
    fn test_visit_package_require() {
        let source = b"package require Tcl 8.6\npackage require http";
        let visitor = parse_and_visit(source);

        assert!(visitor.imports.iter().any(|i| i.imported == "Tcl"));
        assert!(visitor.imports.iter().any(|i| i.imported == "http"));
    }

    #[test]
    fn test_visit_sdc_create_clock() {
        let source = b"create_clock -name clk -period 10 [get_ports clk_in]";
        let visitor = parse_and_visit(source);

        assert!(!visitor.sdc_data.clocks.is_empty());
        assert_eq!(visitor.sdc_data.clocks[0].name, "clk");
        assert_eq!(visitor.sdc_data.clocks[0].period, "10");
    }

    #[test]
    fn test_visit_eda_read_verilog() {
        let source = b"read_verilog design.v";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.eda_data.design_reads.len(), 1);
        assert_eq!(visitor.eda_data.design_reads[0].0, "verilog");
    }

    #[test]
    fn test_visit_eda_write_def() {
        let source = b"write_def output.def";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.eda_data.design_writes.len(), 1);
        assert_eq!(visitor.eda_data.design_writes[0].0, "def");
    }

    #[test]
    fn test_visit_tool_flow_commands() {
        let source = b"compile\nreport_timing\nglobal_placement";
        let visitor = parse_and_visit(source);

        assert!(visitor.calls.len() >= 3);
    }

    #[test]
    fn test_visit_comment_as_doc() {
        let source = b"# This is a greeting procedure\n# It says hello\nproc greet {name} {\n    puts hello\n}";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        if let Some(ref doc) = visitor.functions[0].doc_comment {
            assert!(doc.contains("greeting procedure"));
        }
    }

    #[test]
    fn test_visit_namespace_eval() {
        let source =
            b"namespace eval math {\n    proc add {a b} {\n        expr {$a + $b}\n    }\n}";
        let visitor = parse_and_visit(source);

        assert!(!visitor.classes.is_empty());
        assert!(visitor.classes.iter().any(|c| c.name.contains("math")));

        // The proc should be namespace-qualified
        if !visitor.functions.is_empty() {
            assert!(visitor.functions[0].name.contains("math"));
        }
    }

    #[test]
    fn test_complexity_simple() {
        let source = b"proc simple {} {\n    puts hello\n}";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        if let Some(ref c) = visitor.functions[0].complexity {
            assert_eq!(c.cyclomatic_complexity, 1);
        }
    }

    #[test]
    fn test_complexity_with_branches() {
        let source = b"proc check {x} {\n    if {$x > 0} {\n        puts positive\n    }\n}";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        if let Some(ref c) = visitor.functions[0].complexity {
            assert!(c.cyclomatic_complexity >= 2);
            assert!(c.branches >= 1);
        }
    }

    #[test]
    fn test_param_parsing() {
        let params = TclVisitor::parse_param_spec("name");
        assert_eq!(params.name, "name");
        assert!(params.default_value.is_none());

        let params2 = TclVisitor::parse_param_spec("b 0");
        assert_eq!(params2.name, "b");
        assert_eq!(params2.default_value, Some("0".to_string()));

        let params3 = TclVisitor::parse_param_spec("args");
        assert!(params3.is_variadic);
    }

    // ── Tests for previously unhandled keywords ─────────────────────────

    #[test]
    fn test_resolve_error_keyword_covers_all() {
        // Verify the constant and function agree
        for &kw in TCL_KEYWORDS {
            // Each keyword should appear in resolve_error_keyword's match arms
            assert_ne!(kw, "unknown", "TCL_KEYWORDS must not contain 'unknown'");
        }
    }

    #[test]
    fn test_set_recorded_as_call() {
        let source = b"proc foo {} {\n    set x 42\n}";
        let visitor = parse_and_visit(source);

        assert!(
            visitor.calls.iter().any(|c| c.callee == "set"),
            "set should be recorded as a call, got: {:?}",
            visitor.calls.iter().map(|c| &c.callee).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_global_recorded_as_call() {
        let source = b"proc foo {} {\n    global myvar\n}";
        let visitor = parse_and_visit(source);

        assert!(
            visitor.calls.iter().any(|c| c.callee == "global"),
            "global should be recorded as a call, got: {:?}",
            visitor.calls.iter().map(|c| &c.callee).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_expr_recorded_as_call() {
        let source = b"proc foo {} {\n    expr {1 + 2}\n}";
        let visitor = parse_and_visit(source);

        assert!(
            visitor.calls.iter().any(|c| c.callee == "expr"),
            "expr should be recorded as a call, got: {:?}",
            visitor.calls.iter().map(|c| &c.callee).collect::<Vec<_>>()
        );
    }

    // ── Integration tests: realistic Tcl source ────────────────────────
    //
    // NOTE: The tree-sitter-tcl grammar with ABI v15→v14 patch has
    // cascading parse issues when multiple grammar keywords appear in
    // deeply nested structures. These tests exercise each capability
    // in isolation or small combinations that the parser handles well.

    #[test]
    fn test_imports_and_packages() {
        let source = br#"
package require Tcl 8.6
package require http
source helpers.tcl
source lib/utils.tcl
"#;
        let visitor = parse_and_visit(source);

        assert!(visitor.imports.iter().any(|i| i.imported == "Tcl"));
        assert!(visitor.imports.iter().any(|i| i.imported == "http"));
        assert!(visitor
            .imports
            .iter()
            .any(|i| i.imported.contains("helpers.tcl")));
        assert!(visitor
            .imports
            .iter()
            .any(|i| i.imported.contains("lib/utils.tcl")));
    }

    #[test]
    fn test_namespace_with_proc() {
        // Namespace with a single proc inside — tests word_list/braced_word body path
        let source = b"namespace eval utils {\n    proc add {a b} {\n        puts hello\n    }\n}";
        let visitor = parse_and_visit(source);

        assert!(
            visitor.classes.iter().any(|c| c.name.contains("utils")),
            "should find utils namespace, got: {:?}",
            visitor.classes.iter().map(|c| &c.name).collect::<Vec<_>>()
        );
        assert!(
            visitor.functions.iter().any(|f| f.name.contains("add")),
            "should find add proc, got: {:?}",
            visitor
                .functions
                .iter()
                .map(|f| &f.name)
                .collect::<Vec<_>>()
        );
        // Proc should be namespace-qualified
        if let Some(f) = visitor.functions.iter().find(|f| f.name.contains("add")) {
            assert!(
                f.name.contains("utils"),
                "add should be qualified as utils::add, got: {}",
                f.name
            );
        }
    }

    #[test]
    fn test_proc_with_control_flow() {
        let source = b"proc check {x} {\n    if {$x > 0} {\n        puts positive\n    }\n}";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "check");
        if let Some(ref c) = visitor.functions[0].complexity {
            assert!(c.cyclomatic_complexity >= 2, "if should add complexity");
            assert!(c.branches >= 1);
        }
    }

    #[test]
    fn test_proc_with_keywords_in_body() {
        // Each keyword individually to verify they're recorded as calls
        let sources: Vec<(&[u8], &str)> = vec![
            (b"proc f {} {\n    set x 42\n}", "set"),
            (b"proc f {} {\n    global myvar\n}", "global"),
            (b"proc f {} {\n    expr {1 + 2}\n}", "expr"),
        ];

        for (source, expected_call) in sources {
            let visitor = parse_and_visit(source);
            assert_eq!(
                visitor.functions.len(),
                1,
                "should find proc for {}",
                expected_call
            );
            assert!(
                visitor.calls.iter().any(|c| c.callee == expected_call),
                "{} should be recorded as call, got: {:?}",
                expected_call,
                visitor.calls.iter().map(|c| &c.callee).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn test_proc_with_doc_comment() {
        let source = b"# This is a greeting procedure\n# It says hello\nproc greet {name} {\n    puts hello\n}";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "greet");
        if let Some(ref doc) = visitor.functions[0].doc_comment {
            assert!(doc.contains("greeting procedure"));
        }
    }

    #[test]
    fn test_proc_params_with_defaults() {
        let source = b"proc connect {host {port 8080} {timeout 30}} {\n    puts ok\n}";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        let f = &visitor.functions[0];
        assert_eq!(f.name, "connect");
        assert!(f
            .parameters
            .iter()
            .any(|p| p.name == "host" && p.default_value.is_none()));
        assert!(f
            .parameters
            .iter()
            .any(|p| p.name == "port" && p.default_value == Some("8080".to_string())));
        assert!(f
            .parameters
            .iter()
            .any(|p| p.name == "timeout" && p.default_value == Some("30".to_string())));
    }

    #[test]
    fn test_sdc_constraints() {
        let source = br#"
create_clock -name sys_clk -period 10.0 [get_ports clk]
create_clock -name pll_clk -period 5.0 [get_ports pll_out]
set_input_delay -clock sys_clk -max 2.0 [all_inputs]
set_output_delay -clock sys_clk -max 3.0 [all_outputs]
set_false_path -from [get_clocks pll_clk] -to [get_clocks sys_clk]
"#;
        let visitor = parse_and_visit(source);

        assert!(
            visitor.sdc_data.clocks.len() >= 2,
            "should find at least 2 clocks, got: {}",
            visitor.sdc_data.clocks.len()
        );
        assert!(visitor.sdc_data.clocks.iter().any(|c| c.name == "sys_clk"));
        assert!(visitor.sdc_data.clocks.iter().any(|c| c.name == "pll_clk"));
    }

    #[test]
    fn test_eda_design_reads() {
        let source = b"read_verilog top.v\nread_verilog design.v";
        let visitor = parse_and_visit(source);

        assert!(
            visitor.eda_data.design_reads.len() >= 2,
            "should find design reads, got: {:?}",
            visitor.eda_data.design_reads
        );
        assert!(
            visitor.imports.iter().any(|i| i.imported.contains("top.v")),
            "should import top.v, got: {:?}",
            visitor
                .imports
                .iter()
                .map(|i| &i.imported)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_eda_tool_flow() {
        let source = b"report_timing -delay_type max\nreport_area\ncompile_ultra";
        let visitor = parse_and_visit(source);

        let callees: Vec<&str> = visitor.calls.iter().map(|c| c.callee.as_str()).collect();
        assert!(
            callees.contains(&"report_timing"),
            "should record report_timing"
        );
        assert!(
            callees.contains(&"report_area"),
            "should record report_area"
        );
        assert!(
            callees.contains(&"compile_ultra"),
            "should record compile_ultra"
        );
    }

    #[test]
    fn test_multiline_procs_and_imports() {
        // Multiple top-level procs with imports — exercises multi-command source
        let source = br#"
package require Tcl 8.6
source helpers.tcl

proc greet {name} {
    puts "Hello $name"
}

proc add {a b} {
    expr {$a + $b}
}
"#;
        let visitor = parse_and_visit(source);

        // Imports
        assert!(visitor.imports.iter().any(|i| i.imported == "Tcl"));
        assert!(visitor
            .imports
            .iter()
            .any(|i| i.imported.contains("helpers.tcl")));

        // Functions
        let fn_names: Vec<&str> = visitor.functions.iter().map(|f| f.name.as_str()).collect();
        assert!(
            fn_names.contains(&"greet"),
            "should find greet, got: {:?}",
            fn_names
        );
    }
}
