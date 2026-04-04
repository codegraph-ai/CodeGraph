// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

import * as vscode from 'vscode';
import { LanguageClient, RequestType } from 'vscode-languageclient/node';
import { CodeGraphAIProvider } from '../ai/contextProvider';
import {
    DependencyGraphParams,
    DependencyGraphResponse,
    CallGraphParams,
    CallGraphResponse,
    ImpactAnalysisParams,
    ImpactAnalysisResponse,
    ParserMetricsParams,
    ParserMetricsResponse,
} from '../types';
import { GraphVisualizationPanel } from '../views/graphPanel';

// Define custom request types (used for LSP type inference)
// eslint-disable-next-line @typescript-eslint/no-unused-vars
namespace GetDependencyGraphRequest {
    export const type = new RequestType<DependencyGraphParams, DependencyGraphResponse, void>(
        'codegraph/getDependencyGraph'
    );
}

// eslint-disable-next-line @typescript-eslint/no-unused-vars
namespace GetCallGraphRequest {
    export const type = new RequestType<CallGraphParams, CallGraphResponse, void>(
        'codegraph/getCallGraph'
    );
}

// eslint-disable-next-line @typescript-eslint/no-unused-vars
namespace GetImpactAnalysisRequest {
    export const type = new RequestType<ImpactAnalysisParams, ImpactAnalysisResponse, void>(
        'codegraph/analyzeImpact'
    );
}

// eslint-disable-next-line @typescript-eslint/no-unused-vars
namespace GetParserMetricsRequest {
    export const type = new RequestType<ParserMetricsParams, ParserMetricsResponse, void>(
        'codegraph/getParserMetrics'
    );
}

// eslint-disable-next-line @typescript-eslint/no-unused-vars
namespace ReindexWorkspaceRequest {
    export const type = new RequestType<void, void, void>(
        'codegraph/reindexWorkspace'
    );
}

/**
 * Register all CodeGraph commands
 */
export function registerCommands(
    context: vscode.ExtensionContext,
    client: LanguageClient,
    _aiProvider: CodeGraphAIProvider
): void {
    // Helper to safely register commands
    const safeRegisterCommand = (commandId: string, callback: (...args: any[]) => any) => {
        try {
            context.subscriptions.push(vscode.commands.registerCommand(commandId, callback));
        } catch (error) {
            console.warn(`Command ${commandId} already registered, skipping`);
        }
    };

    // Show Dependency Graph
    safeRegisterCommand('codegraph.showDependencyGraph', async () => {
            const editor = vscode.window.activeTextEditor;
            if (!editor) {
                vscode.window.showWarningMessage('CodeGraph: No active editor');
                return;
            }

            try {
                const response = await client.sendRequest('workspace/executeCommand', {
                    command: 'codegraph.getDependencyGraph',
                    arguments: [{
                        uri: editor.document.uri.toString(),
                        depth: vscode.workspace.getConfiguration('codegraph')
                            .get<number>('visualization.defaultDepth', 3),
                        includeExternal: false,
                        direction: 'both',
                    }]
                }) as DependencyGraphResponse;

                GraphVisualizationPanel.createOrShow(
                    context.extensionUri,
                    client,
                    'dependency',
                    response
                );
            } catch (error) {
                vscode.window.showErrorMessage(`CodeGraph: Failed to get dependency graph: ${error}`);
            }
    });

    // Show Call Graph
    safeRegisterCommand('codegraph.showCallGraph', async () => {
            const editor = vscode.window.activeTextEditor;
            if (!editor) {
                vscode.window.showWarningMessage('CodeGraph: No active editor');
                return;
            }

            try {
                const response = await client.sendRequest('workspace/executeCommand', {
                    command: 'codegraph.getCallGraph',
                    arguments: [{
                        uri: editor.document.uri.toString(),
                        position: {
                            line: editor.selection.active.line,
                            character: editor.selection.active.character,
                        },
                        direction: 'both',
                        depth: vscode.workspace.getConfiguration('codegraph')
                            .get<number>('visualization.defaultDepth', 3),
                        includeExternal: false,
                    }]
                }) as CallGraphResponse;

                GraphVisualizationPanel.createOrShow(
                    context.extensionUri,
                    client,
                    'call',
                    response
                );
            } catch (error) {
                vscode.window.showErrorMessage(`CodeGraph: Failed to get call graph: ${error}`);
            }
    });

    // Analyze Impact
    safeRegisterCommand('codegraph.analyzeImpact', async () => {
            const editor = vscode.window.activeTextEditor;
            if (!editor) {
                vscode.window.showWarningMessage('CodeGraph: No active editor');
                return;
            }

            // Ask user for analysis type
            const analysisType = await vscode.window.showQuickPick(
                [
                    { label: 'Modify', value: 'modify', description: 'Impact if this symbol is modified' },
                    { label: 'Delete', value: 'delete', description: 'Impact if this symbol is deleted' },
                    { label: 'Rename', value: 'rename', description: 'Impact if this symbol is renamed' },
                ],
                { placeHolder: 'Select analysis type' }
            );

            if (!analysisType) {
                return;
            }

            try {
                const response = await client.sendRequest('workspace/executeCommand', {
                    command: 'codegraph.analyzeImpact',
                    arguments: [{
                        uri: editor.document.uri.toString(),
                        position: {
                            line: editor.selection.active.line,
                            character: editor.selection.active.character,
                        },
                        analysisType: analysisType.value as 'modify' | 'delete' | 'rename',
                    }]
                }) as ImpactAnalysisResponse;

                // Show impact analysis results
                showImpactAnalysisResults(response);
            } catch (error) {
                vscode.window.showErrorMessage(`CodeGraph: Failed to analyze impact: ${error}`);
            }
    });

    // Show Parser Metrics
    safeRegisterCommand('codegraph.showMetrics', async () => {
            try {
                const response = await client.sendRequest('workspace/executeCommand', {
                    command: 'codegraph.getParserMetrics',
                    arguments: []
                }) as ParserMetricsResponse;

                showMetricsPanel(response);
            } catch (error) {
                vscode.window.showErrorMessage(`CodeGraph: Failed to get metrics: ${error}`);
            }
    });

    // Open AI Chat - Opens VS Code's chat with @codegraph participant
    safeRegisterCommand('codegraph.openAIChat', async () => {
            // Open the VS Code chat view and suggest using @codegraph
            try {
                // Try to open the chat panel with a suggested message
                await vscode.commands.executeCommand('workbench.action.chat.open', {
                    query: '@codegraph '
                });
            } catch {
                // Fallback: just open chat panel if the query parameter isn't supported
                try {
                    await vscode.commands.executeCommand('workbench.action.chat.open');
                    vscode.window.showInformationMessage(
                        'CodeGraph: Use @codegraph in the chat to get code context. ' +
                        'Try: @codegraph explain this function'
                    );
                } catch (error) {
                    // Chat panel not available - show helpful message
                    vscode.window.showInformationMessage(
                        'CodeGraph provides AI context via:\n' +
                        '• @codegraph in any AI chat (Claude, Copilot)\n' +
                        '• Language Model Tools (codegraph_* tools)\n' +
                        'Open any AI chat and type @codegraph'
                    );
                }
            }
    });

    // Reindex Workspace
    safeRegisterCommand('codegraph.reindex', async () => {
            try {
                await vscode.window.withProgress(
                    {
                        location: vscode.ProgressLocation.Notification,
                        title: 'CodeGraph: Reindexing workspace...',
                        cancellable: false,
                    },
                    async () => {
                        await client.sendRequest('workspace/executeCommand', {
                            command: 'codegraph.reindexWorkspace',
                            arguments: []
                        });
                    }
                );
                vscode.window.showInformationMessage('CodeGraph: Workspace reindexed successfully');
            } catch (error) {
                vscode.window.showErrorMessage(`CodeGraph: Failed to reindex workspace: ${error}`);
            }
    });

    // Index Directory - pick folders to index on demand
    safeRegisterCommand('codegraph.indexDirectory', async () => {
            const uris = await vscode.window.showOpenDialog({
                canSelectFolders: true,
                canSelectFiles: false,
                canSelectMany: true,
                openLabel: 'Index',
                title: 'Select directories to index',
            });

            if (!uris || uris.length === 0) {
                return;
            }

            const paths = uris.map(uri => uri.fsPath);

            try {
                await vscode.window.withProgress(
                    {
                        location: vscode.ProgressLocation.Notification,
                        title: `CodeGraph: Indexing ${paths.length} director${paths.length === 1 ? 'y' : 'ies'}...`,
                        cancellable: false,
                    },
                    async () => {
                        await client.sendRequest('workspace/executeCommand', {
                            command: 'codegraph.indexDirectory',
                            arguments: [{ paths }]
                        });
                    }
                );
                vscode.window.showInformationMessage(
                    `CodeGraph: Indexed ${paths.length} director${paths.length === 1 ? 'y' : 'ies'} successfully`
                );
            } catch (error) {
                vscode.window.showErrorMessage(`CodeGraph: Failed to index directories: ${error}`);
            }
    });
}

/**
 * Show impact analysis results in an output panel
 */
function showImpactAnalysisResults(response: ImpactAnalysisResponse): void {
    const outputChannel = vscode.window.createOutputChannel('CodeGraph Impact Analysis');
    outputChannel.clear();

    outputChannel.appendLine('=== Impact Analysis Results ===\n');
    outputChannel.appendLine(`Summary:`);
    outputChannel.appendLine(`  Files Affected: ${response.summary.filesAffected}`);
    outputChannel.appendLine(`  Breaking Changes: ${response.summary.breakingChanges}`);
    outputChannel.appendLine(`  Warnings: ${response.summary.warnings}`);

    if (response.directImpact.length > 0) {
        outputChannel.appendLine('\n--- Direct Impact ---');
        for (const impact of response.directImpact) {
            const severityIcon = impact.severity === 'breaking' ? '🔴' :
                impact.severity === 'warning' ? '🟡' : '🔵';
            outputChannel.appendLine(`${severityIcon} ${impact.type}: ${impact.uri}`);
            outputChannel.appendLine(`   Line ${impact.range.start.line + 1}`);
        }
    }

    if (response.indirectImpact.length > 0) {
        outputChannel.appendLine('\n--- Indirect Impact ---');
        for (const impact of response.indirectImpact) {
            const severityIcon = impact.severity === 'breaking' ? '🔴' :
                impact.severity === 'warning' ? '🟡' : '🔵';
            outputChannel.appendLine(`${severityIcon} ${impact.uri}`);
            outputChannel.appendLine(`   Path: ${impact.path.join(' → ')}`);
        }
    }

    if (response.affectedTests.length > 0) {
        outputChannel.appendLine('\n--- Affected Tests ---');
        for (const test of response.affectedTests) {
            outputChannel.appendLine(`🧪 ${test.testName}`);
            outputChannel.appendLine(`   ${test.uri}`);
        }
    }

    outputChannel.show();
}

/**
 * Show parser metrics in an output panel
 */
function showMetricsPanel(response: ParserMetricsResponse): void {
    const outputChannel = vscode.window.createOutputChannel('CodeGraph Metrics');
    outputChannel.clear();

    outputChannel.appendLine('=== CodeGraph Parser Metrics ===\n');

    outputChannel.appendLine('Overall:');
    outputChannel.appendLine(`  Files Attempted: ${response.totals.filesAttempted}`);
    outputChannel.appendLine(`  Files Succeeded: ${response.totals.filesSucceeded}`);
    outputChannel.appendLine(`  Files Failed: ${response.totals.filesFailed}`);
    outputChannel.appendLine(`  Total Entities: ${response.totals.totalEntities}`);
    outputChannel.appendLine(`  Success Rate: ${(response.totals.successRate * 100).toFixed(1)}%`);

    outputChannel.appendLine('\nBy Language:');
    for (const metric of response.metrics) {
        outputChannel.appendLine(`\n  ${metric.language.toUpperCase()}:`);
        outputChannel.appendLine(`    Files: ${metric.filesSucceeded}/${metric.filesAttempted}`);
        outputChannel.appendLine(`    Entities: ${metric.totalEntities}`);
        outputChannel.appendLine(`    Relationships: ${metric.totalRelationships}`);
        outputChannel.appendLine(`    Parse Time: ${metric.totalParseTimeMs}ms (avg: ${metric.avgParseTimeMs}ms)`);
    }

    outputChannel.show();
}

