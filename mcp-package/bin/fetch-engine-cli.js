#!/usr/bin/env node
// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

"use strict";

/**
 * Fetch the CodeGraph engine on demand.
 *
 * The postinstall does this automatically, but never fails the install if it
 * cannot - a transient network problem should not roll back a package whose
 * CLI, hooks and docs all work. This is the retry, and the way to force a
 * re-download of an engine that was corrupted or replaced.
 */

const path = require("path");
const { ensureEngine, platformBinaryName, ENGINE_VERSION } = require("./fetch-engine");

const force = process.argv.includes("--force");
// The engine release this package ships against, not the package's own version:
// the release assets are tagged with the engine's version.
const version = ENGINE_VERSION;
const targetDir = __dirname;

const binaryName = platformBinaryName();
if (!binaryName) {
  console.error(`No CodeGraph engine is published for ${process.platform}-${process.arch}.`);
  process.exit(1);
}

console.log(`Fetching CodeGraph engine ${version} for ${process.platform}-${process.arch}`);

ensureEngine(version, targetDir, {
  force,
  onProgress: (asset) => console.log(`  ↓ ${asset}`),
})
  .then(({ binary, fetched }) => {
    if (fetched.length === 0) {
      console.log(`Already present at ${binary} (use --force to re-download)`);
    } else {
      console.log(`✓ Verified and installed: ${binary}`);
    }
  })
  .catch((err) => {
    console.error(`✗ ${err.message}`);
    console.error("");
    // The only supported hand-install is this exact path: codegraph-mcp
    // resolves the engine from its own bin directory and nowhere else.
    console.error("If this machine has no network access, supply the engine yourself:");
    console.error(`  - place it at ${path.join(targetDir, binaryName)}`);
    console.error("  - on Windows, put onnxruntime.dll in that directory too");
    process.exit(1);
  });
