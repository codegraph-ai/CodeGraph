// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

import * as vscode from 'vscode';
import * as os from 'os';
import {
    LanguageClient,
    LanguageClientOptions,
    ServerOptions,
    TransportKind,
} from 'vscode-languageclient/node';
import { registerCommands } from './commands';
import { registerTreeDataProviders } from './views/treeProviders';
import { CodeGraphAIProvider } from './ai/contextProvider';
import { CodeGraphToolManager } from './ai/toolManager';
import { getServerPath } from './server';
import { createReporter, setServerEdition, type Reporter } from './telemetry/reporter';

let client: LanguageClient;
let aiProvider: CodeGraphAIProvider;
let toolManager: CodeGraphToolManager;
let reporter: Reporter;
let serverUptimeStart = 0;
let serverRestartCount = 0;

export async function activate(context: vscode.ExtensionContext): Promise<void> {
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
    reporter = createReporter(context);
    context.subscriptions.push({ dispose: () => { void reporter.dispose(); } });
    reporter.activationStart({
        enabledSetting: config.get<boolean>('enabled', true),
        workspaceFolders: vscode.workspace.workspaceFolders?.length ?? 0,
        hasMultiRoot: (vscode.workspace.workspaceFolders?.length ?? 0) > 1,
    });

    if (!config.get<boolean>('enabled', true)) {
        return;
    }

    // Determine server binary path
    const serverInfo = getServerPath(context);
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

    // Server options - add Windows-specific spawn options
    const isWindows = os.platform() === 'win32';
    const serverOptions: ServerOptions = {
        command: serverModule,
        args: [],
        transport: TransportKind.stdio,
        options: {
            // On Windows, we need shell: true to properly spawn .exe files
            shell: isWindows,
            // Ensure proper working directory
            cwd: context.extensionPath,
        },
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
                fullBodyEmbedding: latestConfig.get<boolean>('fullBodyEmbedding'),
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
        reporter.activationServerStartResult({
            outcome: String(error).toLowerCase().includes('timeout') ? 'timeout' : 'spawn_fail',
            durationMs: Date.now() - serverStartBegan,
            serverBinaryFound: !!serverInfo.path,
        });
        return;
    }

    // Watch for unexpected server state changes — crashes fire here.
    context.subscriptions.push(
        client.onDidChangeState((evt) => {
            // evt.newState: 1=Stopped, 2=Starting, 3=Running
            if (evt.newState === 1 && serverUptimeStart > 0) {
                reporter.serverCrash({
                    uptimeSeconds: (Date.now() - serverUptimeStart) / 1000,
                    restartCount: serverRestartCount,
                    lastToolName: toolManager?.lastToolName,
                });
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
        toolManager = new CodeGraphToolManager(client, reporter);
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

    // Check if workspace is indexed — prompt if not
    try {
        const check = await client.sendRequest<any>('workspace/executeCommand', {
            command: 'codegraph.symbolSearch',
            arguments: [{ query: '*', limit: 1 }],
        });
        if (!check?.results?.length) {
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
                            reportIndexCompleted(reporter, startedAt, result);
                            vscode.window.showInformationMessage(`Indexed ${result?.files_indexed ?? 0} files`);
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
        await client.stop();
    }
}

/**
 * Map the reindex-RPC response (which now ships `by_language` /
 * `parser_errors_by_language` / `duration_ms` from the server) into
 * the appropriate telemetry events. Two events fire per index:
 *   - `index.completed` with the aggregate numbers
 *   - `index.languageBreakdown` with the per-language file counts
 * The wall-clock duration is computed locally for cancel/error paths
 * but the server-side `duration_ms` is used when present (it excludes
 * network RTT and is more accurate for product-decision purposes).
 */
function reportIndexCompleted(r: Reporter, localStartedAt: number, result: any): void {
    const fileCount = typeof result?.files_indexed === 'number' ? result.files_indexed : 0;
    const durationMs =
        typeof result?.duration_ms === 'number'
            ? Number(result.duration_ms)
            : Date.now() - localStartedAt;
    r.indexCompleted({ outcome: 'ok', durationMs, fileCount });

    const byLanguage = result?.by_language;
    if (byLanguage && typeof byLanguage === 'object') {
        const map = new Map<any, number>();
        for (const [lang, count] of Object.entries(byLanguage)) {
            if (typeof count === 'number') map.set(lang as any, count);
        }
        if (map.size > 0) r.indexLanguageBreakdown(map as any);
    }
}
