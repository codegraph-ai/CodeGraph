// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Best-effort init-phase breadcrumb.
//!
//! Native crashes (OOM-kill, SIGSEGV/SIGILL in the ONNX runtime) never run
//! the panic hook, so telemetry can only see them as `hard_crash` with no
//! cause. We stamp the current init phase to `~/.codegraph/last-phase.<pid>.json`;
//! on the next start the VS Code extension reads it and reports e.g.
//! `hard_crash` @ `onnx_load`, pinpointing where the process died. Every
//! operation is best-effort and never panics.

use std::path::PathBuf;

fn codegraph_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(PathBuf::from(home).join(".codegraph"))
}

/// Record the current init phase, overwriting this process's marker.
pub fn mark(phase: &str) {
    let Some(dir) = codegraph_dir() else {
        return;
    };
    let _ = std::fs::create_dir_all(&dir);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let pid = std::process::id();
    // `phase` is always a fixed ASCII literal — no JSON escaping needed.
    let json = format!("{{\"phase\":\"{phase}\",\"ts\":{ts},\"pid\":{pid}}}");
    let _ = std::fs::write(dir.join(format!("last-phase.{pid}.json")), json);
}

/// Remove this process's phase marker on clean shutdown so it can't be
/// misread as the crash phase of a later process.
pub fn clear() {
    if let Some(dir) = codegraph_dir() {
        let _ = std::fs::remove_file(dir.join(format!("last-phase.{}.json", std::process::id())));
    }
}
