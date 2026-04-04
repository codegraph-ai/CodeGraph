#!/usr/bin/env node
"use strict";

const path = require("path");
const os = require("os");
const fs = require("fs");
const { execFileSync } = require("child_process");

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

const platform = PLATFORM_MAP[os.platform()];
const arch = ARCH_MAP[os.arch()];

if (!platform || !arch) {
  console.warn(
    `⚠ codegraph-mcp: unsupported platform ${os.platform()}-${os.arch()}`
  );
  process.exit(0);
}

const ext = platform === "win32" ? ".exe" : "";
const binaryName = `codegraph-server-${platform}-${arch}${ext}`;
const binaryPath = path.join(__dirname, binaryName);

if (!fs.existsSync(binaryPath)) {
  console.warn(`⚠ codegraph-mcp: binary not found for ${platform}-${arch}`);
  console.warn(`  Expected: ${binaryPath}`);
  process.exit(0);
}

if (platform !== "win32") {
  try {
    fs.chmodSync(binaryPath, 0o755);
  } catch {
    // Ignore permission errors
  }
}

try {
  const output = execFileSync(binaryPath, ["--info"], {
    timeout: 10000,
    encoding: "utf8",
  });
  console.log(`✓ codegraph-mcp installed: ${output.trim().split("\n")[0]}`);
} catch (err) {
  console.warn(`⚠ codegraph-mcp: binary exists but --info check failed`);
  console.warn(`  ${err.message}`);
}
