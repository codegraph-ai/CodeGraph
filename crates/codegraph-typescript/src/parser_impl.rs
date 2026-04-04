// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Implementation of the CodeParser trait for TypeScript/JavaScript

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

/// TypeScript/JavaScript language parser implementing the CodeParser trait
pub struct TypeScriptParser {
    config: ParserConfig,
    metrics: Mutex<ParserMetrics>,
}

impl TypeScriptParser {
    /// Create a new TypeScript parser with default configuration
    pub fn new() -> Self {
        Self {
            config: ParserConfig::default(),
            metrics: Mutex::new(ParserMetrics::default()),
        }
    }

    /// Create a new TypeScript parser with custom configuration
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
        ir: &CodeIR,
        graph: &mut CodeGraph,
        file_path: &Path,
    ) -> Result<FileInfo, ParserError> {
        mapper::ir_to_graph(ir, graph, file_path)
    }
}

impl Default for TypeScriptParser {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeParser for TypeScriptParser {
    fn language(&self) -> &str {
        "typescript"
    }

    fn file_extensions(&self) -> &[&str] {
        &[".ts", ".tsx", ".js", ".jsx"]
    }

    fn parse_file(&self, path: &Path, graph: &mut CodeGraph) -> Result<FileInfo, ParserError> {
        let start = Instant::now();

        // Check file size
        let metadata =
            fs::metadata(path).map_err(|e| ParserError::IoError(path.to_path_buf(), e))?;

        if metadata.len() as usize > self.config.max_file_size {
            return Err(ParserError::FileTooLarge(
                path.to_path_buf(),
                metadata.len() as usize,
            ));
        }

        // Read file
        let source =
            fs::read_to_string(path).map_err(|e| ParserError::IoError(path.to_path_buf(), e))?;

        // Parse source
        let result = self.parse_source(&source, path, graph);

        // Update metrics
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

        // Extract entities and relationships into IR
        let ir = extractor::extract(source, file_path, &self.config)?;

        // Insert IR into graph
        let mut file_info = self.ir_to_graph(&ir, graph, file_path)?;

        // Add timing and size information
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

impl TypeScriptParser {
    /// Parse files sequentially
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

    /// Parse files in parallel using rayon
    fn parse_files_parallel(
        &self,
        paths: &[PathBuf],
        graph: &mut CodeGraph,
    ) -> Result<ProjectInfo, ParserError> {
        use rayon::prelude::*;

        let graph_mutex = Mutex::new(graph);

        // Configure thread pool if parallel_workers is specified
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
        let parser = TypeScriptParser::new();
        assert_eq!(parser.language(), "typescript");
    }

    #[test]
    fn test_file_extensions() {
        let parser = TypeScriptParser::new();
        assert_eq!(parser.file_extensions(), &[".ts", ".tsx", ".js", ".jsx"]);
    }

    #[test]
    fn test_can_parse() {
        let parser = TypeScriptParser::new();
        assert!(parser.can_parse(Path::new("test.ts")));
        assert!(parser.can_parse(Path::new("test.tsx")));
        assert!(parser.can_parse(Path::new("test.js")));
        assert!(parser.can_parse(Path::new("test.jsx")));
        assert!(!parser.can_parse(Path::new("test.rs")));
    }
}
