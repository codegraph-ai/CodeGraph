// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Per-case execution. Sets up a temporary workspace, spawns the
//! codegraph-server binary, calls the tool, compares the response.

use crate::case::{NormalizeOpts, Setup, TestCase, WorkspaceLayout};
use crate::compare::compare;
use crate::jsonrpc::McpClient;
use crate::normalize::{normalize, normalize_expected};
use crate::profiles;
use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub struct CaseResult {
    pub id: String,
    pub passed: bool,
    pub diff: String,
    pub duration: Duration,
    pub error: Option<String>,
    /// The normalized actual response, populated only when the runner
    /// is invoked in bless mode. Used by main to overwrite the
    /// case's `expect.data` block.
    pub blessed_actual: Option<serde_json::Value>,
}

pub fn run_case(
    case: &TestCase,
    binary: &Path,
    fixtures_root: &Path,
    active_embedding_model: Option<&str>,
    bless: bool,
) -> CaseResult {
    let started = Instant::now();
    match run_inner(case, binary, fixtures_root, active_embedding_model, bless) {
        Ok(result) => CaseResult {
            id: case.id.clone(),
            passed: result.passed,
            diff: result.diff,
            duration: started.elapsed(),
            error: None,
            blessed_actual: result.blessed_actual,
        },
        Err(e) => CaseResult {
            id: case.id.clone(),
            passed: false,
            diff: String::new(),
            duration: started.elapsed(),
            error: Some(format!("{:#}", e)),
            blessed_actual: None,
        },
    }
}

struct InnerResult {
    passed: bool,
    diff: String,
    blessed_actual: Option<serde_json::Value>,
}

fn run_inner(
    case: &TestCase,
    binary: &Path,
    fixtures_root: &Path,
    active_embedding_model: Option<&str>,
    bless: bool,
) -> Result<InnerResult> {
    // 0a. Resolve effective normalisation: per-tool profile (base) +
    //     per-case overrides (overlay). Case wins per-field; vec
    //     fields concatenate.
    let resolved_opts = NormalizeOpts::merge(
        profiles::default_for(&case.invoke.tool),
        case.expect.normalize.clone(),
    );

    // 0b. Embedding-model gate. If the resolved opts pin a model and
    //     the active model differs (or wasn't supplied), fail loudly —
    //     these cases encode cosine values that change when the model
    //     changes.
    if let Some(pinned) = resolved_opts.embedding_model.as_deref() {
        match active_embedding_model {
            Some(active) if active == pinned => {}
            Some(active) => {
                return Err(anyhow!(
                    "case pins embedding_model={} but active is {} — re-bless expectations or pass --embedding-model {}",
                    pinned, active, pinned,
                ));
            }
            None => {
                return Err(anyhow!(
                    "case pins embedding_model={} but harness was started without --embedding-model — refusing to run",
                    pinned,
                ));
            }
        }
    }

    // 1. Build workspace
    let workspace = make_workspace(&case.setup, fixtures_root)?;
    let workspace_path = workspace.path().to_path_buf();
    let fixture_in_workspace = resolve_fixture_path(&case.setup, &workspace_path)?;

    // 2. Spawn server, complete handshake
    let mut client = McpClient::spawn(binary, &workspace_path)?;

    // 3. Substitute ${fixture} / ${workspace} in args (case YAMLs use
    //    these so the real tempdir path can be injected at runtime).
    //    Expected JSON does NOT get this treatment — it's compared
    //    against the normalised response which already has the
    //    placeholders restored.
    let fixture_str = fixture_in_workspace.to_string_lossy().to_string();
    let workspace_str = workspace_path.to_string_lossy().to_string();
    let args = crate::compare::substitute_placeholders(
        &case.invoke.args,
        &fixture_str,
        &workspace_str,
    );

    // 4. Call tool
    let timeout = Duration::from_millis(case.invoke.timeout_ms);
    let response = client
        .call_tool(&case.invoke.tool, &args, timeout)
        .with_context(|| format!("call_tool({})", case.invoke.tool))?;

    // 5. Shutdown
    let _ = client.shutdown();

    // 6. Unwrap the MCP envelope. Many tools wrap their payload as
    //    `{ "content": [{ "type": "text", "text": "<json>" }] }`.
    //    Parse the inner JSON if present; otherwise use the raw response.
    let raw = unwrap_mcp_content(&response).unwrap_or(response);

    // 7. Normalise — strip volatile fields, substitute paths back to
    //    `${fixture}` / `${workspace}` placeholders, then per-case
    //    passes (sort, float-round, extra strips). Apply the same
    //    safe subset to expected so authoring is forgiving (no need
    //    to pre-sort or pre-round). Uses the resolved profile+case
    //    merge from step 0a so authoring is also profile-forgiving.
    let normalised = normalize(&raw, &workspace_str, &fixture_str, &resolved_opts);

    // Bless mode short-circuits: skip comparison, hand the normalized
    // actual back to main to splice into the YAML's `expect.data`.
    if bless {
        return Ok(InnerResult {
            passed: true,
            diff: String::new(),
            blessed_actual: Some(normalised),
        });
    }

    let expected = normalize_expected(&case.expect.data, &resolved_opts);

    // 8. Compare
    let comp = compare(&normalised, &expected, case.expect.r#match)?;

    Ok(InnerResult {
        passed: comp.passed,
        diff: comp.diff,
        blessed_actual: None,
    })
}

/// Copy the fixture (single file or directory) into a fresh tempdir
/// and return a handle to it. The harness owns the tempdir and drops
/// it when the case finishes.
fn make_workspace(setup: &Setup, fixtures_root: &Path) -> Result<tempfile::TempDir> {
    let src = fixtures_root.join(&setup.fixture);
    if !src.exists() {
        return Err(anyhow!(
            "fixture not found: {} (relative to {})",
            setup.fixture,
            fixtures_root.display()
        ));
    }
    let workspace = tempfile::Builder::new()
        .prefix("codegraph-harness-")
        .tempdir()
        .context("create tempdir for workspace")?;

    match setup.workspace_layout {
        WorkspaceLayout::SingleFile => {
            let dest_name = src
                .file_name()
                .ok_or_else(|| anyhow!("fixture has no filename: {}", src.display()))?;
            let dest = workspace.path().join(dest_name);
            std::fs::copy(&src, &dest)
                .with_context(|| format!("copy {} -> {}", src.display(), dest.display()))?;
        }
        WorkspaceLayout::MultiFile => {
            let dir = if src.is_dir() { src.clone() } else { src.parent().unwrap().to_path_buf() };
            copy_dir_recursive(&dir, workspace.path())?;
        }
    }
    Ok(workspace)
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    for entry in walkdir::WalkDir::new(src) {
        let entry = entry?;
        let rel = entry.path().strip_prefix(src)?;
        let target = dst.join(rel);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&target)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

fn resolve_fixture_path(setup: &Setup, workspace: &Path) -> Result<PathBuf> {
    let fixture_relative = Path::new(&setup.fixture)
        .file_name()
        .ok_or_else(|| anyhow!("fixture has no filename: {}", setup.fixture))?;
    Ok(workspace.join(fixture_relative))
}

/// Many MCP tools wrap their payload as
/// `{ "content": [ { "type": "text", "text": "<json>" } ] }`. Unwrap
/// the inner JSON if present so case YAML can describe the *tool's*
/// output, not the MCP envelope. Returns None if the wrapper isn't
/// present (caller falls back to comparing the raw response).
fn unwrap_mcp_content(response: &serde_json::Value) -> Option<serde_json::Value> {
    let content = response.get("content")?.as_array()?;
    let first = content.first()?;
    let text = first.get("text")?.as_str()?;
    serde_json::from_str(text).ok()
}
