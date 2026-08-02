#!/usr/bin/env node
// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

"use strict";

/**
 * Serves a fake release over loopback and downloads from it.
 *
 * Run with `node test/fetch-engine.test.js`. No test framework: this package
 * has no dev dependencies and adding one to check a download would be a poor
 * trade.
 *
 * The cases worth covering are the destructive ones. A corrupted transfer must
 * install nothing and leave nothing behind, and Windows must never end up with
 * an engine and no `onnxruntime.dll` - that combination downloads cleanly and
 * then fails at startup, which is harder to diagnose than an obvious failure.
 */

const crypto = require("crypto");
const fs = require("fs");
const http = require("http");
const os = require("os");
const path = require("path");

const { ensureEngine, requiredAssets, platformBinaryName, WINDOWS_SIDECAR } =
  require("../bin/fetch-engine");

const VERSION = "0.20.0";
let failures = 0;

function check(ok, message) {
  console.log((ok ? "PASS " : "FAIL ") + message);
  if (!ok) failures++;
}

/** A release server whose assets and checksums the test controls. */
function startRelease(assets) {
  const routes = {};
  for (const [name, body] of Object.entries(assets)) {
    const content = Buffer.from(body.content);
    routes[`/v${VERSION}/${name}`] = content;
    const digest =
      body.checksum ?? crypto.createHash("sha256").update(content).digest("hex");
    routes[`/v${VERSION}/${name}.sha256`] = Buffer.from(`${digest}  ${name}\n`);
  }
  const server = http.createServer((req, res) => {
    const body = routes[req.url];
    if (!body) {
      res.writeHead(404);
      res.end();
      return;
    }
    res.writeHead(200, { "Content-Length": body.length });
    res.end(body);
  });
  return new Promise((resolve) =>
    server.listen(0, "127.0.0.1", () =>
      resolve({ server, baseUrl: `http://127.0.0.1:${server.address().port}` })
    )
  );
}

function scratch() {
  return fs.mkdtempSync(path.join(os.tmpdir(), "codegraph-fetch-"));
}

async function run() {
  // --- a verified download installs the engine -------------------------
  {
    const dir = scratch();
    const name = platformBinaryName();
    const assets = { [name]: { content: "engine" } };
    for (const asset of requiredAssets().slice(1)) assets[asset] = { content: "sidecar" };

    const release = await startRelease(assets);
    try {
      const { binary, fetched } = await ensureEngine(VERSION, dir, { baseUrl: release.baseUrl });
      check(fs.readFileSync(binary, "utf8") === "engine", "engine content is what the release served");
      check(fetched.length === requiredAssets().length, "every required asset was fetched");
      for (const asset of requiredAssets()) {
        check(fs.existsSync(path.join(dir, asset)), `${asset} is installed`);
      }
    } finally {
      release.server.close();
      fs.rmSync(dir, { recursive: true, force: true });
    }
  }

  // --- a corrupted download installs nothing ---------------------------
  {
    const dir = scratch();
    const name = platformBinaryName();
    const assets = { [name]: { content: "engine", checksum: "0".repeat(64) } };
    for (const a of requiredAssets().slice(1)) assets[a] = { content: "sidecar" };
    const { server, baseUrl } = await startRelease(assets);
    try {
      let threw = null;
      await ensureEngine(VERSION, dir, { baseUrl }).catch((e) => (threw = e));
      check(threw !== null && /checksum/i.test(threw.message), "a checksum mismatch is reported");
      check(!fs.existsSync(path.join(dir, name)), "a mismatched engine is not installed");
      const leftovers = fs.readdirSync(dir);
      check(leftovers.length === 0, `nothing is left behind (found ${JSON.stringify(leftovers)})`);
    } finally {
      server.close();
      fs.rmSync(dir, { recursive: true, force: true });
    }
  }

  // --- an already-present engine is not re-downloaded ------------------
  {
    const dir = scratch();
    const name = platformBinaryName();
    for (const asset of requiredAssets()) fs.writeFileSync(path.join(dir, asset), "existing");
    // Serve nothing: any fetch attempt would 404 and fail the call.
    const { server, baseUrl } = await startRelease({});
    try {
      const { fetched } = await ensureEngine(VERSION, dir, { baseUrl });
      check(fetched.length === 0, "an existing install is left alone");
      check(fs.readFileSync(path.join(dir, name), "utf8") === "existing", "it is not overwritten");
    } finally {
      server.close();
      fs.rmSync(dir, { recursive: true, force: true });
    }
  }

  // --- a missing release fails loudly ----------------------------------
  {
    const dir = scratch();
    const { server, baseUrl } = await startRelease({});
    try {
      let threw = null;
      await ensureEngine(VERSION, dir, { baseUrl }).catch((e) => (threw = e));
      check(threw !== null, "a missing asset fails rather than reporting success");
      check(fs.readdirSync(dir).length === 0, "nothing is left behind after a failure");
    } finally {
      server.close();
      fs.rmSync(dir, { recursive: true, force: true });
    }
  }

  // --- the windows sidecar rule ----------------------------------------
  check(
    requiredAssets("win32", "x64").includes(WINDOWS_SIDECAR),
    "windows requires the runtime library the engine loads"
  );
  check(
    !requiredAssets("linux", "x64").includes(WINDOWS_SIDECAR),
    "other platforms do not"
  );

  console.log("");
  console.log(`${failures} failure(s)`);
  process.exit(failures ? 1 : 0);
}

run().catch((err) => {
  console.error(err);
  process.exit(1);
});
