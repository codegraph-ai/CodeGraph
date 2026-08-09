#!/usr/bin/env node
"use strict";

const { spawn } = require("child_process");
const path = require("path");
const os = require("os");
const fs = require("fs");

/** Package version, reported on every event including pre-startup crashes. */
const WRAPPER_VERSION = require("../package.json").version;

// ── PostHog telemetry (opt-out via CODEGRAPH_TELEMETRY=off) ──────────

const POSTHOG_KEY = "phc_pkWuLX7azFafdd7rqY4bfKhZ3aobCT9unTy9zSkXH3xB";
const POSTHOG_HOST = "https://us.posthog.com";
const TELEMETRY_ENABLED =
  (process.env.CODEGRAPH_TELEMETRY || "on").toLowerCase() !== "off";

let posthog = null;
let machineId = null;
// Set when WE initiate shutdown (forwarded signal) so a normal exit isn't
// misreported as a crash.
let intentionalShutdown = false;

if (TELEMETRY_ENABLED) {
  try {
    // posthog-node is an optional peer — skip silently if missing
    const { PostHog } = require("posthog-node");
    posthog = new PostHog(POSTHOG_KEY, {
      host: POSTHOG_HOST,
      flushAt: 10,
      flushInterval: 30000,
    });
    // Stable machine ID: hash of hostname + homedir (no PII sent)
    const crypto = require("crypto");
    machineId = crypto
      .createHash("sha256")
      .update(`${os.hostname()}:${os.homedir()}`)
      .digest("hex");
  } catch {
    // posthog-node not installed — telemetry disabled gracefully
  }
}

function sendTelemetry(eventData) {
  if (!posthog || !machineId) return;
  try {
    const { event, ...properties } = eventData;
    posthog.capture({
      distinctId: machineId,
      event: event || "mcp.unknown",
      properties: {
        ...properties,
        serverEdition: "community",
        transport: "mcp",
        // mcp.start carried the version and crashes did not - so a crash before
        // startup, the case that most needs attributing to a release, was the
        // one that could not be.
        version: WRAPPER_VERSION,
        os: os.platform(),
        arch: os.arch(),
        nodeVersion: process.version,
      },
    });
  } catch {
    // Never block the server on telemetry failures
  }
}

// ── Crash-loop protection ───────────────────────────────────────────
//
// The VS Code extension and the JetBrains plugin both stop restarting after
// three crashes in a minute. This wrapper cannot do that in memory: an MCP
// client respawns it as a brand new process each time, so the count has to
// outlive the process. It lives in a small file keyed by the arguments, since
// a loop is by definition the same invocation failing the same way.
const LOOP_STATE = path.join(os.homedir(), ".codegraph", "mcp-failures.json");
const LOOP_WINDOW_MS = 60_000;
const LOOP_THRESHOLD = 3;

function argsKey(argv) {
  return require("crypto").createHash("sha256").update(argv.join("\u0000")).digest("hex").slice(0, 16);
}

/** Recent failures for this exact invocation, oldest first. */
function readFailures(key) {
  try {
    const all = JSON.parse(fs.readFileSync(LOOP_STATE, "utf8"));
    const now = Date.now();
    return (all[key] || []).filter((t) => now - t < LOOP_WINDOW_MS);
  } catch {
    return [];
  }
}

function recordFailure(key) {
  try {
    fs.mkdirSync(path.dirname(LOOP_STATE), { recursive: true });
    let all = {};
    try {
      all = JSON.parse(fs.readFileSync(LOOP_STATE, "utf8"));
    } catch {
      // Missing or corrupt - start fresh rather than fail the exit path.
    }
    const now = Date.now();
    const recent = (all[key] || []).filter((t) => now - t < LOOP_WINDOW_MS);
    recent.push(now);
    // Only ever track the current invocation: stale keys from configs the user
    // has since fixed would otherwise accumulate forever.
    fs.writeFileSync(LOOP_STATE, JSON.stringify({ [key]: recent }));
    return recent.length;
  } catch {
    return 1;
  }
}

function clearFailures() {
  try {
    fs.unlinkSync(LOOP_STATE);
  } catch {
    // Nothing recorded, or unwritable - neither is worth reporting.
  }
}

function flushAndExit(code) {
  if (posthog) {
    posthog
      .shutdown()
      .catch(() => {})
      .finally(() => process.exit(code));
    // Hard timeout — don't hang on flush
    setTimeout(() => process.exit(code), 2000);
  } else {
    process.exit(code);
  }
}

// ── Binary discovery ─────────────────────────────────────────────────

const PLATFORM_MAP = {
  darwin: "darwin",
  linux: "linux",
  win32: "win32",
};

const ARCH_MAP = {
  arm64: "arm64",
  x64: "x64",
  x86_64: "x64",
};

function getBinaryName() {
  const platform = PLATFORM_MAP[os.platform()];
  const arch = ARCH_MAP[os.arch()];

  if (!platform || !arch) {
    console.error(
      `Unsupported platform: ${os.platform()}-${os.arch()}`
    );
    process.exit(1);
  }

  const ext = platform === "win32" ? ".exe" : "";
  return `codegraph-server-${platform}-${arch}${ext}`;
}

function findBinary() {
  // An explicit engine wins. The postinstall already tells users to set this
  // when a download fails or the machine is air-gapped, and until now it did
  // nothing - the advice pointed at a variable this function never read.
  const override = process.env.CODEGRAPH_SERVER_PATH;
  if (override) {
    if (fs.existsSync(override)) return override;
    console.error(`CODEGRAPH_SERVER_PATH is set but no file exists there: ${override}`);
    process.exit(1);
  }

  const binaryName = getBinaryName();
  const binDir = process.env.CODEGRAPH_BIN_DIR || __dirname;
  const binaryPath = path.join(binDir, binaryName);

  if (fs.existsSync(binaryPath)) {
    return binaryPath;
  }

  console.error(`Binary not found: ${binaryPath}`);
  console.error(`Platform: ${os.platform()}-${os.arch()}`);
  console.error(
    `Available binaries: ${fs
      .readdirSync(binDir)
      .filter((f) => f.startsWith("codegraph-server-"))
      .join(", ") || "none"}`
  );
  process.exit(1);
}

// ── Spawn the Rust binary ────────────────────────────────────────────

const binaryPath = findBinary();

// Model B (opt-in): route this session through the shared socket engine via a
// thin `--connect` relay (~20 MB) instead of a full per-session server (~360 MB+).
// The relay auto-spawns the engine on first use; the engine holds one model
// across all sessions/projects. Unix-only for now; OFF by default so existing
// behavior is unchanged until the engine is proven in the wild.
const USE_ENGINE =
  ["1", "true", "on", "yes"].includes(
    (process.env.CODEGRAPH_ENGINE || "").toLowerCase()
  ) && os.platform() !== "win32";
// Arguments the client passed through its MCP config, minus anything this
// wrapper supplies itself.
//
// Every doc and example writes `--mcp`, so users naturally put it in their MCP
// config too - and this wrapper already adds it. clap rejects the duplicate
// with "the argument '--mcp' cannot be used multiple times" and exits 2 before
// the engine emits any telemetry, so the client respawns with the same config
// and fails identically, forever. That single collision produced 656k crash
// events across ~134 machines in one month.
//
// Mode flags are dropped rather than passed through: this wrapper decides the
// mode, so a client that names one is either agreeing with us (harmless) or
// asking for a mode the wrapper cannot deliver (which would be a confusing
// half-configured server).
const WRAPPER_OWNED_FLAGS = new Set(["--mcp", "--connect", "--stdio"]);
const clientArgs = process.argv.slice(2).filter((a) => !WRAPPER_OWNED_FLAGS.has(a));

const args = USE_ENGINE
  ? ["--connect", "--workspace", process.cwd(), ...clientArgs]
  : ["--mcp", ...clientArgs];

// stdin/stdout are inherited (JSON-RPC channel — untouched).
// stderr is piped so we can intercept TEL: lines for PostHog.
const child = spawn(binaryPath, args, {
  stdio: ["inherit", "inherit", "pipe"],
  env: process.env,
});

// Parse stderr: forward TEL: lines to PostHog, pass everything else through
let stderrBuf = "";
child.stderr.on("data", (chunk) => {
  stderrBuf += chunk.toString();
  let newlineIdx;
  while ((newlineIdx = stderrBuf.indexOf("\n")) !== -1) {
    const line = stderrBuf.substring(0, newlineIdx);
    stderrBuf = stderrBuf.substring(newlineIdx + 1);

    if (line.startsWith("TEL: ")) {
      try {
        const data = JSON.parse(line.substring(5));
        // The engine only reports mcp.start once it is past argument parsing
        // and actually serving, so this is the signal that the configuration
        // works and any recorded failures are history.
        if (data && data.event === "mcp.start") clearFailures();
        sendTelemetry(data);
      } catch {
        // Malformed TEL line — ignore
      }
    } else {
      // Forward non-telemetry stderr to the real stderr
      process.stderr.write(line + "\n");
    }
  }
});

child.on("error", (err) => {
  console.error(`Failed to start codegraph-mcp: ${err.message}`);
  flushAndExit(1);
});

child.on("exit", (code, signal) => {
  // Flush remaining stderr buffer
  if (stderrBuf.trim()) {
    process.stderr.write(stderrBuf);
  }
  // Report abnormal, non-self-initiated exits so the MCP channel surfaces WHY
  // the server died (mirrors the extension's server.crash exit info): a unix
  // signal (SIGSEGV / SIGKILL=OOM) or a non-zero / Windows exit code.
  const abnormal =
    !intentionalShutdown &&
    (signal != null || (typeof code === "number" && code !== 0));
  if (abnormal) {
    const failures = recordFailure(argsKey(args));
    const looping = failures >= LOOP_THRESHOLD;

    // Exit 2 is clap refusing the command line. It is deterministic, so the
    // client will respawn into the identical failure - say so plainly, with
    // the arguments, because the user cannot see them anywhere else.
    if (code === 2) {
      process.stderr.write(
        `\ncodegraph-mcp: the engine rejected its arguments and exited 2.\n` +
          `  arguments: ${args.join(" ")}\n` +
          `  This is a configuration problem, not a crash. Check the "args" in\n` +
          `  your MCP client config - the wrapper already supplies --mcp.\n`
      );
    }
    if (looping) {
      process.stderr.write(
        `codegraph-mcp: failed ${failures} times in under a minute with the same\n` +
          `  arguments. Not reporting further failures for this configuration.\n`
      );
    }

    // Report the first failures, then one summary, then nothing. Without this
    // a single misconfigured machine sends a crash event every few seconds for
    // as long as its client keeps respawning - one sent 504,256.
    if (!looping) {
      sendTelemetry({
        event: "mcp.crash",
        exitCode: typeof code === "number" ? code : -1,
        exitSignal: signal || "none",
      });
    } else if (failures === LOOP_THRESHOLD) {
      sendTelemetry({
        event: "mcp.crashloop",
        exitCode: typeof code === "number" ? code : -1,
        exitSignal: signal || "none",
        failures,
      });
    }
    // Flush before exiting so the crash event isn't lost.
    flushAndExit(typeof code === "number" ? code : 1);
  } else if (signal) {
    process.kill(process.pid, signal);
  } else {
    flushAndExit(code ?? 1);
  }
});

for (const sig of ["SIGINT", "SIGTERM", "SIGHUP"]) {
  process.on(sig, () => {
    intentionalShutdown = true;
    child.kill(sig);
  });
}
