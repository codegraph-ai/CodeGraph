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
 * Records which release the engines in a directory came from.
 *
 * Without it a managed install is identified by filename alone, so an engine
 * left behind by an older client is indistinguishable from the one this client
 * was built against and gets reused forever.
 */
const VERSION_MARKER = ".engine-version";

/**
 * Asset name for the running platform, matching the names
 * publish-release-assets.sh uploads. Returns null when unsupported, so callers
 * can degrade instead of throwing during an install.
 */
function platformBinaryName(platform = os.platform(), arch = os.arch()) {
  const p = PLATFORM_MAP[platform];
  const a = ARCH_MAP[arch];
  if (!p || !a) return null;
  // macOS is the only platform published for both architectures.
  if (p === "darwin") return `codegraph-server-darwin-${a}`;
  // Windows on ARM runs x64 executables under the OS's own emulation layer, so
  // the x64 asset is the correct answer there and refusing it would leave those
  // users with no engine at all.
  if (p === "win32") return "codegraph-server-win32-x64.exe";
  // Linux has no such layer. Handing the x64 build to an arm64 machine installs
  // ~30 MB that cannot execute, which surfaces as an exec-format error at first
  // use rather than as the unsupported platform it is.
  return a === "x64" ? "codegraph-server-linux-x64" : null;
}

/** Numeric release components, or null when [version] is not one. */
function versionParts(version) {
  const core = String(version || "").trim().split("-")[0];
  if (!core) return null;
  const parts = core.split(".").map((part) => Number.parseInt(part, 10));
  return parts.every((part) => Number.isInteger(part)) ? parts : null;
}

/**
 * Release order of two versions: -1, 0 or 1, and null when either side is not a
 * plain numeric version.
 *
 * Callers need "older" rather than "different". `~/.codegraph/bin` is shared by
 * the CLI, the VS Code extension and the JetBrains plugin, which ship on
 * independent schedules; treating any difference as staleness makes each client
 * reinstall its own engine over the other's on every launch, forever.
 */
function compareVersions(a, b) {
  const left = versionParts(a);
  const right = versionParts(b);
  if (!left || !right) return null;
  for (let i = 0; i < Math.max(left.length, right.length); i++) {
    const difference = (left[i] || 0) - (right[i] || 0);
    if (difference !== 0) return difference < 0 ? -1 : 1;
  }
  return 0;
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
    let file = null;
    /**
     * A transfer can die on the request (a socket reset), on the response (a
     * message destroyed after the headers), or on the write stream (a full
     * disk). All three must reject rather than throw uncaught - inside an npm
     * postinstall that is the difference between a warning and a failed
     * install - and all three must close the write stream, because a staged
     * file with a live handle on it cannot be unlinked on Windows.
     */
    const fail = (error) => {
      if (file) file.destroy();
      reject(error);
    };
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
        file = fs.createWriteStream(destination);
        res.on("error", fail);
        file.on("error", fail);
        file.on("finish", () => file.close(resolve));
        res.pipe(file);
      })
      .on("error", fail);
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
 *
 * The staged name is unique per call. This directory is shared, and the
 * download is no longer one deliberate click: several VS Code windows can
 * activate at once and each would otherwise truncate, hash and unlink the same
 * `.partial` file, producing a spurious checksum failure or - worse - promoting
 * a half-written engine into place.
 */
async function fetchVerified(asset, version, targetDir, { baseUrl = RELEASE_BASE } = {}) {
  const assetUrl = `${baseUrl}/v${version}/${asset}`;
  const expected = (await readText(`${assetUrl}.sha256`)).trim().split(/\s+/)[0].toLowerCase();

  const stamp = `${process.pid}.${crypto.randomBytes(6).toString("hex")}`;
  const staged = path.join(targetDir, `.${asset}.${stamp}.partial`);
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
 * Which release the engines in targetDir came from, or null when unknown -
 * either nothing is installed, or it predates the marker.
 */
function installedVersion(targetDir) {
  try {
    return fs.readFileSync(path.join(targetDir, VERSION_MARKER), "utf8").trim() || null;
  } catch {
    return null;
  }
}

/**
 * True when the engine installed in targetDir predates [version] and should be
 * replaced. An unmarked install counts as stale: it predates the marker, so
 * which build it is cannot be established.
 *
 * A *newer* engine is left alone. All three clients share this directory and
 * ship independently, so one of them finding a newer engine is normal, and
 * replacing it with its own older one only starts a downgrade war the other
 * client undoes on its next launch.
 */
function isStale(targetDir, version) {
  const installed = installedVersion(targetDir);
  if (installed === null) return true;
  const order = compareVersions(installed, version);
  return order === null ? true : order < 0;
}

/**
 * Ensure the engine for this platform is present in targetDir, at [version].
 *
 * A binary left by an older client is replaced rather than reused: clients ship
 * in lockstep with the engine they were built against, so "a file with the
 * right name exists" is not the same question as "the right engine is here".
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

  const stale = isStale(targetDir, version);
  const fetched = [];
  for (const asset of assets) {
    const destination = path.join(targetDir, asset);
    if (fs.existsSync(destination) && !options.force && !stale) continue;
    if (options.onProgress) options.onProgress(asset);
    await fetchVerified(asset, version, targetDir, options);
    fetched.push(asset);
  }
  // Written last, and only when this call put the engine there: a marker
  // recorded before the assets are verified would claim an install that a later
  // failure never completed, and one recorded over an untouched newer install
  // would mislabel someone else's engine as ours.
  if (fetched.length > 0 || installedVersion(targetDir) === null) {
    fs.writeFileSync(path.join(targetDir, VERSION_MARKER), `${version}\n`);
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
  VERSION_MARKER,
  platformBinaryName,
  requiredAssets,
  installedVersion,
  compareVersions,
  isStale,
  ensureEngine,
  fetchVerified,
  sha256,
};
