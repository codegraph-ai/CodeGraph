#!/usr/bin/env node
// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0
//
// Idempotent installer for the codegraph PreToolUse hook in
// ~/.claude/settings.json. Adds a hooks entry if absent, leaves it
// alone if already present (matched on the script path).
//
// Usage:
//   codegraph-mcp install-hooks           # interactive (asks confirmation)
//   codegraph-mcp install-hooks --force   # skip confirmation
//   codegraph-mcp install-hooks --dry     # print what would change, exit
//   codegraph-mcp install-hooks --uninstall  # remove the hook entry
//
// Exit codes:
//   0  success (or already-installed, or user declined)
//   1  IO error
//   2  user error (bad args)

"use strict";

const fs = require("fs");
const os = require("os");
const path = require("path");
const readline = require("readline");

const SETTINGS_PATH = path.join(os.homedir(), ".claude", "settings.json");

// Pick the right hook script per platform. Windows uses PowerShell,
// everything else uses bash. Both scripts mirror each other's behavior.
const IS_WINDOWS = os.platform() === "win32";
const HOOK_SCRIPT_NAME = IS_WINDOWS
  ? "codegraph-pre-edit.ps1"
  : "codegraph-pre-edit.sh";
const HOOK_SCRIPT_REL = path.join(__dirname, "..", "hooks", HOOK_SCRIPT_NAME);
const HOOK_SCRIPT_ABS = fs.existsSync(HOOK_SCRIPT_REL)
  ? fs.realpathSync(HOOK_SCRIPT_REL)
  : HOOK_SCRIPT_REL;

// On Windows the `command` field needs to invoke PowerShell explicitly
// because Claude Code can't run a `.ps1` file directly via cmd.exe.
// On Unix the bash shebang handles it; we just point at the script.
const HOOK_COMMAND = IS_WINDOWS
  ? `powershell.exe -NoProfile -ExecutionPolicy Bypass -File "${HOOK_SCRIPT_ABS}"`
  : HOOK_SCRIPT_ABS;

const args = process.argv.slice(2);
const FORCE = args.includes("--force");
const DRY = args.includes("--dry");
const UNINSTALL = args.includes("--uninstall");

function confirm(prompt) {
  if (FORCE) return Promise.resolve(true);
  if (!process.stdin.isTTY) {
    // Non-interactive: default to NO. User must pass --force.
    return Promise.resolve(false);
  }
  const rl = readline.createInterface({ input: process.stdin, output: process.stdout });
  return new Promise((resolve) => {
    rl.question(`${prompt} [y/N] `, (answer) => {
      rl.close();
      resolve(/^y(es)?$/i.test(answer.trim()));
    });
  });
}

function loadSettings() {
  if (!fs.existsSync(SETTINGS_PATH)) {
    return {};
  }
  try {
    return JSON.parse(fs.readFileSync(SETTINGS_PATH, "utf8"));
  } catch (err) {
    console.error(`✗ Failed to parse ${SETTINGS_PATH}: ${err.message}`);
    console.error(`  The file isn't valid JSON. Fix it manually before running this installer.`);
    process.exit(1);
  }
}

function findCodegraphHookEntry(matcherEntry) {
  // Returns the index of any pre-existing codegraph hook in matcherEntry.hooks,
  // or -1 if absent. Match on the command path containing
  // `codegraph-pre-edit.sh` OR `codegraph-pre-edit.ps1` so legacy / moved
  // installs don't get duplicated, and switching platforms (or upgrading
  // a Unix install on a Windows host) updates rather than appends.
  if (!matcherEntry || !Array.isArray(matcherEntry.hooks)) return -1;
  return matcherEntry.hooks.findIndex(
    (h) =>
      h &&
      typeof h.command === "string" &&
      (h.command.includes("codegraph-pre-edit.sh") ||
        h.command.includes("codegraph-pre-edit.ps1"))
  );
}

function installInto(settings) {
  if (!settings.hooks) settings.hooks = {};
  if (!Array.isArray(settings.hooks.PreToolUse)) settings.hooks.PreToolUse = [];

  // Find or create the matcher entry for Edit/Write/MultiEdit
  let matcherEntry = settings.hooks.PreToolUse.find(
    (e) => e && e.matcher === "Edit|Write|MultiEdit"
  );
  if (!matcherEntry) {
    matcherEntry = { matcher: "Edit|Write|MultiEdit", hooks: [] };
    settings.hooks.PreToolUse.push(matcherEntry);
  }

  const existingIdx = findCodegraphHookEntry(matcherEntry);
  const hookEntry = { type: "command", command: HOOK_COMMAND };

  if (existingIdx >= 0) {
    const existing = matcherEntry.hooks[existingIdx];
    if (existing.command === HOOK_COMMAND) {
      return { changed: false, reason: "already-installed" };
    }
    // Path differs — update it (handles npm reinstall to a different prefix,
    // or a Unix-installed entry being refreshed on a Windows host with the
    // PowerShell variant).
    matcherEntry.hooks[existingIdx] = hookEntry;
    return { changed: true, reason: "path-updated", oldPath: existing.command };
  }

  matcherEntry.hooks.push(hookEntry);
  return { changed: true, reason: "added" };
}

function uninstallFrom(settings) {
  if (!settings.hooks || !Array.isArray(settings.hooks.PreToolUse)) {
    return { changed: false, reason: "no-hooks-section" };
  }
  let removed = 0;
  for (const matcherEntry of settings.hooks.PreToolUse) {
    if (!matcherEntry || !Array.isArray(matcherEntry.hooks)) continue;
    const before = matcherEntry.hooks.length;
    matcherEntry.hooks = matcherEntry.hooks.filter(
      (h) =>
        !(
          h &&
          typeof h.command === "string" &&
          (h.command.includes("codegraph-pre-edit.sh") ||
            h.command.includes("codegraph-pre-edit.ps1"))
        )
    );
    removed += before - matcherEntry.hooks.length;
  }
  // Clean up empty matcher entries
  settings.hooks.PreToolUse = settings.hooks.PreToolUse.filter(
    (e) => e && Array.isArray(e.hooks) && e.hooks.length > 0
  );
  if (settings.hooks.PreToolUse.length === 0) {
    delete settings.hooks.PreToolUse;
  }
  if (Object.keys(settings.hooks).length === 0) {
    delete settings.hooks;
  }
  return { changed: removed > 0, reason: removed > 0 ? "removed" : "not-found", count: removed };
}

function writeSettings(settings) {
  const dir = path.dirname(SETTINGS_PATH);
  if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });
  // Preserve trailing newline + 2-space indent (Claude Code's convention)
  fs.writeFileSync(SETTINGS_PATH, JSON.stringify(settings, null, 2) + "\n", "utf8");
}

async function main() {
  if (!fs.existsSync(HOOK_SCRIPT_ABS)) {
    console.error(`✗ Hook script not found at ${HOOK_SCRIPT_ABS}`);
    console.error(`  This usually means the npm package is corrupted. Reinstall: npm i -g @astudioplus/codegraph-mcp`);
    process.exit(1);
  }

  const settings = loadSettings();
  const settingsBefore = JSON.stringify(settings);

  if (UNINSTALL) {
    const result = uninstallFrom(settings);
    if (!result.changed) {
      console.log(`ℹ  No codegraph hook installed. Nothing to remove.`);
      process.exit(0);
    }
    if (DRY) {
      console.log(`Would remove ${result.count} codegraph hook entry from ${SETTINGS_PATH}.`);
      process.exit(0);
    }
    if (!(await confirm(`Remove ${result.count} codegraph hook entry from ${SETTINGS_PATH}?`))) {
      console.log("Cancelled.");
      process.exit(0);
    }
    writeSettings(settings);
    console.log(`✓ Removed ${result.count} codegraph hook entry from ${SETTINGS_PATH}`);
    process.exit(0);
  }

  const result = installInto(settings);
  if (!result.changed) {
    console.log(`✓ codegraph hook already installed at ${SETTINGS_PATH}`);
    console.log(`  Hook script: ${HOOK_SCRIPT_ABS}`);
    process.exit(0);
  }

  if (DRY) {
    console.log(`Would ${result.reason === "added" ? "ADD" : "UPDATE"} codegraph hook in ${SETTINGS_PATH}:`);
    console.log(`  Matcher: Edit|Write|MultiEdit`);
    console.log(`  Command: ${HOOK_SCRIPT_ABS}`);
    if (result.oldPath) console.log(`  (replacing previous path: ${result.oldPath})`);
    console.log("");
    console.log("Resulting settings.json (relevant excerpt):");
    console.log(JSON.stringify({ hooks: settings.hooks }, null, 2));
    process.exit(0);
  }

  console.log("This will install the codegraph PreToolUse hook into:");
  console.log(`  ${SETTINGS_PATH}`);
  console.log("The hook adds a one-line context nudge before Edit/Write/MultiEdit on source files");
  console.log("in workspaces. It NEVER blocks tool calls and produces no output for non-source files.");
  console.log("");

  if (!(await confirm("Install?"))) {
    console.log("Cancelled. Run with --force to skip this prompt, or --uninstall to remove later.");
    process.exit(0);
  }

  writeSettings(settings);
  console.log(`✓ Installed codegraph hook at ${SETTINGS_PATH}`);
  console.log(`  Hook script: ${HOOK_SCRIPT_ABS}`);
  console.log("  Restart your Claude Code session to pick up the new hook.");
}

main().catch((err) => {
  console.error(`✗ ${err.message}`);
  process.exit(1);
});
