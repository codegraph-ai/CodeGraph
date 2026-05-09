// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Test-case schema (deserialised from `*.case.yml`).
//!
//! P1 only supports a subset — `setup.fixture`, `invoke.tool`,
//! `invoke.args`, `expect.match` (exact only), `expect.data`. P2+
//! adds normalisation rules, tolerance bands, and additional
//! match modes.

use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
pub struct TestCase {
    /// Globally-unique hierarchical id. Convention:
    /// `<tool>.<feature>.<language>_<scenario>`.
    pub id: String,
    /// Human-readable explanation; surfaces in failure output.
    #[serde(default)]
    #[allow(dead_code)]
    pub description: String,
    pub setup: Setup,
    pub invoke: Invoke,
    pub expect: Expect,
    /// Source file the case was loaded from. Filled in by the discoverer,
    /// not by the YAML.
    #[serde(skip)]
    pub source_path: PathBuf,
}

#[derive(Debug, Deserialize)]
pub struct Setup {
    /// Fixture path relative to `crates/codegraph-harness/fixtures/`.
    /// Resolved to an absolute path during execution.
    pub fixture: String,
    /// `single_file` — copy just the named file into a tempdir.
    /// `multi_file` — copy the entire directory containing the file.
    #[serde(default = "default_workspace_layout")]
    pub workspace_layout: WorkspaceLayout,
    /// Whether to wait for the index to be ready before invoking the
    /// tool. Default `true`. Set `false` for tests of pre-index state.
    #[serde(default = "default_pre_index")]
    #[allow(dead_code)]
    pub pre_index: bool,
}

fn default_workspace_layout() -> WorkspaceLayout {
    WorkspaceLayout::SingleFile
}

fn default_pre_index() -> bool {
    true
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceLayout {
    SingleFile,
    MultiFile,
}

#[derive(Debug, Deserialize)]
pub struct Invoke {
    /// MCP tool name — the `codegraph_*` identifier as registered in
    /// the server's tools/list response.
    pub tool: String,
    /// Tool arguments — passed verbatim into the JSON-RPC `arguments`
    /// field. May contain `${fixture}` / `${workspace}` placeholders
    /// substituted at runtime.
    pub args: serde_json::Value,
    /// Per-tool timeout in milliseconds. Default 10000.
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
}

fn default_timeout_ms() -> u64 {
    10_000
}

#[derive(Debug, Deserialize)]
pub struct Expect {
    /// Match strategy. P1 only supports `exact`.
    #[serde(default = "default_match_mode")]
    pub r#match: MatchMode,
    /// Per-case normalisation overrides. Defaults are conservative:
    /// volatile fields stripped, paths substituted, backslashes
    /// folded — but no array sorting or float rounding unless opted in.
    #[serde(default)]
    pub normalize: NormalizeOpts,
    /// Expected JSON value. May contain `${fixture}` placeholders
    /// substituted at runtime.
    pub data: serde_json::Value,
}

/// Per-case knobs for the normalisation pipeline. Applied to BOTH
/// the actual response and the expected JSON so authoring is forgiving:
/// a case author doesn't need to pre-sort arrays or pre-round floats —
/// the pipeline does it for both sides before comparison.
///
/// All `Option` fields participate in the P4 merge: the per-tool
/// profile from `profiles::default_for` provides the base; the case's
/// `expect.normalize` block is the overlay; the case wins for any
/// `Some` value. `Vec` fields concatenate (profile entries first, then
/// case entries) so a case can extend a profile's strip list without
/// resetting it.
#[derive(Debug, Deserialize, Default, Clone)]
pub struct NormalizeOpts {
    /// Sort all arrays-of-objects by their canonical JSON string.
    /// `None` = inherit from profile (treated as `false` if profile
    /// also unset). Set to `false` explicitly to override a profile
    /// default.
    #[serde(default)]
    pub sort_arrays: Option<bool>,
    /// Round every float in the response to N decimal places. `None` =
    /// no rounding. Use for embedding-dependent tools where the last
    /// few digits drift between ONNX runtime versions.
    #[serde(default)]
    pub float_decimals: Option<u8>,
    /// Extra volatile field names to strip on top of the always-on
    /// list in `normalize.rs::VOLATILE_FIELDS`. For one-off fields
    /// that haven't earned global-strip status yet.
    #[serde(default)]
    pub extra_volatile: Vec<String>,
    /// Per-case opt-OUT from the global VOLATILE_FIELDS list. Use when
    /// the field is the actual subject of the assertion (e.g. `score`
    /// for a `find_similar` tolerance test).
    #[serde(default)]
    pub keep_volatile: Vec<String>,
    /// If set, the harness asserts the active embedding model matches
    /// this name (passed via `--embedding-model` CLI flag) before
    /// invoking the tool. Cases that depend on cosine values fail
    /// loudly when the underlying model has changed and expectations
    /// haven't been re-blessed.
    #[serde(default)]
    pub embedding_model: Option<String>,
    /// Patterns of objects to remove from any array containing them.
    /// Each pattern is a partial object — an element matches if every
    /// `(key, value)` in the pattern is present and equal in the
    /// element. Used to strip non-deterministic result categories
    /// from tools that mix deterministic and non-deterministic output
    /// in one array (e.g. symbol_search results that include both
    /// `match_reason: SymbolName` and embedding-timing-dependent
    /// `match_reason: Semantic`). Recursive.
    #[serde(default)]
    pub drop_where: Vec<serde_json::Value>,
}

impl NormalizeOpts {
    /// Merge a per-tool profile (base) with a per-case overlay. The
    /// overlay wins for any `Some` value; `Vec`s concatenate with
    /// profile entries first. Used by the runner after looking up
    /// `profiles::default_for(tool_name)`.
    pub fn merge(base: NormalizeOpts, overlay: NormalizeOpts) -> NormalizeOpts {
        let mut extra_volatile = base.extra_volatile;
        for v in overlay.extra_volatile {
            if !extra_volatile.contains(&v) {
                extra_volatile.push(v);
            }
        }
        let mut keep_volatile = base.keep_volatile;
        for v in overlay.keep_volatile {
            if !keep_volatile.contains(&v) {
                keep_volatile.push(v);
            }
        }
        let mut drop_where = base.drop_where;
        for p in overlay.drop_where {
            if !drop_where.contains(&p) {
                drop_where.push(p);
            }
        }
        NormalizeOpts {
            sort_arrays: overlay.sort_arrays.or(base.sort_arrays),
            float_decimals: overlay.float_decimals.or(base.float_decimals),
            extra_volatile,
            keep_volatile,
            embedding_model: overlay.embedding_model.or(base.embedding_model),
            drop_where,
        }
    }

    /// Resolve `sort_arrays` to a concrete bool — `None` means off.
    pub fn sort_arrays_on(&self) -> bool {
        self.sort_arrays.unwrap_or(false)
    }
}

fn default_match_mode() -> MatchMode {
    MatchMode::Exact
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MatchMode {
    Exact,
    /// P3 placeholder — same shape, ignore extras.
    Structural,
    /// P3 placeholder — expected is a subset.
    Contains,
    /// P3 placeholder — only assert array lengths.
    CountOnly,
}

impl TestCase {
    /// Load a case from a YAML file.
    pub fn from_path(path: &std::path::Path) -> anyhow::Result<Self> {
        let raw = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("read {}: {}", path.display(), e))?;
        let mut case: TestCase = serde_yaml::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("parse {} as YAML: {}", path.display(), e))?;
        case.source_path = path.to_path_buf();
        Ok(case)
    }
}
