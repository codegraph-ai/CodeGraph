// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use crate::config::ParserConfig;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use tracing::{debug, info, instrument, warn};

/// Information about a parsed file
#[derive(Debug, Clone)]
pub struct FileInfo {
    /// Path to the parsed file
    pub file_path: PathBuf,

    /// Function entity IDs
    pub functions: Vec<String>,

    /// Class entity IDs
    pub classes: Vec<String>,

    /// Module entity ID
    pub modules: Vec<String>,

    /// Trait entity IDs
    pub traits: Vec<String>,

    /// Number of lines in the file
    pub lines: usize,

    /// Time taken to parse
    pub parse_time: Duration,
}

impl FileInfo {
    /// Create a new FileInfo
    pub fn new(file_path: PathBuf) -> Self {
        Self {
            file_path,
            functions: Vec::new(),
            classes: Vec::new(),
            modules: Vec::new(),
            traits: Vec::new(),
            lines: 0,
            parse_time: Duration::from_secs(0),
        }
    }

    /// Get total entity count
    pub fn entity_count(&self) -> usize {
        self.functions.len() + self.classes.len() + self.modules.len() + self.traits.len()
    }
}

/// Information about a parsed project
#[derive(Debug, Clone)]
pub struct ProjectInfo {
    /// All successfully parsed files
    pub files: Vec<FileInfo>,

    /// Failed files with error messages
    pub failed_files: HashMap<PathBuf, String>,

    /// Total number of functions across all files
    pub total_functions: usize,

    /// Total number of classes across all files
    pub total_classes: usize,

    /// Total number of traits across all files
    pub total_traits: usize,

    /// Total number of lines across all files
    pub total_lines: usize,

    /// Total time taken to parse entire project
    pub total_time: Duration,
}

impl ProjectInfo {
    /// Create a new ProjectInfo
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            failed_files: HashMap::new(),
            total_functions: 0,
            total_classes: 0,
            total_traits: 0,
            total_lines: 0,
            total_time: Duration::from_secs(0),
        }
    }

    /// Calculate success rate as percentage
    pub fn success_rate(&self) -> f64 {
        let total = self.files.len() + self.failed_files.len();
        if total == 0 {
            return 100.0;
        }
        (self.files.len() as f64 / total as f64) * 100.0
    }

    /// Calculate average parse time per file
    pub fn avg_parse_time(&self) -> Duration {
        if self.files.is_empty() {
            return Duration::from_secs(0);
        }
        self.total_time / self.files.len() as u32
    }

    /// Add a successfully parsed file
    pub fn add_file(&mut self, file_info: FileInfo) {
        self.total_functions += file_info.functions.len();
        self.total_classes += file_info.classes.len();
        self.total_traits += file_info.traits.len();
        self.total_lines += file_info.lines;
        self.total_time += file_info.parse_time;
        self.files.push(file_info);
    }

    /// Add a failed file
    pub fn add_failure(&mut self, path: PathBuf, error: String) {
        self.failed_files.insert(path, error);
    }
}

impl Default for ProjectInfo {
    fn default() -> Self {
        Self::new()
    }
}

/// Main parser for Python source code
pub struct Parser {
    config: ParserConfig,
}

impl Parser {
    /// Create a new parser with default configuration
    pub fn new() -> Self {
        Self {
            config: ParserConfig::default(),
        }
    }

    /// Create a parser with custom configuration
    pub fn with_config(config: ParserConfig) -> Self {
        Self { config }
    }

    /// Get the parser configuration
    pub fn config(&self) -> &ParserConfig {
        &self.config
    }

    /// Parse Python source code from a string
    ///
    /// # Arguments
    ///
    /// * `source` - Python source code as a string
    /// * `file_path` - Path to the source file (for error reporting)
    /// * `graph` - Mutable reference to the code graph
    ///
    /// # Returns
    ///
    /// A `FileInfo` with information about the parsed entities
    pub fn parse_source(
        &self,
        source: &str,
        file_path: &std::path::Path,
        graph: &mut codegraph::CodeGraph,
    ) -> crate::error::Result<FileInfo> {
        use std::time::Instant;

        let start = Instant::now();

        // Extract entities from source code
        let ir = crate::extractor::extract(source, file_path, &self.config).map_err(|e| {
            crate::error::ParseError::SyntaxError {
                file: file_path.display().to_string(),
                line: 0,
                column: 0,
                message: e,
            }
        })?;

        // Build graph from IR
        let file_id = crate::builder::build_graph(graph, &ir, file_path.to_str().unwrap_or(""))?;

        // Create FileInfo from IR
        let mut file_info = FileInfo::new(file_path.to_path_buf());

        // Convert function entities to strings for tracking
        // Methods from classes are already included in ir.functions with parent_class set
        // So we just need all functions, using qualified names for methods
        file_info.functions = ir
            .functions
            .iter()
            .map(|f| {
                if let Some(ref class_name) = f.parent_class {
                    format!("{}.{}", class_name, f.name)
                } else {
                    f.name.clone()
                }
            })
            .collect();

        file_info.classes = ir.classes.iter().map(|c| c.name.clone()).collect();
        file_info.traits = ir.traits.iter().map(|t| t.name.clone()).collect();

        if let Some(ref module) = ir.module {
            file_info.modules.push(module.name.clone());
            file_info.lines = module.line_count;
        }

        file_info.parse_time = start.elapsed();

        // Store the file_id for later use (could be added to FileInfo if needed)
        let _ = file_id;

        Ok(file_info)
    }

    /// Parse a Python file
    ///
    /// # Arguments
    ///
    /// * `file_path` - Path to the Python file
    /// * `graph` - Mutable reference to the code graph
    ///
    /// # Returns
    ///
    /// A `FileInfo` with information about the parsed entities
    #[instrument(skip(self, graph), fields(file = %file_path.display()))]
    pub fn parse_file(
        &self,
        file_path: &std::path::Path,
        graph: &mut codegraph::CodeGraph,
    ) -> crate::error::Result<FileInfo> {
        use std::fs;

        debug!("Starting file parse");

        // Validate file extension
        if let Some(ext) = file_path.extension() {
            if let Some(ext_str) = ext.to_str() {
                if !self.config.should_parse_extension(ext_str) {
                    warn!("Invalid file extension: {}", ext_str);
                    return Err(crate::error::ParseError::InvalidConfig(format!(
                        "File extension not allowed: {file_path:?}"
                    )));
                }
            }
        }

        // Check file size
        let metadata = fs::metadata(file_path).map_err(|e| crate::error::ParseError::IoError {
            path: file_path.to_path_buf(),
            source: e,
        })?;

        if metadata.len() > self.config.max_file_size as u64 {
            warn!("File too large: {} bytes", metadata.len());
            return Err(crate::error::ParseError::FileTooLarge {
                path: file_path.to_path_buf(),
                max_size: self.config.max_file_size,
                actual_size: metadata.len() as usize,
            });
        }

        // Read file contents
        let source =
            fs::read_to_string(file_path).map_err(|e| crate::error::ParseError::IoError {
                path: file_path.to_path_buf(),
                source: e,
            })?;

        // Parse the source
        let result = self.parse_source(&source, file_path, graph)?;

        info!(
            functions = result.functions.len(),
            classes = result.classes.len(),
            lines = result.lines,
            time_ms = result.parse_time.as_millis(),
            "File parsed successfully"
        );

        Ok(result)
    }

    /// Parse all Python files in a directory recursively
    ///
    /// # Arguments
    ///
    /// * `dir_path` - Path to the directory to parse
    /// * `graph` - Mutable reference to the code graph
    ///
    /// # Returns
    ///
    /// A `ProjectInfo` with information about all parsed files
    #[instrument(skip(self, graph), fields(dir = %dir_path.display()))]
    pub fn parse_directory(
        &self,
        dir_path: &std::path::Path,
        graph: &mut codegraph::CodeGraph,
    ) -> crate::error::Result<ProjectInfo> {
        use std::time::Instant;
        use walkdir::WalkDir;

        let start = Instant::now();
        let mut project_info = ProjectInfo::new();

        info!("Starting directory parse");

        // Collect all Python files in the directory
        let mut files_to_parse = Vec::new();

        for entry in WalkDir::new(dir_path)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                // Skip excluded directories
                if e.file_type().is_dir() {
                    if let Some(name) = e.file_name().to_str() {
                        return !self.config.should_exclude_dir(name);
                    }
                }
                true
            })
        {
            match entry {
                Ok(entry) => {
                    if entry.file_type().is_file() {
                        if let Some(ext) = entry.path().extension() {
                            if let Some(ext_str) = ext.to_str() {
                                if self.config.should_parse_extension(ext_str) {
                                    files_to_parse.push(entry.path().to_path_buf());
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    // Record walkdir errors as failed files
                    if let Some(path) = e.path() {
                        project_info.add_failure(path.to_path_buf(), e.to_string());
                    }
                }
            }
        }

        // Parse files (sequential or parallel based on config)
        if self.config.parallel {
            self.parse_files_parallel(&files_to_parse, graph, &mut project_info)?;
        } else {
            self.parse_files_sequential(&files_to_parse, graph, &mut project_info);
        }

        project_info.total_time = start.elapsed();

        info!(
            files_parsed = project_info.files.len(),
            files_failed = project_info.failed_files.len(),
            total_functions = project_info.total_functions,
            total_classes = project_info.total_classes,
            total_lines = project_info.total_lines,
            total_time_ms = project_info.total_time.as_millis(),
            success_rate = project_info.success_rate(),
            "Directory parse completed"
        );

        Ok(project_info)
    }

    /// Parse files sequentially
    fn parse_files_sequential(
        &self,
        files: &[PathBuf],
        graph: &mut codegraph::CodeGraph,
        project_info: &mut ProjectInfo,
    ) {
        for file_path in files {
            match self.parse_file(file_path, graph) {
                Ok(file_info) => {
                    project_info.add_file(file_info);
                }
                Err(e) => {
                    project_info.add_failure(file_path.clone(), e.to_string());
                }
            }
        }
    }

    /// Parse files in parallel
    fn parse_files_parallel(
        &self,
        files: &[PathBuf],
        graph: &mut codegraph::CodeGraph,
        project_info: &mut ProjectInfo,
    ) -> crate::error::Result<()> {
        use rayon::prelude::*;
        use std::sync::Mutex;

        let graph_mutex = Mutex::new(graph);
        let project_info_mutex = Mutex::new(project_info);

        // Configure thread pool if num_threads is specified
        let pool = if let Some(num_threads) = self.config.num_threads {
            rayon::ThreadPoolBuilder::new()
                .num_threads(num_threads)
                .build()
                .map_err(|e| {
                    crate::error::ParseError::InvalidConfig(format!(
                        "Failed to create thread pool: {e}"
                    ))
                })?
        } else {
            rayon::ThreadPoolBuilder::new().build().map_err(|e| {
                crate::error::ParseError::InvalidConfig(format!(
                    "Failed to create thread pool: {e}"
                ))
            })?
        };

        pool.install(|| {
            files.par_iter().for_each(|file_path| {
                // Parse file with a temporary graph, then merge
                // Note: This is simplified - in production we'd want better synchronization
                let parse_result = {
                    let mut graph = graph_mutex.lock().unwrap();
                    self.parse_file(file_path, &mut graph)
                };

                let mut project_info = project_info_mutex.lock().unwrap();
                match parse_result {
                    Ok(file_info) => {
                        project_info.add_file(file_info);
                    }
                    Err(e) => {
                        project_info.add_failure(file_path.clone(), e.to_string());
                    }
                }
            });
        });

        Ok(())
    }
}

impl Default for Parser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_info_new() {
        let info = FileInfo::new(PathBuf::from("test.py"));
        assert_eq!(info.file_path, PathBuf::from("test.py"));
        assert_eq!(info.entity_count(), 0);
    }

    #[test]
    fn test_project_info_success_rate() {
        let mut info = ProjectInfo::new();
        assert_eq!(info.success_rate(), 100.0);

        info.add_file(FileInfo::new(PathBuf::from("file1.py")));
        info.add_file(FileInfo::new(PathBuf::from("file2.py")));
        info.add_failure(PathBuf::from("file3.py"), "error".to_string());

        assert_eq!(info.success_rate(), 66.66666666666666);
    }

    #[test]
    fn test_parser_new() {
        let parser = Parser::new();
        assert!(parser.config().include_private);
    }
}
