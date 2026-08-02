// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

"use strict";

/**
 * Fetches the CodeGraph engine for the current platform from its GitHub
 * release, verifying it against the published checksum.
 *
 * Every distribution channel used to carry all four platform binaries: the npm
 * package was 88 MB compressed and 498 MB unpacked, the VSIX 118 MB, and each
 * user could run exactly one of the four. The binaries are now published once
 * as release assets and each channel fetches only what it needs.
 *
 * This module is the single implementation of that contract for the JavaScript
 * channels - the npm postinstall and the VS Code extension - so the URL layout,
 * the checksum format and the Windows sidecar rule cannot drift between them.
 * The JetBrains plugin implements the same contract in Kotlin
 * (jetbrains/.../EngineDownloader.kt); the contract is documented in
 * scripts/publish-release-assets.sh, which produces the assets.
 */

const fs = require("fs");
const os = require("os");
const path = require("path");
const crypto = require("crypto");
const https = require("https");
const http = require("http");

/**
 * Pick the transport from the URL scheme. Release assets are always https;
 * this exists so the download path itself can be exercised against a local
 * server, rather than being the one part nothing covers.
 */
function transportFor(url) {
  return url.startsWith("http://") ? http : https;
}

const RELEASE_BASE = "https://github.com/codegraph-ai/CodeGraph/releases/download";

/** Windows loads this next to the executable; without it the engine cannot start. */
const WINDOWS_SIDECAR = "onnxruntime.dll";

const PLATFORM_MAP = { darwin: "darwin", linux: "linux", win32: "win32" };
const ARCH_MAP = { arm64: "arm64", x64: "x64", x86_64: "x64" };

/**
 * Asset name for the running platform, matching the names
 * publish-release-assets.sh uploads. Returns null when unsupported, so callers
 * can degrade instead of throwing during an install.
 */
function platformBinaryName(platform = os.platform(), arch = os.arch()) {
  const p = PLATFORM_MAP[platform];
  const a = ARCH_MAP[arch];
  if (!p || !a) return null;
  // Only x64 is published for Windows and Linux today.
  if (p === "win32") return "codegraph-server-win32-x64.exe";
  if (p === "linux") return "codegraph-server-linux-x64";
  return `codegraph-server-darwin-${a}`;
}

/** Everything this platform needs on disk, in the order it should be fetched. */
function requiredAssets(platform = os.platform(), arch = os.arch()) {
  const binary = platformBinaryName(platform, arch);
  if (!binary) return [];
  // The sidecar is not optional: fetching only the executable produces an
  // install that succeeds and then fails at startup.
  return PLATFORM_MAP[platform] === "win32" ? [binary, WINDOWS_SIDECAR] : [binary];
}

function download(url, destination, { redirects = 5 } = {}) {
  return new Promise((resolve, reject) => {
    if (redirects < 0) return reject(new Error(`too many redirects for ${url}`));
    transportFor(url)
      .get(url, { headers: { "User-Agent": "codegraph-installer" } }, (res) => {
        // GitHub release assets always redirect to object storage.
        if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
          res.resume();
          return resolve(download(res.headers.location, destination, { redirects: redirects - 1 }));
        }
        if (res.statusCode !== 200) {
          res.resume();
          return reject(new Error(`HTTP ${res.statusCode} for ${url}`));
        }
        const file = fs.createWriteStream(destination);
        res.pipe(file);
        file.on("finish", () => file.close(resolve));
        file.on("error", reject);
      })
      .on("error", reject);
  });
}

function readText(url, options = {}) {
  const redirects = options.redirects === undefined ? 5 : options.redirects;
  return new Promise((resolve, reject) => {
    if (redirects < 0) return reject(new Error(`too many redirects for ${url}`));
    transportFor(url)
      .get(url, { headers: { "User-Agent": "codegraph-installer" } }, (res) => {
        if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
          res.resume();
          return resolve(readText(res.headers.location, { redirects: redirects - 1 }));
        }
        if (res.statusCode !== 200) {
          res.resume();
          return reject(new Error(`HTTP ${res.statusCode} for ${url}`));
        }
        let body = "";
        res.setEncoding("utf8");
        res.on("data", (chunk) => (body += chunk));
        res.on("end", () => resolve(body));
      })
      .on("error", reject);
  });
}

function sha256(file) {
  return new Promise((resolve, reject) => {
    const hash = crypto.createHash("sha256");
    fs.createReadStream(file)
      .on("data", (chunk) => hash.update(chunk))
      .on("end", () => resolve(hash.digest("hex")))
      .on("error", reject);
  });
}

/**
 * Fetch one asset into targetDir, verified.
 *
 * Staged first and moved into place only once the checksum matches, so an
 * interrupted or corrupted download can never leave behind something that
 * later looks like a valid install.
 */
async function fetchVerified(asset, version, targetDir, { baseUrl = RELEASE_BASE } = {}) {
  const assetUrl = `${baseUrl}/v${version}/${asset}`;
  const expected = (await readText(`${assetUrl}.sha256`)).trim().split(/\s+/)[0].toLowerCase();

  const staged = path.join(targetDir, `.${asset}.partial`);
  try {
    await download(assetUrl, staged);
    const actual = await sha256(staged);
    if (actual.toLowerCase() !== expected) {
      throw new Error(
        `${asset} failed checksum verification (expected ${expected}, got ${actual})`
      );
    }
    fs.renameSync(staged, path.join(targetDir, asset));
  } finally {
    if (fs.existsSync(staged)) fs.unlinkSync(staged);
  }
}

/**
 * Ensure the engine for this platform is present in targetDir.
 *
 * @returns {Promise<{binary: string, fetched: string[]}>} path to the engine
 *   and which assets were downloaded (empty when everything was already there).
 */
async function ensureEngine(version, targetDir, options = {}) {
  const assets = requiredAssets(options.platform, options.arch);
  if (assets.length === 0) {
    throw new Error(`no CodeGraph engine is published for ${os.platform()}-${os.arch()}`);
  }

  fs.mkdirSync(targetDir, { recursive: true });

  const fetched = [];
  for (const asset of assets) {
    const destination = path.join(targetDir, asset);
    if (fs.existsSync(destination) && !options.force) continue;
    if (options.onProgress) options.onProgress(asset);
    await fetchVerified(asset, version, targetDir, options);
    fetched.push(asset);
  }

  const binary = path.join(targetDir, assets[0]);
  if (os.platform() !== "win32") {
    try {
      fs.chmodSync(binary, 0o755);
    } catch {
      // A read-only install location is the user's to fix; the download itself
      // succeeded and reporting a chmod failure as a download failure misleads.
    }
  }
  return { binary, fetched };
}

module.exports = {
  RELEASE_BASE,
  WINDOWS_SIDECAR,
  platformBinaryName,
  requiredAssets,
  ensureEngine,
  fetchVerified,
  sha256,
};
