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
}
