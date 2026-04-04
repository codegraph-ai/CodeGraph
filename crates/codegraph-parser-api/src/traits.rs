// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use crate::{config::ParserConfig, errors::ParserError, metrics::ParserMetrics};
use codegraph::{CodeGraph, NodeId};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Information about a successfully parsed file
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileInfo {
    /// Path to the source file
    pub file_path: PathBuf,

    /// Node ID of the file/module in the graph
    pub file_id: NodeId,

    /// Node IDs of all functions extracted
    pub functions: Vec<NodeId>,

    /// Node IDs of all classes extracted
    pub classes: Vec<NodeId>,

    /// Node IDs of all traits/interfaces extracted
    pub traits: Vec<NodeId>,

    /// Node IDs of all imports extracted
    pub imports: Vec<NodeId>,

    /// Time taken to parse this file
    #[serde(with = "duration_serde")]
    pub parse_time: Duration,

    /// Number of lines in the file
    pub line_count: usize,

    /// File size in bytes
    pub byte_count: usize,
}

// Helper module for serializing Duration
mod duration_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        duration.as_secs().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs: u64 = u64::deserialize(deserializer)?;
        Ok(Duration::from_secs(secs))
    }
}

impl FileInfo {
    /// Total number of entities extracted
    pub fn entity_count(&self) -> usize {
        self.functions.len() + self.classes.len() + self.traits.len()
    }
}

/// Aggregate information about a parsed project
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectInfo {
    /// Information about each successfully parsed file
    pub files: Vec<FileInfo>,

    /// Total number of functions across all files
    pub total_functions: usize,

    /// Total number of classes across all files
    pub total_classes: usize,

    /// Total parse time for all files
    #[serde(with = "duration_serde")]
    pub total_parse_time: Duration,

    /// Files that failed to parse (path, error message)
    pub failed_files: Vec<(PathBuf, String)>,
}

impl ProjectInfo {
    /// Total number of files processed (success + failure)
    pub fn total_files(&self) -> usize {
        self.files.len() + self.failed_files.len()
    }

    /// Success rate (0.0 to 1.0)
    pub fn success_rate(&self) -> f64 {
        if self.total_files() == 0 {
            0.0
        } else {
            self.files.len() as f64 / self.total_files() as f64
        }
    }

    /// Average parse time per file
    pub fn avg_parse_time(&self) -> Duration {
        if self.files.is_empty() {
            Duration::ZERO
        } else {
            self.total_parse_time / self.files.len() as u32
        }
    }
}

/// Core trait that all language parsers must implement
///
/// This trait defines the contract for extracting code entities and relationships
/// from source code and inserting them into a CodeGraph database.
///
/// # Thread Safety
/// Implementations must be `Send + Sync` to support parallel parsing.
///
/// # Example
/// ```rust,ignore
/// use codegraph_parser_api::{CodeParser, ParserConfig};
/// use codegraph::CodeGraph;
///
/// struct MyParser {
///     config: ParserConfig,
/// }
///
/// impl CodeParser for MyParser {
///     fn language(&self) -> &str {
///         "mylang"
///     }
///
///     fn file_extensions(&self) -> &[&str] {
///         &[".my"]
///     }
///
///     // ... implement other required methods
/// }
/// ```
pub trait CodeParser: Send + Sync {
    /// Returns the language identifier (lowercase, e.g., "python", "rust")
    fn language(&self) -> &str;

    /// Returns supported file extensions (e.g., [".py", ".pyw"])
    fn file_extensions(&self) -> &[&str];

    /// Parse a single file and insert entities/relationships into the graph
    ///
    /// **Note on Metrics**: This method updates parser metrics
    /// (files_attempted, files_succeeded, etc.). Use `metrics()` to retrieve
    /// statistics after parsing operations.
    ///
    /// # Arguments
    /// * `path` - Path to the source file
    /// * `graph` - Mutable reference to the CodeGraph database
    ///
    /// # Returns
    /// `FileInfo` containing metadata about parsed entities
    ///
    /// # Errors
    /// Returns `ParserError` if:
    /// - File cannot be read
    /// - Source code has syntax errors
    /// - Graph insertion fails
    fn parse_file(&self, path: &Path, graph: &mut CodeGraph) -> Result<FileInfo, ParserError>;

    /// Parse source code string and insert into graph
    ///
    /// Useful for parsing code snippets or in-memory source.
    ///
    /// **Note on Metrics**: This method does NOT update parser metrics
    /// (files_attempted, files_succeeded, etc.). Only `parse_file()` updates
    /// metrics to avoid double-counting when `parse_source()` is called
    /// internally by `parse_file()`.
    ///
    /// # Arguments
    /// * `source` - Source code string
    /// * `file_path` - Logical path for this source (used for graph nodes)
    /// * `graph` - Mutable reference to the CodeGraph database
    fn parse_source(
        &self,
        source: &str,
        file_path: &Path,
        graph: &mut CodeGraph,
    ) -> Result<FileInfo, ParserError>;

    /// Parse multiple files (can be overridden for parallel parsing)
    ///
    /// Default implementation parses files sequentially. Override this
    /// for parallel parsing implementation.
    ///
    /// # Arguments
    /// * `paths` - List of file paths to parse
    /// * `graph` - Mutable reference to the CodeGraph database
    ///
    /// # Returns
    /// `ProjectInfo` containing aggregate statistics
    fn parse_files(
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
            total_functions,
            total_classes,
            total_parse_time,
            failed_files,
        })
    }

    /// Parse a directory recursively
    ///
    /// # Arguments
    /// * `dir` - Directory path to parse
    /// * `graph` - Mutable reference to the CodeGraph database
    fn parse_directory(
        &self,
        dir: &Path,
        graph: &mut CodeGraph,
    ) -> Result<ProjectInfo, ParserError> {
        let paths = self.discover_files(dir)?;
        self.parse_files(&paths, graph)
    }

    /// Discover parseable files in a directory
    ///
    /// Default implementation walks the directory and filters by extension.
    /// Can be overridden for custom discovery logic.
    fn discover_files(&self, dir: &Path) -> Result<Vec<PathBuf>, ParserError> {
        use std::fs;

        let mut files = Vec::new();
        let extensions = self.file_extensions();

        fn walk_dir(
            dir: &Path,
            extensions: &[&str],
            files: &mut Vec<PathBuf>,
        ) -> Result<(), ParserError> {
            if !dir.is_dir() {
                return Ok(());
            }

            for entry in
                fs::read_dir(dir).map_err(|e| ParserError::IoError(dir.to_path_buf(), e))?
            {
                let entry = entry.map_err(|e| ParserError::IoError(dir.to_path_buf(), e))?;
                let path = entry.path();

                if path.is_dir() {
                    walk_dir(&path, extensions, files)?;
                } else if let Some(ext) = path.extension() {
                    let ext_str = format!(".{}", ext.to_string_lossy());
                    if extensions.contains(&ext_str.as_str()) {
                        files.push(path);
                    }
                }
            }

            Ok(())
        }

        walk_dir(dir, extensions, &mut files)?;
        Ok(files)
    }

    /// Check if this parser can handle the given file
    ///
    /// Default implementation checks file extension.
    fn can_parse(&self, path: &Path) -> bool {
        if let Some(ext) = path.extension() {
            let ext_str = format!(".{}", ext.to_string_lossy());
            self.file_extensions().contains(&ext_str.as_str())
        } else {
            false
        }
    }

    /// Get parser configuration
    fn config(&self) -> &ParserConfig;

    /// Get accumulated metrics
    ///
    /// Returns current parsing metrics (files processed, time taken, etc.)
    fn metrics(&self) -> ParserMetrics;

    /// Reset metrics
    ///
    /// Clears accumulated metrics. Useful for benchmarking.
    fn reset_metrics(&mut self);
}
