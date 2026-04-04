// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Implementation of the CodeParser trait for COBOL

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

/// COBOL language parser implementing the CodeParser trait
pub struct CobolParser {
    config: ParserConfig,
    metrics: Mutex<ParserMetrics>,
}

impl CobolParser {
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

impl Default for CobolParser {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeParser for CobolParser {
    fn language(&self) -> &str {
        "cobol"
    }

    fn file_extensions(&self) -> &[&str] {
        &[".cob", ".cbl", ".cobol", ".cpy"]
    }

    fn can_parse(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| matches!(ext, "cob" | "cbl" | "cobol" | "cpy"))
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

impl CobolParser {
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

    const MINIMAL_COBOL: &str = concat!(
        "       identification division.\n",
        "       program-id. MINIMAL.\n",
        "       procedure division.\n",
        "       stop run.\n",
    );

    #[test]
    fn test_language() {
        let parser = CobolParser::new();
        assert_eq!(parser.language(), "cobol");
    }

    #[test]
    fn test_file_extensions() {
        let parser = CobolParser::new();
        let exts = parser.file_extensions();
        assert!(exts.contains(&".cob"));
        assert!(exts.contains(&".cbl"));
        assert!(exts.contains(&".cobol"));
        assert!(exts.contains(&".cpy"));
    }

    #[test]
    fn test_can_parse() {
        let parser = CobolParser::new();
        assert!(parser.can_parse(Path::new("program.cob")));
        assert!(parser.can_parse(Path::new("program.cbl")));
        assert!(parser.can_parse(Path::new("program.cobol")));
        assert!(parser.can_parse(Path::new("copybook.cpy")));
        assert!(!parser.can_parse(Path::new("main.go")));
        assert!(!parser.can_parse(Path::new("main.rs")));
        assert!(!parser.can_parse(Path::new("script.py")));
    }

    #[test]
    fn test_parse_minimal_cobol() {
        let parser = CobolParser::new();
        let mut graph = CodeGraph::in_memory().unwrap();
        let result = parser.parse_source(MINIMAL_COBOL, Path::new("minimal.cob"), &mut graph);
        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let info = result.unwrap();
        assert_eq!(info.classes.len(), 1);
    }

    #[test]
    fn test_parse_with_paragraph() {
        let source = concat!(
            "       identification division.\n",
            "       program-id. PROG.\n",
            "       procedure division.\n",
            "       MAIN-PARA.\n",
            "           stop run.\n",
        );
        let parser = CobolParser::new();
        let mut graph = CodeGraph::in_memory().unwrap();
        let result = parser.parse_source(source, Path::new("prog.cob"), &mut graph);
        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let info = result.unwrap();
        assert_eq!(info.classes.len(), 1);
        assert_eq!(info.functions.len(), 1);
    }

    #[test]
    fn test_metrics_updated_on_parse_source() {
        let parser = CobolParser::new();
        let mut graph = CodeGraph::in_memory().unwrap();
        let _ = parser.parse_source(MINIMAL_COBOL, Path::new("test.cob"), &mut graph);
        // parse_source doesn't update metrics (only parse_file does)
        let metrics = parser.metrics();
        assert_eq!(metrics.files_attempted, 0);
    }
}
