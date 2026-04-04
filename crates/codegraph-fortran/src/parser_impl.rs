// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Implementation of the CodeParser trait for Fortran

use codegraph::CodeGraph;
use codegraph_parser_api::{
    CodeIR, CodeParser, FileInfo, ParserConfig, ParserError, ParserMetrics, ProjectInfo,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::extractor;
use crate::mapper;

/// Fortran language parser implementing the CodeParser trait
pub struct FortranParser {
    config: ParserConfig,
    metrics: Mutex<ParserMetrics>,
}

impl FortranParser {
    pub fn new() -> Self {
        Self {
            config: ParserConfig::default(),
            metrics: Mutex::new(ParserMetrics::default()),
        }
    }

    pub fn with_config(config: ParserConfig) -> Self {
        Self {
            config,
            metrics: Mutex::new(ParserMetrics::default()),
        }
    }

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

    fn ir_to_graph(
        &self,
        ir: &CodeIR,
        graph: &mut CodeGraph,
        file_path: &Path,
    ) -> Result<FileInfo, ParserError> {
        mapper::ir_to_graph(ir, graph, file_path)
    }
}

impl Default for FortranParser {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeParser for FortranParser {
    fn language(&self) -> &str {
        "fortran"
    }

    fn file_extensions(&self) -> &[&str] {
        &[
            ".f", ".f90", ".f95", ".f03", ".f08", ".for", ".ftn", ".F", ".F90",
        ]
    }

    fn can_parse(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| {
                matches!(
                    ext,
                    "f" | "f90" | "f95" | "f03" | "f08" | "for" | "ftn" | "F" | "F90"
                )
            })
            .unwrap_or(false)
    }

    fn parse_file(&self, path: &Path, graph: &mut CodeGraph) -> Result<FileInfo, ParserError> {
        let start = Instant::now();
        let metadata =
            fs::metadata(path).map_err(|e| ParserError::IoError(path.to_path_buf(), e))?;

        if metadata.len() as usize > self.config.max_file_size {
            return Err(ParserError::FileTooLarge(
                path.to_path_buf(),
                metadata.len() as usize,
            ));
        }

        let source =
            fs::read_to_string(path).map_err(|e| ParserError::IoError(path.to_path_buf(), e))?;
        let result = self.parse_source(&source, path, graph);

        let duration = start.elapsed();
        if let Ok(ref info) = result {
            self.update_metrics(true, duration, info.entity_count(), 0);
        } else {
            self.update_metrics(false, duration, 0, 0);
        }

        result
    }

    fn parse_source(
        &self,
        source: &str,
        file_path: &Path,
        graph: &mut CodeGraph,
    ) -> Result<FileInfo, ParserError> {
        let start = Instant::now();
        let ir = extractor::extract(source, file_path, &self.config)?;
        let mut file_info = self.ir_to_graph(&ir, graph, file_path)?;

        file_info.parse_time = start.elapsed();
        file_info.line_count = source.lines().count();
        file_info.byte_count = source.len();

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

    fn parse_files(
        &self,
        paths: &[PathBuf],
        graph: &mut CodeGraph,
    ) -> Result<ProjectInfo, ParserError> {
        self.parse_files_sequential(paths, graph)
    }
}

impl FortranParser {
    fn parse_files_sequential(
        &self,
        paths: &[PathBuf],
        graph: &mut CodeGraph,
    ) -> Result<ProjectInfo, ParserError> {
        let mut files = Vec::new();
        let mut failed_files = Vec::new();
        let mut total_functions = 0;
        let mut total_classes = 0;
        let mut total_parse_time = Duration::ZERO;

        for path in paths {
            match self.parse_file(path, graph) {
                Ok(info) => {
                    total_functions += info.functions.len();
                    total_classes += info.classes.len();
                    total_parse_time += info.parse_time;
                    files.push(info);
                }
                Err(e) => {
                    failed_files.push((path.clone(), e.to_string()));
                }
            }
        }

        Ok(ProjectInfo {
            files,
            failed_files,
            total_functions,
            total_classes,
            total_parse_time,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language() {
        let parser = FortranParser::new();
        assert_eq!(parser.language(), "fortran");
    }

    #[test]
    fn test_file_extensions() {
        let parser = FortranParser::new();
        let exts = parser.file_extensions();
        assert!(exts.contains(&".f90"));
        assert!(exts.contains(&".f"));
        assert!(exts.contains(&".for"));
    }

    #[test]
    fn test_can_parse() {
        let parser = FortranParser::new();
        assert!(parser.can_parse(Path::new("main.f90")));
        assert!(parser.can_parse(Path::new("lib.f")));
        assert!(parser.can_parse(Path::new("module.for")));
        assert!(parser.can_parse(Path::new("prog.f95")));
        assert!(!parser.can_parse(Path::new("main.go")));
        assert!(!parser.can_parse(Path::new("main.rs")));
        assert!(!parser.can_parse(Path::new("script.py")));
    }

    #[test]
    fn test_parse_simple_program() {
        let parser = FortranParser::new();
        let mut graph = CodeGraph::in_memory().unwrap();
        let source = "program hello\n  implicit none\n  print *, 'Hello'\nend program hello\n";
        let result = parser.parse_source(source, Path::new("hello.f90"), &mut graph);
        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let info = result.unwrap();
        assert_eq!(info.classes.len(), 1);
    }

    #[test]
    fn test_parse_module() {
        let parser = FortranParser::new();
        let mut graph = CodeGraph::in_memory().unwrap();
        let source = "module mymod\n  implicit none\nend module mymod\n";
        let result = parser.parse_source(source, Path::new("mymod.f90"), &mut graph);
        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let info = result.unwrap();
        assert_eq!(info.classes.len(), 1);
    }

    #[test]
    fn test_parse_subroutine() {
        let parser = FortranParser::new();
        let mut graph = CodeGraph::in_memory().unwrap();
        let source =
            "subroutine greet(name)\n  character(*), intent(in) :: name\nend subroutine greet\n";
        let result = parser.parse_source(source, Path::new("greet.f90"), &mut graph);
        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let info = result.unwrap();
        assert!(!info.functions.is_empty());
    }
}
