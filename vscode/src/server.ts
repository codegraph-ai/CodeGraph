// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

import * as os from 'os';
import * as path from 'path';
import * as fs from 'fs';
import * as vscode from 'vscode';
import { execSync } from 'child_process';
import { managedEnginePath, platformBinaryName } from './engineDownload';

export interface ServerInfo {
    path: string;
    edition: 'pro' | 'community';
}

/**
 * Get the path to the LSP server binary for the current platform.
 *
 * Resolution order:
 * 1. CodeGraph Pro binary (if installed)
 * 2. Community binary (packaged with extension)
 * 3. Development builds (cargo target dir)
 * 4. The engine downloaded into ~/.codegraph/bin, shared with the other clients
 */
export function getServerPath(context: vscode.ExtensionContext): ServerInfo {
    // Try pro binary first — check PATH and common locations
    const proBinary = findProBinary();
    if (proBinary) {
        return { path: proBinary, edition: 'pro' };
    }

    // Fall back to community binary
    const communityBinary = findCommunityBinary(context);
    return { path: communityBinary, edition: 'community' };
}

function findProBinary(): string | null {
    const platform = os.platform();
    const binaryName = platform === 'win32' ? 'codegraph-pro.exe' : 'codegraph-pro';

    // Check PATH
    try {
        const which = platform === 'win32' ? 'where' : 'which';
        const result = execSync(`${which} ${binaryName}`, { encoding: 'utf8', timeout: 2000 });
        const binPath = result.trim().split('\n')[0];
        if (binPath && fs.existsSync(binPath)) {
            return binPath;
        }
    } catch {
        // Not in PATH
    }

    // Check common install locations
    const home = os.homedir();
    const candidates = [
        path.join(home, '.codegraph-pro', 'bin', binaryName),
        path.join(home, '.local', 'bin', binaryName),
        `/usr/local/bin/${binaryName}`,
    ];

    for (const candidate of candidates) {
        if (fs.existsSync(candidate)) {
            return candidate;
        }
    }

    return null;
}

function findCommunityBinary(context: vscode.ExtensionContext): string {
    const platform = os.platform();

    // One place decides which platform gets which asset - engineDownload.ts,
    // which re-exports the rule the npm postinstall and this client share. A
    // second copy here is how a platform ends up resolving a name nothing ever
    // downloads, and it would silently defeat the update path, which compares a
    // resolved path against `managedEnginePath()`.
    const binaryName = platformBinaryName();

    // Packaged binary — only present in a VSIX built with binaries bundled.
    // The published VSIX no longer carries one; see engineDownload.ts.
    const packagedPath = binaryName
        ? context.asAbsolutePath(path.join('bin', binaryName))
        : null;
    if (packagedPath && fs.existsSync(packagedPath)) {
        return packagedPath;
    }

    // Cargo release build (development)
    const releasePath = context.asAbsolutePath(
        path.join('..', 'crates', 'codegraph-server', 'target', 'release', 'codegraph-server')
    );
    if (fs.existsSync(releasePath)) {
        return releasePath;
    }

    // Cargo workspace release build
    const wsReleasePath = context.asAbsolutePath(
        path.join('..', 'target', 'release', 'codegraph-server')
    );
    if (fs.existsSync(wsReleasePath)) {
        return wsReleasePath;
    }

    // Debug build
    const debugPath = context.asAbsolutePath(
        path.join('..', 'target', 'debug', 'codegraph-server')
    );
    if (fs.existsSync(debugPath)) {
        return debugPath;
    }

    // Windows variants
    if (platform === 'win32') {
        for (const p of [releasePath, wsReleasePath, debugPath]) {
            const exe = p + '.exe';
            if (fs.existsSync(exe)) {
                return exe;
            }
        }
    }

    // Engine downloaded on demand, shared with the CLI and the JetBrains
    // plugin so a user who installed via any channel is found by all of them.
    //
    // Last, and after the cargo paths on purpose. Those only exist in a source
    // checkout, so ordinary installs never reach past this point anyway, while
    // a contributor who has also downloaded an engine - through npm, the
    // JetBrains plugin, or an earlier prompt - would otherwise silently run
    // that one instead of the build they just made.
    const managedPath = managedEnginePath();
    if (managedPath && fs.existsSync(managedPath)) {
        return managedPath;
    }

    // A platform with no published engine reaches here too, once the cargo
    // paths have been tried: a contributor on such a machine builds their own,
    // and saying "not found" while pointing at the build command serves both
    // cases better than refusing outright.
    throw new Error(
        binaryName
            ? `CodeGraph server binary not found. Expected at: ${packagedPath}\n` +
              `For development, build with: cargo build --release -p codegraph-server`
            : `CodeGraph does not publish an engine for ${platform}-${os.arch()}.\n` +
              `Build one with: cargo build --release -p codegraph-server`
    );
}
