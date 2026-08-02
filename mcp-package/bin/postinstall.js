#!/usr/bin/env node
"use strict";

const path = require("path");
const os = require("os");
const fs = require("fs");
const { execFileSync } = require("child_process");

const { ensureEngine, platformBinaryName } = require("./fetch-engine");

const platform = os.platform();
const arch = os.arch();

// One place decides which platforms have a published engine: fetch-engine.js,
// which also names the asset. A second copy of that rule here is how a platform
// with no build ends up downloading someone else's binary.
const binaryName = platformBinaryName();

if (!binaryName) {
  console.warn(`⚠ codegraph-mcp: unsupported platform ${platform}-${arch}`);
  process.exit(0);
}

const binaryPath = path.join(__dirname, binaryName);
const version = require("../package.json").version;

// The engine is fetched rather than bundled. Shipping all four platform
// binaries made this package 88 MB compressed and 498 MB unpacked so that every
// user could run exactly one of them. Fetching keeps the path identical -
// `<pkg>/bin/codegraph-server-<platform>-<arch>` - which matters because
// consumers resolve it directly, the PR-review workflow among them.
//
// CODEGRAPH_SKIP_BINARY_FETCH exists for air-gapped installs and for anyone
// vendoring the binary themselves; the file already being present skips the
// fetch anyway.
(async () => {
  if (!fs.existsSync(binaryPath) && !process.env.CODEGRAPH_SKIP_BINARY_FETCH) {
    try {
      console.log(`codegraph-mcp: fetching engine ${version} for ${platform}-${arch}...`);
      const { fetched } = await ensureEngine(version, __dirname, {
        onProgress: (asset) => console.log(`  ↓ ${asset}`),
      });
      if (fetched.length > 0) console.log(`✓ codegraph-mcp: engine downloaded and verified`);
    } catch (err) {
      // Never fail the install over this: npm would roll back a package whose
      // CLI, hooks and docs are all perfectly usable, and the engine can still
      // be supplied by hand.
      console.warn(`⚠ codegraph-mcp: could not download the engine — ${err.message}`);
      console.warn(`  Retry with: npx codegraph-mcp-fetch-engine`);
      console.warn(`  Or set CODEGRAPH_SERVER_PATH to an engine you already have.`);
    }
  }

  if (!fs.existsSync(binaryPath)) {
    console.warn(`⚠ codegraph-mcp: no engine at ${binaryPath}`);
    return;
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

  // Fetch the distilled static embedding model (best-effort) from the
  // release-independent `model` GitHub release. Only needed for
  // `--embedding-model static`; skipped if already present or if
  // CODEGRAPH_SKIP_MODEL_FETCH is set. Never fails the install.
  if (!process.env.CODEGRAPH_SKIP_MODEL_FETCH) {
    const MODEL = "jina-code-static-256";
    const modelDir = path.join(os.homedir(), ".codegraph", "static_models", MODEL);
    if (!fs.existsSync(path.join(modelDir, "model.safetensors"))) {
      try {
        fs.mkdirSync(modelDir, { recursive: true });
        const url = `https://github.com/codegraph-ai/CodeGraph/releases/download/model/${MODEL}.tar.gz`;
        const tgz = path.join(modelDir, "_model.tar.gz");
        execFileSync("curl", ["-fsSL", url, "-o", tgz], { timeout: 180000 });
        execFileSync("tar", ["xzf", tgz, "-C", modelDir], { timeout: 60000 });
        fs.unlinkSync(tgz);
        console.log(`✓ codegraph-mcp: static embedding model ready (${modelDir})`);
      } catch {
        console.warn(
          `ℹ codegraph-mcp: static model not fetched (optional — only for --embedding-model static)`
        );
      }
    }
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
})();
