// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Fetches the engine for this platform when the VSIX does not carry one.
//!
//! The VSIX used to bundle all four platform binaries (118 MB, of which a user
//! can run one). The binaries are now published once as GitHub release assets
//! and each channel fetches only what it needs — the npm package does this in
//! its postinstall, the JetBrains plugin in Kotlin, and this is the VS Code
//! half. A VSIX has no install hook, so the fetch happens on first activation.
//!
//! The download contract — URL layout, checksum file format, and the Windows
//! sidecar rule — is shared with `mcp-package/bin/fetch-engine.js`, which this
//! module re-exports rather than reimplements, so the three clients cannot
//! disagree about where the engine lives or how it is verified.

import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import * as vscode from 'vscode';

// The canonical implementation lives with the npm package; esbuild follows the
// path and inlines it into out/extension.js, so both JavaScript channels ship
// the same code rather than two implementations that drift.
// eslint-disable-next-line @typescript-eslint/no-var-requires
const fetchEngine = require('../../mcp-package/bin/fetch-engine.js');

/** Where downloaded engines live, shared with the CLI and the JetBrains plugin. */
export function managedInstallDir(): string {
    return path.join(os.homedir(), '.codegraph', 'bin');
}

/** The engine asset for this platform, or null when none is published. */
export function platformBinaryName(): string | null {
    return fetchEngine.platformBinaryName();
}

/** Path the engine would occupy once downloaded, or null on an unsupported platform. */
export function managedEnginePath(): string | null {
    const name = platformBinaryName();
    return name ? path.join(managedInstallDir(), name) : null;
}

/**
 * Download the engine for [version], reporting progress in the notification
 * area.
 *
 * Offered rather than automatic: this pulls a native binary that runs with the
 * user's permissions, and doing that unasked on first activation is not the
 * extension's decision to make.
 */
export async function downloadEngine(version: string): Promise<string> {
    return vscode.window.withProgress(
        {
            location: vscode.ProgressLocation.Notification,
            title: `Downloading the CodeGraph engine ${version}`,
            cancellable: false,
        },
        async (progress) => {
            const { binary } = await fetchEngine.ensureEngine(version, managedInstallDir(), {
                onProgress: (asset: string) => progress.report({ message: asset }),
            });
            return binary as string;
        },
    );
}

/**
 * Offer to replace a managed engine that predates this extension release.
 *
 * The managed engine is resolved by filename, so without this an engine left
 * behind by a previous release is found and reused indefinitely and a client
 * built against a newer engine keeps talking to the old one.
 *
 * Offered rather than done, for the same reason `downloadEngine` is: this is
 * ~30 MB of native binary that will run with the user's permissions. It is also
 * deliberately not awaited by the caller - the engine on disk still works, so
 * blocking activation behind a transfer would cost every surface the extension
 * provides for an update that is not urgent.
 *
 * Only an *older* engine counts. `~/.codegraph/bin` is shared with the CLI and
 * the JetBrains plugin, which ship on their own schedules; treating a newer
 * engine as a mismatch would have the two clients reinstall over each other on
 * every launch.
 */
export async function offerEngineUpdateIfStale(version: string): Promise<void> {
    const engine = managedEnginePath();
    if (!engine || !fs.existsSync(engine)) {
        return;
    }
    if (!fetchEngine.isStale(managedInstallDir(), version)) {
        return;
    }

    const installed = fetchEngine.installedVersion(managedInstallDir()) ?? 'an unknown version';
    const choice = await vscode.window.showInformationMessage(
        `The installed CodeGraph engine (${installed}) predates this extension (${version}). ` +
        'They ship together, so features this build expects may be missing.',
        'Update',
        'Not Now',
    );
    if (choice !== 'Update') {
        return;
    }

    try {
        await downloadEngine(version);
        vscode.window.showInformationMessage(
            `CodeGraph engine ${version} installed. It takes effect the next time the engine starts.`,
        );
    } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        // Not fatal: the engine already on disk still runs, and losing a working
        // install over one version of drift is worse than the drift.
        vscode.window.showWarningMessage(
            `Could not update the CodeGraph engine to ${version}: ${message}`,
        );
    }
}

/**
 * Ask whether to download, then do it.
 *
 * Returns the engine path, or null if the user declined or it failed — callers
 * treat that as "no engine", which is the same state they already handle.
 */
export async function offerEngineDownload(version: string): Promise<string | null> {
    if (!platformBinaryName()) {
        vscode.window.showErrorMessage(
            `CodeGraph does not publish an engine for ${os.platform()}-${os.arch()}. ` +
            'Point the extension at your own build with the codegraph.serverPath setting.',
        );
        return null;
    }

    const choice = await vscode.window.showInformationMessage(
        'CodeGraph needs its analysis engine, which is downloaded separately for your platform.',
        'Download',
        'Not Now',
    );
    if (choice !== 'Download') {
        return null;
    }

    try {
        const binary = await downloadEngine(version);
        vscode.window.showInformationMessage('CodeGraph engine installed.');
        return binary;
    } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        // A checksum failure is not a network failure, and saying so matters:
        // one is worth retrying, the other means something served the wrong
        // bytes.
        vscode.window.showErrorMessage(
            /checksum/i.test(message)
                ? `The downloaded engine failed verification and was discarded: ${message}`
                : `Could not download the CodeGraph engine: ${message}`,
        );
        return null;
    }
}
