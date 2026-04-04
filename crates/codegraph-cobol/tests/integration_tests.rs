// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Integration tests for the COBOL parser

use codegraph::CodeGraph;
use codegraph_cobol::CobolParser;
use codegraph_parser_api::CodeParser;
use std::io::Write;
use std::path::Path;
use tempfile::NamedTempFile;

/// COBOL fixed-format helper: 7-space indent for Area A keywords
const INDENT: &str = "       ";

fn cobol_program(name: &str, body: &str) -> String {
    format!(
        "{INDENT}identification division.\n\
         {INDENT}program-id. {name}.\n\
         {body}"
    )
}

fn procedure_div(stmts: &str) -> String {
    format!("{INDENT}procedure division.\n{stmts}")
}

#[test]
fn test_parse_minimal_program() {
    let parser = CobolParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = cobol_program("MINIMAL", &procedure_div(&format!("{INDENT}stop run.\n")));
    let result = parser.parse_source(&source, Path::new("minimal.cob"), &mut graph);
    assert!(result.is_ok(), "Failed: {:?}", result.err());
    let info = result.unwrap();
    assert_eq!(info.classes.len(), 1, "Expected 1 program");
}

#[test]
fn test_program_name_extracted() {
    let parser = CobolParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = cobol_program("HELLO", &procedure_div(&format!("{INDENT}stop run.\n")));
    let result = parser.parse_source(&source, Path::new("hello.cob"), &mut graph);
    assert!(result.is_ok(), "Failed: {:?}", result.err());
    let info = result.unwrap();

    assert_eq!(info.classes.len(), 1);
    let prog_node = graph.get_node(info.classes[0]).unwrap();
    let prog_name = prog_node.properties.get_string("name").unwrap();
    assert_eq!(prog_name, "HELLO");
}

#[test]
fn test_paragraph_extracted() {
    let parser = CobolParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let body = procedure_div(&format!(
        "{INDENT}MAIN-PARA.\n\
         {INDENT}    stop run.\n"
    ));
    let source = cobol_program("PARA-PROG", &body);
    let result = parser.parse_source(&source, Path::new("para.cob"), &mut graph);
    assert!(result.is_ok(), "Failed: {:?}", result.err());
    let info = result.unwrap();

    assert_eq!(info.classes.len(), 1, "Expected 1 program");
    assert_eq!(info.functions.len(), 1, "Expected 1 paragraph");

    let para_node = graph.get_node(info.functions[0]).unwrap();
    let para_name = para_node.properties.get_string("name").unwrap();
    assert_eq!(para_name, "MAIN-PARA");
}

#[test]
fn test_multiple_paragraphs() {
    let parser = CobolParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let body = procedure_div(&format!(
        "{INDENT}PARA-A.\n\
         {INDENT}    continue.\n\
         {INDENT}PARA-B.\n\
         {INDENT}    stop run.\n"
    ));
    let source = cobol_program("MULTI", &body);
    let result = parser.parse_source(&source, Path::new("multi.cob"), &mut graph);
    assert!(result.is_ok(), "Failed: {:?}", result.err());
    let info = result.unwrap();

    assert_eq!(info.functions.len(), 2, "Expected 2 paragraphs");
    let names: Vec<String> = info
        .functions
        .iter()
        .map(|id| {
            graph
                .get_node(*id)
                .unwrap()
                .properties
                .get_string("name")
                .unwrap()
                .to_string()
        })
        .collect();
    assert!(names.contains(&"PARA-A".to_string()), "Missing PARA-A");
    assert!(names.contains(&"PARA-B".to_string()), "Missing PARA-B");
}

#[test]
fn test_copy_statement_extracted() {
    let parser = CobolParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = format!(
        "{INDENT}identification division.\n\
         {INDENT}program-id. COPYPROG.\n\
         {INDENT}data division.\n\
         {INDENT}working-storage section.\n\
         {INDENT}copy MYBOOK.\n\
         {INDENT}procedure division.\n\
         {INDENT}stop run.\n"
    );
    let result = parser.parse_source(&source, Path::new("copy.cob"), &mut graph);
    assert!(result.is_ok(), "Failed: {:?}", result.err());
    let info = result.unwrap();

    assert!(
        !info.imports.is_empty(),
        "Expected COPY statement as import"
    );
    let import_node = graph.get_node(info.imports[0]).unwrap();
    let imported = import_node.properties.get_string("name").unwrap();
    assert_eq!(imported, "MYBOOK");
}

#[test]
fn test_can_parse_extensions() {
    let parser = CobolParser::new();
    assert!(parser.can_parse(Path::new("program.cob")));
    assert!(parser.can_parse(Path::new("program.cbl")));
    assert!(parser.can_parse(Path::new("program.cobol")));
    assert!(parser.can_parse(Path::new("copybook.cpy")));
    assert!(!parser.can_parse(Path::new("script.py")));
    assert!(!parser.can_parse(Path::new("main.rs")));
}

#[test]
fn test_file_info_line_count() {
    let parser = CobolParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = cobol_program("LINES", &procedure_div(&format!("{INDENT}stop run.\n")));
    let result = parser.parse_source(&source, Path::new("lines.cob"), &mut graph);
    assert!(result.is_ok());
    let info = result.unwrap();
    assert!(info.line_count > 0);
}

#[test]
fn test_parse_file_from_disk() {
    let parser = CobolParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = cobol_program("FILEPROG", &procedure_div(&format!("{INDENT}stop run.\n")));
    let mut tmp = NamedTempFile::with_suffix(".cob").unwrap();
    tmp.write_all(source.as_bytes()).unwrap();

    let result = parser.parse_file(tmp.path(), &mut graph);
    assert!(result.is_ok(), "Failed: {:?}", result.err());
    let info = result.unwrap();
    assert_eq!(info.classes.len(), 1);

    // Verify metrics updated
    let metrics = parser.metrics();
    assert_eq!(metrics.files_attempted, 1);
    assert_eq!(metrics.files_succeeded, 1);
}

#[test]
fn test_language_identifier() {
    let parser = CobolParser::new();
    assert_eq!(parser.language(), "cobol");
}

#[test]
fn test_paragraph_linked_to_program() {
    let parser = CobolParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let body = procedure_div(&format!("{INDENT}MAIN-PARA.\n{INDENT}    stop run.\n"));
    let source = cobol_program("LINKED", &body);
    let result = parser.parse_source(&source, Path::new("linked.cob"), &mut graph);
    assert!(result.is_ok(), "Failed: {:?}", result.err());
    let info = result.unwrap();

    assert_eq!(info.classes.len(), 1);
    assert_eq!(info.functions.len(), 1);

    // Verify the paragraph has parent_class set to program name
    let para_node = graph.get_node(info.functions[0]).unwrap();
    let parent = para_node.properties.get_string("parent_class");
    assert_eq!(
        parent,
        Some("LINKED"),
        "Paragraph should be linked to LINKED program"
    );
}

#[test]
fn test_cobol_programming_course() {
    use codegraph::CodeGraph;
    use codegraph_cobol::CobolParser;
    use codegraph_parser_api::CodeParser;

    let src_dir = std::path::Path::new("/Users/anvanster/projects/docs/cobol-programming-course");
    if !src_dir.exists() {
        eprintln!("Skipping: cobol-programming-course not found");
        return;
    }

    let parser = CobolParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();
    let mut total_files = 0u32;
    let mut total_functions = 0u32;
    let mut parse_errors = 0u32;

    fn find_cobol_files(dir: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    find_cobol_files(&path, files);
                } else if path.extension().is_some_and(|e| {
                    let e = e.to_string_lossy().to_lowercase();
                    matches!(e.as_str(), "cob" | "cbl" | "cpy" | "cobol")
                }) {
                    files.push(path);
                }
            }
        }
    }

    let mut cobol_files = Vec::new();
    find_cobol_files(src_dir, &mut cobol_files);

    for path in &cobol_files {
        total_files += 1;
        match parser.parse_file(path, &mut graph) {
            Ok(fi) => {
                total_functions += fi.functions.len() as u32;
            }
            Err(e) => {
                parse_errors += 1;
                eprintln!(
                    "  FAIL: {}: {}",
                    path.file_name().unwrap().to_string_lossy(),
                    e
                );
            }
        }
    }

    eprintln!("\n=== COBOL Programming Course ===");
    eprintln!("Files:     {total_files}");
    eprintln!("Functions: {total_functions}");
    eprintln!("Errors:    {parse_errors}");
    eprintln!("Nodes:     {}", graph.node_count());
    eprintln!("Edges:     {}", graph.edge_count());

    let success_rate = if total_files > 0 {
        (total_files - parse_errors) as f64 / total_files as f64
    } else {
        0.0
    };
    eprintln!("Success:   {:.0}%", success_rate * 100.0);

    assert!(
        total_files >= 5,
        "Expected >=5 COBOL files, got {total_files}"
    );
}

#[test]
fn test_aws_carddemo_cobol() {
    use codegraph::CodeGraph;
    use codegraph_cobol::CobolParser;
    use codegraph_parser_api::CodeParser;

    let src_dir =
        std::path::Path::new("/Users/anvanster/projects/docs/aws-mainframe-modernization-carddemo");
    if !src_dir.exists() {
        eprintln!("Skipping: aws-mainframe-modernization-carddemo not found");
        return;
    }

    let parser = CobolParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();
    let mut total_files = 0u32;
    let mut total_functions = 0u32;
    let mut parse_errors = 0u32;
    let mut error_files = Vec::new();

    fn find_cobol_files(dir: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    find_cobol_files(&path, files);
                } else if path.extension().is_some_and(|e| {
                    let e = e.to_string_lossy().to_lowercase();
                    matches!(e.as_str(), "cob" | "cbl" | "cpy" | "cobol")
                }) {
                    files.push(path);
                }
            }
        }
    }

    let mut cobol_files = Vec::new();
    find_cobol_files(src_dir, &mut cobol_files);

    for path in &cobol_files {
        total_files += 1;
        match parser.parse_file(path, &mut graph) {
            Ok(fi) => {
                total_functions += fi.functions.len() as u32;
            }
            Err(e) => {
                parse_errors += 1;
                error_files.push((
                    path.file_name().unwrap().to_string_lossy().to_string(),
                    format!("{}", e),
                ));
            }
        }
    }

    eprintln!("\n=== AWS CardDemo (Credit Card Management) ===");
    eprintln!("Files:     {total_files}");
    eprintln!("Functions: {total_functions}");
    eprintln!("Errors:    {parse_errors}");
    eprintln!("Nodes:     {}", graph.node_count());
    eprintln!("Edges:     {}", graph.edge_count());

    if !error_files.is_empty() {
        eprintln!("\nFailed files:");
        for (name, err) in &error_files {
            let short = if err.len() > 80 { &err[..80] } else { err };
            eprintln!("  {name}: {short}");
        }
    }

    let success_rate = if total_files > 0 {
        (total_files - parse_errors) as f64 / total_files as f64
    } else {
        0.0
    };
    eprintln!("Success:   {:.0}%", success_rate * 100.0);

    assert!(
        total_files > 50,
        "Expected >50 COBOL files, got {total_files}"
    );
    assert!(
        success_rate > 0.5,
        "Less than 50% success rate: {:.0}%",
        success_rate * 100.0
    );
}
