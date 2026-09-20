// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use std::process::Stdio;
use std::time::Duration;

#[tokio::test]
async fn graph_only_indexes_source_without_initializing_memory_or_models() {
    let temporary = tempfile::Builder::new()
        .prefix("codegraph-harness-")
        .tempdir()
        .unwrap();
    let workspace = temporary.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::write(
        workspace.join("source.rs"),
        "pub fn graph_only_symbol() {}\n",
    )
    .unwrap();
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_codegraph-server"));
    command
        .args(["--graph-only", "--embedding-model", "static", "--workspace"])
        .arg(&workspace)
        .args([
            "--run-tool",
            "codegraph_symbol_search",
            "--tool-args",
            r#"{"query":"graph_only_symbol"}"#,
        ])
        .env("HOME", temporary.path())
        .env("USERPROFILE", temporary.path())
        .env("CODEGRAPH_TELEMETRY", "off")
        .env(
            "CODEGRAPH_STATIC_MODEL",
            temporary.path().join("absent-model"),
        )
        .env("CODEGRAPH_SKIP_MEMORY_CHECK", "1")
        .env("RUST_LOG", "info")
        .stdin(Stdio::null())
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(30), command.output())
        .await
        .unwrap()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        result["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["symbol"]["name"] == "graph_only_symbol"),
        "{result}"
    );
    assert!(!stderr.contains("MemoryManager::initialize"), "{stderr}");
    assert!(!workspace.join(".codegraph-state/memory").exists());
    assert!(!temporary.path().join(".codegraph/fastembed_cache").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn concurrent_relays_auto_start_one_configured_engine() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let temporary = tempfile::Builder::new()
        .prefix("codegraph-harness-")
        .tempdir()
        .unwrap();
    let workspace = temporary.path().join("workspace");
    std::fs::create_dir_all(workspace.join("generated")).unwrap();
    std::fs::write(workspace.join("source.rs"), "pub fn shared_symbol() {}\n").unwrap();
    std::fs::write(
        workspace.join("generated/ignored.rs"),
        "pub fn unwanted_symbol() {}\n",
    )
    .unwrap();
    // Unix socket paths have a small length limit, especially on macOS.
    let socket_dir = tempfile::tempdir().unwrap();
    let socket = socket_dir.path().join("engine.sock");
    let mut clients = Vec::new();
    for _ in 0..2 {
        let child = tokio::process::Command::new(env!("CARGO_BIN_EXE_codegraph-server"))
            .args(["--connect", "--socket"])
            .arg(&socket)
            .arg("--workspace")
            .arg(&workspace)
            .args([
                "--graph-only",
                "--profile",
                "core",
                "--exclude",
                "generated",
                "--max-files",
                "1",
                "--embedding-model",
                "static",
            ])
            .env("HOME", temporary.path())
            .env("USERPROFILE", temporary.path())
            .env("CODEGRAPH_TELEMETRY", "off")
            .env(
                "CODEGRAPH_STATIC_MODEL",
                temporary.path().join("absent-model"),
            )
            .env("CODEGRAPH_SKIP_MEMORY_CHECK", "1")
            .env("CODEGRAPH_ENGINE_IDLE_SECS", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        clients.push(child);
    }
    let requests = concat!(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"codegraph_symbol_search\",\"arguments\":{\"query\":\"symbol\"}}}\n",
    );
    // Queue both clients before reading either reply, exercising concurrent attach.
    for child in &mut clients {
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(requests.as_bytes())
            .await
            .unwrap();
    }
    for child in &mut clients {
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        for id in [1, 2] {
            let line = tokio::time::timeout(Duration::from_secs(30), lines.next_line())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            let response: serde_json::Value = serde_json::from_str(&line).unwrap();
            assert_eq!(response["id"], id);
            assert!(response.get("error").is_none(), "{response}");
            if id == 1 {
                assert_eq!(response["result"]["tools"].as_array().unwrap().len(), 8);
            } else {
                let result: serde_json::Value = serde_json::from_str(
                    response["result"]["content"][0]["text"].as_str().unwrap(),
                )
                .unwrap();
                let names: Vec<_> = result["results"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|entry| entry["symbol"]["name"].as_str().unwrap())
                    .collect();
                assert_eq!(names, ["shared_symbol"]);
            }
        }
    }
    for child in &mut clients {
        drop(child.stdin.take());
        let status = tokio::time::timeout(Duration::from_secs(10), child.wait())
            .await
            .unwrap()
            .unwrap();
        assert!(status.success());
    }
    tokio::time::timeout(Duration::from_secs(10), async {
        while socket.exists() {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("engine did not exit after its last client disconnected");
    assert!(!workspace.join(".codegraph-state/memory").exists());
    assert!(!temporary.path().join(".codegraph/fastembed_cache").exists());
}
