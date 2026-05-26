#!/usr/bin/env node
"use strict";

const { spawn } = require("child_process");
const path = require("path");
const os = require("os");
const fs = require("fs");

// ── PostHog telemetry (opt-out via CODEGRAPH_TELEMETRY=off) ──────────

const POSTHOG_KEY = "phc_pkWuLX7azFafdd7rqY4bfKhZ3aobCT9unTy9zSkXH3xB";
const POSTHOG_HOST = "https://us.posthog.com";
const TELEMETRY_ENABLED =
  (process.env.CODEGRAPH_TELEMETRY || "on").toLowerCase() !== "off";

let posthog = null;
let machineId = null;

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
        os: os.platform(),
        arch: os.arch(),
        nodeVersion: process.version,
      },
    });
  } catch {
    // Never block the server on telemetry failures
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
  const binaryName = getBinaryName();
  const binDir = __dirname;
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
const args = ["--mcp", ...process.argv.slice(2)];

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
  if (signal) {
    process.kill(process.pid, signal);
  } else {
    flushAndExit(code ?? 1);
  }
});

for (const sig of ["SIGINT", "SIGTERM", "SIGHUP"]) {
  process.on(sig, () => child.kill(sig));
}
