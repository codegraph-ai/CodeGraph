// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';

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
        disposable,
        channel,
        // Every event the extension reports, keyed by method name.
        reporterCalls: {} as Record<string, unknown[][]>,
        // Listeners the extension registers for the language client's state
        // changes: 1 = Stopped, 2 = Starting, 3 = Running.
        stateListeners: [] as ((evt: { oldState: number; newState: number }) => void)[],
        clientStart: vi.fn(async () => { }),
        clientStop: vi.fn(async () => { }),
        updateEngineIfStale: vi.fn(async () => { }),
    };
});

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
        showInformationMessage: vi.fn(async () => undefined),
        showWarningMessage: vi.fn(async () => undefined),
        showErrorMessage: vi.fn(async () => undefined),
        withProgress: vi.fn(async (_opts: unknown, task: () => Promise<unknown>) => task()),
    },
    commands: {
        registerCommand: () => mocks.disposable,
        executeCommand: vi.fn(async () => undefined),
    },
    extensions: { getExtension: () => undefined },
    StatusBarAlignment: { Right: 2 },
    ProgressLocation: { Notification: 15 },
    version: '1.90.0',
}));

vi.mock('vscode-languageclient/node', () => ({
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
    installEngine: vi.fn(async () => ({ binary: '/mock/engine', attempts: 1 })),
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

// An engine swap stops the running engine, installs the new binary and starts
// it again. None of that is a crash, so none of it may be reported as one -
// including the case the engine actually hits: the replacement binary will not
// run. vscode-languageclient reports a failed start as the public state
// Stopped, the very transition a real crash produces.
describe('engine swap crash telemetry', () => {
    beforeEach(() => {
        vi.clearAllMocks();
        context.subscriptions = [];
        mocks.stateListeners = [];
        mocks.clientStart.mockImplementation(async () => { });
        mocks.clientStop.mockImplementation(async () => { });
        for (const key of Object.keys(mocks.reporterCalls)) delete mocks.reporterCalls[key];
    });

    afterEach(() => {
        vi.resetModules();
    });

    it('reports no crash when the swapped-in engine fails to start', async () => {
        const lifecycle = await activateAndCaptureLifecycle();

        await lifecycle.stop();
        emitState(1, 3);

        // Both transitions arrive while `client.start()` is still awaited -
        // the client sets its state before start() settles, either way.
        mocks.clientStart.mockImplementationOnce(async () => {
            emitState(1, 2);
            throw new Error('exec format error');
        });
        await expect(lifecycle.start()).rejects.toThrow('exec format error');

        expect(mocks.reporterCalls.serverCrash ?? []).toHaveLength(0);
        expect(mocks.reporterCalls.serverRestart ?? []).toHaveLength(0);
    });

    it('reports no crash when the swapped-in engine starts', async () => {
        const lifecycle = await activateAndCaptureLifecycle();

        await lifecycle.stop();
        emitState(1, 3);

        mocks.clientStart.mockImplementationOnce(async () => {
            emitState(3, 2);
        });
        await lifecycle.start();

        expect(mocks.reporterCalls.serverCrash ?? []).toHaveLength(0);
        expect(mocks.reporterCalls.serverRestart ?? []).toHaveLength(0);
    });

    it('still reports an unexpected stop as a crash', async () => {
        await activateAndCaptureLifecycle();

        emitState(1, 3);

        expect(mocks.reporterCalls.serverCrash ?? []).toHaveLength(1);
    });
});
