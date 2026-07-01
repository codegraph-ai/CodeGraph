// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

/// Configuration for Python parser behavior
#[derive(Debug, Clone)]
pub struct ParserConfig {
    /// Include private entities (names starting with _)
    pub include_private: bool,

    /// Include test functions (names starting with test_)
    pub include_tests: bool,

    /// Parse and extract docstrings
    pub parse_docs: bool,

    /// Maximum file size in bytes (files larger than this will be skipped)
    pub max_file_size: usize,

    /// File extensions to parse (default: [".py"])
    pub file_extensions: Vec<String>,

    /// Directories to exclude from project parsing
    pub exclude_dirs: Vec<String>,

    /// Enable parallel processing for multi-file projects
    pub parallel: bool,

    /// Number of threads for parallel processing (None = use default)
    pub num_threads: Option<usize>,
}

impl Default for ParserConfig {
    fn default() -> Self {
        Self {
            include_private: true,
            include_tests: true,
            parse_docs: true,
            max_file_size: 10 * 1024 * 1024, // 10MB default
            file_extensions: vec![".py".to_string()],
            exclude_dirs: vec![
                "__pycache__".to_string(),
                ".git".to_string(),
                ".venv".to_string(),
                "venv".to_string(),
                "env".to_string(),
                ".tox".to_string(),
                "dist".to_string(),
                "build".to_string(),
                ".eggs".to_string(),
                "*.egg-info".to_string(),
            ],
            parallel: false,
            num_threads: None,
        }
    }
}

impl ParserConfig {
    /// Validate the configuration
    pub fn validate(&self) -> Result<(), String> {
        if let Some(threads) = self.num_threads {
            if threads == 0 {
                return Err("num_threads must be greater than 0".to_string());
            }
        }

        if self.max_file_size == 0 {
            return Err("max_file_size must be greater than 0".to_string());
        }

        if self.file_extensions.is_empty() {
            return Err("file_extensions cannot be empty".to_string());
        }

        Ok(())
    }

    /// Check if a file extension should be parsed
    pub fn should_parse_extension(&self, extension: &str) -> bool {
        self.file_extensions.iter().any(|ext| {
            let ext = ext.trim_start_matches('.');
            extension.trim_start_matches('.') == ext
        })
    }

    /// Check if a directory should be excluded
    pub fn should_exclude_dir(&self, dir_name: &str) -> bool {
        self.exclude_dirs.iter().any(|excluded| {
            // Handle glob patterns like *.egg-info
            if excluded.contains('*') {
                let pattern = excluded.replace('*', "");
                dir_name.contains(&pattern)
            } else {
                dir_name == excluded
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ParserConfig::default();
        assert!(config.include_private);
        assert!(config.include_tests);
        assert!(config.parse_docs);
        assert_eq!(config.max_file_size, 10 * 1024 * 1024);
        assert!(!config.parallel);
    }

    #[test]
    fn test_validate() {
        let mut config = ParserConfig::default();
        assert!(config.validate().is_ok());

        config.num_threads = Some(0);
        assert!(config.validate().is_err());

        config.num_threads = Some(4);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_rejects_zero_max_file_size() {
        // The max_file_size == 0 guard (distinct from the num_threads guard)
        // returns its own error message.
        let mut config = ParserConfig::default();
        config.max_file_size = 0;
        assert_eq!(
            config.validate().unwrap_err(),
            "max_file_size must be greater than 0"
        );
    }

    #[test]
    fn test_validate_rejects_empty_file_extensions() {
        // An empty extension list is invalid - nothing would ever be parsed.
        let mut config = ParserConfig::default();
        config.file_extensions.clear();
        assert_eq!(
            config.validate().unwrap_err(),
            "file_extensions cannot be empty"
        );
    }

    #[test]
    fn test_validate_rejects_zero_num_threads_message() {
        // The num_threads == 0 guard returns its own distinct message. The
        // pre-existing test_validate only asserted `.is_err()` here, leaving the
        // exact wording unpinned (unlike the max_file_size / file_extensions
        // guards, whose messages are asserted verbatim above).
        let mut config = ParserConfig::default();
        config.num_threads = Some(0);
        assert_eq!(
            config.validate().unwrap_err(),
            "num_threads must be greater than 0"
        );
    }

    #[test]
    fn test_should_parse_extension() {
        let config = ParserConfig::default();
        assert!(config.should_parse_extension(".py"));
        assert!(config.should_parse_extension("py"));
        assert!(!config.should_parse_extension(".rs"));
    }

    #[test]
    fn test_should_parse_extension_is_case_sensitive() {
        // Matching is a byte-for-byte `==` after trimming the leading dot, so an
        // uppercase extension does NOT match the lowercase ".py" default.
        let config = ParserConfig::default();
        assert!(!config.should_parse_extension("PY"));
        assert!(!config.should_parse_extension(".PY"));
    }

    #[test]
    fn test_should_exclude_dir() {
        let config = ParserConfig::default();
        assert!(config.should_exclude_dir("__pycache__"));
        assert!(config.should_exclude_dir(".venv"));
        assert!(config.should_exclude_dir("mypackage.egg-info"));
        assert!(!config.should_exclude_dir("src"));
    }

    #[test]
    fn test_should_exclude_dir_glob_matches_substring_not_just_suffix() {
        // The `*.egg-info` pattern strips the '*' to ".egg-info" and uses
        // `contains`, so it matches the fragment anywhere in the name - not just
        // as a trailing suffix. A dir with trailing characters after the fragment
        // still matches.
        let config = ParserConfig::default();
        assert!(config.should_exclude_dir("foo.egg-info.bak"));
        // The leading dot is part of the fragment: a bare "egg-info" (no dot)
        // does not contain ".egg-info" and is therefore not excluded.
        assert!(!config.should_exclude_dir("egg-info"));
    }

    #[test]
    fn test_should_exclude_dir_non_glob_requires_exact_match() {
        // Non-glob entries (no '*') use `==`, so a partial or decorated name is
        // not excluded even though it shares a prefix with an excluded dir.
        let config = ParserConfig::default();
        assert!(!config.should_exclude_dir("build/"));
        assert!(!config.should_exclude_dir("mybuild"));
        assert!(!config.should_exclude_dir("__pycache__2"));
    }
}
