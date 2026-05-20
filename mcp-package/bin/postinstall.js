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

// Hint about the optional Claude Code hook. Installation is opt-in to avoid
// silently modifying the user's ~/.claude/settings.json. Both Unix
// (bash) and Windows (PowerShell) variants are shipped — the installer
// picks the right one for the current OS.
{
  const scriptName =
    platform === "win32" ? "codegraph-pre-edit.ps1" : "codegraph-pre-edit.sh";
  const hookScriptPath = path.join(__dirname, "..", "hooks", scriptName);
  if (fs.existsSync(hookScriptPath)) {
    console.log("");
    console.log("ℹ Optional: enable automatic context injection in Claude Code:");
    console.log("    npx codegraph-mcp-install-hooks");
    console.log("  Adds a PreToolUse hook that nudges agents to fetch graph context");
    console.log("  before Edit/Write on source files. Idempotent, opt-out via --uninstall.");
  }
}
