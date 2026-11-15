// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

import { describe, it, expect, beforeAll, beforeEach, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
    window: {
        withProgress: vi.fn(),
        showWarningMessage: vi.fn(),
        showErrorMessage: vi.fn(),
    },
    commands: { executeCommand: vi.fn() },
    engine: {
        ENGINE_VERSION: '0.21.0',
        ensureEngine: vi.fn(),
        isStale: vi.fn(),
        platformBinaryName: vi.fn(),
    },
}));

vi.mock('vscode', () => ({
    window: mocks.window,
    commands: mocks.commands,
    ProgressLocation: { Notification: 15 },
}));

// The engine is only ever looked for, never read, and the install directory
// belongs to the developer running the suite.
vi.mock('fs', () => ({ existsSync: () => true }));

let engineDownload: typeof import('./engineDownload');

beforeAll(async () => {
    // The module under test reaches the shared fetcher through `require`, which
    // keeps the npm package and the extension on one implementation but leaves
    // it outside the module graph Vitest can mock. Node's own cache is where
    // that `require` looks, so a fake release lives there.
    const { createRequire } = await import('node:module');
    const require_ = createRequire(import.meta.url);
    const fetcher = require_.resolve('../../mcp-package/bin/fetch-engine.js');
    require_.cache[fetcher] = {
        id: fetcher,
        filename: fetcher,
        loaded: true,
        exports: mocks.engine,
    } as unknown as NodeJS.Module;
    engineDownload = await import('./engineDownload');
});

describe('shouldSkipAutomaticUpdate', () => {
    const now = 1_000_000_000_000;

    it('attempts the update when no earlier failure is recorded', () => {
        expect(engineDownload.shouldSkipAutomaticUpdate(undefined, '0.21.0', now)).toBe(false);
    });

    it('holds off a version that failed less than a day ago', () => {
        const failure = { version: '0.21.0', at: now - engineDownload.UPDATE_RETRY_BACKOFF_MS + 1 };
        expect(engineDownload.shouldSkipAutomaticUpdate(failure, '0.21.0', now)).toBe(true);
    });

    it('tries the same version again once a day has passed', () => {
        const failure = { version: '0.21.0', at: now - engineDownload.UPDATE_RETRY_BACKOFF_MS };
        expect(engineDownload.shouldSkipAutomaticUpdate(failure, '0.21.0', now)).toBe(false);
    });

    it('does not hold off a different version', () => {
        const failure = { version: '0.20.0', at: now - 1 };
        expect(engineDownload.shouldSkipAutomaticUpdate(failure, '0.21.0', now)).toBe(false);
    });
});

const FAILURE_KEY = 'codegraph.engineUpdateFailure';
const BINARY = '/home/u/.codegraph/bin/codegraph-server-linux-x64';

/** The globalState the extension records the last failed automatic update in. */
function fakeState() {
    const values = new Map<string, unknown>();
    return {
        values,
        memento: {
            get: (key: string) => values.get(key),
            update: async (key: string, value: unknown) => {
                if (value === undefined) {
                    values.delete(key);
                } else {
                    values.set(key, value);
                }
            },
        } as unknown as Parameters<typeof engineDownload.updateEngineIfStale>[1],
    };
}

/** An engine this window is running, whose `start` outcome the test chooses. */
function fakeLifecycle(start: () => Promise<void>) {
    return {
        isRunning: () => true,
        stop: vi.fn(async () => { }),
        start: vi.fn(start),
    };
}

function warnings(): string[] {
    return mocks.window.showWarningMessage.mock.calls.map((call) => String(call[0]));
}

describe('an engine that is installed but will not start', () => {
    beforeEach(() => {
        vi.clearAllMocks();
        mocks.window.withProgress.mockImplementation(
            async (
                _options: unknown,
                task: (progress: { report: () => void }) => Promise<unknown>,
            ) => task({ report: () => { } }),
        );
        mocks.window.showWarningMessage.mockResolvedValue(undefined);
        mocks.window.showErrorMessage.mockResolvedValue(undefined);
        mocks.engine.platformBinaryName.mockReturnValue('codegraph-server-linux-x64');
        mocks.engine.isStale.mockReturnValue(true);
        // A real install stops the running engine once every asset is verified.
        mocks.engine.ensureEngine.mockImplementation(
            async (
                _version: string,
                _dir: string,
                options: { beforeInstall?: () => Promise<void> },
            ) => {
                await options.beforeInstall?.();
                return { binary: BINARY, fetched: ['codegraph-server-linux-x64'] };
            },
        );
    });

    it('is not recorded as a failed automatic update', async () => {
        const state = fakeState();
        const lifecycle = fakeLifecycle(async () => {
            throw new Error('exited with code 127');
        });

        await engineDownload.updateEngineIfStale('0.21.0', state.memento, lifecycle);

        expect(lifecycle.start).toHaveBeenCalledOnce();
        expect(state.values.get(FAILURE_KEY)).toBeUndefined();
        expect(warnings()).toEqual([
            'The CodeGraph engine was updated to 0.21.0 but did not start again: exited with code 127',
        ]);
        // No Update Now: the engine is updated, and updating it again would not
        // start it.
        expect(mocks.commands.executeCommand).not.toHaveBeenCalled();
    });

    it('still reports the install to the caller of installEngine', async () => {
        const lifecycle = fakeLifecycle(async () => {
            throw new Error('exited with code 127');
        });

        const result = await engineDownload.installEngine('0.21.0', lifecycle);

        expect(result).toEqual({ binary: BINARY, startFailed: true });
        expect(mocks.engine.ensureEngine).toHaveBeenCalledOnce();
        expect(mocks.window.showErrorMessage).not.toHaveBeenCalled();
        expect(warnings()).toEqual([
            'The CodeGraph engine was updated to 0.21.0 but did not start again: exited with code 127',
        ]);
    });

    it('reports a clean swap as an install that started', async () => {
        const lifecycle = fakeLifecycle(async () => { });

        const result = await engineDownload.installEngine('0.21.0', lifecycle);

        expect(result).toEqual({ binary: BINARY, startFailed: false });
        expect(warnings()).toEqual([]);
    });

    it('leaves a real download failure reported and backed off', async () => {
        const state = fakeState();
        mocks.engine.ensureEngine.mockRejectedValue(new Error('socket hang up'));
        const lifecycle = fakeLifecycle(async () => { });

        await engineDownload.updateEngineIfStale('0.21.0', state.memento, lifecycle);

        expect(state.values.get(FAILURE_KEY)).toMatchObject({ version: '0.21.0' });
        expect(warnings()).toEqual([
            'Could not update the CodeGraph engine to 0.21.0: socket hang up',
        ]);
    });

    it('restarts the engine it stopped when the install itself fails', async () => {
        const state = fakeState();
        mocks.engine.ensureEngine.mockImplementation(
            async (
                _version: string,
                _dir: string,
                options: { beforeInstall?: () => Promise<void> },
            ) => {
                await options.beforeInstall?.();
                throw new Error('EBUSY');
            },
        );
        const lifecycle = fakeLifecycle(async () => { });

        await engineDownload.updateEngineIfStale('0.21.0', state.memento, lifecycle);

        expect(lifecycle.stop).toHaveBeenCalledOnce();
        expect(lifecycle.start).toHaveBeenCalledOnce();
        expect(state.values.get(FAILURE_KEY)).toMatchObject({ version: '0.21.0' });
    });
});
