#!/usr/bin/env node
"use strict";

const { spawn } = require("child_process");
const path = require("path");
const os = require("os");
const fs = require("fs");

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

const binaryPath = findBinary();

// Pass --mcp plus all user args to the binary
const args = ["--mcp", ...process.argv.slice(2)];

const child = spawn(binaryPath, args, {
  stdio: "inherit",
  env: process.env,
});

child.on("error", (err) => {
  console.error(`Failed to start codegraph-mcp: ${err.message}`);
  process.exit(1);
});

child.on("exit", (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
  } else {
    process.exit(code ?? 1);
  }
});

for (const sig of ["SIGINT", "SIGTERM", "SIGHUP"]) {
  process.on(sig, () => child.kill(sig));
}
