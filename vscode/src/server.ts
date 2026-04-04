// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

import * as os from 'os';
import * as path from 'path';
import * as fs from 'fs';
import * as vscode from 'vscode';
import { execSync } from 'child_process';

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
    const arch = os.arch();

    let binaryName: string;
    switch (platform) {
        case 'linux':
            binaryName = 'codegraph-server-linux-x64';
            break;
        case 'darwin':
            binaryName = arch === 'arm64'
                ? 'codegraph-server-darwin-arm64'
                : 'codegraph-server-darwin-x64';
            break;
        case 'win32':
            binaryName = 'codegraph-server-win32-x64.exe';
            break;
        default:
            throw new Error(`Unsupported platform: ${platform}`);
    }

    // Packaged binary (production)
    const packagedPath = context.asAbsolutePath(path.join('bin', binaryName));
    if (fs.existsSync(packagedPath)) {
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

    throw new Error(
        `CodeGraph server binary not found. Expected at: ${packagedPath}\n` +
        `For development, build with: cargo build --release -p codegraph-server`
    );
}
