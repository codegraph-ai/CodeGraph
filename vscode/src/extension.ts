// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

import * as vscode from 'vscode';
import * as os from 'os';
import * as fs from 'fs';
import * as path from 'path';
import * as cp from 'child_process';
import {
    LanguageClient,
    LanguageClientOptions,
    ServerOptions,
} from 'vscode-languageclient/node';
import { registerCommands } from './commands';
import { registerTreeDataProviders } from './views/treeProviders';
import { registerCodeLens } from './views/codeLensProvider';
import { CodeGraphAIProvider } from './ai/contextProvider';
import { CodeGraphToolManager } from './ai/toolManager';
import { getServerPath } from './server';
import { engineVersion, managedEnginePath, offerEngineDownload, offerEngineUpdateIfStale } from './engineDownload';
import { createReporter, setServerEdition, type Reporter } from './telemetry/reporter';
import { detectMachineProfile } from './telemetry/machineProfile';
import { handleIndexOutcome, filesIndexed, reportIndexTelemetry } from './funnel';

let client: LanguageClient;
let aiProvider: CodeGraphAIProvider;
let toolManager: CodeGraphToolManager;
let reporter: Reporter;
let serverUptimeStart = 0;
let serverRestartCount = 0;
// Captured from the server child process's `exit` so a crash report can say
// WHY it died: a unix signal (SIGSEGV / SIGKILL=OOM-kill / SIGABRT) or a
// Windows exit code (0xC0000005 = access violation, AV TerminateProcess, …).
let lastExitCode: number | null = null;
let lastExitSignal: string | null = null;

// Crash-loop detection: if the server crashes MAX_RAPID_CRASHES times
// within RAPID_CRASH_WINDOW_MS, stop restarting and show a persistent
// error. Without this, machines with antivirus/glibc/OOM issues generate
// 50+ crash events per week in an infinite restart loop.
const MAX_RAPID_CRASHES = 3;
const RAPID_CRASH_WINDOW_MS = 60_000;
let rapidCrashTimestamps: number[] = [];
let crashLoopDetected = false;
// Set true right before an intentional server stop (crash-loop give-up,
// deactivate) so the onDidChangeState→Stopped that follows isn't logged
// as a crash. Consume-once: the handler resets it after skipping.
let expectedShutdown = false;

/**
 * Read + classify the server's crash breadcrumb. The server's panic hook
 * drops `~/.codegraph/last-crash.<pid>.json` with an enum `class`/`site`
 * (never source or message text). Returns the allowlisted cause; if no
 * fresh breadcrumb exists the crash was a signal/segfault/OOM-kill that
 * couldn't run the hook → `hard_crash`. Best-effort; never throws.
 */
function readAndClassifyCrash(): { cause: string; phase?: string } {
    try {
        const dir = path.join(os.homedir(), '.codegraph');
        let files: string[];
        try {
            files = fs.readdirSync(dir);
        } catch {
            return { cause: 'hard_crash' };
        }
        // Newest matching marker, but only trusted within ~15s of this crash
        // so a stale file from a prior session can't mislabel the current one.
        const pickFresh = (re: RegExp): Record<string, unknown> | undefined => {
            let best: { file: string; mtime: number } | undefined;
            for (const f of files) {
                if (!re.test(f)) continue;
                try {
                    const m = fs.statSync(path.join(dir, f)).mtimeMs;
                    if (!best || m > best.mtime) best = { file: f, mtime: m };
                } catch {
                    /* ignore unreadable entry */
                }
            }
            if (!best || Date.now() - best.mtime > 15_000) return undefined;
            try {
                return JSON.parse(fs.readFileSync(path.join(dir, best.file), 'utf8'));
            } catch {
                return undefined;
            }
        };

        // Panic breadcrumb → precise cause; absent → signal/segfault/OOM-kill.
        let cause = 'hard_crash';
        const crumb = pickFresh(/^last-crash\..*\.json$/);
        if (crumb) {
            if (crumb.kind === 'signal') cause = 'signal';
            else if (crumb.kind === 'panic' && typeof crumb.class === 'string') cause = crumb.class;
        }

        // Phase marker → WHERE the (native) death happened, e.g. onnx_load.
        let phase: string | undefined;
        const ph = pickFresh(/^last-phase\..*\.json$/);
        if (ph && typeof ph.phase === 'string') phase = ph.phase;

        // Clean up all crash + phase markers so none lingers to mislabel later.
        for (const f of files) {
            if (/^last-(crash|phase)\..*\.json$/.test(f)) {
                try {
                    fs.unlinkSync(path.join(dir, f));
                } catch {
                    /* ignore */
                }
            }
        }
        return { cause, phase };
    } catch {
        return { cause: 'hard_crash' };
    }
}

/**
 * Read + report the server's poison-recovery breadcrumb
 * (`~/.codegraph/last-recovery.<pid>.json`, written whenever the startup
 * load found sentinel evidence). Unlike crash markers there is no freshness
 * window: the newest breadcrumb is reported once and ALL of them deleted,
 * so the cohort whose recovery never fires finally shows us why. Counts,
 * bools and a 3-value enum only. Best-effort; never throws.
 */
function reportRecoveryBreadcrumb(): void {
    try {
        const dir = path.join(os.homedir(), '.codegraph');
        let files: string[];
        try {
            files = fs.readdirSync(dir);
        } catch {
            return;
        }
        const matches = files.filter((f) => /^last-recovery\..*\.json$/.test(f));
        if (matches.length === 0) return;

        let best: { file: string; mtime: number } | undefined;
        for (const f of matches) {
            try {
                const m = fs.statSync(path.join(dir, f)).mtimeMs;
                if (!best || m > best.mtime) best = { file: f, mtime: m };
            } catch {
                /* ignore unreadable entry */
            }
        }
        if (best) {
            try {
                const crumb = JSON.parse(fs.readFileSync(path.join(dir, best.file), 'utf8'));
                reporter.serverRecovery({
                    found: typeof crumb.found === 'number' ? crumb.found : 0,
                    alive: typeof crumb.alive === 'number' ? crumb.alive : 0,
                    dead: typeof crumb.dead === 'number' ? crumb.dead : 0,
                    legacy: crumb.legacy === true,
                    bump: typeof crumb.bump === 'string' ? crumb.bump : 'none',
                    generation: typeof crumb.generation === 'number' ? crumb.generation : 0,
                    sweptOk: typeof crumb.sweptOk === 'number' ? crumb.sweptOk : 0,
                    sweptFail: typeof crumb.sweptFail === 'number' ? crumb.sweptFail : 0,
                });
            } catch {
                /* malformed breadcrumb — skip */
            }
        }
        for (const f of matches) {
            try {
                fs.unlinkSync(path.join(dir, f));
            } catch {
                /* ignore */
            }
        }
    } catch {
        /* never block activation on telemetry */
    }
}

// Idempotency guard. VS Code calls activate() once per host, but a stale
// client surviving a host reload can re-run server-command registration and
// throw `command 'codegraph.getDependencyGraph' already exists` (seen in
// spawn_fail telemetry). Bail if activation already ran in this host.
let hasActivated = false;

export async function activate(context: vscode.ExtensionContext): Promise<void> {
    if (hasActivated) {
        return;
    }
    hasActivated = true;

    const config = vscode.workspace.getConfiguration('codegraph', vscode.workspace.workspaceFolders?.[0]?.uri);

    // Debug output channel (enabled via codegraph.debug setting)
    const debugEnabled = config.get<boolean>('debug', false);
    const debugChannel = debugEnabled ? vscode.window.createOutputChannel('CodeGraph Debug') : null;
    const debug = (msg: string) => {
        if (debugChannel) { debugChannel.appendLine(msg); }
        console.log(`[CodeGraph] ${msg}`);
    };

    if (debugEnabled && debugChannel) {
        debugChannel.show(true);
        debug(`Version: ${context.extension.packageJSON.version}`);
        debug(`Workspace folders: ${vscode.workspace.workspaceFolders?.map(f => f.uri.fsPath).join(', ') ?? 'none'}`);
        debug(`indexOnStartup: ${config.get('indexOnStartup')} (inspect: ${JSON.stringify(config.inspect('indexOnStartup'))})`);
        debug(`indexPaths: ${JSON.stringify(config.get('indexPaths'))}`);
        debug(`excludePatterns: ${JSON.stringify(config.get('excludePatterns'))}`);
        debug(`maxFileSizeKB: ${config.get('maxFileSizeKB')}`);
        debug(`embeddingModel: ${config.get('embeddingModel')}`);
    }

    // Initialize the telemetry reporter early — its first event fires
    // before any other side effect so we can see if activation itself
    // is consistently failing. All hard opt-out gates are enforced
    // inside the reporter; this construction is always safe.
    //
    // Speculatively label the edition as 'community' BEFORE firing
    // activation.start — getServerPath() resolves the actual binary
    // later, and if a pro binary is on PATH we'll upgrade the label to
    // 'pro' there. Without this, activation.start ships with
    // `serverEdition: "unknown"` and dashboards undercount community
    // activations.
    reporter = createReporter(context);
    setServerEdition('community');
    context.subscriptions.push({ dispose: () => { void reporter.dispose(); } });
    reporter.activationStart({
        enabledSetting: config.get<boolean>('enabled', true),
        workspaceFolders: vscode.workspace.workspaceFolders?.length ?? 0,
        hasMultiRoot: (vscode.workspace.workspaceFolders?.length ?? 0) > 1,
    });
    reportRecoveryBreadcrumb();

    if (!config.get<boolean>('enabled', true)) {
        return;
    }

    // Pro/community coexistence guard. If the paid CodeGraph Pro extension
    // is installed and enabled, it runs its own server that owns the index
    // database (RocksDB holds an exclusive lock). Starting the community
    // server too crashes on the lock conflict. Defer to Pro and nudge the
    // user — once — to uninstall the redundant free extension.
    const proExtension = vscode.extensions.getExtension('aStudioPlus.codegraph-pro');
    if (proExtension) {
        setServerEdition('pro');
        reporter.activationServerStartResult({
            outcome: 'pro_detected',
            durationMs: 0,
            serverBinaryFound: true,
        });

        const proStatus = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 100);
        proStatus.text = '$(shield) CodeGraph Pro';
        proStatus.tooltip = 'CodeGraph Pro is active — the free CodeGraph extension is idle to avoid conflicts.';
        proStatus.show();
        context.subscriptions.push(proStatus);

        const NOTIFIED_KEY = 'codegraph.proCoexistenceNotified';
        if (!context.globalState.get<boolean>(NOTIFIED_KEY)) {
            void context.globalState.update(NOTIFIED_KEY, true);
            void vscode.window
                .showWarningMessage(
                    'CodeGraph Pro is installed. The free "CodeGraph" extension is redundant and can ' +
                        'conflict with it (both manage the same index database). Uninstall the free ' +
                        'extension to avoid errors.',
                    'Manage Extensions',
                )
                .then((choice) => {
                    if (choice === 'Manage Extensions') {
                        void vscode.commands.executeCommand('workbench.extensions.search', '@installed codegraph');
                    }
                });
        }
        return;
    }

    // The one command registered before the engine is resolved, because
    // declining the download below ends activation and nothing after it -
    // commands, tree views, lenses - is ever contributed. Without this, a user
    // who says "Not Now" and later changes their mind has no way back short of
    // reloading the window and guessing that the prompt returns; the npm
    // channel ships `codegraph-mcp-fetch-engine` for exactly that case.
    //
    // Reloading is what puts a late download to use, so the command offers it -
    // but only when activation did stop early, since in a session that already
    // has an engine running a reload prompt is noise.
    context.subscriptions.push(
        vscode.commands.registerCommand('codegraph.downloadEngine', async () => {
            const activationStopped = !client;
            if (!(await offerEngineDownload(engineVersion()))) {
                return;
            }
            if (!activationStopped) {
                return;
            }
            const choice = await vscode.window.showInformationMessage(
                'Reload the window to start the CodeGraph engine.',
                'Reload Window',
            );
            if (choice === 'Reload Window') {
                void vscode.commands.executeCommand('workbench.action.reloadWindow');
            }
        }),
    );

    // Determine server binary path — may upgrade the edition label from
    // 'community' to 'pro' if the user has the pro binary on PATH.
    //
    // The published VSIX no longer bundles engines: shipping all four platform
    // binaries meant a 118 MB download for the one a user can actually run.
    // When none is found we offer to fetch this platform's engine, which is
    // also where an npm- or JetBrains-installed engine gets picked up, since
    // all three channels share ~/.codegraph/bin.
    let serverInfo: ReturnType<typeof getServerPath>;
    try {
        serverInfo = getServerPath(context);
    } catch {
        const downloaded = await offerEngineDownload(engineVersion());
        if (!downloaded) {
            reporter.activationServerStartResult({
                outcome: 'spawn_fail',
                durationMs: 0,
                serverBinaryFound: false,
                errorHint: 'engine_not_installed',
            });
            return;
        }
        serverInfo = getServerPath(context);
    }

    // The managed engine is found by filename alone, so one installed by an
    // earlier release would otherwise be reused forever. The extension and the
    // engine ship in lockstep, so offer to bring it up to the engine release
    // this build expects - only when it is the binary we actually resolved,
    // since a pro, bundled or locally built engine is the user's to manage.
    //
    // Not awaited: the engine on disk still runs, and holding activation - and
    // with it the language client, the tree views and the lenses - behind a
    // 30 MB transfer on a slow network is a far worse trade than one release of
    // drift. The lifecycle callbacks read `client` lazily for the same reason:
    // by the time the user answers the prompt it has been created and started.
    if (serverInfo.path === managedEnginePath()) {
        void offerEngineUpdateIfStale(engineVersion(), context.globalState, {
            isRunning: () => client?.isRunning() ?? false,
            stop: () => client.stop(),
            start: () => client.start(),
        });
    }

    setServerEdition(serverInfo.edition === 'pro' ? 'pro' : 'community');

    // Log server path for debugging
    console.log(`[CodeGraph] Platform: ${os.platform()}`);
    console.log(`[CodeGraph] Server binary: ${serverInfo.path}`);
    console.log(`[CodeGraph] Edition: ${serverInfo.edition}`);

    // Status bar — show edition
    const statusItem = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 100);
    statusItem.text = serverInfo.edition === 'pro' ? '$(shield) CodeGraph Pro' : '$(symbol-misc) CodeGraph';
    statusItem.tooltip = `CodeGraph ${serverInfo.edition} edition`;
    statusItem.show();
    context.subscriptions.push(statusItem);

    const serverModule = serverInfo.path;

    // Spawn the server ourselves (function-form ServerOptions) so we can
    // observe its exit code/signal for crash diagnostics. Spawning the binary
    // directly with no shell also fixes the Windows path-with-spaces bug
    // (issue #2) properly — Node passes argv without shell word-splitting, so
    // `C:\Users\First Last\...\codegraph-server.exe` no longer breaks at the
    // space the way `shell:true` + cmd.exe did. stdio defaults to pipes, which
    // vscode-languageclient uses for the LSP transport (stderr → outputChannel).
    const serverOptions: ServerOptions = () => {
        // When the static (model2vec) embedding model is selected, point the
        // server at the model dir via CODEGRAPH_STATIC_MODEL — the server
        // resolves the static path from this env, falling back to
        // ~/.codegraph/static_models/jina-code-static-256.
        const wsFolder = vscode.workspace.workspaceFolders?.[0]?.uri;
        const cfg = vscode.workspace.getConfiguration('codegraph', wsFolder);
        const spawnEnv = { ...process.env };
        if (cfg.get<string>('embeddingModel') === 'static') {
            // staticModelPath override, else the model bundled next to the binary.
            const staticModelPath = cfg.get<string>('staticModelPath')
                || path.join(context.extensionPath, 'bin', 'jina-code-static-256');
            spawnEnv.CODEGRAPH_STATIC_MODEL = staticModelPath;
        }
        const child = cp.spawn(serverModule, [], { cwd: context.extensionPath, env: spawnEnv });
        child.once('exit', (code, signal) => {
            lastExitCode = code;
            lastExitSignal = signal;
        });
        return Promise.resolve(child);
    };

    // Client options
    const clientOptions: LanguageClientOptions = {
        documentSelector: [
            { scheme: 'file', language: 'python' },
            { scheme: 'file', language: 'rust' },
            { scheme: 'file', language: 'typescript' },
            { scheme: 'file', language: 'javascript' },
            { scheme: 'file', language: 'typescriptreact' },
            { scheme: 'file', language: 'javascriptreact' },
            { scheme: 'file', language: 'go' },
            { scheme: 'file', language: 'c' },
            { scheme: 'file', language: 'java' },
            { scheme: 'file', language: 'cpp' },
            { scheme: 'file', language: 'kotlin' },
            { scheme: 'file', language: 'csharp' },
        ],
        synchronize: {
            fileEvents: vscode.workspace.createFileSystemWatcher('**/*'),
        },
        outputChannel: vscode.window.createOutputChannel('CodeGraph'),
        traceOutputChannel: vscode.window.createOutputChannel('CodeGraph Trace'),
        initializationOptions: () => {
            // Re-read config at init time (not activation time) to pick up workspace settings.
            // Pass workspace folder URI for scope to ensure .vscode/settings.json is included.
            const wsFolder = vscode.workspace.workspaceFolders?.[0]?.uri;
            const latestConfig = vscode.workspace.getConfiguration('codegraph', wsFolder);
            const opts = {
                extensionPath: context.extensionPath,
                indexOnStartup: latestConfig.get<boolean>('indexOnStartup'),
                excludePatterns: latestConfig.get<string[]>('excludePatterns'),
                indexPaths: latestConfig.get<string[]>('indexPaths'),
                maxFileSizeKB: latestConfig.get<number>('maxFileSizeKB'),
                embeddingModel: latestConfig.get<string>('embeddingModel'),
                staticModelPath: latestConfig.get<string>('staticModelPath'),
                fullBodyEmbedding: latestConfig.get<boolean>('fullBodyEmbedding'),
                embedOnOpen: latestConfig.get<boolean>('embedOnOpen'),
            };
            console.log('[CodeGraph] Initialization options:', JSON.stringify(opts));
            return opts;
        },
    };

    // Create the language client
    client = new LanguageClient(
        'codegraph',
        'CodeGraph Language Server',
        serverOptions,
        clientOptions
    );

    // Start the client
    const serverStartBegan = Date.now();
    try {
        await client.start();
        serverUptimeStart = Date.now();
        vscode.window.showInformationMessage('CodeGraph: Language server started');
        reporter.activationServerStartResult({
            outcome: 'ok',
            durationMs: Date.now() - serverStartBegan,
            serverBinaryFound: true,
        });
    } catch (error) {
        vscode.window.showErrorMessage(`CodeGraph: Failed to start language server: ${error}`);
        const errStr = String(error);
        const isTimeout = errStr.toLowerCase().includes('timeout');

        // Extract a short, privacy-safe hint from the error for diagnostics.
        // Never send file paths or user-specific content — only the error
        // class (glibc, ENOENT, EACCES, vcruntime, antivirus patterns).
        let errorHint = 'unknown';
        const lower = errStr.toLowerCase();
        if (isTimeout) errorHint = 'timeout';
        else if (lower.includes('enoent')) errorHint = 'ENOENT';
        else if (lower.includes('eacces')) errorHint = 'EACCES';
        else if (lower.includes('glibc') || lower.includes('libc')) errorHint = 'glibc_missing';
        else if (lower.includes('vcruntime') || lower.includes('msvcp')) errorHint = 'vcruntime_missing';
        else if (lower.includes('permission')) errorHint = 'permission_denied';
        else if (lower.includes('virus') || lower.includes('quarantine') || lower.includes('blocked')) errorHint = 'antivirus_blocked';
        else if (lower.includes('not a valid win32') || lower.includes('bad cpu') || lower.includes('exec format')) errorHint = 'wrong_architecture';
        else if (lower.includes('eperm')) errorHint = 'EPERM';
        // Handshake-death failures — the server process spawned but died
        // during the LSP handshake. These dominate real spawn_fails (was
        // landing in the null/unknown bucket). EPIPE/stream-destroyed mean
        // the server exited immediately (missing dep, panic-on-start, or
        // the Windows path-with-spaces bug from issue #2 on old versions).
        else if (lower.includes('epipe')) errorHint = 'EPIPE';
        else if (lower.includes('write after') || lower.includes('stream was destroyed')) errorHint = 'stream_destroyed';
        else if (lower.includes('connection got disposed') || lower.includes('connection is disposed') || lower.includes('pending response rejected')) errorHint = 'connection_disposed';
        else if (lower.includes('already exists')) errorHint = 'command_conflict';
        else if (lower.includes('spawn')) errorHint = 'spawn_error';
        else {
            // Last resort: first 80 chars, strip anything that looks like a path
            errorHint = errStr.substring(0, 80).replace(/[\/\\][^\s:]+/g, '<path>');
        }

        reporter.activationServerStartResult({
            outcome: isTimeout ? 'timeout' : 'spawn_fail',
            durationMs: Date.now() - serverStartBegan,
            serverBinaryFound: !!serverInfo.path,
            errorHint,
        });
        return;
    }

    // Watch for unexpected server state changes — crashes fire here.
    context.subscriptions.push(
        client.onDidChangeState((evt) => {
            // evt.newState: 1=Stopped, 2=Starting, 3=Running
            if (evt.newState === 1 && serverUptimeStart > 0) {
                // Intentional stop (crash-loop give-up / deactivate) — don't
                // count it as a crash. Consume-once.
                if (expectedShutdown) {
                    expectedShutdown = false;
                    return;
                }
                const now = Date.now();
                const uptimeSeconds = (now - serverUptimeStart) / 1000;
                const { cause: crashCause, phase: crashPhase } = readAndClassifyCrash();

                reporter.serverCrash({
                    uptimeSeconds,
                    restartCount: serverRestartCount,
                    lastToolName: toolManager?.lastToolName,
                    crashCause,
                    crashPhase,
                    exitCode: lastExitCode ?? undefined,
                    exitSignal: lastExitSignal ?? undefined,
                });
                lastExitCode = null;
                lastExitSignal = null;

                // Crash-loop detection: track rapid crashes and stop
                // restarting if we hit the cap. Prevents infinite
                // restart loops on machines with antivirus, missing
                // runtime deps, or OOM conditions (telemetry showed
                // single machines generating 50+ crash events/week).
                rapidCrashTimestamps.push(now);
                rapidCrashTimestamps = rapidCrashTimestamps.filter(
                    (t) => now - t < RAPID_CRASH_WINDOW_MS,
                );

                if (rapidCrashTimestamps.length >= MAX_RAPID_CRASHES) {
                    crashLoopDetected = true;
                    expectedShutdown = true;
                    client.stop().catch(() => {});
                    vscode.window
                        .showErrorMessage(
                            `CodeGraph: Server crashed ${MAX_RAPID_CRASHES} times in ${RAPID_CRASH_WINDOW_MS / 1000}s — stopped restarting. ` +
                            'This is usually caused by antivirus software, missing runtime libraries, ' +
                            'or insufficient memory. Check the Output panel (CodeGraph Debug) for details.',
                            'Retry',
                            'Open Output',
                        )
                        .then((choice) => {
                            if (choice === 'Retry') {
                                crashLoopDetected = false;
                                rapidCrashTimestamps = [];
                                serverRestartCount = 0;
                                client.start().catch(() => {});
                            } else if (choice === 'Open Output') {
                                vscode.commands.executeCommand(
                                    'workbench.action.output.toggleOutput',
                                );
                            }
                        });
                    return;
                }
            }
            if (evt.newState === 3 && serverUptimeStart > 0) {
                serverRestartCount += 1;
                serverUptimeStart = Date.now();
                reporter.serverRestart('crash');
            }
        }),
    );

    // Create AI context provider
    aiProvider = new CodeGraphAIProvider(client);

    // Register Language Model Tools for autonomous AI agent access
    try {
        toolManager = new CodeGraphToolManager(client, reporter, context);
        toolManager.registerTools();
        const lmAvailable = !!(vscode as any).lm;
        reporter.activationToolRegistration({
            lmApiAvailable: lmAvailable,
            toolsRegistered: lmAvailable ? 32 : 0,
            vscodeTooOld: !lmAvailable,
        });
        console.log('[CodeGraph] AI tools registered and available to AI agents');
    } catch (error) {
        console.error('[CodeGraph] Failed to register Language Model Tools:', error);
        vscode.window.showWarningMessage(`CodeGraph: Could not register AI tools: ${error}`);
        reporter.activationToolRegistration({
            lmApiAvailable: false,
            toolsRegistered: 0,
            vscodeTooOld: true,
        });
    }

    // Settings snapshot once per session — observe what defaults users override.
    reporter.engagementSettingsSnapshot();

    // One-time machine fingerprint (bucketed/enum only, no PII) to triage the
    // graph_load crash cohort — does it skew toward cloud-synced data dirs,
    // third-party AV, VMs, or low RAM? Detection runs off the activation path
    // (the Windows AV probe is async) and never blocks startup.
    void detectMachineProfile()
        .then((profile) => reporter.engagementMachineProfile(profile))
        .catch(() => {
            /* never block on telemetry */
        });

    // Check if workspace is indexed — prompt if not.
    // Delay the check briefly: the server loads the persisted graph and
    // rebuilds search indexes after the LSP handshake. A symbolSearch
    // fired immediately can hit an empty index and falsely trigger the
    // "not indexed" prompt even when 13k+ nodes are already loaded.
    await new Promise((r) => setTimeout(r, 2000));
    try {
        const check = await client.sendRequest<any>('workspace/executeCommand', {
            command: 'codegraph.symbolSearch',
            arguments: [{ query: '*', limit: 1 }],
        });
        const alreadyIndexed = !!check?.results?.length;
        // Drives the codegraphSymbols empty-state welcome (index CTA vs.
        // "open a file") and any `codegraph.indexed`-gated UI.
        void vscode.commands.executeCommand('setContext', 'codegraph.indexed', alreadyIndexed);
        if (!alreadyIndexed) {
            const choice = await vscode.window.showInformationMessage(
                'CodeGraph: Workspace not indexed. Index now for full code intelligence?',
                'Index Workspace',
                'Later',
            );
            if (choice === 'Index Workspace') {
                reporter.indexRequested('activation_prompt');
                const startedAt = Date.now();
                await vscode.window.withProgress(
                    { location: vscode.ProgressLocation.Notification, title: 'CodeGraph: Indexing workspace...' },
                    async () => {
                        try {
                            const result = await client.sendRequest<any>('workspace/executeCommand', {
                                command: 'codegraph.reindexWorkspace',
                                arguments: [{}],
                            });
                            reportIndexTelemetry(reporter, startedAt, result);
                            const fileCount = filesIndexed(result);
                            // handleIndexOutcome syncs the codegraph.indexed
                            // context key and shows zero-file recovery or the
                            // one-time first-index steer. Confirm success here
                            // only when it didn't show its own prompt.
                            const action = await handleIndexOutcome(context, reporter, fileCount);
                            if (action === 'none' && fileCount > 0) {
                                vscode.window.showInformationMessage(
                                    `CodeGraph: Indexed ${fileCount.toLocaleString()} file${fileCount === 1 ? '' : 's'}`,
                                );
                            }
                        } catch (err) {
                            reporter.indexCompleted({
                                outcome: 'error',
                                durationMs: Date.now() - startedAt,
                                fileCount: 0,
                                errorCategory: 'other',
                            });
                            throw err;
                        }
                    },
                );
            }
        }
    } catch {
        // Server not ready — ensureIndexed() in toolManager will catch later
    }

    // Watch for settings changes and push to LSP server
    context.subscriptions.push(
        vscode.workspace.onDidChangeConfiguration(async (e) => {
            if (e.affectsConfiguration('codegraph') && client) {
                const wsFolder = vscode.workspace.workspaceFolders?.[0]?.uri;
                const updated = vscode.workspace.getConfiguration('codegraph', wsFolder);
                const newConfig = {
                    indexOnStartup: updated.get<boolean>('indexOnStartup'),
                    excludePatterns: updated.get<string[]>('excludePatterns'),
                    indexPaths: updated.get<string[]>('indexPaths'),
                    maxFileSizeKB: updated.get<number>('maxFileSizeKB'),
                    embedOnOpen: updated.get<boolean>('embedOnOpen'),
                };
                try {
                    await client.sendRequest('workspace/executeCommand', {
                        command: 'codegraph.updateConfiguration',
                        arguments: [newConfig],
                    });
                    console.log('[CodeGraph] Configuration updated:', JSON.stringify(newConfig));
                } catch (error) {
                    console.error('[CodeGraph] Failed to update configuration:', error);
                }
            }
        })
    );

    // Register commands, tree providers, etc.
    registerCommands(context, client, aiProvider, reporter);
    registerTreeDataProviders(context, client, reporter);
    registerCodeLens(context, client, reporter);

    // Add debug command to verify tool registration
    context.subscriptions.push(
        vscode.commands.registerCommand('codegraph.debugTools', async () => {
            try {
                // Check if vscode.lm exists
                if (!(vscode as any).lm) {
                    vscode.window.showErrorMessage('❌ vscode.lm API not available. VS Code version may be too old (need 1.90+)');
                    return;
                }

                // Get all registered tools (API might be different)
                const lmApi = (vscode as any).lm;
                let allTools: any[] = [];

                // Try to get tools
                if (typeof lmApi.tools === 'function') {
                    allTools = await lmApi.tools();
                } else if (Array.isArray(lmApi.tools)) {
                    allTools = lmApi.tools;
                } else {
                    vscode.window.showWarningMessage('Unable to access vscode.lm.tools - API shape unknown');
                }

                const codegraphTools = allTools.filter(t => t && t.name && t.name.startsWith('codegraph_'));

                // Show results
                const message = [
                    '📊 CodeGraph Tools Debug Info:',
                    `VS Code version: ${vscode.version}`,
                    `Total LM tools: ${allTools.length}`,
                    `CodeGraph tools: ${codegraphTools.length}`,
                    '',
                    codegraphTools.length > 0 ? 'CodeGraph tools found:' : 'No CodeGraph tools found',
                    ...codegraphTools.map(t => `  ✓ ${t.name}`)
                ].join('\n');

                vscode.window.showInformationMessage(message, { modal: true });

                // Also log to console
                console.log('=== CodeGraph Tools Debug ===');
                console.log('VS Code version:', vscode.version);
                console.log('All tools:', allTools.map(t => t?.name || 'unnamed'));
                console.log('CodeGraph tools:', codegraphTools.map(t => t.name));
                console.log('Tool manager instance:', toolManager);
                console.log('Tool manager disposables count:', (toolManager as any).disposables?.length);
            } catch (error) {
                vscode.window.showErrorMessage(`Error checking tools: ${error}`);
                console.error('Debug tools error:', error);
            }
        })
    );

    // Add to disposables
    context.subscriptions.push(client, toolManager);

    // Set context for conditional UI
    vscode.commands.executeCommand('setContext', 'codegraph.enabled', true);
}

export async function deactivate(): Promise<void> {
    if (reporter) {
        await reporter.dispose();
    }
    if (client) {
        expectedShutdown = true;
        await client.stop();
    }
}

