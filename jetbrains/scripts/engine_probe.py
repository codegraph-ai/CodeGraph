#!/usr/bin/env python3
# Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
# SPDX-License-Identifier: Apache-2.0

"""Contract check between the JetBrains plugin and the CodeGraph engine.

Replays, over raw stdio, exactly what the plugin sends: the `initialize` request
built by `CodeGraphConnectionProvider.getInitializationOptions()`, followed by
the `workspace/executeCommand` calls the plugin makes. It needs no IDE, so it
runs in CI and answers the question the IDE cannot answer quickly: is the
protocol contract still intact?

It also diffs `CodeGraphCommand.kt` against the command list the engine
advertises, which is the drift guard for that hand-transcribed enum.

Usage:
    python3 jetbrains/scripts/engine_probe.py <path-to-codegraph-server> <workspace-root>
"""

import json
import os
import re
import subprocess
import sys
import threading
import time

if len(sys.argv) != 3:
    sys.exit(__doc__)

BIN, ROOT = sys.argv[1], os.path.abspath(sys.argv[2])

# Commands the engine dispatches but deliberately does not advertise. Each entry
# needs a reason: an unadvertised command is invisible to clients that gate on
# ServerCapabilities, which is how LSP4IJ behaves.
UNADVERTISED_BY_DESIGN = {
    # Dispatched at backend.rs, absent from executeCommandProvider.commands.
    # VS Code reaches it through the custom-request form so it never noticed.
    "codegraph.getDocumentCodeLens": "not yet advertised; tracked as a server fix",
}

failures = []


def check(ok, message):
    print(("PASS " if ok else "FAIL ") + message)
    if not ok:
        failures.append(message)


proc = subprocess.Popen(
    [BIN],
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    cwd=ROOT,
)

_next_id = [0]


def send(method, params, notify=False):
    msg = {"jsonrpc": "2.0", "method": method, "params": params}
    if not notify:
        _next_id[0] += 1
        msg["id"] = _next_id[0]
    body = json.dumps(msg).encode()
    proc.stdin.write(b"Content-Length: %d\r\n\r\n" % len(body) + body)
    proc.stdin.flush()
    return msg.get("id")


def read_message():
    headers = {}
    while True:
        line = proc.stdout.readline()
        if not line:
            return None
        line = line.strip()
        if not line:
            break
        key, _, value = line.decode().partition(":")
        headers[key.strip().lower()] = value.strip()
    length = int(headers.get("content-length", 0))
    return json.loads(proc.stdout.read(length)) if length else None


def await_response(want_id, timeout=180):
    deadline = time.time() + timeout
    while time.time() < deadline:
        msg = read_message()
        if msg is None:
            sys.exit("engine closed the stream")
        if msg.get("id") == want_id and ("result" in msg or "error" in msg):
            return msg
    sys.exit(f"timed out waiting for response to id={want_id}")


def execute_command(command, arguments):
    rid = send("workspace/executeCommand", {"command": command, "arguments": [arguments]})
    return await_response(rid)


threading.Thread(
    target=lambda: [sys.stderr.write("[engine] " + line.decode(errors="replace"))
                    for line in iter(proc.stderr.readline, b"")],
    daemon=True,
).start()

# Mirrors CodeGraphConnectionProvider.getInitializationOptions(), except that
# indexOnStartup is forced off: the probe checks the protocol, not the indexer,
# and a full workspace index would dominate its runtime.
init_options = {
    "extensionPath": os.path.expanduser("~/.codegraph/jetbrains"),
    "indexOnStartup": False,
    "excludePatterns": ["**/node_modules/**", "**/target/**", "**/.git/**"],
    "indexPaths": [],
    "maxFileSizeKB": 1024,
    "embeddingModel": "bge-small",
    "staticModelPath": None,
    "fullBodyEmbedding": True,
    "embedOnOpen": True,
}

rid = send(
    "initialize",
    {
        "processId": os.getpid(),
        "rootUri": "file://" + ROOT,
        "capabilities": {"workspace": {"executeCommand": {"dynamicRegistration": True}}},
        "initializationOptions": init_options,
        "workspaceFolders": [{"uri": "file://" + ROOT, "name": os.path.basename(ROOT)}],
    },
)
response = await_response(rid)
capabilities = response["result"]["capabilities"]
advertised = set(capabilities.get("executeCommandProvider", {}).get("commands", []))
check(bool(advertised), f"initialize -> {len(advertised)} commands advertised")

send("initialized", {}, notify=True)

response = execute_command("codegraph.getParserMetrics", {})
check("error" not in response, f"getParserMetrics -> {str(response.get('error') or 'ok')[:120]}")

response = execute_command("codegraph.symbolSearch", {"query": "main", "limit": 5})
check("error" not in response, f"symbolSearch -> {json.dumps(response.get('result'))[:160]}")

# getDocumentCodeLens backs the Code Vision surface. Call it directly rather
# than trusting the advertised list, because that list is currently incomplete.
response = execute_command(
    "codegraph.getDocumentCodeLens", {"uri": "file://" + os.path.join(ROOT, "README.md")}
)
check("error" not in response, f"getDocumentCodeLens -> {str(response.get('error') or 'ok')[:120]}")

enum_path = os.path.join(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
    "src/main/kotlin/ai/codegraph/jetbrains/lsp/CodeGraphCommand.kt",
)
declared = set()
with open(enum_path) as handle:
    for line in handle:
        if '("codegraph.' in line:
            declared.add(line.split('"')[1])

missing = sorted(advertised - declared)
check(not missing, f"CodeGraphCommand.kt covers every advertised command (missing: {missing})")

undocumented = sorted(declared - advertised - set(UNADVERTISED_BY_DESIGN))
check(
    not undocumented,
    f"every declared-but-unadvertised command has a recorded reason (undocumented: {undocumented})",
)

# Settings defaults must agree with the VS Code client. They are separate
# hand-written files, and a divergence is invisible until it changes behaviour:
# defaulting indexOnStartup to true made the engine index during `initialize`
# while the plugin was still deciding whether to prompt for an index.
PARITY_KEYS = {
    "indexOnStartup": "codegraph.indexOnStartup",
    "maxFileSizeKB": "codegraph.maxFileSizeKB",
    "embeddingModel": "codegraph.embeddingModel",
    "fullBodyEmbedding": "codegraph.fullBodyEmbedding",
    "embedOnOpen": "codegraph.embedOnOpen",
}

KOTLIN_LITERALS = {"true": True, "false": False}


def kotlin_defaults(path):
    """Parse `@JvmField var name: Type = value` declarations."""
    found = {}
    pattern = re.compile(r"var\s+(\w+)\s*:\s*[\w<>]+\s*=\s*([^\n]+)")
    with open(path) as handle:
        for line in handle:
            match = pattern.search(line)
            if not match:
                continue
            name, raw = match.group(1), match.group(2).strip().rstrip(",")
            if raw in KOTLIN_LITERALS:
                found[name] = KOTLIN_LITERALS[raw]
            elif raw.isdigit():
                found[name] = int(raw)
            elif raw.startswith('"') and raw.endswith('"'):
                found[name] = raw[1:-1]
    return found


plugin_root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
settings_path = os.path.join(
    plugin_root, "src/main/kotlin/ai/codegraph/jetbrains/settings/CodeGraphSettings.kt"
)
vscode_package = os.path.join(os.path.dirname(plugin_root), "vscode/package.json")

if os.path.exists(vscode_package):
    kotlin = kotlin_defaults(settings_path)
    with open(vscode_package) as handle:
        contributes = json.load(handle)["contributes"]["configuration"]
    properties = (contributes[0] if isinstance(contributes, list) else contributes)["properties"]

    drifted = [
        f"{kotlin_key}={kotlin.get(kotlin_key)!r} but {vscode_key}={properties[vscode_key].get('default')!r}"
        for kotlin_key, vscode_key in PARITY_KEYS.items()
        if vscode_key in properties and kotlin.get(kotlin_key) != properties[vscode_key].get("default")
    ]
    check(not drifted, f"settings defaults match the VS Code client ({'; '.join(drifted)})")
else:
    print("SKIP settings-defaults parity (vscode/package.json not found)")

rid = send("shutdown", {})
await_response(rid, timeout=30)
send("exit", {}, notify=True)

# The engine currently ignores `exit` and terminates only when stdin closes.
# Both real clients force-kill the process, so this costs correctness rather
# than leaked processes - but the probe must not hang on it, and should say so
# out loud if it ever starts behaving.
EXIT_GRACE_SECONDS = 5
try:
    proc.wait(timeout=EXIT_GRACE_SECONDS)
    print(f"NOTE engine honoured `exit` within {EXIT_GRACE_SECONDS}s")
except subprocess.TimeoutExpired:
    print(f"NOTE engine ignored `exit` (known deviation); closing stdin instead")
    proc.stdin.close()
    try:
        proc.wait(timeout=30)
    except subprocess.TimeoutExpired:
        proc.kill()

print()
print(f"{len(failures)} failure(s)")
sys.exit(1 if failures else 0)
