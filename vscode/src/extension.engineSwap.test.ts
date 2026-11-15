// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import type { State as ClientState } from 'vscode-languageclient/node';

// Stands alone from `extension.test.ts`, which is quarantined on the lost
// `@vsforge` harness (see vitest.config.ts): the vscode surface activation
// touches is mocked locally so this suite actually runs.
const mocks = vi.hoisted(() => {
    const disposable = { dispose: () => { } };
    const channel = {
        appendLine: vi.fn(),
        append: vi.fn(),
        clear: vi.fn(),
        show: vi.fn(),
        hide: vi.fn(),
        dispose: vi.fn(),
    };
    return {
        // vscode-languageclient's public client states. The real module
        // cannot be imported here (it requires the real `vscode`), so they
        // are restated - and pinned to the enum itself at compile time
        // below, because the numbering is not the order it reads.
        State: { Stopped: 1, Running: 2, Starting: 3 } as const,
        disposable,
        channel,
        // Every event the extension reports, keyed by method name.
        reporterCalls: {} as Record<string, unknown[][]>,
        // Listeners the extension registers for the language client's
        // state changes.
        stateListeners: [] as ((evt: { oldState: number; newState: number }) => void)[],
        clientStart: vi.fn(async () => { }),
        clientStop: vi.fn(async () => { }),
        updateEngineIfStale: vi.fn(async () => { }),
        installEngine: vi.fn(async () => ({ binary: '/mock/engine', startFailed: false })),
        showInformationMessage: vi.fn(async (..._args: unknown[]) => undefined),
        // Command handlers the extension registers, keyed by command id.
        commandHandlers: {} as Record<string, () => Promise<void>>,
    };
});

type AssertTrue<T extends true> = T;
type _StatesPinnedToClient = [
    AssertTrue<ClientState.Stopped extends typeof mocks.State.Stopped ? true : false>,
    AssertTrue<ClientState.Running extends typeof mocks.State.Running ? true : false>,
    AssertTrue<ClientState.Starting extends typeof mocks.State.Starting ? true : false>,
];
const State = mocks.State;

const emitState = (newState: number, oldState: number) => {
    for (const listener of [...mocks.stateListeners]) {
        listener({ oldState, newState });
    }
};

vi.mock('vscode', () => ({
    workspace: {
        getConfiguration: () => ({
            get: (_key: string, fallback?: unknown) => fallback,
            inspect: () => undefined,
        }),
        workspaceFolders: undefined,
        createFileSystemWatcher: () => mocks.disposable,
        onDidChangeConfiguration: () => mocks.disposable,
    },
    window: {
        createOutputChannel: () => mocks.channel,
        createStatusBarItem: () => ({
            text: '',
            tooltip: '',
            show: vi.fn(),
            hide: vi.fn(),
            dispose: vi.fn(),
        }),
        showInformationMessage: mocks.showInformationMessage,
        showWarningMessage: vi.fn(async () => undefined),
        showErrorMessage: vi.fn(async () => undefined),
        withProgress: vi.fn(async (_opts: unknown, task: () => Promise<unknown>) => task()),
    },
    commands: {
        registerCommand: (id: string, handler: () => Promise<void>) => {
            mocks.commandHandlers[id] = handler;
            return mocks.disposable;
        },
        executeCommand: vi.fn(async () => undefined),
    },
    extensions: { getExtension: () => undefined },
    StatusBarAlignment: { Right: 2 },
    ProgressLocation: { Notification: 15 },
    version: '1.90.0',
}));

vi.mock('vscode-languageclient/node', () => ({
    State: mocks.State,
    LanguageClient: class {
        start = mocks.clientStart;
        stop = mocks.clientStop;
        isRunning = () => true;
        // `sendRequest` only serves the post-start "is it indexed?" probe.
        sendRequest = async () => ({ results: [{}] });
        onDidChangeState = (listener: (evt: { oldState: number; newState: number }) => void) => {
            mocks.stateListeners.push(listener);
            return mocks.disposable;
        };
    },
    LanguageClientOptions: vi.fn(),
    ServerOptions: vi.fn(),
    TransportKind: { stdio: 0 },
}));

vi.mock('./telemetry/reporter', () => {
    const stub: Record<string, (...args: unknown[]) => Promise<void>> = {};
    const proxy = new Proxy(stub, {
        get(target, prop: string) {
            if (!target[prop]) {
                target[prop] = (...args: unknown[]) => {
                    (mocks.reporterCalls[prop] ||= []).push(args);
                    return Promise.resolve();
                };
            }
            return target[prop];
        },
    });
    return {
        createReporter: () => proxy,
        setServerEdition: vi.fn(),
        describeArgShape: vi.fn(() => 'none'),
    };
});

vi.mock('./telemetry/machineProfile', () => ({
    detectMachineProfile: vi.fn(async () => ({})),
}));
vi.mock('./engineDownload', () => ({
    clearUpdateFailure: vi.fn(async () => { }),
    engineVersion: () => '0.21.0',
    installEngine: mocks.installEngine,
    managedEnginePath: () => '/mock/engine',
    updateEngineIfStale: mocks.updateEngineIfStale,
}));
vi.mock('./server', () => ({
    getServerPath: () => ({ path: '/mock/engine', edition: 'community' }),
    engineSpawnEnv: () => ({}),
}));
vi.mock('./commands', () => ({ registerCommands: vi.fn() }));
vi.mock('./views/treeProviders', () => ({ registerTreeDataProviders: vi.fn() }));
vi.mock('./views/codeLensProvider', () => ({ registerCodeLens: vi.fn() }));
vi.mock('./ai/contextProvider', () => ({ CodeGraphAIProvider: class { } }));
vi.mock('./ai/toolManager', () => ({
    CodeGraphToolManager: class {
        registerTools() { }
    },
}));
vi.mock('./funnel', () => ({
    handleIndexOutcome: vi.fn(async () => 'none'),
    filesIndexed: () => 0,
    reportIndexTelemetry: vi.fn(),
}));
// Recovery breadcrumbs belong to the developer running the suite.
vi.mock('fs', () => ({
    readdirSync: () => [],
    statSync: () => ({ mtimeMs: 0 }),
    readFileSync: () => '{}',
    unlinkSync: vi.fn(),
    existsSync: () => true,
}));

const context = {
    subscriptions: [] as { dispose: () => void }[],
    extensionPath: '/mock/extension',
    extension: { packageJSON: { version: '0.21.1' } },
    globalState: {
        get: vi.fn(() => undefined),
        update: vi.fn(async () => { }),
        keys: () => [],
    },
};

/**
 * Run activation far enough to own a started engine, and hand back the
 * lifecycle the engine swap drives it with. Activation is not awaited: it
 * sleeps for seconds after this point waiting on the index probe, and the
 * swap it hands to `updateEngineIfStale` is already live by then.
 */
async function activateAndCaptureLifecycle() {
    const { activate } = await import('./extension');
    void activate(context as never).catch(() => { });
    // The state listener is registered after the client has started, which is
    // the point activation owns a running engine.
    await vi.waitFor(() => expect(mocks.stateListeners.length).toBeGreaterThan(0));
    const lifecycle = mocks.updateEngineIfStale.mock.calls.at(-1)?.[2];
    expect(lifecycle).toBeDefined();
    // Activation's own start is reported separately; the swap starts clean.
    for (const key of Object.keys(mocks.reporterCalls)) delete mocks.reporterCalls[key];
    return lifecycle as { stop: () => Promise<void>; start: () => Promise<void> };
}

const infoMessages = (): string[] =>
    mocks.showInformationMessage.mock.calls.map((call) => String(call[0]));

beforeEach(() => {
    vi.clearAllMocks();
    context.subscriptions = [];
    mocks.stateListeners = [];
    mocks.clientStart.mockImplementation(async () => { });
    mocks.clientStop.mockImplementation(async () => { });
    mocks.installEngine.mockImplementation(async () => ({
        binary: '/mock/engine',
        startFailed: false,
    }));
    for (const key of Object.keys(mocks.commandHandlers)) delete mocks.commandHandlers[key];
    for (const key of Object.keys(mocks.reporterCalls)) delete mocks.reporterCalls[key];
});

afterEach(() => {
    vi.resetModules();
});

// An engine swap stops the running engine, installs the new binary and starts
// it again. None of that is a crash, so none of it may be reported as one -
// including the case the engine actually hits: the replacement binary will not
// run. vscode-languageclient reports a failed start as the public state
// Stopped, the very transition a real crash produces.
describe('engine swap crash telemetry', () => {
    it('reports no crash when the swapped-in engine fails to start', async () => {
        const lifecycle = await activateAndCaptureLifecycle();

        await lifecycle.stop();
        emitState(State.Stopped, State.Running);

        // The real sequence: `client.start()` publishes Starting, then - when
        // the binary will not run - StartFailed, whose public state is
        // Stopped. Both arrive while start() is still awaited.
        mocks.clientStart.mockImplementationOnce(async () => {
            emitState(State.Starting, State.Stopped);
            emitState(State.Stopped, State.Starting);
            throw new Error('exec format error');
        });
        await expect(lifecycle.start()).rejects.toThrow('exec format error');

        expect(mocks.reporterCalls.serverCrash ?? []).toHaveLength(0);
        expect(mocks.reporterCalls.serverRestart ?? []).toHaveLength(0);
    });

    it('reports no crash when the swapped-in engine starts', async () => {
        const lifecycle = await activateAndCaptureLifecycle();

        await lifecycle.stop();
        emitState(State.Stopped, State.Running);

        mocks.clientStart.mockImplementationOnce(async () => {
            emitState(State.Starting, State.Stopped);
            emitState(State.Running, State.Starting);
        });
        await lifecycle.start();

        expect(mocks.reporterCalls.serverCrash ?? []).toHaveLength(0);
        expect(mocks.reporterCalls.serverRestart ?? []).toHaveLength(0);
    });

    it('still reports an unexpected stop as a crash', async () => {
        await activateAndCaptureLifecycle();

        emitState(State.Stopped, State.Running);

        expect(mocks.reporterCalls.serverCrash ?? []).toHaveLength(1);
    });

    it('times a crash whose restart never reaches Running from that attempt', async () => {
        await activateAndCaptureLifecycle();

        let nowMs = Date.now();
        const clock = vi.spyOn(Date, 'now').mockImplementation(() => nowMs);
        try {
            nowMs += 600_000;
            emitState(State.Stopped, State.Running);

            // The restart publishes Starting, then fails before Running.
            emitState(State.Starting, State.Stopped);
            nowMs += 2_000;
            emitState(State.Stopped, State.Starting);
        } finally {
            clock.mockRestore();
        }

        const crashes = (mocks.reporterCalls.serverCrash ?? []) as [{ uptimeSeconds: number }][];
        expect(crashes).toHaveLength(2);
        expect(crashes[0][0].uptimeSeconds).toBeCloseTo(600, 0);
        expect(crashes[1][0].uptimeSeconds).toBe(2);
    });

    it('reports a crash restart only once the engine is running again', async () => {
        await activateAndCaptureLifecycle();

        emitState(State.Stopped, State.Running);
        emitState(State.Starting, State.Stopped);

        expect(mocks.reporterCalls.serverRestart ?? []).toHaveLength(0);

        emitState(State.Running, State.Starting);

        expect(mocks.reporterCalls.serverRestart ?? []).toEqual([['crash']]);
    });
});

// The install and the restart are two outcomes. An engine that was replaced
// but will not run has already been warned about, and confirming the install
// after that warning is the difference between reloading the window and
// assuming all is well.
describe('codegraph.downloadEngine reporting', () => {
    it('does not confirm an install whose engine failed to start', async () => {
        await activateAndCaptureLifecycle();
        mocks.installEngine.mockImplementation(async () => ({
            binary: '/mock/engine',
            startFailed: true,
        }));

        await mocks.commandHandlers['codegraph.downloadEngine']();

        expect(infoMessages()).not.toContain('CodeGraph engine installed.');
    });

    it('confirms an install whose engine started', async () => {
        await activateAndCaptureLifecycle();

        await mocks.commandHandlers['codegraph.downloadEngine']();

        expect(infoMessages()).toContain('CodeGraph engine installed.');
    });
});
