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

/// Markers we own and may delete. Deliberately excludes `last-recovery.`:
/// those are reported once with no freshness window, so a client that has not
/// read one yet still needs it, however old it is.
const SWEEPABLE_PREFIXES: [&str; 2] = ["last-phase.", "last-crash."];

/// How old a marker must be before we consider removing it. Both clients only
/// trust a breadcrumb within ~15 seconds of the crash it describes, so an hour
/// is far past the point where one can still explain anything - while leaving
/// an enormous margin for a client that is slow to read it.
const SWEEP_MIN_AGE: std::time::Duration = std::time::Duration::from_secs(60 * 60);

/// Delete markers left behind by processes that no longer exist.
///
/// [`clear`] only removes the current process's marker, and it runs after the
/// LSP serve loop returns - which does not happen when a client force-kills the
/// engine, as both clients do. So every killed process used to leave its marker
/// behind permanently: 310 of them accumulated on one machine over two months.
///
/// The cost is not the disk space, it is that stale markers make the clients'
/// freshness window the only thing standing between a months-old marker and a
/// wrong crash diagnosis today.
///
/// Two conditions, both required, so this can never destroy a live diagnosis:
/// the marker is older than [`SWEEP_MIN_AGE`], *and* its process is gone. Age
/// alone would delete the marker of a long-running engine that later crashes;
/// liveness alone would race a client that has not yet read a fresh crash.
/// Best-effort throughout - housekeeping must never break startup.
pub fn sweep_orphans() {
    if let Some(dir) = codegraph_dir() {
        sweep_orphans_in(&dir);
    }
}

/// [`sweep_orphans`] against an explicit directory.
///
/// Split out so the policy can be tested without setting `HOME`, which is
/// process-global and would race the other tests in this binary.
fn sweep_orphans_in(dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    let own_pid = std::process::id();
    let now = std::time::SystemTime::now();
    let mut system: Option<sysinfo::System> = None;
    let mut removed = 0usize;

    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };

        let Some(pid) = SWEEPABLE_PREFIXES
            .iter()
            .find_map(|prefix| name.strip_prefix(prefix))
            .and_then(|rest| rest.strip_suffix(".json"))
            .and_then(|pid| pid.parse::<u32>().ok())
        else {
            continue;
        };
        if pid == own_pid {
            continue;
        }

        let old_enough = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age >= SWEEP_MIN_AGE);
        if !old_enough {
            continue;
        }

        // Only pay for the process table once, and only if something is
        // actually old enough to be a candidate.
        let system = system.get_or_insert_with(sysinfo::System::new);
        if system.refresh_process(sysinfo::Pid::from_u32(pid)) {
            continue;
        }

        if std::fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }

    if removed > 0 {
        tracing::info!("[crash_phase] swept {removed} orphaned breadcrumb(s)");
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
    use std::time::{Duration, SystemTime};

    /// A pid that cannot be running: above the maximum any platform allocates,
    /// so it can never collide with a live process on the test machine.
    const DEAD_PID: u32 = 4_000_000_000;

    /// Scratch `.codegraph` directory, removed on drop.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "codegraph-sweep-{tag}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        /// Write a marker and backdate it, so the age branch can be exercised
        /// without the test sleeping.
        fn write(&self, name: &str, age: Duration) {
            let path = self.0.join(name);
            std::fs::write(&path, "{}").unwrap();
            std::fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(SystemTime::now() - age)
                .unwrap();
        }

        fn exists(&self, name: &str) -> bool {
            self.0.join(name).exists()
        }

        fn sweep(&self) {
            sweep_orphans_in(&self.0);
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn sweeps_old_markers_from_dead_processes() {
        let scratch = Scratch::new("dead");
        let phase = format!("last-phase.{DEAD_PID}.json");
        let crash = format!("last-crash.{DEAD_PID}.json");
        scratch.write(&phase, SWEEP_MIN_AGE * 2);
        scratch.write(&crash, SWEEP_MIN_AGE * 2);

        scratch.sweep();

        assert!(!scratch.exists(&phase));
        assert!(!scratch.exists(&crash));
    }

    #[test]
    fn keeps_recent_markers_even_from_dead_processes() {
        // The client may not have read this crash yet - deleting it would
        // destroy the diagnosis for the crash that just happened.
        let scratch = Scratch::new("recent");
        let name = format!("last-crash.{DEAD_PID}.json");
        scratch.write(&name, Duration::from_secs(5));

        scratch.sweep();

        assert!(scratch.exists(&name));
    }

    #[test]
    fn keeps_markers_belonging_to_live_processes() {
        // A long-running engine sitting idle: old marker, live process. Removing
        // it would lose the phase attribution if it later crashes hard.
        let scratch = Scratch::new("live");
        let name = format!("last-phase.{}.json", std::process::id());
        scratch.write(&name, SWEEP_MIN_AGE * 2);

        scratch.sweep();

        assert!(scratch.exists(&name));
    }

    #[test]
    fn never_touches_recovery_breadcrumbs() {
        // Recovery markers are reported once with no freshness window, so an old
        // one is still meaningful to a client that has not read it.
        let scratch = Scratch::new("recovery");
        let name = format!("last-recovery.{DEAD_PID}.json");
        scratch.write(&name, SWEEP_MIN_AGE * 100);

        scratch.sweep();

        assert!(scratch.exists(&name));
    }

    #[test]
    fn ignores_files_that_are_not_markers() {
        let scratch = Scratch::new("unrelated");
        scratch.write("graph.db", SWEEP_MIN_AGE * 2);
        scratch.write("last-phase.not-a-pid.json", SWEEP_MIN_AGE * 2);

        scratch.sweep();

        assert!(scratch.exists("graph.db"));
        assert!(scratch.exists("last-phase.not-a-pid.json"));
    }

    #[test]
    fn missing_directory_is_not_an_error() {
        let absent = std::env::temp_dir().join(format!(
            "codegraph-sweep-absent-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));

        sweep_orphans_in(&absent);
    }
}
