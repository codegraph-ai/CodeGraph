// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Implementation of the CodeParser trait for Bash

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

/// Bash language parser implementing the CodeParser trait
pub struct BashParser {
    config: ParserConfig,
    metrics: Mutex<ParserMetrics>,
}

impl BashParser {
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

impl Default for BashParser {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeParser for BashParser {
    fn language(&self) -> &str {
        "bash"
    }

    fn file_extensions(&self) -> &[&str] {
        &[".sh", ".bash", ".zsh"]
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

impl BashParser {
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
        let parser = BashParser::new();
        assert_eq!(parser.language(), "bash");
    }

    #[test]
    fn test_file_extensions() {
        let parser = BashParser::new();
        assert_eq!(parser.file_extensions(), &[".sh", ".bash", ".zsh"]);
    }

    #[test]
    fn test_can_parse() {
        let parser = BashParser::new();
        assert!(parser.can_parse(Path::new("script.sh")));
        assert!(parser.can_parse(Path::new("script.bash")));
        assert!(parser.can_parse(Path::new("script.zsh")));
        assert!(!parser.can_parse(Path::new("main.py")));
    }

    use std::io::Write;

    /// A small but syntactically complete Bash script touching every extracted
    /// entity kind: one `source` directive (import) and one function definition.
    /// Bash has no class or trait concept - the visitor carries only functions,
    /// imports, and calls - so this pins functions=1/classes=0/traits=0/
    /// imports=1 with entity_count=1.
    const SAMPLE: &str = r#"#!/bin/bash
source ./lib.sh

greet() {
    echo "hello"
}
"#;

    fn graph() -> CodeGraph {
        CodeGraph::in_memory().expect("in-memory graph")
    }

    fn write_file(dir: &Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        let mut f = fs::File::create(&path).expect("create temp file");
        f.write_all(content.as_bytes()).expect("write temp file");
        path
    }

    #[test]
    fn test_default_matches_new_metrics() {
        let default = BashParser::default().metrics();
        let new = BashParser::new().metrics();
        assert_eq!(default.files_attempted, new.files_attempted);
        assert_eq!(default.files_attempted, 0);
        assert_eq!(default.total_entities, 0);
    }

    #[test]
    fn test_with_config_and_accessor() {
        let cfg = ParserConfig::default().with_max_file_size(4242);
        let parser = BashParser::with_config(cfg);
        assert_eq!(parser.config().max_file_size, 4242);
    }

    #[test]
    fn test_parse_source_extracts_each_entity_kind() {
        let parser = BashParser::new();
        let mut g = graph();
        let info = parser
            .parse_source(SAMPLE, Path::new("deploy.sh"), &mut g)
            .expect("parse ok");
        assert_eq!(info.functions.len(), 1, "one function definition");
        assert_eq!(info.classes.len(), 0, "Bash has no classes");
        assert_eq!(info.traits.len(), 0, "Bash has no traits");
        assert_eq!(info.imports.len(), 1, "one source directive");
        assert_eq!(info.entity_count(), 1, "functions only");
    }

    #[test]
    fn test_parse_source_records_line_and_byte_counts() {
        let parser = BashParser::new();
        let mut g = graph();
        let info = parser
            .parse_source(SAMPLE, Path::new("deploy.sh"), &mut g)
            .expect("parse ok");
        assert_eq!(info.line_count, SAMPLE.lines().count());
        assert_eq!(info.byte_count, SAMPLE.len());
    }

    #[test]
    fn test_parse_source_comment_only_yields_no_entities() {
        // Bash is tree-sitter-based and error-tolerant, so a comment-only
        // source parses cleanly and simply extracts nothing.
        let parser = BashParser::new();
        let mut g = graph();
        let src = "# just a comment\n";
        let info = parser
            .parse_source(src, Path::new("empty.sh"), &mut g)
            .expect("comment-only source still parses");
        assert_eq!(info.functions.len(), 0);
        assert_eq!(info.classes.len(), 0);
        assert_eq!(info.traits.len(), 0);
        assert_eq!(info.imports.len(), 0);
        assert_eq!(info.line_count, 1);
        assert_eq!(info.byte_count, src.len());
    }

    #[test]
    fn test_parse_source_does_not_touch_metrics() {
        // Only parse_file updates metrics; parse_source is metric-free.
        let parser = BashParser::new();
        let mut g = graph();
        parser
            .parse_source(SAMPLE, Path::new("deploy.sh"), &mut g)
            .expect("parse ok");
        let m = parser.metrics();
        assert_eq!(m.files_attempted, 0);
        assert_eq!(m.files_succeeded, 0);
    }

    #[test]
    fn test_parse_file_success_updates_metrics() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_file(dir.path(), "deploy.sh", SAMPLE);
        let parser = BashParser::new();
        let mut g = graph();
        let info = parser.parse_file(&path, &mut g).expect("parse ok");
        assert_eq!(info.functions.len(), 1);
        let m = parser.metrics();
        assert_eq!(m.files_attempted, 1);
        assert_eq!(m.files_succeeded, 1);
        assert_eq!(m.files_failed, 0);
        assert_eq!(m.total_entities, info.entity_count());
    }

    #[test]
    fn test_parse_file_missing_file_is_io_error() {
        let parser = BashParser::new();
        let mut g = graph();
        let err = parser
            .parse_file(Path::new("/no/such/file.sh"), &mut g)
            .expect_err("missing file should error");
        assert!(matches!(err, ParserError::IoError(..)));
        // A pre-read failure never reaches update_metrics.
        assert_eq!(parser.metrics().files_attempted, 0);
    }

    #[test]
    fn test_parse_file_too_large() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_file(dir.path(), "big.sh", SAMPLE);
        let parser = BashParser::with_config(ParserConfig::default().with_max_file_size(4));
        let mut g = graph();
        let err = parser
            .parse_file(&path, &mut g)
            .expect_err("oversized file should error");
        assert!(matches!(err, ParserError::FileTooLarge(..)));
        // The size guard also short-circuits before metrics are touched.
        assert_eq!(parser.metrics().files_attempted, 0);
    }

    #[test]
    fn test_reset_metrics_zeroes_state() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_file(dir.path(), "deploy.sh", SAMPLE);
        let mut parser = BashParser::new();
        let mut g = graph();
        parser.parse_file(&path, &mut g).expect("parse ok");
        assert_eq!(parser.metrics().files_attempted, 1);
        parser.reset_metrics();
        let m = parser.metrics();
        assert_eq!(m.files_attempted, 0);
        assert_eq!(m.files_succeeded, 0);
        assert_eq!(m.total_entities, 0);
    }

    #[test]
    fn test_metrics_accumulate_across_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let a = write_file(dir.path(), "a.sh", SAMPLE);
        let b = write_file(dir.path(), "b.sh", SAMPLE);
        let parser = BashParser::new();
        let mut g = graph();
        parser.parse_file(&a, &mut g).expect("parse a");
        parser.parse_file(&b, &mut g).expect("parse b");
        let m = parser.metrics();
        assert_eq!(m.files_attempted, 2);
        assert_eq!(m.files_succeeded, 2);
    }

    #[test]
    fn test_parse_files_sequential_success() {
        let dir = tempfile::tempdir().expect("tempdir");
        let a = write_file(dir.path(), "a.sh", SAMPLE);
        let b = write_file(dir.path(), "b.sh", SAMPLE);
        let parser = BashParser::new(); // parallel = false by default
        let mut g = graph();
        let project = parser.parse_files(&[a, b], &mut g).expect("parse ok");
        assert_eq!(project.files.len(), 2);
        assert!(project.failed_files.is_empty());
        assert_eq!(project.total_functions, 2);
        assert_eq!(project.total_classes, 0);
    }

    #[test]
    fn test_parse_files_partitions_failures() {
        let dir = tempfile::tempdir().expect("tempdir");
        let good = write_file(dir.path(), "good.sh", SAMPLE);
        let missing = dir.path().join("missing.sh");
        let parser = BashParser::new();
        let mut g = graph();
        let project = parser
            .parse_files(&[good, missing.clone()], &mut g)
            .expect("parse ok");
        assert_eq!(project.files.len(), 1);
        assert_eq!(project.failed_files.len(), 1);
        assert_eq!(project.failed_files[0].0, missing);
    }

    #[test]
    fn test_parse_files_parallel() {
        let dir = tempfile::tempdir().expect("tempdir");
        let a = write_file(dir.path(), "a.sh", SAMPLE);
        let b = write_file(dir.path(), "b.sh", SAMPLE);
        let cfg = ParserConfig::default().with_parallel(true);
        let parser = BashParser::with_config(cfg);
        let mut g = graph();
        let project = parser.parse_files(&[a, b], &mut g).expect("parse ok");
        assert_eq!(project.files.len(), 2);
        assert!(project.failed_files.is_empty());
        assert_eq!(project.total_functions, 2);
    }

    #[test]
    fn test_parse_files_parallel_with_worker_count() {
        let dir = tempfile::tempdir().expect("tempdir");
        let a = write_file(dir.path(), "a.sh", SAMPLE);
        let cfg = ParserConfig {
            parallel: true,
            parallel_workers: Some(2),
            ..ParserConfig::default()
        };
        let parser = BashParser::with_config(cfg);
        let mut g = graph();
        let project = parser.parse_files(&[a], &mut g).expect("parse ok");
        assert_eq!(project.files.len(), 1);
        assert_eq!(project.total_functions, 1);
    }
}
