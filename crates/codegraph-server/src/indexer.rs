// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Shared indexer — used by both MCP and LSP backends.
//!
//! Provides hash-based incremental indexing, configurable directory exclusions,
//! and cross-file import resolution. Persists file hashes via [`IndexState`] so
//! unchanged files are skipped across server restarts.

use crate::index_state::IndexState;
use crate::parser_registry::ParserRegistry;
use crate::watcher::GraphUpdater;
use codegraph::CodeGraph;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

/// Configuration for a single indexing run.
#[derive(Debug, Clone)]
pub struct IndexConfig {
    /// Directory names to always skip (e.g. `node_modules`, `target`).
    pub exclude_dirs: Vec<String>,
    /// Glob patterns for additional exclusions (user-configured).
    pub exclude_patterns: Vec<String>,
    /// Maximum file size in bytes. Files larger than this are skipped.
    pub max_file_size_bytes: u64,
    /// Maximum recursion depth for directory traversal.
    pub max_depth: u32,
    /// Maximum number of files to index in a single run.
    pub max_files: usize,
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            exclude_dirs: Self::default_exclude_dirs(),
            exclude_patterns: Self::default_exclude_patterns(),
            max_file_size_bytes: 1024 * 1024, // 1 MiB
            max_depth: 20,
            max_files: 5_000,
        }
    }
}

impl IndexConfig {
    /// The hardcoded list of directories that are always excluded.
    ///
    /// Three categories:
    /// 1. **Build / dependency / cache dirs** — never source code, dragging them
    ///    through tree-sitter wastes cycles. Indexer-runaway protection
    ///    (see workspace_indexer_embedding_runaway memory note: codegraph-pro
    ///    MCP startup spinning fastembed/ONNX for 30+ min on large workspaces).
    /// 2. **IDE / tooling state** — ephemeral, churns frequently, no semantic
    ///    value for code intelligence.
    /// 3. **Sensitive credential dirs** — `~/.aws`, `~/.ssh` etc. A user who
    ///    accidentally indexes their home dir (or a project rooted inside it)
    ///    should not have those embedded into graph.db. Defense in depth on
    ///    top of the per-extension secret skip in `default_exclude_patterns`.
    pub fn default_exclude_dirs() -> Vec<String> {
        [
            // Generic build / artifact dirs
            "node_modules",
            "target",
            "dist",
            "build",
            "out",
            "coverage",
            "htmlcov",
            "results",
            "logs",
            "tmp",
            // VCS / SCM metadata
            ".git",
            // Python tooling
            "__pycache__",
            "vendor",
            ".venv",
            "venv",
            ".tox",
            ".pytest_cache",
            ".mypy_cache",
            ".ruff_cache",
            ".eggs",
            // Node / JS tooling
            ".next",
            ".nuxt",
            ".svelte-kit",
            ".parcel-cache",
            ".npm",
            ".yarn",
            ".pnpm-store",
            ".cache",
            // Rust / Ruby / Java / Gradle
            ".cargo",
            ".bundle",
            ".gradle",
            ".kotlin",
            // Apple / iOS
            "DerivedData",
            "Pods",
            "xcuserdata",
            // CMake out-of-source build dirs
            "cmake-build-debug",
            "cmake-build-release",
            // Test / benchmark / example / binary-output dirs
            "benches",
            "examples",
            "fixtures",
            "cases",
            "bin",
            // IDE state
            ".idea",
            ".vscode-test",
            ".fleet",
            // Infrastructure-as-code state
            ".terraform",
            ".terragrunt-cache",
            ".serverless",
            // Sensitive credential / config dirs — never code, often secret
            ".aws",
            ".ssh",
            ".gnupg",
            ".kube",
            ".docker",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect()
    }

    /// Built-in glob patterns for files we should never index — binary
    /// archives, prebuilt artifacts, OS metadata. These shapes are
    /// near-universally non-source and dragging them through tree-sitter
    /// + embedding wastes cycles. Bounty 2026-05-03: a `bounty/` workspace
    /// containing thousands of `.tar.gz` / `.deb` proof bundles caused a
    /// 4.3 GB / 644% CPU runaway during initial embedding because the
    /// indexer didn't filter binary file extensions.
    pub fn default_exclude_patterns() -> Vec<String> {
        [
            // Archives / packaged artifacts
            "**/*.tar.gz",
            "**/*.tar.bz2",
            "**/*.tar.xz",
            "**/*.tgz",
            "**/*.tbz2",
            "**/*.zip",
            "**/*.7z",
            "**/*.rar",
            "**/*.deb",
            "**/*.rpm",
            "**/*.pkg",
            "**/*.dmg",
            "**/*.iso",
            "**/*.img",
            // Compiled / native binaries
            "**/*.exe",
            "**/*.dll",
            "**/*.so",
            "**/*.dylib",
            "**/*.bin",
            "**/*.o",
            "**/*.a",
            "**/*.lib",
            "**/*.obj",
            "**/*.pdb",
            // Compiled bytecode (already in __pycache__/ etc. but glob covers stragglers)
            "**/*.pyc",
            "**/*.pyo",
            "**/*.class",
            "**/*.jar",
            "**/*.war",
            // Disk images / proof bundles
            "**/*.qcow2",
            "**/*.vmdk",
            "**/*.vdi",
            "**/*.vhd",
            // Office / PDF / media (not code)
            "**/*.pdf",
            "**/*.docx",
            "**/*.xlsx",
            "**/*.pptx",
            "**/*.png",
            "**/*.jpg",
            "**/*.jpeg",
            "**/*.gif",
            "**/*.bmp",
            "**/*.svg",
            "**/*.mp3",
            "**/*.mp4",
            "**/*.mov",
            "**/*.avi",
            "**/*.webm",
            // OS metadata
            "**/.DS_Store",
            "**/Thumbs.db",
            // Misc bulky non-source
            "**/*.sqlite",
            "**/*.db",
            "**/*.lock",
            // Package artifacts
            "**/*.vsix",
            "**/*.tgz",
            "**/*.whl",
            "**/*.gem",
            "**/*.nupkg",
            // Vendored tree-sitter parser sources (generated C, not user code).
            // Two globs needed: one matches the dir itself so it's skipped
            // during directory walk, the other matches files inside it.
            "**/tree-sitter-*-src",
            "**/tree-sitter-*-src/**",
            // Cryptographic material — never code; never want this content
            // either parsed OR embedded into graph.db. Coverage:
            //   - Private keys: PEM / DER / PKCS#12 / PKCS#8 / OpenSSH
            //   - Certificates: PEM / DER / certificate bundles
            //   - PGP / GPG keyrings + ASCII-armored signatures
            //   - Password databases (KeePass)
            //   - Web-auth artifacts (htpasswd, netrc)
            //   - SSH key filenames per OpenSSH convention
            //   - Terraform state (contains secrets after apply)
            "**/*.pem",
            "**/*.key",
            "**/*.priv",
            "**/*.privkey",
            "**/*.p12",
            "**/*.pfx",
            "**/*.crt",
            "**/*.cer",
            "**/*.der",
            "**/*.gpg",
            "**/*.asc",
            "**/*.kdb",
            "**/*.kdbx",
            "**/.htpasswd",
            "**/.netrc",
            "**/id_rsa",
            "**/id_rsa.pub",
            "**/id_ed25519",
            "**/id_ed25519.pub",
            "**/id_dsa",
            "**/id_ecdsa",
            "**/known_hosts",
            "**/authorized_keys",
            "**/*.tfstate",
            "**/*.tfstate.backup",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect()
    }

    /// Bounty 2026-05-03 — extend the exclude_patterns from a workspace's
    /// `.codegraphignore` file if present. Format is simple gitignore-like:
    /// one pattern per line, `#` for comments, blank lines ignored. Not a
    /// full gitignore parser (no `!` negation, no path-anchored `/`-prefix
    /// semantics) — keeps complexity bounded while solving the bounty
    /// workspace runaway.
    ///
    /// If `.codegraphignore` doesn't exist, this is a no-op (returns
    /// without modifying `self`). Errors reading the file are logged
    /// and ignored.
    pub fn extend_from_codegraphignore(&mut self, workspace_root: &Path) {
        let path = workspace_root.join(".codegraphignore");
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                if e.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!("Failed to read {}: {}", path.display(), e);
                }
                return;
            }
        };
        let mut added = 0usize;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            self.exclude_patterns.push(trimmed.to_string());
            added += 1;
        }
        if added > 0 {
            tracing::info!("Loaded {} patterns from {}", added, path.display());
        }
    }

    /// Build a `GlobSet` from `exclude_patterns`.
    pub(crate) fn build_exclude_set(&self) -> globset::GlobSet {
        let mut builder = globset::GlobSetBuilder::new();
        for pattern in &self.exclude_patterns {
            match globset::Glob::new(pattern) {
                Ok(g) => {
                    builder.add(g);
                }
                Err(e) => {
                    tracing::warn!("Invalid exclude pattern '{}': {}", pattern, e);
                }
            }
        }
        builder.build().unwrap_or_else(|e| {
            tracing::warn!("Failed to build exclude GlobSet: {}", e);
            globset::GlobSet::empty()
        })
    }
}

/// Result of an indexing run.
#[derive(Debug, Clone, Default)]
pub struct IndexResult {
    /// Total files encountered (parsed + skipped).
    pub total_files: usize,
    /// Files that were actually parsed (new or changed).
    pub files_parsed: usize,
    /// Files skipped because their content hash was unchanged.
    pub files_skipped: usize,
    /// Per-language file counts (key: parser's `language()` string).
    /// Used for `index.languageBreakdown` telemetry so we know which
    /// parsers are actually exercised on real workspaces. Counts include
    /// both parsed and hash-skipped files (all files the parser claimed).
    pub by_language: std::collections::HashMap<String, usize>,
    /// Per-language parser error counts. A parser error here means the
    /// tree-sitter parse step itself failed — distinct from "no files
    /// of this language found." Drives parser-quality prioritization.
    pub parser_errors_by_language: std::collections::HashMap<String, usize>,
}

/// Shared indexer for walking directories, hashing files, and parsing them
/// into a [`CodeGraph`].
pub struct Indexer {
    parsers: Arc<ParserRegistry>,
    index_state: Arc<Mutex<IndexState>>,
}

impl Indexer {
    /// Create a new indexer backed by the given parser registry and index state.
    pub fn new(parsers: Arc<ParserRegistry>, index_state: Arc<Mutex<IndexState>>) -> Self {
        Self {
            parsers,
            index_state,
        }
    }

    /// Compute a fast content hash (FNV-1a 64-bit).
    pub fn hash_content(content: &[u8]) -> u64 {
        let mut hash: u64 = 0xcbf29ce484222325;
        for &byte in content {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash
    }

    /// Index all workspace folders, resolve cross-file imports, and persist
    /// index state.
    ///
    /// This is the main entry-point for full (re-)indexing.
    pub async fn index_workspace(
        &self,
        graph: &Arc<RwLock<CodeGraph>>,
        folders: &[PathBuf],
        config: &IndexConfig,
    ) -> IndexResult {
        // Crash-phase breadcrumb: parsing runs native tree-sitter grammars over
        // arbitrary source — the leading suspect for the remaining win32
        // 0xC0000005 startup crashes. Shared by the MCP and LSP backends, so
        // this one guard tags every parse pass. Resets to `serving` on return.
        let _phase = crate::crash_phase::enter("index_parse");
        let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut result = IndexResult::default();

        for folder in folders {
            // Bounty 2026-05-03 — extend exclude_patterns from this
            // folder's `.codegraphignore` if present. Per-folder so each
            // workspace can have its own rules; doesn't pollute other
            // folders' configs.
            let mut folder_config = config.clone();
            folder_config.extend_from_codegraphignore(folder);
            let (total, parsed, skipped, by_lang, parser_errors) = self
                .index_directory(graph, folder, &folder_config, 0, counter.clone())
                .await;
            result.total_files += total;
            result.files_parsed += parsed;
            result.files_skipped += skipped;
            for (lang, count) in by_lang {
                *result.by_language.entry(lang).or_insert(0) += count;
            }
            for (lang, count) in parser_errors {
                *result.parser_errors_by_language.entry(lang).or_insert(0) += count;
            }
        }

        // Resolve cross-file imports
        {
            let mut g = graph.write().await;
            GraphUpdater::resolve_cross_file_imports(&mut g);
        }

        // Detect runtime dependencies
        {
            let mut g = graph.write().await;
            let routes = crate::runtime_deps::detect_route_handlers(&mut g);
            let clients = crate::runtime_deps::detect_http_client_calls(&mut g);
            if routes > 0 || clients > 0 {
                let edges = crate::runtime_deps::create_runtime_call_edges(&mut g);
                tracing::info!(
                    "Runtime deps: {} routes, {} clients, {} edges",
                    routes,
                    clients,
                    edges
                );
            }
        }

        // Persist index state
        {
            let state = self.index_state.lock().await;
            state.save();
        }

        result
    }

    /// Recursively walk a directory and index supported files.
    ///
    /// Returns `(total_encountered, files_parsed, files_skipped)`.
    pub fn index_directory<'a>(
        &'a self,
        graph: &'a Arc<RwLock<CodeGraph>>,
        dir: &'a Path,
        config: &'a IndexConfig,
        depth: u32,
        counter: Arc<std::sync::atomic::AtomicUsize>,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = (
                        usize,
                        usize,
                        usize,
                        std::collections::HashMap<String, usize>,
                        std::collections::HashMap<String, usize>,
                    ),
                > + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            use std::sync::atomic::Ordering;

            let empty_by_lang: std::collections::HashMap<String, usize> = Default::default();
            let empty_errors: std::collections::HashMap<String, usize> = Default::default();

            if depth > config.max_depth {
                tracing::warn!(
                    "Skipping {:?}: exceeded max indexing depth of {}",
                    dir,
                    config.max_depth
                );
                return (0, 0, 0, empty_by_lang, empty_errors);
            }

            if counter.load(Ordering::Relaxed) >= config.max_files {
                return (0, 0, 0, empty_by_lang, empty_errors);
            }

            let exclude_set = config.build_exclude_set();
            let supported_extensions = self.parsers.supported_extensions();

            tracing::debug!("Scanning directory: {:?}", dir);

            let mut total = 0usize;
            let mut parsed = 0usize;
            let mut skipped = 0usize;
            let mut by_language: std::collections::HashMap<String, usize> = Default::default();
            let mut parser_errors: std::collections::HashMap<String, usize> = Default::default();

            let entries = match std::fs::read_dir(dir) {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!("Cannot read directory {:?}: {}", dir, e);
                    return (0, 0, 0, empty_by_lang, empty_errors);
                }
            };

            for entry in entries.flatten() {
                if counter.load(Ordering::Relaxed) >= config.max_files {
                    tracing::warn!(
                        "Reached max indexed file limit of {}; stopping",
                        config.max_files
                    );
                    break;
                }

                let path = entry.path();

                // Skip hidden files and directories
                if let Some(name) = path.file_name() {
                    if name.to_string_lossy().starts_with('.') {
                        continue;
                    }
                }

                if path.is_dir() {
                    let dir_name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();

                    // Skip hardcoded exclude directories
                    if config.exclude_dirs.iter().any(|e| e == &dir_name) {
                        continue;
                    }

                    // Skip directories matching user-configured exclude globs
                    let path_str = path.to_string_lossy();
                    if exclude_set.is_match(path_str.as_ref())
                        || exclude_set.is_match(dir_name.as_str())
                    {
                        tracing::info!("Skipping {:?}: matched exclude pattern", path);
                        continue;
                    }

                    let (t, p, s, child_by_lang, child_errors) = self
                        .index_directory(graph, &path, config, depth + 1, counter.clone())
                        .await;
                    total += t;
                    parsed += p;
                    skipped += s;
                    for (lang, count) in child_by_lang {
                        *by_language.entry(lang).or_insert(0) += count;
                    }
                    for (lang, count) in child_errors {
                        *parser_errors.entry(lang).or_insert(0) += count;
                    }
                } else if path.is_file() {
                    // Skip files matching exclude globs
                    let path_str = path.to_string_lossy();
                    if exclude_set.is_match(path_str.as_ref()) {
                        continue;
                    }

                    // Skip files that exceed the configurable size limit
                    if let Ok(metadata) = std::fs::metadata(&path) {
                        if metadata.len() > config.max_file_size_bytes {
                            tracing::info!(
                                "Skipping {:?}: file size {} exceeds limit of {}",
                                path,
                                metadata.len(),
                                config.max_file_size_bytes
                            );
                            continue;
                        }
                    }

                    // Check if file has a supported extension
                    if let Some(ext) = path.extension() {
                        let ext_str = ext.to_string_lossy();
                        let ext_with_dot = format!(".{}", ext_str);
                        let is_supported = supported_extensions
                            .iter()
                            .any(|e| *e == ext_str.as_ref() || *e == ext_with_dot);

                        if is_supported {
                            // Resolve the parser before parsing so we can
                            // attribute the file (and any error) to a
                            // specific language for telemetry.
                            let language = self
                                .parsers
                                .parser_for_path(&path)
                                .map(|p| p.language().to_string())
                                .unwrap_or_else(|| "unknown".to_string());
                            match self.index_file(graph, &path).await {
                                Ok(was_parsed) => {
                                    total += 1;
                                    counter.fetch_add(1, Ordering::Relaxed);
                                    *by_language.entry(language).or_insert(0) += 1;
                                    if was_parsed {
                                        parsed += 1;
                                    } else {
                                        skipped += 1;
                                        tracing::trace!("Skipped unchanged: {:?}", path);
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!("Failed to index {:?}: {}", path, e);
                                    *parser_errors.entry(language).or_insert(0) += 1;
                                }
                            }
                        }
                    }
                }
            }

            (total, parsed, skipped, by_language, parser_errors)
        })
    }

    /// Index a single file. Returns `Ok(true)` if the file was parsed,
    /// `Ok(false)` if it was skipped because the content hash is unchanged.
    pub async fn index_file(
        &self,
        graph: &Arc<RwLock<CodeGraph>>,
        path: &Path,
    ) -> Result<bool, String> {
        // Read content and compute hash
        let content = std::fs::read(path).map_err(|e| format!("Read error: {e}"))?;
        let hash = Self::hash_content(&content);

        // Check if file content has changed since last index
        {
            let state = self.index_state.lock().await;
            if let Some(cached_hash) = state.get_hash(path) {
                if cached_hash == hash {
                    return Ok(false); // Unchanged
                }
            }
        }

        // File is new or changed — remove old nodes and parse
        {
            let mut g = graph.write().await;
            let path_str = path.to_string_lossy().to_string();
            if let Ok(old_nodes) = g.query().property("path", path_str).execute() {
                for old_id in old_nodes {
                    let _ = g.delete_node(old_id);
                }
            }

            match self.parsers.parse_file(path, &mut g) {
                Ok(_file_info) => {
                    drop(g);
                    // Update hash in index state
                    let mut state = self.index_state.lock().await;
                    state.set_hash(path.to_path_buf(), hash);
                    Ok(true)
                }
                Err(e) => Err(format!("{:?}", e)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_exclude_patterns_includes_binary_archives() {
        let patterns = IndexConfig::default_exclude_patterns();
        // Bounty 2026-05-03 specifically called out these archive shapes.
        assert!(patterns.iter().any(|p| p.contains("tar.gz")));
        assert!(patterns.iter().any(|p| p.contains("zip")));
        assert!(patterns.iter().any(|p| p.contains("deb")));
        assert!(patterns.iter().any(|p| p.contains("DS_Store")));
    }

    #[test]
    fn default_config_has_default_exclude_patterns() {
        let config = IndexConfig::default();
        assert!(
            !config.exclude_patterns.is_empty(),
            "default IndexConfig must populate exclude_patterns"
        );
    }

    #[test]
    fn extend_from_codegraphignore_appends_patterns() {
        let tmp = tempfile::tempdir().expect("tmp");
        std::fs::write(
            tmp.path().join(".codegraphignore"),
            "# comment line\n\nproof-*-*/\n*.poc.cpp\n  spaced-pattern.zip  \n",
        )
        .unwrap();
        let mut config = IndexConfig {
            exclude_dirs: vec![],
            exclude_patterns: vec!["already-here".to_string()],
            max_file_size_bytes: 1024,
            max_depth: 5,
            max_files: 100,
        };
        config.extend_from_codegraphignore(tmp.path());
        // Pre-existing patterns are kept.
        assert!(config.exclude_patterns.iter().any(|p| p == "already-here"));
        // New patterns appended (3 valid lines, comment + blank skipped).
        assert!(config.exclude_patterns.iter().any(|p| p == "proof-*-*/"));
        assert!(config.exclude_patterns.iter().any(|p| p == "*.poc.cpp"));
        // Whitespace trimmed.
        assert!(config
            .exclude_patterns
            .iter()
            .any(|p| p == "spaced-pattern.zip"));
    }

    #[test]
    fn extend_from_codegraphignore_no_op_when_file_missing() {
        let tmp = tempfile::tempdir().expect("tmp");
        let mut config = IndexConfig::default();
        let before_len = config.exclude_patterns.len();
        config.extend_from_codegraphignore(tmp.path());
        assert_eq!(
            config.exclude_patterns.len(),
            before_len,
            "missing .codegraphignore must not change exclude_patterns"
        );
    }

    #[test]
    fn build_exclude_set_matches_default_archive_patterns() {
        let config = IndexConfig::default();
        let set = config.build_exclude_set();
        // Files matching defaults should be filtered.
        assert!(set.is_match("/repo/proof-bundle.tar.gz"));
        assert!(set.is_match("/repo/MSRC-VULN-12345.zip"));
        assert!(set.is_match("/path/to/.DS_Store"));
        assert!(set.is_match("/path/to/binary.so"));
        // Source files should NOT be filtered.
        assert!(!set.is_match("/repo/src/main.rs"));
        assert!(!set.is_match("/repo/src/handler.go"));
        assert!(!set.is_match("/repo/lib/foo.cpp"));
    }

    #[test]
    fn default_exclude_dirs_includes_python_node_and_credential_dirs() {
        let dirs = IndexConfig::default_exclude_dirs();
        // Python tooling caches
        assert!(dirs.iter().any(|d| d == ".venv"));
        assert!(dirs.iter().any(|d| d == ".pytest_cache"));
        assert!(dirs.iter().any(|d| d == ".mypy_cache"));
        // Node tooling
        assert!(dirs.iter().any(|d| d == ".next"));
        assert!(dirs.iter().any(|d| d == ".yarn"));
        // iOS / Apple
        assert!(dirs.iter().any(|d| d == "Pods"));
        // IaC
        assert!(dirs.iter().any(|d| d == ".terraform"));
        // Sensitive credential dirs
        assert!(dirs.iter().any(|d| d == ".aws"));
        assert!(dirs.iter().any(|d| d == ".ssh"));
        assert!(dirs.iter().any(|d| d == ".gnupg"));
        assert!(dirs.iter().any(|d| d == ".kube"));
    }

    #[test]
    fn build_exclude_set_filters_secret_file_extensions() {
        let config = IndexConfig::default();
        let set = config.build_exclude_set();
        // Private keys + cert bundles — must never be indexed or embedded.
        assert!(set.is_match("/repo/keys/server.pem"));
        assert!(set.is_match("/repo/secret.key"));
        assert!(set.is_match("/repo/identity.p12"));
        assert!(set.is_match("/repo/codesign.pfx"));
        // Certificates
        assert!(set.is_match("/repo/cert.crt"));
        assert!(set.is_match("/repo/cert.cer"));
        assert!(set.is_match("/repo/cert.der"));
        // PGP / GPG
        assert!(set.is_match("/repo/release.gpg"));
        assert!(set.is_match("/repo/release.asc"));
        // Password DBs
        assert!(set.is_match("/repo/passwords.kdbx"));
        // SSH conventional filenames (no extension)
        assert!(set.is_match("/home/user/.ssh/id_rsa"));
        assert!(set.is_match("/home/user/.ssh/id_ed25519"));
        assert!(set.is_match("/home/user/.ssh/authorized_keys"));
        assert!(set.is_match("/home/user/.ssh/known_hosts"));
        // Web auth
        assert!(set.is_match("/repo/.htpasswd"));
        assert!(set.is_match("/repo/.netrc"));
        // Terraform state (secrets after apply)
        assert!(set.is_match("/repo/terraform.tfstate"));
        assert!(set.is_match("/repo/terraform.tfstate.backup"));
        // Source files MUST still pass — bg: a file called `crypto.rs`
        // looks key-adjacent but is normal source.
        assert!(!set.is_match("/repo/src/crypto.rs"));
        assert!(!set.is_match("/repo/src/keys.go"));
        assert!(!set.is_match("/repo/src/auth.py"));
    }

    // ---- Indexer runtime tests (hash_content, index_file, index_directory,
    // index_workspace) ----

    fn indexer_for() -> Indexer {
        let parsers = Arc::new(ParserRegistry::new());
        let index_state = Arc::new(Mutex::new(crate::index_state::IndexState::new("")));
        Indexer::new(parsers, index_state)
    }

    fn empty_graph() -> Arc<RwLock<CodeGraph>> {
        Arc::new(RwLock::new(CodeGraph::in_memory().unwrap()))
    }

    async fn node_count_for_path(graph: &Arc<RwLock<CodeGraph>>, path: &Path) -> usize {
        let g = graph.read().await;
        let path_str = path.to_string_lossy().to_string();
        g.query()
            .property("path", path_str)
            .execute()
            .map(|ids| ids.len())
            .unwrap_or(0)
    }

    #[test]
    fn hash_content_empty_is_fnv_offset_basis() {
        // FNV-1a with no bytes returns the untouched 64-bit offset basis.
        assert_eq!(Indexer::hash_content(&[]), 0xcbf29ce484222325);
    }

    #[test]
    fn hash_content_is_deterministic_and_content_sensitive() {
        let a = Indexer::hash_content(b"fn main() {}");
        let b = Indexer::hash_content(b"fn main() {}");
        let c = Indexer::hash_content(b"fn main() {} ");
        assert_eq!(a, b, "same bytes must hash identically");
        assert_ne!(a, c, "one extra byte must change the hash");
    }

    #[tokio::test]
    async fn index_file_parses_new_file_and_populates_graph() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("foo.py");
        std::fs::write(&file, "def foo():\n    return 1\n").unwrap();

        let indexer = indexer_for();
        let graph = empty_graph();
        let parsed = indexer.index_file(&graph, &file).await.unwrap();
        assert!(parsed, "a brand-new file must report parsed=true");
        assert!(
            node_count_for_path(&graph, &file).await > 0,
            "parsing must add at least one node for the file"
        );
    }

    #[tokio::test]
    async fn index_file_skips_unchanged_file_on_second_call() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("foo.py");
        std::fs::write(&file, "def foo():\n    return 1\n").unwrap();

        let indexer = indexer_for();
        let graph = empty_graph();
        assert!(indexer.index_file(&graph, &file).await.unwrap());
        // Second call with identical content hits the cached-hash short-circuit.
        assert!(
            !indexer.index_file(&graph, &file).await.unwrap(),
            "unchanged content must report parsed=false"
        );
    }

    #[tokio::test]
    async fn index_file_reparses_after_content_change() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("foo.py");
        std::fs::write(&file, "def foo():\n    return 1\n").unwrap();

        let indexer = indexer_for();
        let graph = empty_graph();
        assert!(indexer.index_file(&graph, &file).await.unwrap());
        // Change the content - hash differs, so it must reparse.
        std::fs::write(
            &file,
            "def foo():\n    return 2\n\ndef bar():\n    return 3\n",
        )
        .unwrap();
        assert!(
            indexer.index_file(&graph, &file).await.unwrap(),
            "changed content must report parsed=true"
        );
        assert!(node_count_for_path(&graph, &file).await > 0);
    }

    #[tokio::test]
    async fn index_file_returns_err_for_missing_file() {
        let indexer = indexer_for();
        let graph = empty_graph();
        let missing = Path::new("/definitely/not/here/nope.py");
        let err = indexer.index_file(&graph, missing).await.unwrap_err();
        assert!(
            err.starts_with("Read error:"),
            "unreadable path must surface a read error, got: {err}"
        );
    }

    #[tokio::test]
    async fn index_directory_counts_files_by_language() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.py"), "def a(): pass\n").unwrap();
        std::fs::write(tmp.path().join("b.py"), "def b(): pass\n").unwrap();

        let indexer = indexer_for();
        let graph = empty_graph();
        let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (total, parsed, skipped, by_lang, errors) = indexer
            .index_directory(&graph, tmp.path(), &IndexConfig::default(), 0, counter)
            .await;
        assert_eq!(total, 2);
        assert_eq!(parsed, 2);
        assert_eq!(skipped, 0);
        assert_eq!(by_lang.get("python").copied().unwrap_or(0), 2);
        assert!(errors.is_empty());
    }

    #[tokio::test]
    async fn index_directory_skips_excluded_dirs_and_hidden_and_unsupported() {
        let tmp = tempfile::tempdir().unwrap();
        // Supported source file at the root.
        std::fs::write(tmp.path().join("keep.py"), "def keep(): pass\n").unwrap();
        // Excluded build dir with a source file inside — must not be walked.
        std::fs::create_dir(tmp.path().join("node_modules")).unwrap();
        std::fs::write(
            tmp.path().join("node_modules").join("skip.py"),
            "def skip(): pass\n",
        )
        .unwrap();
        // Hidden file — skipped by the leading-dot rule.
        std::fs::write(tmp.path().join(".hidden.py"), "def h(): pass\n").unwrap();
        // Unsupported extension — no parser, ignored entirely.
        std::fs::write(tmp.path().join("data.unknownext"), "noise\n").unwrap();

        let indexer = indexer_for();
        let graph = empty_graph();
        let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (total, parsed, _skipped, by_lang, _errors) = indexer
            .index_directory(&graph, tmp.path(), &IndexConfig::default(), 0, counter)
            .await;
        assert_eq!(total, 1, "only keep.py should be counted");
        assert_eq!(parsed, 1);
        assert_eq!(by_lang.get("python").copied().unwrap_or(0), 1);
    }

    #[tokio::test]
    async fn index_directory_honors_max_depth() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("sub");
        std::fs::create_dir(&nested).unwrap();
        std::fs::write(nested.join("deep.py"), "def deep(): pass\n").unwrap();

        let indexer = indexer_for();
        let graph = empty_graph();
        let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        // max_depth 0: the root is depth 0 (walked), but recursing into `sub`
        // happens at depth 1 which exceeds the cap and returns early.
        let config = IndexConfig {
            max_depth: 0,
            ..IndexConfig::default()
        };
        let (total, _parsed, _skipped, _by_lang, _errors) = indexer
            .index_directory(&graph, tmp.path(), &config, 0, counter)
            .await;
        assert_eq!(total, 0, "the nested file below max_depth must be skipped");
    }

    #[tokio::test]
    async fn index_directory_honors_max_files() {
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..5 {
            std::fs::write(
                tmp.path().join(format!("f{i}.py")),
                format!("def f{i}(): pass\n"),
            )
            .unwrap();
        }

        let indexer = indexer_for();
        let graph = empty_graph();
        let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let config = IndexConfig {
            max_files: 2,
            ..IndexConfig::default()
        };
        let (total, _parsed, _skipped, _by_lang, _errors) = indexer
            .index_directory(&graph, tmp.path(), &config, 0, counter)
            .await;
        assert_eq!(total, 2, "must stop after max_files are indexed");
    }

    #[tokio::test]
    async fn index_directory_skips_oversized_files() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("big.py"), "x = 1\n".repeat(1000)).unwrap();

        let indexer = indexer_for();
        let graph = empty_graph();
        let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let config = IndexConfig {
            max_file_size_bytes: 10, // smaller than the file
            ..IndexConfig::default()
        };
        let (total, parsed, _skipped, _by_lang, _errors) = indexer
            .index_directory(&graph, tmp.path(), &config, 0, counter)
            .await;
        assert_eq!(total, 0);
        assert_eq!(parsed, 0);
    }

    #[tokio::test]
    async fn index_workspace_aggregates_across_folders_and_reindexes_clean() {
        let tmp = tempfile::tempdir().unwrap();
        let folder_a = tmp.path().join("a");
        let folder_b = tmp.path().join("b");
        std::fs::create_dir(&folder_a).unwrap();
        std::fs::create_dir(&folder_b).unwrap();
        std::fs::write(folder_a.join("one.py"), "def one(): pass\n").unwrap();
        std::fs::write(folder_b.join("two.py"), "def two(): pass\n").unwrap();

        let indexer = indexer_for();
        let graph = empty_graph();
        let folders = vec![folder_a.clone(), folder_b.clone()];
        let config = IndexConfig::default();

        let result = indexer.index_workspace(&graph, &folders, &config).await;
        assert_eq!(result.total_files, 2);
        assert_eq!(result.files_parsed, 2);
        assert_eq!(result.files_skipped, 0);
        assert_eq!(result.by_language.get("python").copied().unwrap_or(0), 2);

        // A second run over the same unchanged workspace must hash-skip both.
        let again = indexer.index_workspace(&graph, &folders, &config).await;
        assert_eq!(again.total_files, 2);
        assert_eq!(again.files_parsed, 0);
        assert_eq!(again.files_skipped, 2);
    }
}
