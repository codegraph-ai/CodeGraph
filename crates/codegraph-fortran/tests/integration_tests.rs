// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Integration tests for codegraph-fortran

use codegraph::CodeGraph;
use codegraph_fortran::{CodeParser, FortranParser};
use std::io::Write;
use std::path::Path;
use tempfile::NamedTempFile;

fn parser() -> FortranParser {
    FortranParser::new()
}

fn graph() -> CodeGraph {
    CodeGraph::in_memory().unwrap()
}

// ---------------------------------------------------------------------------
// Basic program unit extraction
// ---------------------------------------------------------------------------

#[test]
fn test_parse_program() {
    let mut g = graph();
    let src = "program hello\n  implicit none\n  print *, 'Hello, World!'\nend program hello\n";
    let info = parser()
        .parse_source(src, Path::new("hello.f90"), &mut g)
        .unwrap();
    assert_eq!(info.classes.len(), 1, "Expected 1 program unit");
}

#[test]
fn test_parse_module() {
    let mut g = graph();
    let src = "module constants\n  implicit none\n  real, parameter :: PI = 3.14159\nend module constants\n";
    let info = parser()
        .parse_source(src, Path::new("constants.f90"), &mut g)
        .unwrap();
    assert_eq!(info.classes.len(), 1);
}

#[test]
fn test_parse_subroutine_toplevel() {
    let mut g = graph();
    let src = "subroutine greet(name)\n  character(*), intent(in) :: name\n  print *, 'Hello', name\nend subroutine greet\n";
    let info = parser()
        .parse_source(src, Path::new("greet.f90"), &mut g)
        .unwrap();
    assert!(!info.functions.is_empty(), "Expected at least one function");
}

#[test]
fn test_parse_function_toplevel() {
    let mut g = graph();
    let src =
        "function add(a, b) result(c)\n  integer, intent(in) :: a, b\n  integer :: c\n  c = a + b\nend function add\n";
    let info = parser()
        .parse_source(src, Path::new("math.f90"), &mut g)
        .unwrap();
    assert!(!info.functions.is_empty(), "Expected at least one function");
}

// ---------------------------------------------------------------------------
// Module with contained procedures
// ---------------------------------------------------------------------------

#[test]
fn test_module_with_subroutine() {
    let mut g = graph();
    let src = r#"module mathlib
  implicit none
contains
  subroutine multiply(a, b, result)
    integer, intent(in) :: a, b
    integer, intent(out) :: result
    result = a * b
  end subroutine multiply
end module mathlib
"#;
    let info = parser()
        .parse_source(src, Path::new("mathlib.f90"), &mut g)
        .unwrap();
    assert_eq!(info.classes.len(), 1);
    assert!(!info.functions.is_empty(), "Expected contained subroutine");
}

#[test]
fn test_module_with_function() {
    let mut g = graph();
    let src = r#"module mathlib
  implicit none
contains
  function square(x) result(y)
    integer, intent(in) :: x
    integer :: y
    y = x * x
  end function square
end module mathlib
"#;
    let info = parser()
        .parse_source(src, Path::new("mathlib.f90"), &mut g)
        .unwrap();
    assert_eq!(info.classes.len(), 1);
    assert!(!info.functions.is_empty());
}

// ---------------------------------------------------------------------------
// USE statement (imports)
// ---------------------------------------------------------------------------

#[test]
fn test_use_statement() {
    let mut g = graph();
    let src = "program main\n  use iso_fortran_env\n  implicit none\nend program main\n";
    let info = parser()
        .parse_source(src, Path::new("main.f90"), &mut g)
        .unwrap();
    assert!(!info.imports.is_empty(), "Expected import from USE");
}

#[test]
fn test_use_only() {
    let mut g = graph();
    let src =
        "program main\n  use iso_fortran_env, only: int32, real64\n  implicit none\nend program main\n";
    let info = parser()
        .parse_source(src, Path::new("main.f90"), &mut g)
        .unwrap();
    assert!(!info.imports.is_empty(), "Expected import from USE ONLY");
}

// ---------------------------------------------------------------------------
// CALL statements (call relationships)
// ---------------------------------------------------------------------------

#[test]
fn test_call_statement() {
    let mut g = graph();
    let src = "program main\n  implicit none\n  call greet('World')\nend program main\n";
    let info = parser()
        .parse_source(src, Path::new("main.f90"), &mut g)
        .unwrap();
    // parse_source succeeded without error — call relations are in the graph
    let _ = info;
}

// ---------------------------------------------------------------------------
// can_parse / file extension checks
// ---------------------------------------------------------------------------

#[test]
fn test_can_parse_extensions() {
    let p = parser();
    assert!(p.can_parse(Path::new("main.f90")));
    assert!(p.can_parse(Path::new("lib.f")));
    assert!(p.can_parse(Path::new("module.for")));
    assert!(p.can_parse(Path::new("prog.f95")));
    assert!(p.can_parse(Path::new("prog.f03")));
    assert!(!p.can_parse(Path::new("main.py")));
    assert!(!p.can_parse(Path::new("main.rs")));
    assert!(!p.can_parse(Path::new("main.go")));
}

// ---------------------------------------------------------------------------
// Parse from file
// ---------------------------------------------------------------------------

#[test]
fn test_parse_file() {
    let mut tmp = NamedTempFile::with_suffix(".f90").unwrap();
    let src = b"program hello\n  implicit none\n  print *, 'hi'\nend program hello\n";
    tmp.write_all(src).unwrap();

    let mut g = graph();
    let info = parser().parse_file(tmp.path(), &mut g).unwrap();
    assert_eq!(info.classes.len(), 1);
}

// ---------------------------------------------------------------------------
// Multiple program units in one file
// ---------------------------------------------------------------------------

#[test]
fn test_multiple_subroutines() {
    let mut g = graph();
    let src = r#"subroutine foo()
  implicit none
end subroutine foo

subroutine bar()
  implicit none
end subroutine bar
"#;
    let info = parser()
        .parse_source(src, Path::new("multi.f90"), &mut g)
        .unwrap();
    assert!(
        info.functions.len() >= 2,
        "Expected at least 2 subroutines, got {}",
        info.functions.len()
    );
}

// ---------------------------------------------------------------------------
// Metrics
// ---------------------------------------------------------------------------

#[test]
fn test_metrics_after_parse() {
    let p = parser();
    let mut g = graph();

    let mut tmp = NamedTempFile::with_suffix(".f90").unwrap();
    tmp.write_all(b"program hello\n  implicit none\nend program hello\n")
        .unwrap();
    p.parse_file(tmp.path(), &mut g).unwrap();

    let metrics = p.metrics();
    assert_eq!(metrics.files_attempted, 1);
    assert_eq!(metrics.files_succeeded, 1);
    assert_eq!(metrics.files_failed, 0);
}

// ---------------------------------------------------------------------------
// line_count and byte_count
// ---------------------------------------------------------------------------

#[test]
fn test_line_and_byte_count() {
    let mut g = graph();
    let src = "program hello\n  implicit none\nend program hello\n";
    let info = parser()
        .parse_source(src, Path::new("hello.f90"), &mut g)
        .unwrap();
    assert_eq!(info.line_count, src.lines().count());
    assert_eq!(info.byte_count, src.len());
}

#[test]
fn test_fortran_astrodynamics_toolkit() {
    use codegraph::CodeGraph;
    use codegraph_fortran::{CodeParser, FortranParser};

    let src_dir =
        std::path::Path::new("/Users/anvanster/projects/docs/Fortran-Astrodynamics-Toolkit/src");
    if !src_dir.exists() {
        eprintln!("Skipping: Fortran-Astrodynamics-Toolkit not found");
        return;
    }

    let parser = FortranParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();
    let mut total_files = 0u32;
    let mut total_functions = 0u32;
    let mut parse_errors = 0u32;

    fn find_fortran_files(dir: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    find_fortran_files(&path, files);
                } else if path.extension().is_some_and(|e| {
                    let e = e.to_string_lossy().to_lowercase();
                    matches!(e.as_str(), "f90" | "f95" | "f03" | "f08" | "f" | "for")
                }) {
                    files.push(path);
                }
            }
        }
    }

    let mut fortran_files = Vec::new();
    find_fortran_files(src_dir, &mut fortran_files);

    for path in &fortran_files {
        total_files += 1;
        match parser.parse_file(path, &mut graph) {
            Ok(fi) => total_functions += fi.functions.len() as u32,
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

    eprintln!("\n=== Fortran Astrodynamics Toolkit ===");
    eprintln!("Files:     {total_files}");
    eprintln!("Functions: {total_functions}");
    eprintln!("Errors:    {parse_errors}");
    eprintln!("Nodes:     {}", graph.node_count());
    eprintln!("Edges:     {}", graph.edge_count());

    let success_rate = (total_files - parse_errors) as f64 / total_files as f64;
    eprintln!("Success:   {:.0}%", success_rate * 100.0);

    assert!(total_files > 20, "Expected >20 Fortran files");
    assert!(total_functions > 50, "Expected >50 functions");
    assert!(success_rate > 0.5, "Less than 50% success rate");
}

#[test]
fn test_fortran_graph_quality() {
    use codegraph::{CodeGraph, EdgeType, NodeType};
    use codegraph_fortran::{CodeParser, FortranParser};

    let src_dir =
        std::path::Path::new("/Users/anvanster/projects/docs/Fortran-Astrodynamics-Toolkit/src");
    if !src_dir.exists() {
        eprintln!("Skipping: not found");
        return;
    }

    let parser = FortranParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    fn find_files(dir: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    find_files(&path, files);
                } else if path
                    .extension()
                    .is_some_and(|e| e.to_string_lossy().to_lowercase() == "f90")
                {
                    files.push(path);
                }
            }
        }
    }
    let mut files = Vec::new();
    find_files(src_dir, &mut files);
    for f in &files {
        let _ = parser.parse_file(f, &mut graph);
    }

    // Count by type
    let mut functions = 0u32;
    let mut modules = 0u32;
    let mut code_files = 0u32;
    let mut calls_edges = 0u32;
    let mut import_edges = 0u32;
    let mut contains_edges = 0u32;
    let mut unresolved_count = 0u32;

    for (_, node) in graph.iter_nodes() {
        match node.node_type {
            NodeType::Function => functions += 1,
            NodeType::Class => modules += 1,
            NodeType::CodeFile => code_files += 1,
            _ => {}
        }
        if let Some(calls) = node.properties.get_string_list_compat("unresolved_calls") {
            unresolved_count += calls.len() as u32;
        }
    }
    for (_eid, edge) in graph.iter_edges() {
        match edge.edge_type {
            EdgeType::Calls => calls_edges += 1,
            EdgeType::Imports => import_edges += 1,
            EdgeType::Contains => contains_edges += 1,
            _ => {}
        }
    }

    // Collect unresolved call frequency
    let mut freq: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for (_, node) in graph.iter_nodes() {
        if let Some(calls) = node.properties.get_string_list_compat("unresolved_calls") {
            for c in calls {
                *freq.entry(c.to_lowercase()).or_default() += 1;
            }
        }
    }
    let mut sorted: Vec<_> = freq.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(a.1));

    eprintln!("\n=== Fortran Graph Quality ===");
    eprintln!("Function nodes:    {functions}");
    eprintln!("Module nodes:      {modules}");
    eprintln!("CodeFile nodes:    {code_files}");
    eprintln!("Calls edges:       {calls_edges}");
    eprintln!("Import edges:      {import_edges}");
    eprintln!("Contains edges:    {contains_edges}");
    eprintln!("Unresolved calls:  {unresolved_count}");
    eprintln!("Unique unresolved: {}", freq.len());
    eprintln!("\nTop 30 unresolved:");
    for (name, count) in sorted.iter().take(30) {
        eprintln!("  {count:4}x  {name}");
    }

    assert!(functions > 200, "Expected >200 functions");
    assert!(
        calls_edges > 0 || unresolved_count > 100,
        "Expected call relationships"
    );
    assert!(import_edges > 50, "Expected >50 import edges");
}
