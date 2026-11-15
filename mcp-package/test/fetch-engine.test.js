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

const {
  ensureEngine,
  requiredAssets,
  platformBinaryName,
  PUBLISHED_BINARIES,
  redirectTarget,
  installedVersion,
  compareVersions,
  ENGINE_VERSION,
  EngineInUseError,
  WINDOWS_SIDECAR,
  VERSION_MARKER,
} = require("../bin/fetch-engine");

const VERSION = "0.20.0";
let failures = 0;

function check(ok, message) {
  console.log((ok ? "PASS " : "FAIL ") + message);
  if (!ok) failures++;
}

/**
 * A release server whose assets and checksums the test controls.
 *
 * With `redirect`, every asset is served one 302 away at a *relative*
 * `Location`, which is how real object storage answers and what the transports
 * cannot resolve on their own.
 */
function startRelease(assets, { redirect = false } = {}) {
  const routes = {};
  for (const [name, body] of Object.entries(assets)) {
    const content = Buffer.from(body.content);
    routes[`/v${VERSION}/${name}`] = content;
    const digest =
      body.checksum ?? crypto.createHash("sha256").update(content).digest("hex");
    routes[`/v${VERSION}/${name}.sha256`] = Buffer.from(`${digest}  ${name}\n`);
  }
  const server = http.createServer((req, res) => {
    const target = redirect ? req.url.replace(/^\/objects/, "") : req.url;
    const body = routes[target];
    if (!body) {
      res.writeHead(404);
      res.end();
      return;
    }
    if (redirect && target === req.url) {
      res.writeHead(302, { Location: `/objects${req.url}` });
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

  // --- an arm64 linux machine installs the arm64 engine ----------------
  // The whole install, not just the name mapping: before linux-arm64 was
  // published this threw, and the failure worth guarding against now is the
  // quiet one - the x64 asset is served alongside, so a rule that fell back to
  // it would install cleanly here and only fail when the user tried to run it.
  {
    const dir = scratch();
    const release = await startRelease({
      "codegraph-server-linux-arm64": { content: "arm64 engine" },
      "codegraph-server-linux-x64": { content: "x64 engine" },
    });
    try {
      const { binary, fetched } = await ensureEngine(VERSION, dir, {
        platform: "linux",
        arch: "arm64",
        baseUrl: release.baseUrl,
      });
      check(
        path.basename(binary) === "codegraph-server-linux-arm64",
        "an arm64 linux install fetches the arm64 engine"
      );
      check(fs.readFileSync(binary, "utf8") === "arm64 engine", "and it is the arm64 build");
      check(
        fetched.length === 1 && !fs.existsSync(path.join(dir, "codegraph-server-linux-x64")),
        "and nothing else, least of all the x64 build"
      );
      check(installedVersion(dir) === VERSION, "and the release it came from is recorded");
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

  // --- an already-present engine of the same version is not re-downloaded --
  {
    const dir = scratch();
    const name = platformBinaryName();
    for (const asset of requiredAssets()) fs.writeFileSync(path.join(dir, asset), "existing");
    fs.writeFileSync(path.join(dir, VERSION_MARKER), `${VERSION}\n`);
    // Serve nothing: any fetch attempt would 404 and fail the call.
    const { server, baseUrl } = await startRelease({});
    try {
      const { fetched } = await ensureEngine(VERSION, dir, { baseUrl });
      check(fetched.length === 0, "an existing install of the same version is left alone");
      check(fs.readFileSync(path.join(dir, name), "utf8") === "existing", "it is not overwritten");
    } finally {
      server.close();
      fs.rmSync(dir, { recursive: true, force: true });
    }
  }

  // --- an engine from an older client is replaced ----------------------
  // Resolving by filename alone is what let a client keep talking to the
  // engine a previous release installed, forever.
  {
    const dir = scratch();
    const name = platformBinaryName();
    for (const asset of requiredAssets()) fs.writeFileSync(path.join(dir, asset), "stale");
    fs.writeFileSync(path.join(dir, VERSION_MARKER), "0.19.1\n");

    const assets = { [name]: { content: "engine" } };
    for (const asset of requiredAssets().slice(1)) assets[asset] = { content: "sidecar" };
    const { server, baseUrl } = await startRelease(assets);
    try {
      const { binary, fetched } = await ensureEngine(VERSION, dir, { baseUrl });
      check(fetched.length === requiredAssets().length, "a version mismatch re-fetches every asset");
      check(fs.readFileSync(binary, "utf8") === "engine", "the stale engine is replaced");
      check(installedVersion(dir) === VERSION, "the installed version is recorded");
    } finally {
      server.close();
      fs.rmSync(dir, { recursive: true, force: true });
    }
  }

  // --- an engine from a newer client is left alone ---------------------
  // This directory is shared by the CLI, the VS Code extension and the
  // JetBrains plugin, which ship independently. Replacing a newer engine with
  // this client's older one is a downgrade the other client undoes on its next
  // launch, and the two then take turns forever.
  {
    const dir = scratch();
    const name = platformBinaryName();
    for (const asset of requiredAssets()) fs.writeFileSync(path.join(dir, asset), "newer");
    fs.writeFileSync(path.join(dir, VERSION_MARKER), "0.21.0\n");
    // Serve nothing: any fetch attempt would 404 and fail the call.
    const { server, baseUrl } = await startRelease({});
    try {
      const { fetched } = await ensureEngine(VERSION, dir, { baseUrl });
      check(fetched.length === 0, "a newer install is not downgraded");
      check(fs.readFileSync(path.join(dir, name), "utf8") === "newer", "its engine is untouched");
      check(installedVersion(dir) === "0.21.0", "and its version marker is left as it was");
    } finally {
      server.close();
      fs.rmSync(dir, { recursive: true, force: true });
    }
  }

  // --- an unmarked install is treated as unknown, not as current -------
  {
    const dir = scratch();
    for (const asset of requiredAssets()) fs.writeFileSync(path.join(dir, asset), "unmarked");
    check(installedVersion(dir) === null, "an install with no marker reports no version");
  }

  // --- release ordering -------------------------------------------------
  check(compareVersions("0.19.1", "0.20.0") === -1, "0.19.1 precedes 0.20.0");
  check(compareVersions("0.21.0", "0.20.0") === 1, "0.21.0 follows 0.20.0");
  check(compareVersions("0.20.0", "0.20.0") === 0, "a version equals itself");
  check(compareVersions("0.20", "0.20.0") === 0, "missing components read as zero");
  check(compareVersions("0.20.0-beta.1", "0.20.0") === 0, "a prerelease compares by its core");
  check(compareVersions("nightly", "0.20.0") === null, "an unparseable version has no order");

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

  // --- a transfer that dies after the headers ---------------------------
  // A half-finished download must reject, install nothing, and leave no
  // staged file behind. Silently keeping the truncated bytes would produce an
  // install that looks complete and fails at first use.
  {
    const dir = scratch();
    const name = platformBinaryName();
    const digest = crypto.createHash("sha256").update("engine").digest("hex");
    const server = http.createServer((req, res) => {
      if (req.url.endsWith(".sha256")) {
        res.writeHead(200);
        res.end(`${digest}  ${name}\n`);
        return;
      }
      // Promise far more than we send, then cut the connection.
      res.writeHead(200, { "Content-Length": 4096 });
      res.write("partial");
      res.socket.destroy();
    });
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    const baseUrl = `http://127.0.0.1:${server.address().port}`;
    try {
      let threw = null;
      await ensureEngine(VERSION, dir, { baseUrl }).catch((e) => (threw = e));
      check(threw !== null, "an aborted transfer rejects rather than throwing uncaught");
      check(!fs.existsSync(path.join(dir, name)), "an aborted transfer installs nothing");
      check(fs.readdirSync(dir).length === 0, "an aborted transfer leaves nothing behind");
    } finally {
      server.close();
      fs.rmSync(dir, { recursive: true, force: true });
    }
  }

  // --- a transfer that stops making progress ----------------------------
  // Neither transport times out on its own, so a server (or a proxy) that
  // goes quiet used to hang the update forever. Silence must reject and leave
  // nothing behind; a slow transfer that keeps delivering must still finish,
  // even when it takes far longer than the stall window in total.
  {
    const name = platformBinaryName();
    const content = "engine-".repeat(8);
    const digest = crypto.createHash("sha256").update(content).digest("hex");
    const stallTimeoutMs = 300;
    // The slow case gets a wider window and ticks at a tenth of it, so a
    // scheduling hiccup on a loaded machine cannot be mistaken for silence
    // while the transfer still takes several windows in total.
    const slowStallTimeoutMs = 500;
    const cases = [
      { label: "no response at all", serveBinary: () => {} },
      {
        label: "headers, then silence",
        serveBinary: (res) => {
          res.writeHead(200, { "Content-Length": content.length });
          res.write(content.slice(0, 4));
        },
      },
      {
        label: "a slow transfer that keeps delivering",
        slow: true,
        stallTimeoutMs: slowStallTimeoutMs,
        serveBinary: (res) => {
          res.writeHead(200, { "Content-Length": content.length });
          let sent = 0;
          const tick = setInterval(() => {
            res.write(content.slice(sent, sent + 2));
            sent += 2;
            if (sent >= content.length) {
              clearInterval(tick);
              res.end();
            }
          }, slowStallTimeoutMs / 10);
        },
      },
    ];
    for (const {
      label,
      serveBinary,
      slow = false,
      stallTimeoutMs: caseStallMs = stallTimeoutMs,
    } of cases) {
      const dir = scratch();
      const server = http.createServer((req, res) => {
        if (req.url.endsWith(".sha256")) {
          res.writeHead(200);
          res.end(`${digest}  ${name}\n`);
          return;
        }
        serveBinary(res);
      });
      await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
      const baseUrl = `http://127.0.0.1:${server.address().port}`;
      try {
        const started = Date.now();
        let threw = null;
        await ensureEngine(VERSION, dir, {
          baseUrl,
          stallTimeoutMs: caseStallMs,
          // The sidecar is not under test; serve every asset the same way.
          platform: "linux",
          arch: "x64",
        }).catch((e) => (threw = e));
        const elapsed = Date.now() - started;
        if (slow) {
          check(threw === null, `${label}: completes${threw ? ` (${threw.message})` : ""}`);
          check(elapsed > caseStallMs * 2, `${label}: outlasts the stall window in total`);
        } else {
          check(threw !== null && /no progress/.test(threw.message), `${label}: rejects as stalled`);
          check(elapsed < caseStallMs * 10, `${label}: gives up promptly (${elapsed} ms)`);
          check(fs.readdirSync(dir).length === 0, `${label}: leaves nothing behind`);
        }
      } finally {
        server.closeAllConnections();
        server.close();
        fs.rmSync(dir, { recursive: true, force: true });
      }
    }
  }

  // --- a failed transfer hangs up ---------------------------------------
  // The body does not stop arriving just because the download failed. While
  // the response keeps flowing, a settle that only disarmed the stall timer
  // let the next chunk arm a fresh one, so the rest of a ~120 MB transfer
  // still arrived and the process stayed alive for another stall window after
  // an install that had already failed - inside an npm postinstall, a hang.
  {
    const dir = scratch();
    const name = platformBinaryName();
    const stallTimeoutMs = 300;
    const digest = crypto.createHash("sha256").update("engine").digest("hex");
    let closedAt = null;
    const server = http.createServer((req, res) => {
      if (req.url.endsWith(".sha256")) {
        res.writeHead(200);
        res.end(`${digest}  ${name}\n`);
        return;
      }
      // Promise far more than the client will take and keep delivering it:
      // only the client hanging up can end this transfer.
      res.writeHead(200, { "Content-Length": 1 << 20 });
      const tick = setInterval(() => res.write("engine"), stallTimeoutMs / 10);
      res.on("close", () => {
        clearInterval(tick);
        if (closedAt === null) closedAt = Date.now();
      });
    });
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    const baseUrl = `http://127.0.0.1:${server.address().port}`;
    // A staged file that cannot be opened is the reachable form of a failure
    // that arrives mid-body: the write stream errors while bytes keep coming.
    fs.chmodSync(dir, 0o500);
    let readOnly = false;
    try {
      fs.accessSync(dir, fs.constants.W_OK);
    } catch {
      readOnly = true;
    }
    try {
      if (!readOnly) {
        console.log("SKIP a failed transfer hangs up (cannot make a directory read-only here)");
      } else {
        let threw = null;
        await ensureEngine(VERSION, dir, {
          baseUrl,
          stallTimeoutMs,
          platform: "linux",
          arch: "x64",
        }).catch((e) => (threw = e));
        const failedAt = Date.now();
        check(threw !== null, "an unwritable staging directory fails the install");
        const deadline = failedAt + stallTimeoutMs * 3;
        while (closedAt === null && Date.now() < deadline) {
          await new Promise((resolve) => setTimeout(resolve, 10));
        }
        check(
          closedAt !== null && closedAt - failedAt < stallTimeoutMs,
          `a failed transfer hangs up at once (${closedAt === null ? "still connected" : `${closedAt - failedAt} ms`})`
        );
      }
    } finally {
      fs.chmodSync(dir, 0o700);
      server.closeAllConnections();
      server.close();
      fs.rmSync(dir, { recursive: true, force: true });
    }
  }

  // --- a redirect hangs up too ------------------------------------------
  // A redirect's own body is never read, but it still arrives. Settling the
  // redirect without hanging up leaves a response open with nothing watching
  // it, which keeps an npm postinstall alive after a successful install.
  {
    const dir = scratch();
    const name = platformBinaryName();
    const stallTimeoutMs = 300;
    const digest = crypto.createHash("sha256").update("engine").digest("hex");
    let redirectsOpen = 0;
    const server = http.createServer((req, res) => {
      if (req.url.startsWith("/objects")) {
        const body = req.url.endsWith(".sha256") ? `${digest}  ${name}\n` : "engine";
        res.writeHead(200, { "Content-Length": Buffer.byteLength(body) });
        res.end(body);
        return;
      }
      // Promise a redirect body far larger than it ever delivers: only the
      // client hanging up can end this response.
      res.writeHead(302, {
        Location: `/objects${req.url}`,
        "Content-Length": 1 << 20,
      });
      redirectsOpen++;
      const tick = setInterval(() => res.write("redirect"), stallTimeoutMs / 10);
      res.on("close", () => {
        clearInterval(tick);
        redirectsOpen--;
      });
    });
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    const baseUrl = `http://127.0.0.1:${server.address().port}`;
    try {
      let threw = null;
      await ensureEngine(VERSION, dir, {
        baseUrl,
        stallTimeoutMs,
        platform: "linux",
        arch: "x64",
      }).catch((e) => (threw = e));
      check(threw === null, "an install behind a stalling redirect still completes");
      const installedAt = Date.now();
      const deadline = installedAt + stallTimeoutMs * 3;
      while (redirectsOpen > 0 && Date.now() < deadline) {
        await new Promise((resolve) => setTimeout(resolve, 10));
      }
      check(
        redirectsOpen === 0 && Date.now() - installedAt < stallTimeoutMs,
        `a followed redirect hangs up at once (${redirectsOpen} left open)`
      );
    } finally {
      server.closeAllConnections();
      server.close();
      fs.rmSync(dir, { recursive: true, force: true });
    }
  }

  // --- nothing is installed until everything is verified ----------------
  // Installing each asset as its download finishes is how a Windows install
  // ends up with a new engine beside the old onnxruntime.dll: that combination
  // downloads cleanly and then fails at startup.
  {
    const dir = scratch();
    const name = platformBinaryName();
    const assets = { [name]: { content: "engine" } };
    // Serve the engine but not the sidecar, so the second fetch 404s. On
    // platforms with no sidecar, corrupt the engine instead.
    const sidecars = requiredAssets().slice(1);
    if (sidecars.length === 0) assets[name].checksum = "0".repeat(64);

    const { server, baseUrl } = await startRelease(assets);
    try {
      let threw = null;
      await ensureEngine(VERSION, dir, { baseUrl }).catch((e) => (threw = e));
      check(threw !== null, "a partial release fails rather than half-installing");
      check(!fs.existsSync(path.join(dir, name)), "the verified engine is not installed alone");
      check(fs.readdirSync(dir).length === 0, "and nothing is staged behind");
    } finally {
      server.close();
      fs.rmSync(dir, { recursive: true, force: true });
    }
  }

  // --- the engine is stopped before it is replaced ----------------------
  // A running engine holds its own binary open; on Windows the move fails
  // outright, and elsewhere the old process keeps serving while the version
  // marker records a build nobody runs. The hook fires once every asset is
  // verified, and only when there is something to install.
  {
    const dir = scratch();
    const name = platformBinaryName();
    for (const asset of requiredAssets()) fs.writeFileSync(path.join(dir, asset), "stale");
    fs.writeFileSync(path.join(dir, VERSION_MARKER), "0.19.1\n");

    const assets = { [name]: { content: "engine" } };
    for (const asset of requiredAssets().slice(1)) assets[asset] = { content: "sidecar" };
    const { server, baseUrl } = await startRelease(assets);
    try {
      const seen = [];
      await ensureEngine(VERSION, dir, {
        baseUrl,
        beforeInstall: async () => seen.push(fs.readFileSync(path.join(dir, name), "utf8")),
      });
      check(seen.length === 1, "the install hook runs exactly once");
      check(seen[0] === "stale", "it runs before the old engine is replaced");
      check(
        fs.readFileSync(path.join(dir, name), "utf8") === "engine",
        "and the new engine is in place afterwards"
      );

      const skipped = [];
      await ensureEngine(VERSION, dir, { baseUrl, beforeInstall: async () => skipped.push(1) });
      check(skipped.length === 0, "an up-to-date install does not stop the engine for nothing");
    } finally {
      server.close();
      fs.rmSync(dir, { recursive: true, force: true });
    }
  }

  // --- a locked binary is reported as such ------------------------------
  // Reporting "in use" as a download failure sends the user to debug a network
  // they have no problem with, and the update they cannot complete is then
  // re-offered on every activation.
  {
    const dir = scratch();
    const name = platformBinaryName();
    const assets = { [name]: { content: "engine" } };
    for (const asset of requiredAssets().slice(1)) assets[asset] = { content: "sidecar" };
    const { server, baseUrl } = await startRelease(assets);
    const realRename = fs.renameSync;
    try {
      fs.renameSync = (from, to) => {
        if (path.basename(to) === name) {
          const error = new Error("EBUSY: resource busy or locked");
          error.code = "EBUSY";
          throw error;
        }
        return realRename(from, to);
      };
      let threw = null;
      await ensureEngine(VERSION, dir, { baseUrl }).catch((e) => (threw = e));
      check(threw instanceof EngineInUseError, "a locked binary is reported as in use");
      check(/in use/i.test(threw.message), "and says so in the message");
    } finally {
      fs.renameSync = realRename;
      server.close();
      fs.rmSync(dir, { recursive: true, force: true });
    }
  }

  // --- the pinned engine release ---------------------------------------
  // The clients fetch this version, not their own package version: release
  // assets are tagged with the engine's version, so a client-only patch would
  // otherwise ask for a tag that was never published and get no engine at all.
  check(/^\d+\.\d+\.\d+/.test(ENGINE_VERSION), `a concrete engine version is pinned (${ENGINE_VERSION})`);

  // --- redirects are followed, but never off https ---------------------
  {
    const dir = scratch();
    const name = platformBinaryName();
    const assets = { [name]: { content: "engine" } };
    for (const a of requiredAssets().slice(1)) assets[a] = { content: "sidecar" };
    const { server, baseUrl } = await startRelease(assets, { redirect: true });
    try {
      const { binary } = await ensureEngine(VERSION, dir, { baseUrl });
      check(
        fs.readFileSync(binary, "utf8") === "engine",
        "an asset served behind a relative redirect still arrives"
      );
    } finally {
      server.close();
      fs.rmSync(dir, { recursive: true, force: true });
    }
  }

  // The binary and the checksum that verifies it travel the same hops, so a
  // redirect off https would let one party serve both and have them agree.
  check(
    redirectTarget("https://example.com/v1/engine", "https://cdn.example.net/engine") ===
      "https://cdn.example.net/engine",
    "an https redirect to https is followed"
  );
  let downgrade = null;
  try {
    redirectTarget("https://example.com/v1/engine", "http://cdn.example.net/engine");
  } catch (error) {
    downgrade = error;
  }
  check(downgrade !== null, "a redirect off https is refused rather than followed");
  check(
    redirectTarget("http://127.0.0.1:9/v1/engine", "http://127.0.0.1:9/objects/engine") ===
      "http://127.0.0.1:9/objects/engine",
    "an http origin - the test server - may stay http"
  );
  check(
    redirectTarget("https://example.com/v1/engine", "/objects/engine") ===
      "https://example.com/objects/engine",
    "a relative Location resolves against the URL that produced it"
  );

  // --- the windows sidecar rule ----------------------------------------
  check(
    requiredAssets("win32", "x64").includes(WINDOWS_SIDECAR),
    "windows requires the runtime library the engine loads"
  );
  check(
    !requiredAssets("linux", "x64").includes(WINDOWS_SIDECAR),
    "other platforms do not"
  );

  // --- only published platform/arch pairs resolve to an asset ----------
  // An x64 asset handed to an arm64 Linux machine downloads and chmods cleanly
  // and then fails to exec, which is far harder to read than "not published".
  // Linux arm64 now has its own build, so it must resolve to that one and never
  // to the x64 asset. Windows on ARM stays the exception: it emulates x64.
  check(
    platformBinaryName("linux", "arm64") === "codegraph-server-linux-arm64",
    "linux-arm64 resolves to its own engine, not the x64 one"
  );
  check(
    platformBinaryName("win32", "arm64") === "codegraph-server-win32-x64.exe",
    "win32-arm64 uses the x64 engine, which Windows emulates"
  );
  check(
    requiredAssets("win32", "arm64").includes(WINDOWS_SIDECAR),
    "and still needs the runtime library beside it"
  );
  check(
    platformBinaryName("darwin", "arm64") === "codegraph-server-darwin-arm64",
    "darwin-arm64 does"
  );
  check(platformBinaryName("linux", "x64") === "codegraph-server-linux-x64", "linux-x64 does");
  check(
    requiredAssets("linux", "arm64").length === 1 &&
      requiredAssets("linux", "arm64")[0] === "codegraph-server-linux-arm64",
    "linux-arm64 needs the engine and no sidecar"
  );
  // A platform with no build must still resolve to nothing rather than to
  // someone else's binary.
  check(platformBinaryName("linux", "riscv64") === null, "an unbuilt arch resolves to nothing");
  check(requiredAssets("linux", "riscv64").length === 0, "an unpublished pair needs no assets");
  // Every name the mapping can return has to be a name the release publishes,
  // or an install fetches a 404.
  for (const [p, a] of [
    ["darwin", "arm64"],
    ["darwin", "x64"],
    ["linux", "arm64"],
    ["linux", "x64"],
    ["win32", "x64"],
  ]) {
    const name = platformBinaryName(p, a);
    check(
      PUBLISHED_BINARIES.includes(name),
      `${p}-${a} resolves to a published asset (${name})`
    );
  }

  console.log("");
  console.log(`${failures} failure(s)`);
  process.exit(failures ? 1 : 0);
}

run().catch((err) => {
  console.error(err);
  process.exit(1);
});
