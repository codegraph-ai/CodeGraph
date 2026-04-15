// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Implementation of the CodeParser trait for TOML

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

/// TOML language parser implementing the CodeParser trait
pub struct TomlParser {
    config: ParserConfig,
    metrics: Mutex<ParserMetrics>,
}

impl TomlParser {
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

impl Default for TomlParser {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeParser for TomlParser {
    fn language(&self) -> &str {
        "toml"
    }

    fn file_extensions(&self) -> &[&str] {
        &[".toml"]
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
        if self.config.parallel {
            self.parse_files_parallel(paths, graph)
        } else {
            self.parse_files_sequential(paths, graph)
        }
    }
}

impl TomlParser {
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

    fn parse_files_parallel(
        &self,
        paths: &[PathBuf],
        graph: &mut CodeGraph,
    ) -> Result<ProjectInfo, ParserError> {
        use rayon::prelude::*;

        let graph_mutex = Mutex::new(graph);

        let pool = if let Some(num_threads) = self.config.parallel_workers {
            rayon::ThreadPoolBuilder::new()
                .num_threads(num_threads)
                .build()
                .map_err(|e| {
                    ParserError::GraphError(format!("Failed to create thread pool: {e}"))
                })?
        } else {
            rayon::ThreadPoolBuilder::new().build().map_err(|e| {
                ParserError::GraphError(format!("Failed to create thread pool: {e}"))
            })?
        };

        let results: Vec<_> = pool.install(|| {
            paths
                .par_iter()
                .map(|path| {
                    let mut graph = graph_mutex.lock().unwrap();
                    match self.parse_file(path, &mut graph) {
                        Ok(info) => Ok(info),
                        Err(e) => Err((path.clone(), e.to_string())),
                    }
                })
                .collect()
        });

        let mut files = Vec::new();
        let mut failed_files = Vec::new();
        let mut total_functions = 0;
        let mut total_classes = 0;
        let mut total_parse_time = Duration::ZERO;

        for result in results {
            match result {
                Ok(info) => {
                    total_functions += info.functions.len();
                    total_classes += info.classes.len();
                    total_parse_time += info.parse_time;
                    files.push(info);
                }
                Err((path, error)) => {
                    failed_files.push((path, error));
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
        let parser = TomlParser::new();
        assert_eq!(parser.language(), "toml");
    }

    #[test]
    fn test_file_extensions() {
        let parser = TomlParser::new();
        assert_eq!(parser.file_extensions(), &[".toml"]);
    }

    #[test]
    fn test_can_parse() {
        let parser = TomlParser::new();
        assert!(parser.can_parse(Path::new("Cargo.toml")));
        assert!(parser.can_parse(Path::new("pyproject.toml")));
        assert!(parser.can_parse(Path::new("config/settings.toml")));
        assert!(!parser.can_parse(Path::new("main.rs")));
        assert!(!parser.can_parse(Path::new("config.yaml")));
    }

    #[test]
    fn test_parse_source_basic() {
        let parser = TomlParser::new();
        let mut graph = CodeGraph::in_memory().unwrap();
        let source = r#"
[package]
name = "test"
version = "0.1.0"
"#;
        let result = parser.parse_source(source, Path::new("Cargo.toml"), &mut graph);
        assert!(result.is_ok());
        let info = result.unwrap();
        assert_eq!(info.classes.len(), 1);
        assert!(!info.functions.is_empty());
    }

    #[test]
    fn test_parse_source_empty() {
        let parser = TomlParser::new();
        let mut graph = CodeGraph::in_memory().unwrap();
        let result = parser.parse_source("", Path::new("empty.toml"), &mut graph);
        assert!(result.is_ok());
    }

    #[test]
    fn test_metrics_update_on_success() {
        let parser = TomlParser::new();
        let mut graph = CodeGraph::in_memory().unwrap();
        parser
            .parse_source("[package]\nname = \"x\"", Path::new("t.toml"), &mut graph)
            .unwrap();
        let metrics = parser.metrics();
        assert_eq!(metrics.files_attempted, 1);
        assert_eq!(metrics.files_succeeded, 1);
        assert_eq!(metrics.files_failed, 0);
    }
}
