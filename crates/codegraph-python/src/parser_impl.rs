// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Implementation of the CodeParser trait for Python
//!
//! This module provides the PythonParser struct that implements the
//! codegraph-parser-api::CodeParser trait, making the Python parser compatible
//! with the unified parser API.

use codegraph::{CodeGraph, NodeId};
use codegraph_parser_api::{CodeParser, FileInfo, ParserConfig, ParserError, ParserMetrics};
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Python language parser implementing the CodeParser trait
pub struct PythonParser {
    config: ParserConfig,
    metrics: Mutex<ParserMetrics>,
}

impl PythonParser {
    /// Create a new Python parser with default configuration
    pub fn new() -> Self {
        Self {
            config: ParserConfig::default(),
            metrics: Mutex::new(ParserMetrics::default()),
        }
    }

    /// Create a new Python parser with custom configuration
    pub fn with_config(config: ParserConfig) -> Self {
        Self {
            config,
            metrics: Mutex::new(ParserMetrics::default()),
        }
    }

    /// Update metrics after parsing a file
    fn update_metrics(
        &self,
        success: bool,
        duration: Duration,
        entities: usize,
        relationships: usize,
    ) {
        let mut metrics = self.metrics.lock().unwrap();
        metrics.files_attempted += 1;
        if success {
            metrics.files_succeeded += 1;
        } else {
            metrics.files_failed += 1;
        }
        metrics.total_parse_time += duration;
        metrics.total_entities += entities;
        metrics.total_relationships += relationships;
    }

    /// Convert CodeIR to graph nodes and return FileInfo
    fn ir_to_graph(
        &self,
        ir: &codegraph_parser_api::CodeIR,
        graph: &mut CodeGraph,
        file_path: &Path,
    ) -> Result<FileInfo, ParserError> {
        use codegraph::{EdgeType, NodeType, PropertyMap};
        use std::collections::HashMap;

        let mut node_map: HashMap<String, NodeId> = HashMap::new();
        let mut function_ids = Vec::new();
        let mut class_ids = Vec::new();
        let mut trait_ids = Vec::new();
        let mut import_ids = Vec::new();

        // Create module/file node
        let file_id = if let Some(ref module) = ir.module {
            let mut props = PropertyMap::new()
                .with("name", module.name.clone())
                .with("path", module.path.clone())
                .with("language", module.language.clone())
                .with("line_count", module.line_count.to_string());

            if let Some(ref doc) = module.doc_comment {
                props = props.with("doc", doc.clone());
            }

            let id = graph
                .add_node(NodeType::CodeFile, props)
                .map_err(|e| ParserError::GraphError(e.to_string()))?;
            node_map.insert(module.name.clone(), id);
            id
        } else {
            // Create a default file node
            let file_name = file_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();
            let props = PropertyMap::new()
                .with("name", file_name.clone())
                .with("path", file_path.display().to_string())
                .with("language", "python");

            let id = graph
                .add_node(NodeType::CodeFile, props)
                .map_err(|e| ParserError::GraphError(e.to_string()))?;
            node_map.insert(file_name, id);
            id
        };

        // Add functions
        for func in &ir.functions {
            let mut props = PropertyMap::new()
                .with("name", func.name.clone())
                .with("path", file_path.display().to_string())
                .with("signature", func.signature.clone())
                .with("visibility", func.visibility.clone())
                .with("line_start", func.line_start as i64)
                .with("line_end", func.line_end as i64)
                .with("is_async", func.is_async)
                .with("is_static", func.is_static)
                .with("is_test", func.is_test)
                .with("attributes", func.attributes.clone());

            if let Some(ref doc) = func.doc_comment {
                props = props.with("doc", doc.clone());
            }
            if let Some(ref return_type) = func.return_type {
                props = props.with("return_type", return_type.clone());
            }
            // Detect HTTP route decorators (FastAPI, Flask, Django, etc.)
            if let Some((method, route)) = detect_http_decorator(&func.attributes) {
                props = props
                    .with("http_method", method)
                    .with("route", route)
                    .with("is_entry_point", true);
            }
            if let Some(ref complexity) = func.complexity {
                props = props
                    .with("complexity", complexity.cyclomatic_complexity as i64)
                    .with("complexity_grade", complexity.grade().to_string())
                    .with("complexity_branches", complexity.branches as i64)
                    .with("complexity_loops", complexity.loops as i64)
                    .with("complexity_logical_ops", complexity.logical_operators as i64)
                    .with("complexity_nesting", complexity.max_nesting_depth as i64)
                    .with("complexity_exceptions", complexity.exception_handlers as i64)
                    .with("complexity_early_returns", complexity.early_returns as i64);
            }
            if let Some(ref body) = func.body_prefix {
                props = props.with("body_prefix", body.clone());
            }

            let func_id = graph
                .add_node(NodeType::Function, props)
                .map_err(|e| ParserError::GraphError(e.to_string()))?;

            node_map.insert(func.name.clone(), func_id);
            function_ids.push(func_id);

            // Link function to file
            graph
                .add_edge(file_id, func_id, EdgeType::Contains, PropertyMap::new())
                .map_err(|e| ParserError::GraphError(e.to_string()))?;
        }

        // Add classes
        for class in &ir.classes {
            let mut props = PropertyMap::new()
                .with("name", class.name.clone())
                .with("path", file_path.display().to_string())
                .with("visibility", class.visibility.clone())
                .with("line_start", class.line_start as i64)
                .with("line_end", class.line_end as i64)
                .with("is_abstract", class.is_abstract.to_string());

            if let Some(ref doc) = class.doc_comment {
                props = props.with("doc", doc.clone());
            }
            if let Some(ref body) = class.body_prefix {
                props = props.with("body_prefix", body.clone());
            }

            let class_id = graph
                .add_node(NodeType::Class, props)
                .map_err(|e| ParserError::GraphError(e.to_string()))?;

            node_map.insert(class.name.clone(), class_id);
            class_ids.push(class_id);

            // Link class to file
            graph
                .add_edge(file_id, class_id, EdgeType::Contains, PropertyMap::new())
                .map_err(|e| ParserError::GraphError(e.to_string()))?;

            // Methods are already added via ir.functions with parent_class set
            // Just create edges from class to its methods
            for method in &class.methods {
                let method_name = method.name.clone();
                if let Some(&method_id) = node_map.get(&method_name) {
                    // Link method to class
                    graph
                        .add_edge(class_id, method_id, EdgeType::Contains, PropertyMap::new())
                        .map_err(|e| ParserError::GraphError(e.to_string()))?;
                }
            }
        }

        // Add traits (protocols in Python)
        for trait_entity in &ir.traits {
            let mut props = PropertyMap::new()
                .with("name", trait_entity.name.clone())
                .with("path", file_path.display().to_string())
                .with("visibility", trait_entity.visibility.clone())
                .with("line_start", trait_entity.line_start as i64)
                .with("line_end", trait_entity.line_end as i64);

            if let Some(ref doc) = trait_entity.doc_comment {
                props = props.with("doc", doc.clone());
            }

            let trait_id = graph
                .add_node(NodeType::Interface, props)
                .map_err(|e| ParserError::GraphError(e.to_string()))?;

            node_map.insert(trait_entity.name.clone(), trait_id);
            trait_ids.push(trait_id);

            // Link trait to file
            graph
                .add_edge(file_id, trait_id, EdgeType::Contains, PropertyMap::new())
                .map_err(|e| ParserError::GraphError(e.to_string()))?;
        }

        // Add import nodes and relationships
        for import in &ir.imports {
            let imported_module = &import.imported;

            // Create or get import node
            let import_id = if let Some(&existing_id) = node_map.get(imported_module) {
                existing_id
            } else {
                // Relative imports (from .foo, from ..bar) are internal
                let is_external = !import.imported.starts_with('.');
                let props = PropertyMap::new()
                    .with("name", imported_module.clone())
                    .with("is_external", is_external.to_string());

                let id = graph
                    .add_node(NodeType::Module, props)
                    .map_err(|e| ParserError::GraphError(e.to_string()))?;
                node_map.insert(imported_module.clone(), id);
                id
            };

            import_ids.push(import_id);

            // Create import edge from file to imported module
            let mut edge_props = PropertyMap::new();
            if let Some(ref alias) = import.alias {
                edge_props = edge_props.with("alias", alias.clone());
            }
            if import.is_wildcard {
                edge_props = edge_props.with("is_wildcard", "true");
            }
            if !import.symbols.is_empty() {
                edge_props = edge_props.with("symbols", import.symbols.clone());
            }
            graph
                .add_edge(file_id, import_id, EdgeType::Imports, edge_props)
                .map_err(|e| ParserError::GraphError(e.to_string()))?;
        }

        // Add call relationships
        // Track unresolved calls per caller for cross-file resolution
        let mut unresolved_calls: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();

        for call in &ir.calls {
            if let Some(&caller_id) = node_map.get(&call.caller) {
                if let Some(&callee_id) = node_map.get(&call.callee) {
                    // Both caller and callee are in this file - create direct edge
                    let edge_props = PropertyMap::new()
                        .with("call_site_line", call.call_site_line.to_string())
                        .with("is_direct", call.is_direct.to_string());

                    graph
                        .add_edge(caller_id, callee_id, EdgeType::Calls, edge_props)
                        .map_err(|e| ParserError::GraphError(e.to_string()))?;
                } else {
                    // Callee not found in this file - store for cross-file resolution
                    unresolved_calls
                        .entry(call.caller.clone())
                        .or_default()
                        .push(call.callee.clone());
                }
            }
        }

        // Store unresolved calls on caller nodes for post-processing
        for (caller_name, callees) in unresolved_calls {
            if let Some(&caller_id) = node_map.get(&caller_name) {
                if let Ok(node) = graph.get_node(caller_id) {
                    let mut all_callees: Vec<String> = node
                        .properties
                        .get_string_list_compat("unresolved_calls")
                        .unwrap_or_default();
                    for callee in &callees {
                        if !all_callees.iter().any(|c| c == callee) {
                            all_callees.push(callee.clone());
                        }
                    }
                    let new_props = node
                        .properties
                        .clone()
                        .with("unresolved_calls", all_callees);
                    let _ = graph.update_node_properties(caller_id, new_props);
                }
            }
        }

        // Add inheritance relationships
        for inheritance in &ir.inheritance {
            if let (Some(&child_id), Some(&parent_id)) = (
                node_map.get(&inheritance.child),
                node_map.get(&inheritance.parent),
            ) {
                let edge_props = PropertyMap::new().with("order", inheritance.order.to_string());

                graph
                    .add_edge(child_id, parent_id, EdgeType::Extends, edge_props)
                    .map_err(|e| ParserError::GraphError(e.to_string()))?;
            }
        }

        // Add implementation relationships (class implements protocol/interface)
        for impl_rel in &ir.implementations {
            if let (Some(&implementor_id), Some(&trait_id)) = (
                node_map.get(&impl_rel.implementor),
                node_map.get(&impl_rel.trait_name),
            ) {
                graph
                    .add_edge(
                        implementor_id,
                        trait_id,
                        EdgeType::Implements,
                        PropertyMap::new(),
                    )
                    .map_err(|e| ParserError::GraphError(e.to_string()))?;
            }
        }

        // Count source lines
        let line_count = if let Some(ref module) = ir.module {
            module.line_count
        } else {
            0
        };

        Ok(FileInfo {
            file_path: file_path.to_path_buf(),
            file_id,
            functions: function_ids,
            classes: class_ids,
            traits: trait_ids,
            imports: import_ids,
            parse_time: Duration::ZERO, // Will be set by caller
            line_count,
            byte_count: 0, // Will be set by caller
        })
    }
}

impl Default for PythonParser {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeParser for PythonParser {
    fn language(&self) -> &str {
        "python"
    }

    fn file_extensions(&self) -> &[&str] {
        &[".py", ".pyw"]
    }

    fn parse_file(&self, path: &Path, graph: &mut CodeGraph) -> Result<FileInfo, ParserError> {
        let start = Instant::now();

        // Check file extension
        if !self.can_parse(path) {
            return Err(ParserError::ParseError(
                path.to_path_buf(),
                "Invalid file extension for Python parser".to_string(),
            ));
        }

        // Read file
        let source = std::fs::read_to_string(path)
            .map_err(|e| ParserError::IoError(path.to_path_buf(), e))?;

        // Check file size
        let byte_count = source.len();
        if byte_count > self.config.max_file_size {
            self.update_metrics(false, start.elapsed(), 0, 0);
            return Err(ParserError::FileTooLarge(path.to_path_buf(), byte_count));
        }

        // Parse source
        let mut file_info = self.parse_source(&source, path, graph)?;
        file_info.byte_count = byte_count;

        Ok(file_info)
    }

    fn parse_source(
        &self,
        source: &str,
        file_path: &Path,
        graph: &mut CodeGraph,
    ) -> Result<FileInfo, ParserError> {
        let start = Instant::now();

        // Check size limit
        if source.len() > self.config.max_file_size {
            self.update_metrics(false, start.elapsed(), 0, 0);
            return Err(ParserError::FileTooLarge(
                file_path.to_path_buf(),
                source.len(),
            ));
        }

        // Extract entities using existing extractor
        // Convert ParserConfig to old config format
        let old_config = crate::config::ParserConfig {
            include_private: !self.config.skip_private,
            include_tests: !self.config.skip_tests,
            max_file_size: self.config.max_file_size,
            parallel: self.config.parallel,
            num_threads: self.config.parallel_workers,
            ..Default::default()
        };

        let ir = crate::extractor::extract(source, file_path, &old_config).map_err(|e| {
            self.update_metrics(false, start.elapsed(), 0, 0);
            ParserError::ParseError(file_path.to_path_buf(), e)
        })?;

        // Count entities and relationships
        let entity_count = ir.entity_count();
        let relationship_count = ir.relationship_count();

        // Convert IR to graph
        let mut file_info = self.ir_to_graph(&ir, graph, file_path)?;

        // Set timing and update metrics
        let duration = start.elapsed();
        file_info.parse_time = duration;
        file_info.byte_count = source.len();

        self.update_metrics(true, duration, entity_count, relationship_count);

        Ok(file_info)
    }

    fn config(&self) -> &ParserConfig {
        &self.config
    }

    fn metrics(&self) -> ParserMetrics {
        self.metrics.lock().unwrap().clone()
    }

    fn reset_metrics(&mut self) {
        *self.metrics.lock().unwrap() = ParserMetrics::default();
    }
}

/// Detect HTTP route decorators from Python function attributes.
///
/// Recognizes patterns from FastAPI, Flask, Django REST, Starlette:
/// - `@app.get("/path")`, `@router.post("/path")`
/// - `@app.route("/path", methods=["GET"])`
/// - `@api_view(["GET", "POST"])`
fn detect_http_decorator(attributes: &[String]) -> Option<(String, String)> {
    const HTTP_METHODS: &[&str] = &["get", "post", "put", "delete", "patch", "head", "options"];

    for attr in attributes {
        let lower = attr.to_lowercase();

        // Pattern: @something.METHOD("/path") — FastAPI, Flask, Starlette
        for method in HTTP_METHODS {
            let pattern = format!(".{}(", method);
            if lower.contains(&pattern) {
                let route = extract_first_string_arg(attr).unwrap_or_else(|| "/".to_string());
                return Some((method.to_uppercase(), route));
            }
        }

        // Pattern: @app.route("/path") or @blueprint.route("/path")
        if lower.contains(".route(") {
            let route = extract_first_string_arg(attr).unwrap_or_else(|| "/".to_string());
            // Try to extract methods= from the decorator
            let method = if lower.contains("methods") {
                for m in HTTP_METHODS {
                    if lower.contains(&m.to_uppercase()) || lower.contains(m) {
                        return Some((m.to_uppercase(), route));
                    }
                }
                "GET".to_string()
            } else {
                "GET".to_string()
            };
            return Some((method, route));
        }

        // Pattern: @api_view(["GET"]) — Django REST framework
        if lower.contains("api_view(") {
            let method = HTTP_METHODS
                .iter()
                .find(|m| lower.contains(&m.to_uppercase()))
                .map(|m| m.to_uppercase())
                .unwrap_or_else(|| "GET".to_string());
            return Some((method, "/".to_string()));
        }
    }

    None
}

/// Extract the first quoted string argument from a decorator like `@app.get("/users/{id}")`.
fn extract_first_string_arg(attr: &str) -> Option<String> {
    // Find first quoted string: either "..." or '...'
    let bytes = attr.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let quote = bytes[i];
            let start = i + 1;
            i += 1;
            while i < bytes.len() && bytes[i] != quote {
                i += 1;
            }
            if i < bytes.len() {
                return Some(String::from_utf8_lossy(&bytes[start..i]).to_string());
            }
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_python_parser_new() {
        let parser = PythonParser::new();
        assert_eq!(parser.language(), "python");
    }

    #[test]
    fn test_python_parser_file_extensions() {
        let parser = PythonParser::new();
        let exts = parser.file_extensions();
        assert_eq!(exts.len(), 2);
        assert!(exts.contains(&".py"));
        assert!(exts.contains(&".pyw"));
    }

    #[test]
    fn test_python_parser_can_parse() {
        let parser = PythonParser::new();
        assert!(parser.can_parse(Path::new("test.py")));
        assert!(parser.can_parse(Path::new("test.pyw")));
        assert!(!parser.can_parse(Path::new("test.rs")));
        assert!(!parser.can_parse(Path::new("test.txt")));
    }

    #[test]
    fn test_metrics_initial_state() {
        let parser = PythonParser::new();
        let metrics = parser.metrics();
        assert_eq!(metrics.files_attempted, 0);
        assert_eq!(metrics.files_succeeded, 0);
        assert_eq!(metrics.files_failed, 0);
    }

    #[test]
    fn test_implements_edge_creation() {
        use codegraph::{CodeGraph, EdgeType};
        use codegraph_parser_api::{
            ClassEntity, CodeIR, ImplementationRelation, ModuleEntity, TraitEntity,
        };
        use std::path::PathBuf;

        let parser = PythonParser::new();

        // Create IR with a class implementing a protocol (Python's equivalent of interface)
        let mut ir = CodeIR::new(PathBuf::from("test.py"));
        ir.set_module(ModuleEntity::new("test", "test.py", "python"));
        ir.add_class(ClassEntity::new("MyClass", 1, 20));
        ir.add_trait(TraitEntity::new("MyProtocol", 22, 30));
        ir.add_implementation(ImplementationRelation::new("MyClass", "MyProtocol"));

        let mut graph = CodeGraph::in_memory().unwrap();
        let file_info = parser
            .ir_to_graph(&ir, &mut graph, Path::new("test.py"))
            .unwrap();

        assert_eq!(file_info.classes.len(), 1);
        assert_eq!(file_info.traits.len(), 1);

        // Find class and protocol node IDs
        let class_id = file_info.classes[0];
        let protocol_id = file_info.traits[0];

        // Verify implements edge was created
        let edges = graph.get_edges_between(class_id, protocol_id).unwrap();
        assert!(
            !edges.is_empty(),
            "Should have implements edge between class and protocol"
        );

        let edge = graph.get_edge(edges[0]).unwrap();
        assert_eq!(
            edge.edge_type,
            EdgeType::Implements,
            "Edge should be of type Implements"
        );
    }

    #[test]
    fn test_relative_import_is_internal() {
        use codegraph::CodeGraph;
        use codegraph_parser_api::{CodeIR, ImportRelation, ModuleEntity};
        use std::path::PathBuf;

        let parser = PythonParser::new();
        let mut ir = CodeIR::new(PathBuf::from("test.py"));
        ir.set_module(ModuleEntity::new("test", "test.py", "python"));
        // Relative import: from .utils import foo
        ir.add_import(ImportRelation::new("test", ".utils").with_symbols(vec!["foo".to_string()]));
        // Absolute import: import os
        ir.add_import(ImportRelation::new("test", "os"));

        let mut graph = CodeGraph::in_memory().unwrap();
        let file_info = parser
            .ir_to_graph(&ir, &mut graph, Path::new("test.py"))
            .unwrap();

        assert_eq!(file_info.imports.len(), 2);

        // .utils should be internal (is_external = false)
        let utils_node = graph.get_node(file_info.imports[0]).unwrap();
        assert_eq!(
            utils_node.properties.get_string("is_external"),
            Some("false"),
            "Relative import .utils should be internal"
        );

        // os should be external (is_external = true)
        let os_node = graph.get_node(file_info.imports[1]).unwrap();
        assert_eq!(
            os_node.properties.get_string("is_external"),
            Some("true"),
            "Absolute import os should be external"
        );
    }

    #[test]
    fn test_double_dot_relative_import_is_internal() {
        use codegraph::CodeGraph;
        use codegraph_parser_api::{CodeIR, ImportRelation, ModuleEntity};
        use std::path::PathBuf;

        let parser = PythonParser::new();
        let mut ir = CodeIR::new(PathBuf::from("test.py"));
        ir.set_module(ModuleEntity::new("test", "test.py", "python"));
        // from ..models import Bar
        ir.add_import(
            ImportRelation::new("test", "..models").with_symbols(vec!["Bar".to_string()]),
        );

        let mut graph = CodeGraph::in_memory().unwrap();
        let file_info = parser
            .ir_to_graph(&ir, &mut graph, Path::new("test.py"))
            .unwrap();

        assert_eq!(file_info.imports.len(), 1);

        let node = graph.get_node(file_info.imports[0]).unwrap();
        assert_eq!(
            node.properties.get_string("is_external"),
            Some("false"),
            "Relative import ..models should be internal"
        );
    }
}
