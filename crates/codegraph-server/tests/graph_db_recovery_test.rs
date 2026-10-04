// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! End-to-end checks of how a session treats the shared graph DB when it can't
//! simply open it: another process holding it, or a damaged DB.
//!
//! Each test runs the real `codegraph-server --mcp` binary against a temporary
//! workspace and a temporary HOME, so the user's own `~/.codegraph` is never
//! touched.

use codegraph::RocksDBBackend;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

const MARKER: &str = "graph_db_recovery_marker_fn";

struct Env {
    _root: tempfile::TempDir,
    workspace: PathBuf,
    home: PathBuf,
}

impl Env {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("ws");
        let home = root.path().join("home");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(workspace.join("lib.rs"), format!("fn {MARKER}() {{}}\n")).unwrap();
        Self {
            _root: root,
            workspace,
            home,
        }
    }

    fn codegraph_dir(&self) -> PathBuf {
        self.home.join(".codegraph")
    }

    /// The DB a session opens when no redirect has happened.
    fn first_generation_db(&self) -> PathBuf {
        self.codegraph_dir().join("graph.db")
    }

    fn redirected(&self) -> bool {
        self.codegraph_dir().join("graph.generation").exists()
    }

    /// Run one MCP session and report whether a symbol search finds the marker.
    fn session_finds_marker(&self) -> bool {
        let mut child = Command::new(env!("CARGO_BIN_EXE_codegraph-server"))
            .args(["--mcp", "--graph-only", "--workspace"])
            .arg(&self.workspace)
            .env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let mut request = |id: u64, method: &str, params: Value| -> Value {
            let msg = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
            writeln!(stdin, "{msg}").unwrap();
            loop {
                let mut line = String::new();
                assert!(stdout.read_line(&mut line).unwrap() > 0, "server exited");
                let reply: Value = serde_json::from_str(&line).unwrap();
                if reply["id"] == id {
                    return reply;
                }
            }
        };
        request(
            1,
            "initialize",
            json!({"protocolVersion": "2024-11-05", "capabilities": {},
                   "clientInfo": {"name": "test", "version": "1"}}),
        );
        let reply = request(
            2,
            "tools/call",
            json!({"name": "codegraph_symbol_search", "arguments": {"query": MARKER}}),
        );
        drop(stdin);
        assert!(child.wait().unwrap().success());
        reply.to_string().contains(MARKER)
    }
}

/// Hold the DB open from this process for `hold`, as a sibling session does
/// while it loads or persists.
fn hold_db(path: &Path, hold: Duration) -> std::thread::JoinHandle<()> {
    let holder = RocksDBBackend::open(path).unwrap();
    std::thread::spawn(move || {
        std::thread::sleep(hold);
        drop(holder);
    })
}

#[test]
fn a_session_waits_for_a_sibling_holding_the_db_instead_of_abandoning_it() {
    let env = Env::new();
    assert!(env.session_finds_marker(), "seed session");

    let holder = hold_db(&env.first_generation_db(), Duration::from_secs(2));
    assert!(
        env.session_finds_marker(),
        "session started while the DB was held"
    );
    holder.join().unwrap();

    assert!(
        !env.redirected(),
        "lock contention must not redirect the DB"
    );
    assert!(env.session_finds_marker(), "later session");
}

#[test]
fn a_session_after_a_redirect_reindexes_files_the_old_db_held() {
    let env = Env::new();
    assert!(env.session_finds_marker(), "seed session");

    // An unreadable DB: the session redirects to a fresh, empty generation.
    std::fs::write(
        env.first_generation_db().join("CURRENT"),
        "MANIFEST-999999\n",
    )
    .unwrap();
    assert!(env.session_finds_marker(), "session that redirected");
    assert!(env.redirected());

    assert!(env.session_finds_marker(), "session after the redirect");
}
