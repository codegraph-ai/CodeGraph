#!/usr/bin/env node
// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

"use strict";

/**
 * Guards the argument handling in bin/codegraph-mcp.js.
 *
 * A duplicated `--mcp` made clap exit 2 before the engine emitted any
 * telemetry; the MCP client respawned into the identical failure and 134
 * machines produced 656,283 crash events in a month. The wrapper always
 * supplies `--mcp`, and every doc and example shows `--mcp`, so a user copying
 * one into their MCP config triggered it deterministically.
 *
 * These run the real wrapper against a stub "engine" so the argument contract
 * is checked without a 116 MB binary or a network.
 *
 * Run with `node test/wrapper-args.test.js`.
 */

const { execFileSync, spawnSync } = require("child_process");
const fs = require("fs");
const os = require("os");
const path = require("path");

const WRAPPER = path.join(__dirname, "..", "bin", "codegraph-mcp.js");
const LOOP_STATE = path.join(os.homedir(), ".codegraph", "mcp-failures.json");

let failures = 0;
function check(ok, message) {
  console.log((ok ? "PASS " : "FAIL ") + message);
  if (!ok) failures++;
}

/**
 * A stand-in engine that echoes its argv and exits how the test asks.
 * The wrapper locates the binary by platform name, so the stub takes that name.
 */
function stubEngine(dir, exitCode) {
  const name =
    os.platform() === "win32"
      ? "codegraph-server-win32-x64.exe"
      : `codegraph-server-${os.platform()}-${os.arch() === "arm64" ? "arm64" : "x64"}`;
  const file = path.join(dir, name);
  fs.writeFileSync(
    file,
    `#!/usr/bin/env node\n` +
      `process.stderr.write("ARGV:" + JSON.stringify(process.argv.slice(2)) + "\\n");\n` +
      `process.exit(${exitCode});\n`
  );
  fs.chmodSync(file, 0o755);
  return file;
}

function runWrapper(clientArgs, exitCode) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "cg-wrapper-"));
  stubEngine(dir, exitCode);
  try {
    const result = spawnSync(process.execPath, [WRAPPER, ...clientArgs], {
      // CODEGRAPH_SERVER_PATH is how the wrapper is pointed at a specific
      // engine; falling back to the bundled path would find the real one.
      env: { ...process.env, CODEGRAPH_BIN_DIR: dir, CODEGRAPH_SKIP_MODEL_FETCH: "1" },
      encoding: "utf8",
      timeout: 20000,
      input: "",
    });
    const stderr = result.stderr || "";
    const argvLine = stderr.split("\n").find((l) => l.startsWith("ARGV:"));
    return {
      argv: argvLine ? JSON.parse(argvLine.slice(5)) : null,
      stderr,
      status: result.status,
    };
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
}

// The wrapper resolves its engine through findBinary(); if that cannot be
// redirected by env, these tests cannot isolate and should say so rather than
// silently exercise the real binary.
const probe = runWrapper([], 0);
if (probe.argv === null) {
  console.log(
    "SKIP wrapper argument tests - the stub engine was not used " +
      "(findBinary() ignored CODEGRAPH_BIN_DIR)."
  );
  process.exit(0);
}

// --- the bug: a client that also passes --mcp ------------------------
{
  const { argv } = runWrapper(["--mcp"], 0);
  const mcpCount = argv.filter((a) => a === "--mcp").length;
  check(mcpCount === 1, `client --mcp is not duplicated (saw ${mcpCount})`);
}

// --- mode flags the wrapper owns are dropped, others survive ---------
{
  const { argv } = runWrapper(["--mcp", "--workspace", "/tmp/x"], 0);
  check(argv.includes("--workspace"), "genuine client args are forwarded");
  check(argv.includes("/tmp/x"), "their values are forwarded");
  check(argv.filter((a) => a === "--mcp").length === 1, "only one --mcp reaches the engine");
}

// --- the wrapper still supplies the mode when the client does not ----
{
  const { argv } = runWrapper([], 0);
  check(argv[0] === "--mcp", "wrapper supplies --mcp when the client omits it");
}

// --- exit 2 is explained rather than reported as a crash -------------
{
  try {
    fs.unlinkSync(LOOP_STATE);
  } catch {
    /* nothing recorded */
  }
  const { stderr } = runWrapper(["--whatever"], 2);
  check(/rejected its arguments/.test(stderr), "exit 2 produces a plain explanation");
  check(/configuration problem/.test(stderr), "it is named as configuration, not a crash");
  check(/--mcp --whatever/.test(stderr), "the actual arguments are shown");
}

// --- repeated identical failures stop being reported -----------------
{
  try {
    fs.unlinkSync(LOOP_STATE);
  } catch {
    /* nothing recorded */
  }
  let sawBreaker = false;
  for (let i = 0; i < 3; i++) {
    const { stderr } = runWrapper(["--whatever"], 2);
    if (/Not reporting further failures/.test(stderr)) sawBreaker = true;
  }
  check(sawBreaker, "the breaker engages within three identical failures");
  try {
    fs.unlinkSync(LOOP_STATE);
  } catch {
    /* cleanup */
  }
}

console.log("");
console.log(`${failures} failure(s)`);
process.exit(failures ? 1 : 0);
