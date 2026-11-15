// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Fetches the engine for this platform when the VSIX does not carry one.
//!
//! The VSIX used to bundle all four platform binaries (118 MB, of which a user
//! can run one). The binaries are now published once as GitHub release assets
//! and each channel fetches only what it needs - the npm package does this in
//! its postinstall, the JetBrains plugin in Kotlin, and this is the VS Code
//! half. A VSIX has no install hook, so the fetch happens on first activation.
//!
//! The download contract - URL layout, checksum file format, and the Windows
//! sidecar rule - is shared with `mcp-package/bin/fetch-engine.js`, which this
//! module re-exports rather than reimplements, so the three clients cannot
//! disagree about where the engine lives or how it is verified.

import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import * as vscode from 'vscode';

// The canonical implementation lives with the npm package; esbuild follows the
// path and inlines it into out/extension.js, so both JavaScript channels ship
// the same code rather than two implementations that drift.
// eslint-disable-next-line @typescript-eslint/no-require-imports
const fetchEngine = require('../../mcp-package/bin/fetch-engine.js');

/** Where downloaded engines live, shared with the CLI and the JetBrains plugin. */
export function managedInstallDir(): string {
    return path.join(os.homedir(), '.codegraph', 'bin');
}

/**
 * The engine release this extension fetches.
 *
 * Not the extension's own version. Release assets are tagged with the engine's
 * version, so a VSIX-only patch - a UI fix, a doc change - would ask for
 * `v<extension version>/…` and get a 404 on every fresh install, which now
 * means no engine at all. It is also the number the shared `~/.codegraph/bin`
 * marker is compared against, so all three clients judge staleness by the same
 * yardstick instead of by three independently drifting ones.
 */
export function engineVersion(): string {
    return fetchEngine.ENGINE_VERSION as string;
}

/**
 * How the running engine is stopped and started around an in-place update.
 *
 * The engine holds its own binary open, so replacing it while it runs fails
 * outright on Windows and elsewhere leaves the old process serving requests
 * while the version marker records a build nobody is running.
 */
export interface EngineLifecycle {
    isRunning(): boolean;
    stop(): Promise<void>;
    start(): Promise<void>;
}

/** globalState key holding the last failed automatic update, as an `UpdateFailure`. */
const UPDATE_FAILURE_KEY = 'codegraph.engineUpdateFailure';

/** How long a failed automatic update of one engine version holds off the next attempt. */
export const UPDATE_RETRY_BACKOFF_MS = 24 * 60 * 60 * 1000;

/** The engine version an automatic update last failed to install, and when. */
export interface UpdateFailure {
    version: string;
    at: number;
}

/**
 * Whether the automatic update to [version] should wait out an earlier failure.
 *
 * Each attempt is a full ~120 MB transfer plus an engine stop and restart, and
 * the commonest failure - another window or an MCP client holding the binary -
 * outlasts many window activations. Only a failure of the same version counts:
 * a newer release deserves its own attempt straight away.
 */
export function shouldSkipAutomaticUpdate(
    failure: UpdateFailure | undefined,
    version: string,
    now: number,
): boolean {
    return failure?.version === version && now - failure.at < UPDATE_RETRY_BACKOFF_MS;
}

/** Forget a failed automatic update, once the engine has been installed some other way. */
export async function clearUpdateFailure(state: vscode.Memento): Promise<void> {
    if (state.get<UpdateFailure>(UPDATE_FAILURE_KEY)) {
        await state.update(UPDATE_FAILURE_KEY, undefined);
    }
}

/**
 * Stops a running engine at the last moment before its binary is replaced, and
 * restarts it afterwards whichever way the install ended.
 *
 * The engine holds its own binary open, so it is stopped once every asset is
 * downloaded and verified - stopping it for the length of a transfer that may
 * fail costs the user a working engine for nothing.
 */
function engineSwap(lifecycle: EngineLifecycle | undefined) {
    let stopped = false;
    return {
        beforeInstall: async () => {
            if (!stopped && lifecycle?.isRunning()) {
                stopped = true;
                await lifecycle.stop();
            }
        },
        restart: async () => {
            if (stopped && lifecycle) {
                stopped = false;
                await lifecycle.start();
            }
        },
    };
}

/**
 * Start the engine again once its binary has been replaced.
 *
 * The install is already done by this point, so a start failure is an outcome
 * of its own: reported as a failed update it would hold off this version's
 * automatic attempt for a day over an engine that is in fact installed, and
 * reported as a failed download it sends the user to debug a transfer that
 * worked. Nor may it propagate - the caller still has an installed engine to
 * record. Returns true when the engine was replaced but will not run, so the
 * caller does not also report the install as a plain success.
 */
async function restartAfterInstall(
    swap: ReturnType<typeof engineSwap>,
    version: string,
): Promise<boolean> {
    try {
        await swap.restart();
        return false;
    } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        void vscode.window.showWarningMessage(
            `The CodeGraph engine was updated to ${version} but did not start again: ${message}`,
        );
        return true;
    }
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
 * Automatic, not offered. 0.21.0 asked first, and 73% of the users it reached by
 * auto-update from 0.19.1 - which bundled the engine - ended without one: the
 * prompt is a toast, and a toast that is dismissed or never seen reads as "no".
 * Installing the extension is the consent; the binary comes from this project's
 * own release and is checksum-verified, exactly as the npm postinstall does.
 */
export async function downloadEngine(
    version: string,
    options: { beforeInstall?: () => Promise<void> } = {},
): Promise<string> {
    return vscode.window.withProgress(
        {
            location: vscode.ProgressLocation.Notification,
            title: `Downloading the CodeGraph engine ${version}`,
            cancellable: false,
        },
        async (progress) => {
            const { binary } = await fetchEngine.ensureEngine(version, managedInstallDir(), {
                onProgress: (asset: string) => progress.report({ message: asset }),
                beforeInstall: options.beforeInstall,
            });
            return binary as string;
        },
    );
}

/**
 * Replace a managed engine that predates this extension release.
 *
 * The managed engine is resolved by filename, so without this an engine left
 * behind by a previous release is found and reused indefinitely and a client
 * built against a newer engine keeps talking to the old one.
 *
 * Automatic, like the first download (see `downloadEngine`), and deliberately
 * not awaited by the caller: the engine on disk still works, so blocking
 * activation behind a transfer would cost every surface the extension provides
 * for an update that is not urgent.
 *
 * Only an *older* engine counts. `~/.codegraph/bin` is shared with the CLI and
 * the JetBrains plugin, which ship on their own schedules; treating a newer
 * engine as a mismatch would have the two clients reinstall over each other on
 * every launch.
 *
 * A failure is reported once per engine version and holds off further
 * automatic attempts at that version for a day (see
 * `shouldSkipAutomaticUpdate`). Activation happens once per window, and the
 * commonest failure - another window or an MCP client still running the old
 * binary - repeats until those close, so warning or re-downloading on every
 * window would be noise. The warning's Update Now action runs the
 * `codegraph.downloadEngine` command, which is not held off.
 */
export async function updateEngineIfStale(
    version: string,
    state: vscode.Memento,
    lifecycle?: EngineLifecycle,
): Promise<void> {
    const engine = managedEnginePath();
    if (!engine || !fs.existsSync(engine)) {
        return;
    }
    if (!fetchEngine.isStale(managedInstallDir(), version)) {
        return;
    }
    const previousFailure = state.get<UpdateFailure>(UPDATE_FAILURE_KEY);
    if (shouldSkipAutomaticUpdate(previousFailure, version, Date.now())) {
        return;
    }

    const swap = engineSwap(lifecycle);
    try {
        await downloadEngine(version, { beforeInstall: swap.beforeInstall });
    } catch (error) {
        // A failed update must not leave the user without an engine: the
        // binary on disk is still the one that was running.
        await swap.restart().catch(() => { });
        await state.update(UPDATE_FAILURE_KEY, { version, at: Date.now() } satisfies UpdateFailure);
        if (previousFailure?.version === version) {
            return;
        }
        const message = error instanceof Error ? error.message : String(error);
        // Not fatal: the engine already on disk still runs, and losing a working
        // install over one version of drift is worse than the drift. "In use"
        // gets its own wording because it is the one failure the user can act
        // on, and reading it as a network problem sends them somewhere useless.
        void vscode.window
            .showWarningMessage(
                isEngineInUse(error)
                    ? 'The CodeGraph engine could not be updated because it is still running. ' +
                      'Close other windows or editors using it, then choose Update Now.'
                    : `Could not update the CodeGraph engine to ${version}: ${message}`,
                'Update Now',
            )
            .then((choice) => {
                if (choice === 'Update Now') {
                    void vscode.commands.executeCommand('codegraph.downloadEngine');
                }
            });
        return;
    }
    await clearUpdateFailure(state);
    await restartAfterInstall(swap, version);
}

/** True for the "another process holds the binary" failure `fetch-engine.js` names. */
function isEngineInUse(error: unknown): boolean {
    return error instanceof Error && error.name === 'EngineInUseError';
}

/** Why the engine could not be installed; reported as `engine_<reason>` in telemetry. */
export type EngineInstallFailure = 'unsupported_platform' | 'checksum' | 'in_use' | 'download';

export type EngineInstallResult =
    | { binary: string; startFailed: boolean }
    | { failure: EngineInstallFailure };

/** Waits between attempts; a transient network or proxy hiccup rarely outlasts the second. */
const RETRY_DELAYS_MS = [2_000, 10_000];

/**
 * Install the engine for this platform, retrying transient failures.
 *
 * Never prompts first (see `downloadEngine`). A failure is shown with a Retry
 * button but not awaited, so activation reports it and moves on instead of
 * waiting on a toast the user may never answer. When [lifecycle] is given, an
 * engine this window is running is stopped for the swap and started again.
 */
export async function installEngine(
    version: string,
    lifecycle?: EngineLifecycle,
): Promise<EngineInstallResult> {
    if (!platformBinaryName()) {
        void vscode.window.showErrorMessage(
            `CodeGraph does not publish an engine for ${os.platform()}-${os.arch()}. ` +
            'Point the extension at your own build with the codegraph.serverPath setting.',
        );
        return { failure: 'unsupported_platform' };
    }

    const swap = engineSwap(lifecycle);
    let attempts = 0;
    let lastError: unknown;
    for (;;) {
        attempts++;
        let binary: string;
        try {
            binary = await downloadEngine(version, { beforeInstall: swap.beforeInstall });
        } catch (error) {
            await swap.restart().catch(() => { });
            lastError = error;
            // Retrying cannot fix bytes that fail verification or a binary
            // another process holds open.
            if (isChecksumFailure(error) || isEngineInUse(error) || attempts > RETRY_DELAYS_MS.length) {
                break;
            }
            await new Promise((resolve) => setTimeout(resolve, RETRY_DELAYS_MS[attempts - 1]));
            continue;
        }
        const startFailed = await restartAfterInstall(swap, version);
        return { binary, startFailed };
    }

    const message = lastError instanceof Error ? lastError.message : String(lastError);
    const failure: EngineInstallFailure = isChecksumFailure(lastError)
        ? 'checksum'
        : isEngineInUse(lastError)
          ? 'in_use'
          : 'download';
    // A checksum failure is not a network failure, and saying so matters: one is
    // worth retrying, the other means something served the wrong bytes.
    void vscode.window
        .showErrorMessage(
            failure === 'checksum'
                ? `The downloaded CodeGraph engine failed verification and was discarded: ${message}`
                : `Could not download the CodeGraph engine: ${message}`,
            'Retry',
        )
        .then((choice) => {
            if (choice === 'Retry') {
                void vscode.commands.executeCommand('codegraph.downloadEngine');
            }
        });
    return { failure };
}

function isChecksumFailure(error: unknown): boolean {
    return error instanceof Error && /checksum/i.test(error.message);
}
