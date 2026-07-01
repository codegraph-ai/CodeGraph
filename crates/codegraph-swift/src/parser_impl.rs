// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Swift parser implementation

use codegraph::CodeGraph;
use codegraph_parser_api::{CodeParser, FileInfo, ParserConfig, ParserError, ParserMetrics};
use std::fs;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::extractor;
use crate::mapper;

/// Swift language parser
pub struct SwiftParser {
    config: ParserConfig,
    metrics: Mutex<ParserMetrics>,
}

impl SwiftParser {
    /// Create a new Swift parser with default configuration
    pub fn new() -> Self {
        Self {
            config: ParserConfig::default(),
            metrics: Mutex::new(ParserMetrics::default()),
        }
    }

    /// Create a new Swift parser with custom configuration
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
}

impl Default for SwiftParser {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeParser for SwiftParser {
    fn language(&self) -> &str {
        "swift"
    }

    fn file_extensions(&self) -> &[&str] {
        &[".swift"]
    }

    fn can_parse(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext == "swift")
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
        let start_time = std::time::Instant::now();

        // Extract code entities from source
        let ir = extractor::extract(source, file_path, &self.config)?;

        // Map IR to graph nodes and edges
        let mut file_info = mapper::ir_to_graph(&ir, graph, file_path)?;

        file_info.parse_time = start_time.elapsed();
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language() {
        let parser = SwiftParser::new();
        assert_eq!(parser.language(), "swift");
    }

    #[test]
    fn test_file_extensions() {
        let parser = SwiftParser::new();
        let extensions = parser.file_extensions();
        assert!(extensions.contains(&".swift"));
    }

    #[test]
    fn test_can_parse() {
        let parser = SwiftParser::new();
        assert!(parser.can_parse(Path::new("main.swift")));
        assert!(parser.can_parse(Path::new("ViewController.swift")));
        assert!(!parser.can_parse(Path::new("main.rs")));
        assert!(!parser.can_parse(Path::new("main.cpp")));
    }

    use std::io::Write;
    use std::path::PathBuf;

    /// A small but syntactically complete Swift source touching every
    /// extracted entity kind: one import, one protocol (trait), one class,
    /// and one free function. The protocol carries a required method (which
    /// the visitor records as a `required_method` on the trait, NOT as a
    /// function) and the class is kept method-free, so the only extracted
    /// function is the top-level `add`, pinning the function count at one.
    const SAMPLE: &str = "import Foundation\n\nprotocol Shape {\n    func area() -> Double\n}\n\nclass Point {\n    var x: Int = 0\n}\n\nfunc add(a: Int, b: Int) -> Int {\n    return a + b\n}\n";

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
        let default = SwiftParser::default().metrics();
        let new = SwiftParser::new().metrics();
        assert_eq!(default.files_attempted, new.files_attempted);
        assert_eq!(default.files_attempted, 0);
        assert_eq!(default.total_entities, 0);
    }

    #[test]
    fn test_with_config_and_accessor() {
        let cfg = ParserConfig::default().with_max_file_size(4242);
        let parser = SwiftParser::with_config(cfg);
        assert_eq!(parser.config().max_file_size, 4242);
    }

    #[test]
    fn test_parse_source_extracts_each_entity_kind() {
        let parser = SwiftParser::new();
        let mut g = graph();
        let info = parser
            .parse_source(SAMPLE, Path::new("lib.swift"), &mut g)
            .expect("parse ok");
        assert_eq!(info.functions.len(), 1, "one free function");
        assert_eq!(info.classes.len(), 1, "one class");
        assert_eq!(info.traits.len(), 1, "protocol maps to a trait");
        assert_eq!(info.imports.len(), 1, "one import");
        assert_eq!(info.entity_count(), 3, "functions + classes + traits");
    }

    #[test]
    fn test_parse_source_records_byte_count() {
        let parser = SwiftParser::new();
        let mut g = graph();
        let info = parser
            .parse_source(SAMPLE, Path::new("lib.swift"), &mut g)
            .expect("parse ok");
        assert_eq!(info.byte_count, SAMPLE.len());
    }

    #[test]
    fn test_parse_source_comment_only_yields_no_entities() {
        let parser = SwiftParser::new();
        let mut g = graph();
        let src = "// just a comment\n";
        let info = parser
            .parse_source(src, Path::new("empty.swift"), &mut g)
            .expect("comment-only source still parses");
        assert_eq!(info.functions.len(), 0);
        assert_eq!(info.classes.len(), 0);
        assert_eq!(info.traits.len(), 0);
        assert_eq!(info.imports.len(), 0);
        assert_eq!(info.byte_count, src.len());
    }

    #[test]
    fn test_parse_source_does_not_touch_metrics() {
        // Only parse_file updates metrics; parse_source is metric-free.
        let parser = SwiftParser::new();
        let mut g = graph();
        parser
            .parse_source(SAMPLE, Path::new("lib.swift"), &mut g)
            .expect("parse ok");
        let m = parser.metrics();
        assert_eq!(m.files_attempted, 0);
        assert_eq!(m.files_succeeded, 0);
    }

    #[test]
    fn test_parse_file_success_updates_metrics() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_file(dir.path(), "lib.swift", SAMPLE);
        let parser = SwiftParser::new();
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
        let parser = SwiftParser::new();
        let mut g = graph();
        let err = parser
            .parse_file(Path::new("/no/such/file.swift"), &mut g)
            .expect_err("missing file should error");
        assert!(matches!(err, ParserError::IoError(..)));
        // A pre-read failure never reaches update_metrics.
        assert_eq!(parser.metrics().files_attempted, 0);
    }

    #[test]
    fn test_parse_file_too_large() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_file(dir.path(), "big.swift", SAMPLE);
        let parser = SwiftParser::with_config(ParserConfig::default().with_max_file_size(4));
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
        let path = write_file(dir.path(), "lib.swift", SAMPLE);
        let mut parser = SwiftParser::new();
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
        let a = write_file(dir.path(), "a.swift", SAMPLE);
        let b = write_file(dir.path(), "b.swift", SAMPLE);
        let parser = SwiftParser::new();
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
        let a = write_file(dir.path(), "a.swift", SAMPLE);
        let b = write_file(dir.path(), "b.swift", SAMPLE);
        let parser = SwiftParser::new();
        let mut g = graph();
        let project = parser.parse_files(&[a, b], &mut g).expect("parse ok");
        assert_eq!(project.files.len(), 2);
        assert!(project.failed_files.is_empty());
        assert_eq!(project.total_functions, 2);
        assert_eq!(project.total_classes, 2);
    }

    #[test]
    fn test_parse_files_partitions_failures() {
        let dir = tempfile::tempdir().expect("tempdir");
        let good = write_file(dir.path(), "good.swift", SAMPLE);
        let missing = dir.path().join("missing.swift");
        let parser = SwiftParser::new();
        let mut g = graph();
        let project = parser
            .parse_files(&[good, missing.clone()], &mut g)
            .expect("parse ok");
        assert_eq!(project.files.len(), 1);
        assert_eq!(project.failed_files.len(), 1);
        assert_eq!(project.failed_files[0].0, missing);
    }

    #[test]
    fn test_parse_files_empty_input_yields_empty_project() {
        let parser = SwiftParser::new();
        let mut g = graph();
        let project = parser.parse_files(&[], &mut g).expect("parse ok");
        assert!(project.files.is_empty());
        assert!(project.failed_files.is_empty());
        assert_eq!(project.total_functions, 0);
    }
}
