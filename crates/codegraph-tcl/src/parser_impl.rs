// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! TclParser implementing the CodeParser trait

use codegraph::CodeGraph;
use codegraph_parser_api::{
    CodeParser, FileInfo, ParserConfig, ParserError, ParserMetrics, ProjectInfo,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

use crate::extractor;
use crate::mapper;

pub struct TclParser {
    config: ParserConfig,
    metrics: Mutex<ParserMetrics>,
}

impl TclParser {
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
}

impl Default for TclParser {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeParser for TclParser {
    fn language(&self) -> &str {
        "tcl"
    }

    fn file_extensions(&self) -> &[&str] {
        &[".tcl", ".sdc", ".upf"]
    }

    fn can_parse(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| matches!(ext, "tcl" | "sdc" | "upf"))
            .unwrap_or(false)
    }

    fn parse_file(&self, path: &Path, graph: &mut CodeGraph) -> Result<FileInfo, ParserError> {
        let start_time = Instant::now();

        if !path.exists() {
            return Err(ParserError::IoError(
                path.to_path_buf(),
                std::io::Error::new(std::io::ErrorKind::NotFound, "File not found"),
            ));
        }

        let source =
            fs::read_to_string(path).map_err(|e| ParserError::IoError(path.to_path_buf(), e))?;

        if source.len() > self.config.max_file_size {
            return Err(ParserError::FileTooLarge(path.to_path_buf(), source.len()));
        }

        let mut file_info = self.parse_source(&source, path, graph)?;
        file_info.parse_time = start_time.elapsed();
        file_info.byte_count = source.len();

        // Update metrics
        if let Ok(mut metrics) = self.metrics.lock() {
            metrics.files_attempted += 1;
            metrics.files_succeeded += 1;
            metrics.total_parse_time += file_info.parse_time;
            metrics.total_entities +=
                file_info.functions.len() + file_info.classes.len() + file_info.imports.len();
        }

        Ok(file_info)
    }

    fn parse_source(
        &self,
        source: &str,
        file_path: &Path,
        graph: &mut CodeGraph,
    ) -> Result<FileInfo, ParserError> {
        let (ir, extra) = extractor::extract(source, file_path, &self.config)?;
        mapper::ir_to_graph(&ir, &extra, graph, file_path)
    }

    fn parse_files(
        &self,
        paths: &[PathBuf],
        graph: &mut CodeGraph,
    ) -> Result<ProjectInfo, ParserError> {
        let start_time = Instant::now();
        let mut files = Vec::new();
        let mut failed_files = Vec::new();

        for path in paths {
            match self.parse_file(path, graph) {
                Ok(file_info) => files.push(file_info),
                Err(e) => failed_files.push((path.clone(), e.to_string())),
            }
        }

        Ok(ProjectInfo {
            files,
            failed_files,
            total_functions: 0,
            total_classes: 0,
            total_parse_time: start_time.elapsed(),
        })
    }

    fn parse_directory(
        &self,
        dir: &Path,
        graph: &mut CodeGraph,
    ) -> Result<ProjectInfo, ParserError> {
        let files = self.discover_files(dir)?;
        self.parse_files(&files, graph)
    }

    fn discover_files(&self, dir: &Path) -> Result<Vec<PathBuf>, ParserError> {
        let mut files = Vec::new();

        if !dir.is_dir() {
            return Err(ParserError::IoError(
                dir.to_path_buf(),
                std::io::Error::new(std::io::ErrorKind::NotFound, "Not a directory"),
            ));
        }

        Self::discover_recursive(dir, &mut files);
        Ok(files)
    }

    fn config(&self) -> &ParserConfig {
        &self.config
    }

    fn metrics(&self) -> ParserMetrics {
        self.metrics.lock().unwrap().clone()
    }

    fn reset_metrics(&mut self) {
        if let Ok(mut metrics) = self.metrics.lock() {
            *metrics = ParserMetrics::default();
        }
    }
}

impl TclParser {
    fn discover_recursive(dir: &Path, files: &mut Vec<PathBuf>) {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    Self::discover_recursive(&path, files);
                } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if matches!(ext, "tcl" | "sdc" | "upf") {
                        files.push(path);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph::CodeGraph;

    #[test]
    fn test_parser_creation() {
        let parser = TclParser::new();
        assert_eq!(parser.language(), "tcl");
        assert_eq!(parser.file_extensions(), &[".tcl", ".sdc", ".upf"]);
    }

    #[test]
    fn test_can_parse() {
        let parser = TclParser::new();
        assert!(parser.can_parse(Path::new("script.tcl")));
        assert!(parser.can_parse(Path::new("constraints.sdc")));
        assert!(parser.can_parse(Path::new("power.upf")));
        assert!(!parser.can_parse(Path::new("code.py")));
        assert!(!parser.can_parse(Path::new("main.rs")));
    }

    #[test]
    fn test_parse_source_simple() {
        let parser = TclParser::new();
        let mut graph = CodeGraph::in_memory().unwrap();

        let source = r#"
proc greet {name} {
    puts "Hello $name"
}
"#;
        let result = parser.parse_source(source, Path::new("test.tcl"), &mut graph);
        assert!(result.is_ok());

        let info = result.unwrap();
        assert_eq!(info.functions.len(), 1);
    }

    #[test]
    fn test_parse_source_sdc() {
        let parser = TclParser::new();
        let mut graph = CodeGraph::in_memory().unwrap();

        let source = r#"
create_clock -name clk -period 10 [get_ports clk_in]
set_input_delay -clock clk 0.5 [all_inputs]
"#;
        let result = parser.parse_source(source, Path::new("constraints.sdc"), &mut graph);
        assert!(result.is_ok());

        let info = result.unwrap();
        // SDC data is stored as properties on the file node
        let file_node = graph.get_node(info.file_id).unwrap();
        let sdc_clocks = file_node.properties.get_string("sdc_clocks");
        assert!(sdc_clocks.is_some());
        assert!(sdc_clocks.unwrap().contains("clk"));
    }

    #[test]
    fn test_parse_source_eda_flow() {
        let parser = TclParser::new();
        let mut graph = CodeGraph::in_memory().unwrap();

        let source = r#"
read_verilog design.v
read_liberty lib.db
compile
report_timing
write_def output.def
"#;
        let result = parser.parse_source(source, Path::new("synth.tcl"), &mut graph);
        assert!(result.is_ok());

        let info = result.unwrap();
        assert!(!info.imports.is_empty());

        let file_node = graph.get_node(info.file_id).unwrap();
        let reads = file_node.properties.get_string("eda_design_reads");
        assert!(reads.is_some());
    }

    #[test]
    fn test_metrics_tracking() {
        let parser = TclParser::new();
        let mut graph = CodeGraph::in_memory().unwrap();

        let source = "proc foo {} {}";
        let _ = parser.parse_source(source, Path::new("test.tcl"), &mut graph);

        // parse_source doesn't update metrics (only parse_file does)
        let metrics = parser.metrics();
        assert_eq!(metrics.files_attempted, 0);
    }
}
