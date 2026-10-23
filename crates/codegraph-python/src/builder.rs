// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use codegraph::{helpers, CodeGraph, EdgeType, NodeId, NodeType, PropertyMap};
use codegraph_parser_api::CodeIR;
use std::collections::HashMap;

use crate::error::Result;

/// Build a graph from the intermediate representation.
///
/// Takes a `CodeIR` structure and adds all entities and relationships to the given graph.
///
/// # Arguments
///
/// * `graph` - Mutable reference to the code graph
/// * `ir` - The intermediate representation containing entities and relationships
/// * `file_path` - Path to the source file being processed
///
/// # Returns
///
/// The `NodeId` of the file node created, or an error if building fails.
pub fn build_graph(graph: &mut CodeGraph, ir: &CodeIR, file_path: &str) -> Result<NodeId> {
    // Add the file/module node
    let file_id = helpers::add_file(graph, file_path, "python")
        .map_err(|e| crate::error::ParseError::GraphError(e.to_string()))?;

    // Track entity name -> NodeId mappings for relationship building
    let mut entity_map: HashMap<String, NodeId> = HashMap::new();

    // The extractor lists a class's methods in BOTH `ir.functions` and
    // `ir.classes[].methods` (see `apply_class_extraction`). Without this
    // filter every method was built twice — once as a stray module-level
    // function node and once as a real method node linked to its class. The
    // duplicate left the graph with two nodes carrying the same name, and the
    // call edges landed on the method node while lookups by name hit the
    // orphan, so callers/callees looked empty. Match on name AND line span so
    // a module-level function that merely shares a name with a method is kept.
    let method_spans: std::collections::HashSet<(&str, i64, i64)> = ir
        .classes
        .iter()
        .flat_map(|class| class.methods.iter())
        .map(|method| {
            (
                method.name.as_str(),
                method.line_start as i64,
                method.line_end as i64,
            )
        })
        .collect();

    // Add all functions
    for func in &ir.functions {
        // This function is a class method — the class pass below owns it.
        if method_spans.contains(&(
            func.name.as_str(),
            func.line_start as i64,
            func.line_end as i64,
        )) {
            continue;
        }
        let mut props = PropertyMap::new()
            .with("name", func.name.clone())
            .with("signature", func.signature.clone())
            .with("line_start", func.line_start as i64)
            .with("line_end", func.line_end as i64)
            .with("visibility", func.visibility.clone())
            .with("is_async", func.is_async)
            .with("is_static", func.is_static)
            .with("is_test", func.is_test)
            .with("attributes", func.attributes.clone());

        if let Some(ref body) = func.body_prefix {
            props = props.with("body_prefix", body.clone());
        }

        // Add complexity metrics if available
        if let Some(ref complexity) = func.complexity {
            props = props
                .with("complexity", complexity.cyclomatic_complexity as i64)
                .with("complexity_grade", complexity.grade().to_string())
                .with("complexity_branches", complexity.branches as i64)
                .with("complexity_loops", complexity.loops as i64)
                .with(
                    "complexity_logical_ops",
                    complexity.logical_operators as i64,
                )
                .with("complexity_nesting", complexity.max_nesting_depth as i64)
                .with(
                    "complexity_exceptions",
                    complexity.exception_handlers as i64,
                )
                .with("complexity_early_returns", complexity.early_returns as i64);
        }

        let func_id = graph
            .add_node(NodeType::Function, props)
            .map_err(|e| crate::error::ParseError::GraphError(e.to_string()))?;

        // Add Contains edge from file to function
        graph
            .add_edge(file_id, func_id, EdgeType::Contains, PropertyMap::new())
            .map_err(|e| crate::error::ParseError::GraphError(e.to_string()))?;

        entity_map.insert(func.name.clone(), func_id);
    }

    // Add all classes
    for class in &ir.classes {
        let class_id = helpers::add_class(
            graph,
            file_id,
            &class.name,
            class.line_start as i64,
            class.line_end as i64,
        )
        .map_err(|e| crate::error::ParseError::GraphError(e.to_string()))?;

        if let Some(ref body) = class.body_prefix {
            if let Ok(node) = graph.get_node(class_id) {
                let new_props = node.properties.clone().with("body_prefix", body.clone());
                let _ = graph.update_node_properties(class_id, new_props);
            }
        }

        entity_map.insert(class.name.clone(), class_id);

        // Add methods as functions linked to the class
        for method in &class.methods {
            let method_id = helpers::add_method(
                graph,
                class_id,
                &method.name,
                method.line_start as i64,
                method.line_end as i64,
            )
            .map_err(|e| crate::error::ParseError::GraphError(e.to_string()))?;

            // Track methods with qualified name for call relationships
            let qualified = format!("{}.{}", class.name, method.name);
            entity_map.insert(qualified, method_id);
            // Also register the bare method name so a call like
            // `self.helper()` inside the same file resolves directly, instead
            // of relying on the server's global name lookup (which cannot
            // distinguish two classes that share a method name).
            entity_map.entry(method.name.clone()).or_insert(method_id);
        }
    }

    // Add call relationships
    //
    // A callee that lives in another file is not in this file's entity map, so
    // its edge cannot be drawn here. Those calls are parked on the caller node's
    // `unresolved_calls` property and connected later by the server's
    // cross-file resolution pass (GraphUpdater::resolve_cross_file_imports).
    // Same pattern as codegraph-fortran / codegraph-rust / codegraph-swift.
    let mut unresolved_calls: HashMap<String, Vec<String>> = HashMap::new();

    for call in &ir.calls {
        if let Some(&caller_id) = entity_map.get(&call.caller) {
            if let Some(&callee_id) = entity_map.get(&call.callee) {
                helpers::add_call(graph, caller_id, callee_id, call.call_site_line as i64)
                    .map_err(|e| crate::error::ParseError::GraphError(e.to_string()))?;
            } else {
                unresolved_calls
                    .entry(call.caller.clone())
                    .or_default()
                    .push(call.callee.clone());
            }
        }
    }

    // Store unresolved calls on caller nodes for cross-file resolution
    for (caller_name, callees) in unresolved_calls {
        if let Some(&caller_id) = entity_map.get(&caller_name) {
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
                let new_props = node.properties.clone().with("unresolved_calls", all_callees);
                let _ = graph.update_node_properties(caller_id, new_props);
            }
        }
    }

    // Add import relationships
    for import in &ir.imports {
        let imported_module = &import.imported;

        let import_id = if let Some(&existing_id) = entity_map.get(imported_module) {
            existing_id
        } else {
            let is_external = !imported_module.starts_with('.');
            let props = PropertyMap::new()
                .with("name", imported_module.clone())
                .with("is_external", is_external.to_string());

            let id = graph
                .add_node(NodeType::Module, props)
                .map_err(|e| crate::error::ParseError::GraphError(e.to_string()))?;
            entity_map.insert(imported_module.clone(), id);
            id
        };

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
            .map_err(|e| crate::error::ParseError::GraphError(e.to_string()))?;
    }

    Ok(file_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph_parser_api::{CallRelation, ClassEntity, FunctionEntity, ImportRelation};

    #[test]
    fn test_build_empty_module() {
        let mut graph = CodeGraph::in_memory().unwrap();
        let ir = CodeIR::new(std::path::PathBuf::from("test.py"));

        let result = build_graph(&mut graph, &ir, "test.py");
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_with_function() {
        let mut graph = CodeGraph::in_memory().unwrap();
        let mut ir = CodeIR::new(std::path::PathBuf::from("test.py"));

        ir.add_function(FunctionEntity::new("test_func", 1, 3));

        let result = build_graph(&mut graph, &ir, "test.py");
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_with_class() {
        let mut graph = CodeGraph::in_memory().unwrap();
        let mut ir = CodeIR::new(std::path::PathBuf::from("test.py"));

        ir.add_class(ClassEntity::new("MyClass", 1, 4));

        let result = build_graph(&mut graph, &ir, "test.py");
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_with_relationships() {
        let mut graph = CodeGraph::in_memory().unwrap();
        let mut ir = CodeIR::new(std::path::PathBuf::from("test.py"));

        // Add two functions
        ir.add_function(FunctionEntity::new("caller", 1, 3));
        ir.add_function(FunctionEntity::new("callee", 5, 7));

        // Add call relationship using parser-API constructor
        ir.add_call(CallRelation::new("caller", "callee", 2));

        // Add import using parser-API constructor
        ir.add_import(ImportRelation::new("test", "os"));

        let result = build_graph(&mut graph, &ir, "test.py");
        assert!(result.is_ok());
    }

    #[test]
    fn test_local_call_creates_edge() {
        let mut graph = CodeGraph::in_memory().unwrap();
        let mut ir = CodeIR::new(std::path::PathBuf::from("test.py"));

        ir.add_function(FunctionEntity::new("caller", 1, 3));
        ir.add_function(FunctionEntity::new("callee", 5, 7));
        ir.add_call(CallRelation::new("caller", "callee", 2));

        build_graph(&mut graph, &ir, "test.py").unwrap();

        let funcs = graph
            .query()
            .node_type(NodeType::Function)
            .execute()
            .expect("query functions");
        assert_eq!(funcs.len(), 2);

        let caller = funcs
            .iter()
            .find(|id| {
                graph
                    .get_node(**id)
                    .ok()
                    .and_then(|n| n.properties.get_string("name").map(|n| n == "caller"))
                    .unwrap_or(false)
            })
            .copied()
            .expect("caller node");
        let callee = funcs
            .iter()
            .find(|id| {
                graph
                    .get_node(**id)
                    .ok()
                    .and_then(|n| n.properties.get_string("name").map(|n| n == "callee"))
                    .unwrap_or(false)
            })
            .copied()
            .expect("callee node");

        let edges = graph
            .get_edges_between(caller, callee)
            .expect("edges between caller and callee");
        assert!(
            edges.iter().any(|e| {
                graph
                    .get_edge(*e)
                    .map(|edge| edge.edge_type == EdgeType::Calls)
                    .unwrap_or(false)
            }),
            "expected a Calls edge between caller and callee"
        );
    }

    #[test]
    fn test_class_method_yields_single_node_and_call_edge() {
        // The extractor lists a class's methods in both `ir.functions` and
        // `ir.classes[].methods`. The builder must not build a stray
        // module-level copy of every method — two nodes with the same name
        // split the call edges away from the node that lookups find.
        let source = r#"
class TradingEngine:
    def run_tick(self):
        self._emit_ops("x")

    def _emit_ops(self, x):
        pass
"#;
        let config = crate::config::ParserConfig::default();
        let ir = crate::extractor::extract(source, std::path::Path::new("test.py"), &config)
            .expect("extract source");

        let mut graph = CodeGraph::in_memory().unwrap();
        build_graph(&mut graph, &ir, "test.py").unwrap();

        let funcs = graph
            .query()
            .node_type(NodeType::Function)
            .execute()
            .expect("query functions");
        assert_eq!(
            funcs.len(),
            2,
            "each method must yield exactly one node, got {}",
            funcs.len()
        );

        let find = |name: &str| {
            funcs
                .iter()
                .find(|id| {
                    graph
                        .get_node(**id)
                        .ok()
                        .and_then(|n| n.properties.get_string("name").map(|n| n == name))
                        .unwrap_or(false)
                })
                .copied()
                .expect("node for method")
        };
        let caller = find("run_tick");
        let callee = find("_emit_ops");

        let edges = graph
            .get_edges_between(caller, callee)
            .expect("edges between run_tick and _emit_ops");
        assert!(
            edges.iter().any(|e| {
                graph
                    .get_edge(*e)
                    .map(|edge| edge.edge_type == EdgeType::Calls)
                    .unwrap_or(false)
            }),
            "expected a Calls edge from run_tick to _emit_ops"
        );
    }

    #[test]
    fn test_unresolved_call_is_parked_for_cross_file_resolution() {        let mut graph = CodeGraph::in_memory().unwrap();
        let mut ir = CodeIR::new(std::path::PathBuf::from("test.py"));

        ir.add_function(FunctionEntity::new("caller", 1, 3));
        // The callee lives in another file, so it is not in this file's entity
        // map. It must be parked on the caller node as `unresolved_calls` so the
        // server's cross-file pass can connect it later.
        ir.add_call(CallRelation::new("caller", "partial_close", 2));

        build_graph(&mut graph, &ir, "test.py").unwrap();

        let funcs = graph
            .query()
            .node_type(NodeType::Function)
            .execute()
            .expect("query functions");
        assert_eq!(funcs.len(), 1);

        let node = graph.get_node(funcs[0]).expect("caller node");
        let unresolved = node
            .properties
            .get_string_list_compat("unresolved_calls")
            .unwrap_or_default();
        assert!(
            unresolved.iter().any(|c| c == "partial_close"),
            "callee should be parked for cross-file resolution, got {unresolved:?}"
        );
    }
}
