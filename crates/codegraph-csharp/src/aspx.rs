// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! ASP.NET Web Forms (.aspx) directive extraction.
//!
//! Extracts `<%@ Page %>` and `<%@ Control %>` directives to discover:
//! - CodeBehind file references (→ Imports edge)
//! - Inherited class names (→ Extends edge)
//! - Master page references (→ Imports edge)
//! - User control registrations (→ Imports edge)

use codegraph::{CodeGraph, EdgeType, NodeType, PropertyMap};
use codegraph_parser_api::{FileInfo, ParserError};
use std::path::Path;
use std::time::Duration;

/// Parse an .aspx file and add directive relationships to the graph.
pub fn parse_aspx(
    source: &str,
    file_path: &Path,
    graph: &mut CodeGraph,
) -> Result<FileInfo, ParserError> {
    let file_name = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    // Create file node
    let file_props = PropertyMap::new()
        .with("path", file_path.display().to_string())
        .with("language", "aspx")
        .with("name", file_name.clone());
    let file_id = graph
        .add_node(NodeType::CodeFile, file_props)
        .map_err(|e| ParserError::GraphError(e.to_string()))?;

    let mut import_ids = Vec::new();

    // Extract all <%@ ... %> directives
    for directive in extract_directives(source) {
        match directive.directive_type.as_str() {
            "Page" | "Control" | "Master" => {
                // CodeBehind → imports the .cs file
                if let Some(ref code_behind) = directive.code_behind {
                    let cb_props = PropertyMap::new()
                        .with("name", code_behind.clone())
                        .with("relationship", "CodeBehind");
                    let cb_id = graph
                        .add_node(NodeType::Module, cb_props)
                        .map_err(|e| ParserError::GraphError(e.to_string()))?;
                    let edge_props = PropertyMap::new().with("import_type", "CodeBehind");
                    graph
                        .add_edge(file_id, cb_id, EdgeType::Imports, edge_props)
                        .map_err(|e| ParserError::GraphError(e.to_string()))?;
                    import_ids.push(cb_id);
                }

                // Inherits → extends the base class
                if let Some(ref inherits) = directive.inherits {
                    let class_props = PropertyMap::new()
                        .with("name", inherits.clone())
                        .with("relationship", "Inherits");
                    let class_id = graph
                        .add_node(NodeType::Class, class_props)
                        .map_err(|e| ParserError::GraphError(e.to_string()))?;
                    graph
                        .add_edge(file_id, class_id, EdgeType::Extends, PropertyMap::new())
                        .map_err(|e| ParserError::GraphError(e.to_string()))?;
                }

                // MasterPageFile → imports the master page
                if let Some(ref master) = directive.master_page {
                    let master_props = PropertyMap::new()
                        .with("name", master.clone())
                        .with("relationship", "MasterPage");
                    let master_id = graph
                        .add_node(NodeType::Module, master_props)
                        .map_err(|e| ParserError::GraphError(e.to_string()))?;
                    graph
                        .add_edge(file_id, master_id, EdgeType::Imports, PropertyMap::new())
                        .map_err(|e| ParserError::GraphError(e.to_string()))?;
                    import_ids.push(master_id);
                }
            }
            "Register" => {
                // <%@ Register TagPrefix="uc" TagName="Header" Src="Controls/Header.ascx" %>
                if let Some(ref src) = directive.src {
                    let ctrl_props = PropertyMap::new()
                        .with("name", src.clone())
                        .with("relationship", "UserControl");
                    let ctrl_id = graph
                        .add_node(NodeType::Module, ctrl_props)
                        .map_err(|e| ParserError::GraphError(e.to_string()))?;
                    graph
                        .add_edge(file_id, ctrl_id, EdgeType::Imports, PropertyMap::new())
                        .map_err(|e| ParserError::GraphError(e.to_string()))?;
                    import_ids.push(ctrl_id);
                }
            }
            "Import" => {
                // <%@ Import Namespace="System.Data" %>
                if let Some(ref ns) = directive.namespace {
                    let ns_props = PropertyMap::new().with("name", ns.clone());
                    let ns_id = graph
                        .add_node(NodeType::Module, ns_props)
                        .map_err(|e| ParserError::GraphError(e.to_string()))?;
                    graph
                        .add_edge(file_id, ns_id, EdgeType::Imports, PropertyMap::new())
                        .map_err(|e| ParserError::GraphError(e.to_string()))?;
                    import_ids.push(ns_id);
                }
            }
            _ => {}
        }
    }

    Ok(FileInfo {
        file_path: file_path.to_path_buf(),
        file_id,
        functions: Vec::new(),
        classes: Vec::new(),
        traits: Vec::new(),
        imports: import_ids,
        parse_time: Duration::ZERO,
        line_count: source.lines().count(),
        byte_count: source.len(),
    })
}

/// A parsed ASP.NET directive.
#[derive(Debug)]
struct AspxDirective {
    directive_type: String,
    code_behind: Option<String>,
    inherits: Option<String>,
    master_page: Option<String>,
    src: Option<String>,
    namespace: Option<String>,
}

/// Extract all `<%@ ... %>` directives from ASPX source.
fn extract_directives(source: &str) -> Vec<AspxDirective> {
    let mut directives = Vec::new();
    let mut pos = 0;

    while let Some(start) = source[pos..].find("<%@") {
        let abs_start = pos + start + 3;
        if let Some(end) = source[abs_start..].find("%>") {
            let content = &source[abs_start..abs_start + end].trim();
            if let Some(dir) = parse_directive(content) {
                directives.push(dir);
            }
            pos = abs_start + end + 2;
        } else {
            break;
        }
    }

    directives
}

/// Parse a single directive's content (between <%@ and %>).
fn parse_directive(content: &str) -> Option<AspxDirective> {
    let parts: Vec<&str> = content.splitn(2, char::is_whitespace).collect();
    let directive_type = parts.first()?.to_string();
    let attrs_str = parts.get(1).unwrap_or(&"");

    let mut dir = AspxDirective {
        directive_type,
        code_behind: None,
        inherits: None,
        master_page: None,
        src: None,
        namespace: None,
    };

    // Parse key="value" pairs
    for (key, value) in parse_attributes(attrs_str) {
        match key.to_lowercase().as_str() {
            "codebehind" | "codefile" => dir.code_behind = Some(value),
            "inherits" => dir.inherits = Some(value),
            "masterpagefile" => dir.master_page = Some(value),
            "src" => dir.src = Some(value),
            "namespace" => dir.namespace = Some(value),
            _ => {}
        }
    }

    Some(dir)
}

/// Parse HTML-style key="value" attributes.
fn parse_attributes(s: &str) -> Vec<(String, String)> {
    let mut attrs = Vec::new();
    let mut chars = s.chars().peekable();

    loop {
        // Skip whitespace
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }

        if chars.peek().is_none() {
            break;
        }

        // Read key
        let key: String = chars
            .by_ref()
            .take_while(|c| *c != '=')
            .collect::<String>()
            .trim()
            .to_string();

        if key.is_empty() {
            break;
        }

        // Skip = and opening quote
        while chars
            .peek()
            .is_some_and(|c| *c == '=' || *c == '"' || c.is_whitespace())
        {
            chars.next();
        }

        // Read value until closing quote or whitespace
        let value: String = chars
            .by_ref()
            .take_while(|c| *c != '"' && *c != '\'' && *c != ' ')
            .collect();

        if !key.is_empty() {
            attrs.push((key, value));
        }
    }

    attrs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_page_directive() {
        let source = r#"<%@ Page Language="C#" AutoEventWireup="true" CodeBehind="Default.aspx.cs" Inherits="MyApp.Default" MasterPageFile="~/Site.Master" %>"#;
        let directives = extract_directives(source);
        assert_eq!(directives.len(), 1);
        let d = &directives[0];
        assert_eq!(d.directive_type, "Page");
        assert_eq!(d.code_behind.as_deref(), Some("Default.aspx.cs"));
        assert_eq!(d.inherits.as_deref(), Some("MyApp.Default"));
        assert_eq!(d.master_page.as_deref(), Some("~/Site.Master"));
    }

    #[test]
    fn test_extract_register_directive() {
        let source =
            r#"<%@ Register TagPrefix="uc" TagName="Header" Src="Controls/Header.ascx" %>"#;
        let directives = extract_directives(source);
        assert_eq!(directives.len(), 1);
        assert_eq!(directives[0].src.as_deref(), Some("Controls/Header.ascx"));
    }

    #[test]
    fn test_extract_import_directive() {
        let source = r#"<%@ Import Namespace="System.Data" %>"#;
        let directives = extract_directives(source);
        assert_eq!(directives.len(), 1);
        assert_eq!(directives[0].namespace.as_deref(), Some("System.Data"));
    }

    #[test]
    fn test_multiple_directives() {
        let source = r#"<%@ Page CodeBehind="Default.aspx.cs" Inherits="MyApp.Default" %>
<%@ Import Namespace="System.Data" %>
<%@ Import Namespace="System.Linq" %>
<%@ Register Src="~/Controls/Nav.ascx" TagPrefix="uc" TagName="Nav" %>
<html>
<body>
<uc:Nav runat="server" />
</body>
</html>"#;
        let directives = extract_directives(source);
        assert_eq!(directives.len(), 4);
    }

    #[test]
    fn test_parse_aspx_graph() {
        let source = r#"<%@ Page Language="C#" CodeBehind="Default.aspx.cs" Inherits="MyApp.Pages.Default" %>
<%@ Import Namespace="System.Data" %>
<html><body>Hello</body></html>"#;

        let path = Path::new("/tmp/test.aspx");
        let mut graph = CodeGraph::in_memory().unwrap();
        let info = parse_aspx(source, path, &mut graph).unwrap();

        assert_eq!(info.imports.len(), 2); // CodeBehind + Import

        // Check edges
        let mut has_codebehind = false;
        let mut has_extends = false;
        for (_, edge) in graph.iter_edges() {
            match edge.edge_type {
                EdgeType::Imports => {
                    let target = graph.get_node(edge.target_id).unwrap();
                    let name = target.properties.get_string("name").unwrap_or("");
                    if name == "Default.aspx.cs" {
                        has_codebehind = true;
                    }
                }
                EdgeType::Extends => {
                    let target = graph.get_node(edge.target_id).unwrap();
                    let name = target.properties.get_string("name").unwrap_or("");
                    if name == "MyApp.Pages.Default" {
                        has_extends = true;
                    }
                }
                _ => {}
            }
        }
        assert!(has_codebehind, "Should have CodeBehind import edge");
        assert!(has_extends, "Should have Inherits extends edge");
    }
}
