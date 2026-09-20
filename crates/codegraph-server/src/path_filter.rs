// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Workspace exclusions shared by directory indexing and MCP file events.

use crate::indexer::IndexConfig;
use ignore::gitignore::Gitignore;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub(crate) struct WorkspaceFilter {
    root: PathBuf,
    config: IndexConfig,
    excludes: globset::GlobSet,
    gitignores: HashMap<PathBuf, Gitignore>,
}

impl WorkspaceFilter {
    pub(crate) fn new(root: &Path, config: &IndexConfig) -> Self {
        let mut config = config.clone();
        config.extend_from_codegraphignore(root);
        Self {
            root: root.to_path_buf(),
            excludes: config.build_exclude_set(),
            config,
            gitignores: HashMap::new(),
        }
    }

    pub(crate) fn allows(&mut self, path: &Path, is_dir: bool) -> bool {
        let Ok(relative) = path.strip_prefix(&self.root) else {
            return false;
        };
        let mut current = self.root.clone();
        let mut parents = Vec::new();
        let components: Vec<_> = relative.components().collect();
        let depth = components.len().saturating_sub(usize::from(!is_dir));
        if depth > self.config.max_depth as usize {
            return false;
        }
        for (index, component) in components.iter().enumerate() {
            let directory = index + 1 < components.len() || is_dir;
            parents.push(current.clone());
            current.push(component);
            if self.excluded(&current, directory) || self.gitignored(&current, directory, &parents)
            {
                return false;
            }
        }
        is_dir
            || std::fs::metadata(path).map_or(true, |m| m.len() <= self.config.max_file_size_bytes)
    }

    fn excluded(&self, path: &Path, is_dir: bool) -> bool {
        let Some(name) = path.file_name() else {
            return true;
        };
        let name = name.to_string_lossy();
        name.starts_with('.')
            || self.excludes.is_match(path)
            || (is_dir
                && (self
                    .config
                    .exclude_dirs
                    .iter()
                    .any(|dir| dir == name.as_ref())
                    || self.excludes.is_match(name.as_ref())))
    }

    fn gitignored(&mut self, path: &Path, is_dir: bool, parents: &[PathBuf]) -> bool {
        for parent in parents.iter().rev() {
            let matcher = self.gitignores.entry(parent.clone()).or_insert_with(|| {
                let (matcher, error) = Gitignore::new(parent.join(".gitignore"));
                if let Some(error) = error {
                    tracing::warn!("Invalid .gitignore in {}: {error}", parent.display());
                }
                matcher
            });
            match matcher.matched(path, is_dir) {
                ignore::Match::Ignore(_) => return true,
                ignore::Match::Whitelist(_) => return false,
                ignore::Match::None => {}
            }
        }
        false
    }

    pub(crate) fn reload(&mut self, config: &IndexConfig) {
        *self = Self::new(&self.root, config);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_root_relative_paths_including_deleted_files() {
        let temporary = tempfile::tempdir().unwrap();
        // An excluded name ABOVE the workspace must not exclude the workspace.
        let root = temporary.path().join("target/project");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join(".gitignore"),
            "/cache/\n*.generated.rs\n!keep.generated.rs\n",
        )
        .unwrap();
        std::fs::write(root.join(".codegraphignore"), "**/custom/**\n").unwrap();
        std::fs::write(root.join("src/.gitignore"), "nested.rs\n").unwrap();
        let mut config = IndexConfig::default();
        config.exclude_dirs.push("manual".into());
        let mut filter = WorkspaceFilter::new(&root, &config);
        for (path, expected) in [
            ("src/main.rs", true),
            ("cache/gone.rs", false),
            ("src/cache/allowed.rs", true),
            ("src/nested.rs", false),
            ("nested.rs", true),
            ("other.generated.rs", false),
            ("keep.generated.rs", true),
            ("custom/gone.rs", false),
            ("manual/gone.rs", false),
            (".venv/gone.py", false),
            ("tmp/gone.rs", false),
            ("target/gone.rs", false),
            (".hidden/gone.rs", false),
        ] {
            assert_eq!(filter.allows(&root.join(path), false), expected, "{path}");
        }
        assert!(!filter.allows(&temporary.path().join("outside.rs"), false));
    }

    #[test]
    fn honors_parent_exclusion_and_nested_negation() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("ignored")).unwrap();
        std::fs::write(root.join(".gitignore"), "*.rs\nignored/\n").unwrap();
        std::fs::write(root.join("src/.gitignore"), "!keep.rs\n").unwrap();
        std::fs::write(root.join("ignored/.gitignore"), "!keep.rs\n").unwrap();
        let mut filter = WorkspaceFilter::new(root, &IndexConfig::default());
        assert!(filter.allows(&root.join("src/keep.rs"), false));
        assert!(!filter.allows(&root.join("src/drop.rs"), false));
        assert!(!filter.allows(&root.join("ignored/keep.rs"), false));
    }

    #[test]
    fn reloads_rules_and_enforces_file_size() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        let config = IndexConfig {
            max_file_size_bytes: 4,
            ..IndexConfig::default()
        };
        let mut filter = WorkspaceFilter::new(root, &config);
        let file = root.join("source.rs");
        std::fs::write(&file, "12345").unwrap();
        assert!(!filter.allows(&file, false));
        std::fs::write(&file, "1234").unwrap();
        assert!(filter.allows(&file, false));
        std::fs::write(root.join(".gitignore"), "source.rs\n").unwrap();
        filter.reload(&config);
        assert!(!filter.allows(&file, false));
    }

    #[test]
    fn enforces_the_same_depth_limit_as_directory_indexing() {
        let temporary = tempfile::tempdir().unwrap();
        let config = IndexConfig {
            max_depth: 1,
            ..IndexConfig::default()
        };
        let mut filter = WorkspaceFilter::new(temporary.path(), &config);
        assert!(filter.allows(&temporary.path().join("src/main.rs"), false));
        assert!(!filter.allows(&temporary.path().join("src/deep/main.rs"), false));
        assert!(!filter.allows(&temporary.path().join("src/deep"), true));
    }
}
