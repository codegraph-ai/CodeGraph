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

/// RAII phase marker. Stamps `phase` on creation and resets to `serving` when
/// dropped — i.e. on normal completion or unwind. A native crash (SIGSEGV /
/// 0xC0000005 access violation) never runs the drop, so the phase stays
/// stamped and the next start attributes the crash to it.
///
/// Phases are sequential, not nested: the initial index runs
/// `index_parse → index_persist → index_embed`, each guarded in its own scope
/// so they don't overlap. Resetting to `serving` between/after them is the
/// intended steady state — the process is back to serving requests.
#[must_use = "the phase resets to `serving` as soon as the guard is dropped"]
pub struct PhaseGuard(());

/// Enter a crash phase; the returned guard resets to `serving` on drop.
pub fn enter(phase: &str) -> PhaseGuard {
    mark(phase);
    PhaseGuard(())
}

impl Drop for PhaseGuard {
    fn drop(&mut self) {
        mark("serving");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // All tests here mutate the process-global HOME/USERPROFILE and write to the
    // same PID-keyed marker file, so they must not run concurrently.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Path of this process's marker file under a given fake home.
    fn marker_path(home: &std::path::Path) -> PathBuf {
        home.join(".codegraph")
            .join(format!("last-phase.{}.json", std::process::id()))
    }

    /// Run `f` with HOME/USERPROFILE pointed at a fresh temp dir, restoring the
    /// previous values afterward. Returns the temp dir so callers can inspect it.
    fn with_isolated_home<F: FnOnce(&std::path::Path)>(f: F) {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join(format!("cg-phase-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let prev_home = std::env::var_os("HOME");
        let prev_up = std::env::var_os("USERPROFILE");
        std::env::set_var("HOME", &tmp);
        std::env::set_var("USERPROFILE", &tmp);

        f(&tmp);

        match prev_home {
            Some(h) => std::env::set_var("HOME", h),
            None => std::env::remove_var("HOME"),
        }
        match prev_up {
            Some(h) => std::env::set_var("USERPROFILE", h),
            None => std::env::remove_var("USERPROFILE"),
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn mark_writes_parseable_json_with_phase_and_pid() {
        with_isolated_home(|home| {
            mark("onnx_load");

            let content = std::fs::read_to_string(marker_path(home)).expect("marker written");
            let v: serde_json::Value = serde_json::from_str(&content).expect("valid JSON");
            assert_eq!(v["phase"], "onnx_load");
            assert_eq!(v["pid"], std::process::id());
            assert!(
                v["ts"].as_u64().is_some(),
                "ts is a numeric millisecond stamp"
            );
        });
    }

    #[test]
    fn mark_overwrites_same_process_marker() {
        with_isolated_home(|home| {
            mark("index_parse");
            mark("index_embed");

            let content = std::fs::read_to_string(marker_path(home)).expect("marker written");
            let v: serde_json::Value = serde_json::from_str(&content).expect("valid JSON");
            assert_eq!(v["phase"], "index_embed", "second mark replaces the first");
        });
    }

    #[test]
    fn clear_removes_this_process_marker() {
        with_isolated_home(|home| {
            mark("serving");
            assert!(marker_path(home).exists(), "marker exists before clear");

            clear();
            assert!(!marker_path(home).exists(), "clear removes the marker");
        });
    }

    #[test]
    fn clear_without_existing_marker_is_noop() {
        with_isolated_home(|home| {
            // No mark() first; clear must not panic or create anything.
            clear();
            assert!(!marker_path(home).exists());
        });
    }

    #[test]
    fn enter_stamps_phase_and_guard_resets_to_serving() {
        with_isolated_home(|home| {
            {
                let _guard = enter("index_persist");
                let content = std::fs::read_to_string(marker_path(home)).expect("marker written");
                let v: serde_json::Value = serde_json::from_str(&content).expect("valid JSON");
                assert_eq!(v["phase"], "index_persist", "enter stamps the phase");
            } // guard dropped here

            let content = std::fs::read_to_string(marker_path(home)).expect("marker still present");
            let v: serde_json::Value = serde_json::from_str(&content).expect("valid JSON");
            assert_eq!(
                v["phase"], "serving",
                "dropping the guard resets to serving"
            );
        });
    }

    #[test]
    fn mark_falls_back_to_userprofile_when_home_absent() {
        // Windows path: HOME is unset, so codegraph_dir() must fall through the
        // `.or_else` arm to USERPROFILE rather than short-circuiting on HOME.
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join(format!("cg-phase-up-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let prev_home = std::env::var_os("HOME");
        let prev_up = std::env::var_os("USERPROFILE");
        std::env::remove_var("HOME");
        std::env::set_var("USERPROFILE", &tmp);

        mark("onnx_load");

        let content = std::fs::read_to_string(marker_path(&tmp))
            .expect("marker written under USERPROFILE when HOME is absent");
        let v: serde_json::Value = serde_json::from_str(&content).expect("valid JSON");
        assert_eq!(v["phase"], "onnx_load");

        match prev_home {
            Some(h) => std::env::set_var("HOME", h),
            None => std::env::remove_var("HOME"),
        }
        match prev_up {
            Some(h) => std::env::set_var("USERPROFILE", h),
            None => std::env::remove_var("USERPROFILE"),
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn mark_without_home_is_noop() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev_home = std::env::var_os("HOME");
        let prev_up = std::env::var_os("USERPROFILE");
        std::env::remove_var("HOME");
        std::env::remove_var("USERPROFILE");

        // codegraph_dir() returns None with no home var; mark must not panic.
        mark("onnx_load");

        match prev_home {
            Some(h) => std::env::set_var("HOME", h),
            None => std::env::remove_var("HOME"),
        }
        match prev_up {
            Some(h) => std::env::set_var("USERPROFILE", h),
            None => std::env::remove_var("USERPROFILE"),
        }
    }
}
