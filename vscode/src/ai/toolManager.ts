// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

import * as vscode from 'vscode';
import { LanguageClient } from 'vscode-languageclient/node';
import {
    DependencyGraphResponse,
    CallGraphResponse,
    ImpactAnalysisResponse,
    AIContextResponse,
    EditContextResponse,
    CuratedContextResponse,
    RelatedTestsResponse,
    ComplexityResponse,
    SymbolSearchResponse,
    FindByImportsResponse,
    FindEntryPointsResponse,
    TraverseGraphResponse,
    GetCallersResponse,
    DetailedSymbolResponse,
    FindBySignatureResponse,
    // Memory Layer Types
    MemoryStoreResponse,
    MemorySearchResponse,
    MemoryGetResponse,
    MemoryContextResponse,
    MemoryInvalidateResponse,
    MemoryListResponse,
    MemoryStatsResponse,
} from '../types';

/**
 * Manages Language Model Tool registrations for CodeGraph.
 *
 * This enables AI agents (Claude, GitHub Copilot, etc.) to autonomously
 * discover and use CodeGraph capabilities through VS Code's Language Model API.
 */
export class CodeGraphToolManager {
    private disposables: vscode.Disposable[] = [];
    private indexChecked = false;
    private isIndexed = false;

    constructor(private client: LanguageClient) {}

    /**
     * Check if workspace is indexed. Prompt to index on first tool use.
     */
    private async ensureIndexed(): Promise<void> {
        if (this.indexChecked) return;
        this.indexChecked = true;

        try {
            const result = await this.client.sendRequest<any>(
                'workspace/executeCommand',
                { command: 'codegraph.symbolSearch', arguments: [{ query: '*', limit: 1 }] },
            );
            if (result?.results?.length > 0) {
                this.isIndexed = true;
                return;
            }
        } catch {
            // Server not ready — skip check
            return;
        }

        const choice = await vscode.window.showWarningMessage(
            'Workspace is not indexed yet. Index now for full code intelligence?',
            'Index Workspace',
            'Later',
        );
        if (choice === 'Index Workspace') {
            await vscode.window.withProgress(
                { location: vscode.ProgressLocation.Notification, title: 'CodeGraph: Indexing workspace...' },
                async () => {
                    const result = await this.client.sendRequest<any>(
                        'workspace/executeCommand',
                        { command: 'codegraph.reindexWorkspace', arguments: [{}] },
                    );
                    this.isIndexed = true;
                    vscode.window.showInformationMessage(`Indexed ${result?.files_indexed ?? 0} files`);
                },
            );
        }
    }

    /**
     * Execute an LSP command with a small retry/backoff to smooth over transient timeouts.
     */
    private async sendRequestWithRetry<T>(
        command: string,
        args: unknown,
        token: vscode.CancellationToken,
        options: { retries?: number; delayMs?: number; backoffFactor?: number } = {}
    ): Promise<T> {
        await this.ensureIndexed();
        const retries = options.retries ?? 1;
        let delay = options.delayMs ?? 250;
        const backoffFactor = options.backoffFactor ?? 2;

        for (let attempt = 0; attempt <= retries; attempt++) {
            if (token.isCancellationRequested) {
                throw new Error('cancelled');
            }

            try {
                const result = await this.client.sendRequest(
                    'workspace/executeCommand',
                    { command, arguments: [args] },
                    token
                );
                if (result == null) {
                    throw new Error(`${command} returned null — server may be busy or restarting`);
                }
                return result as T;
            } catch (error) {
                const isLastAttempt = attempt === retries;
                if (isLastAttempt || !this.isRetryableError(error)) {
                    throw error;
                }

                await new Promise(resolve => setTimeout(resolve, delay));
                delay *= backoffFactor;
            }
        }

        throw new Error('Request failed after retries');
    }

    private isRetryableError(error: unknown): boolean {
        const message = String(error).toLowerCase();
        return (
            message.includes('timeout') ||
            message.includes('timed out') ||
            message.includes('temporarily unavailable') ||
            message.includes('requestcancelled') ||
            message.includes('cancelled') ||
            message.includes('canceled')
        );
    }

    /**
     * Handle errors from tool invocations, including cancellation.
     * Returns a user-friendly message appropriate for AI agents.
     */
    private handleToolError(error: unknown, toolName: string, token?: vscode.CancellationToken): vscode.LanguageModelToolResult {
        // Check if operation was cancelled
        if (token?.isCancellationRequested) {
            return new vscode.LanguageModelToolResult([
                new vscode.LanguageModelTextPart(`Operation cancelled: ${toolName} was stopped by user request.`)
            ]);
        }

        // Check for cancellation error patterns
        const errorMessage = String(error);
        if (errorMessage.includes('cancelled') || errorMessage.includes('canceled') || errorMessage.includes('RequestCancelled')) {
            return new vscode.LanguageModelToolResult([
                new vscode.LanguageModelTextPart(`Operation cancelled: ${toolName} was stopped.`)
            ]);
        }

        // Return generic error with context
        return new vscode.LanguageModelToolResult([
            new vscode.LanguageModelTextPart(`Error in ${toolName}: ${errorMessage}`)
        ]);
    }

    /**
     * Register all CodeGraph tools with the Language Model API.
     *
     * Tools are automatically discoverable by all AI agents in VS Code.
     * AI agents can call these tools autonomously without user interaction.
     */
    registerTools(): void {
        console.log('[CodeGraph] Registering Language Model tools...');

        // Check if vscode.lm API exists
        if (!(vscode as any).lm) {
            console.error('[CodeGraph] vscode.lm API not available - VS Code version may be too old (need 1.90+)');
            vscode.window.showWarningMessage('CodeGraph: Language Model Tools require VS Code 1.90+. Tool registration skipped.');
            return;
        }

        if (typeof (vscode as any).lm.registerTool !== 'function') {
            console.error('[CodeGraph] vscode.lm.registerTool is not a function - API might have changed');
            vscode.window.showWarningMessage('CodeGraph: vscode.lm.registerTool not available. Tool registration skipped.');
            return;
        }

        console.log('[CodeGraph] vscode.lm API available, proceeding with tool registration...');

        // Tool 1: Get Dependency Graph
        this.disposables.push(
            vscode.lm.registerTool('codegraph_get_dependency_graph', {
                invoke: async (options, token) => {
                    const input = options.input as { uri: string; depth?: number; includeExternal?: boolean; direction?: 'imports' | 'importedBy' | 'both'; summary?: boolean };
                    const { uri, depth = 3, includeExternal = false, direction = 'both', summary = false } = input;

                    try {
                        const response = await this.sendRequestWithRetry<DependencyGraphResponse>(
                            'codegraph.getDependencyGraph',
                            { uri, depth, includeExternal, direction },
                            token,
                            { retries: 1 }
                        );

                        return new vscode.LanguageModelToolResult([
                            new vscode.LanguageModelTextPart(this.formatDependencyGraph(response, summary))
                        ]);
                    } catch (error) {
                        return this.handleToolError(error, 'dependency graph', token);
                    }
                },
                prepareInvocation: async (options, _token) => {
                    const input = options.input as { uri: string; depth?: number; direction?: string };
                    const { uri, depth = 3, direction = 'both' } = input;
                    const fileName = vscode.Uri.parse(uri).path.split('/').pop();
                    const directionLabel = direction === 'both' ? 'imports and dependents' : direction === 'importedBy' ? 'dependents' : 'imports';

                    return {
                        invocationMessage: `Analyzing ${directionLabel} for ${fileName} (depth: ${depth})...`
                    };
                }
            })
        );

        // Tool 2: Get Call Graph
        this.disposables.push(
            vscode.lm.registerTool('codegraph_get_call_graph', {
                invoke: async (options, token) => {
                    const input = options.input as { uri: string; line: number; character?: number; depth?: number; direction?: 'callers' | 'callees' | 'both'; summary?: boolean };
                    const { uri, line, character = 0, depth = 3, direction = 'both', summary = false } = input;

                    try {
                        const response = await this.sendRequestWithRetry<CallGraphResponse>(
                            'codegraph.getCallGraph',
                            {
                                uri,
                                position: { line, character },
                                depth,
                                direction,
                                includeExternal: false,
                            },
                            token,
                            { retries: 1 }
                        );

                        return new vscode.LanguageModelToolResult([
                            new vscode.LanguageModelTextPart(this.formatCallGraph(response, summary))
                        ]);
                    } catch (error) {
                        return this.handleToolError(error, 'call graph', token);
                    }
                },
                prepareInvocation: async (options, _token) => {
                    const input = options.input as { uri: string; line: number; depth?: number; direction?: string };
                    const { uri, line, depth = 3, direction = 'both' } = input;
                    const fileName = vscode.Uri.parse(uri).path.split('/').pop();
                    const directionLabel = direction === 'both' ? 'callers and callees' : direction;

                    return {
                        invocationMessage: `Analyzing ${directionLabel} for ${fileName}:${line + 1} (depth: ${depth})...`,
                        confirmationMessages: depth > 5 ? {
                            title: 'Deep Call Graph Analysis',
                            message: `Analyzing call graph with depth ${depth} may take longer on large codebases.`
                        } : undefined
                    };
                }
            })
        );

        // Tool 3: Analyze Impact
        this.disposables.push(
            vscode.lm.registerTool('codegraph_analyze_impact', {
                invoke: async (options, token) => {
                    const input = options.input as { uri: string; line: number; character?: number; changeType?: 'modify' | 'delete' | 'rename'; summary?: boolean };
                    const { uri, line, character = 0, changeType = 'modify', summary = false } = input;

                    try {
                        const response = await this.sendRequestWithRetry<ImpactAnalysisResponse>(
                            'codegraph.analyzeImpact',
                            {
                                uri,
                                position: { line, character },
                                analysisType: changeType,
                            },
                            token,
                            { retries: 1 }
                        );

                        return new vscode.LanguageModelToolResult([
                            new vscode.LanguageModelTextPart(this.formatImpactAnalysis(response, summary))
                        ]);
                    } catch (error) {
                        return this.handleToolError(error, 'impact analysis', token);
                    }
                },
                prepareInvocation: async (options, _token) => {
                    const input = options.input as { uri: string; line: number; changeType?: string };
                    const { uri, line, changeType = 'modify' } = input;
                    const fileName = vscode.Uri.parse(uri).path.split('/').pop();

                    return {
                        invocationMessage: `Analyzing ${changeType} impact for ${fileName}:${line + 1}...`,
                        confirmationMessages: changeType === 'delete' ? {
                            title: 'Delete Impact Analysis',
                            message: 'Analyzing what would break if this symbol is deleted.'
                        } : undefined
                    };
                }
            })
        );

        // Tool 4: Get AI Context
        this.disposables.push(
            vscode.lm.registerTool('codegraph_get_ai_context', {
                invoke: async (options, token) => {
                    const input = options.input as { uri: string; line: number; character?: number; intent?: 'explain' | 'modify' | 'debug' | 'test'; maxTokens?: number };
                    const { uri, line, character = 0, intent = 'explain', maxTokens = 4000 } = input;

                    try {
                        const response = await this.sendRequestWithRetry<AIContextResponse>(
                            'codegraph.getAIContext',
                            {
                                uri,
                                position: { line, character },
                                contextType: intent,
                                maxTokens,
                            },
                            token,
                            { retries: 1 }
                        );

                        return new vscode.LanguageModelToolResult([
                            new vscode.LanguageModelTextPart(this.formatAIContext(response))
                        ]);
                    } catch (error) {
                        // Check for cancellation first
                        if (token.isCancellationRequested) {
                            return this.handleToolError(error, 'AI context', token);
                        }

                        const errorMessage = String(error);
                        let helpfulMessage = '# AI Context Unavailable\n\n';

                        if (errorMessage.includes('No symbol at position')) {
                            helpfulMessage += '❌ No code symbol found at the specified position.\n\n';
                            helpfulMessage += '**This could mean:**\n';
                            helpfulMessage += '- The position is in whitespace, comments, or imports\n';
                            helpfulMessage += '- The file has not been indexed by CodeGraph yet\n';
                            helpfulMessage += '- The specified line/character is out of bounds\n\n';
                            helpfulMessage += '**Try:**\n';
                            helpfulMessage += '- Place cursor on a function, class, or variable definition\n';
                            helpfulMessage += '- Run "CodeGraph: Reindex Workspace" to update the index\n';
                            helpfulMessage += '- Verify the file is a supported language (TypeScript, JavaScript, Python, Rust, Go)\n';
                        } else if (errorMessage.includes('cancelled') || errorMessage.includes('canceled')) {
                            return this.handleToolError(error, 'AI context', token);
                        } else {
                            helpfulMessage += `Error: ${errorMessage}\n`;
                        }

                        return new vscode.LanguageModelToolResult([
                            new vscode.LanguageModelTextPart(helpfulMessage)
                        ]);
                    }
                },
                prepareInvocation: async (options, _token) => {
                    const input = options.input as { uri: string; line: number; intent?: string };
                    const { uri, line, intent = 'explain' } = input;
                    const fileName = vscode.Uri.parse(uri).path.split('/').pop();
                    const intentLabels: Record<string, string> = {
                        explain: 'explanation',
                        modify: 'modification',
                        debug: 'debugging',
                        test: 'testing'
                    };

                    return {
                        invocationMessage: `Getting ${intentLabels[intent] || intent} context for ${fileName}:${line + 1}...`
                    };
                }
            })
        );

        // Tool 5: Get Edit Context
        this.disposables.push(
            vscode.lm.registerTool('codegraph_get_edit_context', {
                invoke: async (options, token) => {
                    const input = options.input as { uri: string; line: number; maxTokens?: number };
                    const { uri, line, maxTokens = 8000 } = input;

                    try {
                        const response = await this.sendRequestWithRetry<EditContextResponse>(
                            'codegraph.getEditContext',
                            { uri, line, maxTokens },
                            token,
                            { retries: 1 }
                        );

                        return new vscode.LanguageModelToolResult([
                            new vscode.LanguageModelTextPart(this.formatEditContext(response))
                        ]);
                    } catch (error) {
                        return this.handleToolError(error, 'edit context', token);
                    }
                },
                prepareInvocation: async (options, _token) => {
                    const input = options.input as { uri: string; line: number };
                    const { uri, line } = input;
                    const fileName = vscode.Uri.parse(uri).path.split('/').pop();

                    return {
                        invocationMessage: `Assembling edit context for ${fileName}:${line + 1}...`
                    };
                }
            })
        );

        // Tool 6: Get Curated Context
        this.disposables.push(
            vscode.lm.registerTool('codegraph_get_curated_context', {
                invoke: async (options, token) => {
                    const input = options.input as { query: string; uri?: string; maxTokens?: number; maxSymbols?: number };
                    const { query, uri, maxTokens = 8000, maxSymbols = 5 } = input;

                    try {
                        const response = await this.sendRequestWithRetry<CuratedContextResponse>(
                            'codegraph.getCuratedContext',
                            { query, uri, maxTokens, maxSymbols },
                            token,
                            { retries: 1 }
                        );

                        return new vscode.LanguageModelToolResult([
                            new vscode.LanguageModelTextPart(this.formatCuratedContext(response))
                        ]);
                    } catch (error) {
                        return this.handleToolError(error, 'curated context', token);
                    }
                },
                prepareInvocation: async (options, _token) => {
                    const input = options.input as { query: string };
                    return {
                        invocationMessage: `Searching codebase for context: "${input.query}"...`
                    };
                }
            })
        );

        // Tool 7: Find Related Tests
        this.disposables.push(
            vscode.lm.registerTool('codegraph_find_related_tests', {
                invoke: async (options, token) => {
                    const input = options.input as { uri: string; line?: number; limit?: number };
                    const { uri, line = 0, limit = 10 } = input;

                    try {
                        const response = await this.sendRequestWithRetry<RelatedTestsResponse>(
                            'codegraph.findRelatedTests',
                            {
                                uri,
                                position: { line, character: 0 },
                                limit,
                            },
                            token,
                            { retries: 1 }
                        );

                        return new vscode.LanguageModelToolResult([
                            new vscode.LanguageModelTextPart(this.formatRelatedTests(response))
                        ]);
                    } catch (error) {
                        // Check for cancellation first
                        if (token.isCancellationRequested) {
                            return this.handleToolError(error, 'find related tests', token);
                        }

                        const errorMessage = String(error);
                        if (errorMessage.includes('cancelled') || errorMessage.includes('canceled')) {
                            return this.handleToolError(error, 'find related tests', token);
                        }

                        let helpfulMessage = '# Related Tests Not Found\n\n';

                        if (errorMessage.includes('No symbol at position')) {
                            helpfulMessage += '❌ No code symbol found to search for related tests.\n\n';
                            helpfulMessage += '**This could mean:**\n';
                            helpfulMessage += '- The specified position is not on a testable code element\n';
                            helpfulMessage += '- The file has not been indexed by CodeGraph yet\n';
                            helpfulMessage += '- No tests exist for this code (which might be OK)\n\n';
                            helpfulMessage += '**Try:**\n';
                            helpfulMessage += '- Specify a line with a function or class definition\n';
                            helpfulMessage += '- Run "CodeGraph: Reindex Workspace" to update the index\n';
                            helpfulMessage += '- Check if tests actually exist in your codebase\n';
                        } else {
                            helpfulMessage += `Error: ${errorMessage}\n`;
                        }

                        return new vscode.LanguageModelToolResult([
                            new vscode.LanguageModelTextPart(helpfulMessage)
                        ]);
                    }
                },
                prepareInvocation: async (options, _token) => {
                    const input = options.input as { uri: string; line?: number };
                    const { uri, line } = input;
                    const fileName = vscode.Uri.parse(uri).path.split('/').pop();
                    const lineInfo = line !== undefined ? `:${line + 1}` : '';

                    return {
                        invocationMessage: `Finding tests related to ${fileName}${lineInfo}...`
                    };
                }
            })
        );

        // Tool 6: Get Symbol Info
        this.disposables.push(
            vscode.lm.registerTool('codegraph_get_symbol_info', {
                invoke: async (options, token) => {
                    const input = options.input as { uri: string; line: number; character?: number; includeReferences?: boolean };
                    const { uri, line, character = 0, includeReferences = false } = input;

                    try {
                        // Check for cancellation before starting
                        if (token.isCancellationRequested) {
                            return this.handleToolError(new Error('cancelled'), 'symbol info', token);
                        }

                        // Use existing LSP hover/definition/reference providers
                        const doc = await vscode.workspace.openTextDocument(vscode.Uri.parse(uri));
                        const pos = new vscode.Position(line, character);

                        const hovers = await vscode.commands.executeCommand<vscode.Hover[]>(
                            'vscode.executeHoverProvider',
                            doc.uri,
                            pos
                        );

                        const definitions = await vscode.commands.executeCommand<vscode.Location[]>(
                            'vscode.executeDefinitionProvider',
                            doc.uri,
                            pos
                        );

                        // Only fetch references if explicitly requested (can be slow)
                        let references: vscode.Location[] | undefined;
                        if (includeReferences) {
                            // Check for cancellation before expensive operation
                            if (token.isCancellationRequested) {
                                return this.handleToolError(new Error('cancelled'), 'symbol info', token);
                            }

                            // Use a timeout for reference search (5 seconds)
                            const timeoutPromise = new Promise<vscode.Location[]>((_, reject) => {
                                setTimeout(() => reject(new Error('Reference search timed out after 5s')), 5000);
                            });

                            const refPromise = vscode.commands.executeCommand<vscode.Location[]>(
                                'vscode.executeReferenceProvider',
                                doc.uri,
                                pos
                            );

                            try {
                                references = await Promise.race([refPromise, timeoutPromise]);
                            } catch (timeoutErr) {
                                // Reference search timed out, continue without references
                                console.log('[CodeGraph] Reference search timed out, returning partial results');
                            }
                        }

                        return new vscode.LanguageModelToolResult([
                            new vscode.LanguageModelTextPart(this.formatSymbolInfo({
                                hovers,
                                definitions,
                                references,
                                uri,
                                line,
                                character,
                                referencesIncluded: includeReferences,
                                referencesTimedOut: includeReferences && !references
                            }))
                        ]);
                    } catch (error) {
                        return this.handleToolError(error, 'symbol info', token);
                    }
                },
                prepareInvocation: async (options, _token) => {
                    const input = options.input as { uri: string; line: number; includeReferences?: boolean };
                    const { uri, line, includeReferences = false } = input;
                    const fileName = vscode.Uri.parse(uri).path.split('/').pop();
                    const refNote = includeReferences ? ' (including references - may be slow)' : '';

                    return {
                        invocationMessage: `Getting symbol info for ${fileName}:${line + 1}${refNote}...`,
                        confirmationMessages: includeReferences ? {
                            title: 'Reference Search',
                            message: 'Including references can be slow on large workspaces. Consider using without references for faster results.'
                        } : undefined
                    };
                }
            })
        );

        // Tool 7: Analyze Complexity
        this.disposables.push(
            vscode.lm.registerTool('codegraph_analyze_complexity', {
                invoke: async (options, token) => {
                    const input = options.input as { uri: string; line?: number; threshold?: number; summary?: boolean };
                    const { uri, line, threshold = 10, summary = false } = input;

                    try {
                        const response = await this.sendRequestWithRetry<ComplexityResponse>(
                            'codegraph.analyzeComplexity',
                            { uri, line, threshold, includeMetrics: true },
                            token,
                            { retries: 1 }
                        );

                        return new vscode.LanguageModelToolResult([
                            new vscode.LanguageModelTextPart(this.formatComplexityAnalysis(response, summary))
                        ]);
                    } catch (error) {
                        return this.handleToolError(error, 'complexity analysis', token);
                    }
                },
                prepareInvocation: async (options, _token) => {
                    const input = options.input as { uri: string; line?: number; threshold?: number };
                    const { uri, line, threshold = 10 } = input;
                    const fileName = vscode.Uri.parse(uri).path.split('/').pop();
                    const lineInfo = line !== undefined ? ` at line ${line + 1}` : '';

                    return {
                        invocationMessage: `Analyzing complexity for ${fileName}${lineInfo} (threshold: ${threshold})...`
                    };
                }
            })
        );

        // ==========================================
        // AI Agent Query Primitives (Tools 10-16)
        // Fast, composable queries for AI code exploration
        // ==========================================

        // Tool 10: Symbol Search (BM25-based text search)
        this.disposables.push(
            vscode.lm.registerTool('codegraph_symbol_search', {
                invoke: async (options, token) => {
                    const input = options.input as {
                        query: string;
                        symbolTypes?: string[];
                        limit?: number;
                        includePrivate?: boolean;
                    };
                    const { query, symbolTypes, limit = 20, includePrivate = true } = input;

                    try {
                        const response = await this.sendRequestWithRetry<SymbolSearchResponse>(
                            'codegraph.symbolSearch',
                            { query, symbolTypes, limit, includePrivate },
                            token,
                            { retries: 1 }
                        );

                        return new vscode.LanguageModelToolResult([
                            new vscode.LanguageModelTextPart(this.formatSymbolSearch(response))
                        ]);
                    } catch (error) {
                        return this.handleToolError(error, 'symbol search', token);
                    }
                },
                prepareInvocation: async (options, _token) => {
                    const input = options.input as { query: string; limit?: number };
                    const { query, limit = 20 } = input;

                    return {
                        invocationMessage: `Searching for symbols matching "${query}" (limit: ${limit})...`
                    };
                }
            })
        );

        // Tool 11: Find By Imports
        this.disposables.push(
            vscode.lm.registerTool('codegraph_find_by_imports', {
                invoke: async (options, token) => {
                    const input = options.input as {
                        moduleName?: string;
                        libraries?: string[];
                        matchMode?: 'exact' | 'prefix' | 'contains' | 'fuzzy';
                    };
                    // Support both moduleName (from package.json) and libraries (legacy)
                    const libraries = input.libraries || (input.moduleName ? [input.moduleName] : []);
                    const matchMode = input.matchMode || 'contains';

                    if (libraries.length === 0) {
                        return new vscode.LanguageModelToolResult([
                            new vscode.LanguageModelTextPart('Error: No module name provided. Please specify a moduleName to search for.')
                        ]);
                    }

                    try {
                        const response = await this.sendRequestWithRetry<FindByImportsResponse>(
                            'codegraph.findByImports',
                            { libraries, matchMode },
                            token,
                            { retries: 1 }
                        );

                        return new vscode.LanguageModelToolResult([
                            new vscode.LanguageModelTextPart(this.formatFindByImports(response, libraries))
                        ]);
                    } catch (error) {
                        return this.handleToolError(error, 'find by imports', token);
                    }
                },
                prepareInvocation: async (options, _token) => {
                    const input = options.input as { moduleName?: string; libraries?: string[] };
                    const libraries = input.libraries || (input.moduleName ? [input.moduleName] : []);
                    const displayNames = libraries.length > 0 ? libraries.join(', ') : 'modules';

                    return {
                        invocationMessage: `Finding code that imports ${displayNames}...`
                    };
                }
            })
        );

        // Tool 12: Find Entry Points
        this.disposables.push(
            vscode.lm.registerTool('codegraph_find_entry_points', {
                invoke: async (options, token) => {
                    const input = options.input as {
                        entryType?: 'http_handler' | 'cli_command' | 'public_api' | 'event_handler' | 'test_entry' | 'main';
                    };
                    const { entryType } = input;

                    try {
                        const response = await this.sendRequestWithRetry<FindEntryPointsResponse>(
                            'codegraph.findEntryPoints',
                            { entryType },
                            token,
                            { retries: 1 }
                        );

                        return new vscode.LanguageModelToolResult([
                            new vscode.LanguageModelTextPart(this.formatEntryPoints(response))
                        ]);
                    } catch (error) {
                        return this.handleToolError(error, 'find entry points', token);
                    }
                },
                prepareInvocation: async (options, _token) => {
                    const input = options.input as { entryType?: string };
                    const { entryType } = input;
                    const typeLabel = entryType ? ` of type "${entryType}"` : '';

                    return {
                        invocationMessage: `Finding entry points${typeLabel}...`
                    };
                }
            })
        );

        // Tool 13: Traverse Graph
        this.disposables.push(
            vscode.lm.registerTool('codegraph_traverse_graph', {
                invoke: async (options, token) => {
                    const input = options.input as {
                        startNodeId?: string;
                        uri?: string;
                        line?: number;
                        direction?: 'outgoing' | 'incoming' | 'both';
                        depth?: number;
                        filterSymbolTypes?: string[];
                        maxNodes?: number;
                    };
                    const {
                        startNodeId, uri, line, direction = 'outgoing',
                        depth = 3, filterSymbolTypes, maxNodes = 50
                    } = input;

                    try {
                        const response = await this.sendRequestWithRetry<TraverseGraphResponse>(
                            'codegraph.traverseGraph',
                            { startNodeId, uri, line, direction, depth, filterSymbolTypes, maxNodes },
                            token,
                            { retries: 1 }
                        );

                        return new vscode.LanguageModelToolResult([
                            new vscode.LanguageModelTextPart(this.formatTraverseGraph(response, direction))
                        ]);
                    } catch (error) {
                        return this.handleToolError(error, 'traverse graph', token);
                    }
                },
                prepareInvocation: async (options, _token) => {
                    const input = options.input as { uri?: string; line?: number; direction?: string; depth?: number };
                    const { uri, line, direction = 'outgoing', depth = 3 } = input;
                    const fileName = uri ? vscode.Uri.parse(uri).path.split('/').pop() : 'node';
                    const lineInfo = line !== undefined ? `:${line + 1}` : '';

                    return {
                        invocationMessage: `Traversing ${direction} from ${fileName}${lineInfo} (depth: ${depth})...`
                    };
                }
            })
        );

        // Tool 14: Get Callers
        this.disposables.push(
            vscode.lm.registerTool('codegraph_get_callers', {
                invoke: async (options, token) => {
                    const input = options.input as {
                        nodeId?: string;
                        uri?: string;
                        line?: number;
                        depth?: number;
                    };
                    const { nodeId, uri, line, depth = 1 } = input;

                    try {
                        const response = await this.sendRequestWithRetry<GetCallersResponse>(
                            'codegraph.getCallers',
                            { nodeId, uri, line, depth },
                            token,
                            { retries: 1 }
                        );

                        return new vscode.LanguageModelToolResult([
                            new vscode.LanguageModelTextPart(this.formatCallers(response, 'callers'))
                        ]);
                    } catch (error) {
                        return this.handleToolError(error, 'get callers', token);
                    }
                },
                prepareInvocation: async (options, _token) => {
                    const input = options.input as { uri?: string; line?: number; depth?: number };
                    const { uri, line, depth = 1 } = input;
                    const fileName = uri ? vscode.Uri.parse(uri).path.split('/').pop() : 'symbol';
                    const lineInfo = line !== undefined ? `:${line + 1}` : '';

                    return {
                        invocationMessage: `Finding callers of ${fileName}${lineInfo} (depth: ${depth})...`
                    };
                }
            })
        );

        // Tool 15: Get Callees
        this.disposables.push(
            vscode.lm.registerTool('codegraph_get_callees', {
                invoke: async (options, token) => {
                    const input = options.input as {
                        nodeId?: string;
                        uri?: string;
                        line?: number;
                        depth?: number;
                    };
                    const { nodeId, uri, line, depth = 1 } = input;

                    try {
                        const response = await this.sendRequestWithRetry<GetCallersResponse>(
                            'codegraph.getCallees',
                            { nodeId, uri, line, depth },
                            token,
                            { retries: 1 }
                        );

                        return new vscode.LanguageModelToolResult([
                            new vscode.LanguageModelTextPart(this.formatCallers(response, 'callees'))
                        ]);
                    } catch (error) {
                        return this.handleToolError(error, 'get callees', token);
                    }
                },
                prepareInvocation: async (options, _token) => {
                    const input = options.input as { uri?: string; line?: number; depth?: number };
                    const { uri, line, depth = 1 } = input;
                    const fileName = uri ? vscode.Uri.parse(uri).path.split('/').pop() : 'symbol';
                    const lineInfo = line !== undefined ? `:${line + 1}` : '';

                    return {
                        invocationMessage: `Finding callees of ${fileName}${lineInfo} (depth: ${depth})...`
                    };
                }
            })
        );

        // Tool 16: Get Detailed Symbol Info
        this.disposables.push(
            vscode.lm.registerTool('codegraph_get_detailed_symbol', {
                invoke: async (options, token) => {
                    const input = options.input as {
                        nodeId?: string;
                        uri?: string;
                        line?: number;
                        includeCallers?: boolean;
                        includeCallees?: boolean;
                    };
                    const { nodeId, uri, line, includeCallers = true, includeCallees = true } = input;

                    try {
                        const response = await this.sendRequestWithRetry<DetailedSymbolResponse>(
                            'codegraph.getDetailedSymbolInfo',
                            { nodeId, uri, line, includeCallers, includeCallees },
                            token,
                            { retries: 1 }
                        );

                        return new vscode.LanguageModelToolResult([
                            new vscode.LanguageModelTextPart(this.formatDetailedSymbol(response))
                        ]);
                    } catch (error) {
                        return this.handleToolError(error, 'get detailed symbol', token);
                    }
                },
                prepareInvocation: async (options, _token) => {
                    const input = options.input as { uri?: string; line?: number };
                    const { uri, line } = input;
                    const fileName = uri ? vscode.Uri.parse(uri).path.split('/').pop() : 'symbol';
                    const lineInfo = line !== undefined ? `:${line + 1}` : '';

                    return {
                        invocationMessage: `Getting detailed info for ${fileName}${lineInfo}...`
                    };
                }
            })
        );

        // Tool 17: Find By Signature
        this.disposables.push(
            vscode.lm.registerTool('codegraph_find_by_signature', {
                invoke: async (options, token) => {
                    const input = options.input as {
                        namePattern?: string;
                        returnType?: string;
                        paramCount?: { min: number; max: number };
                        modifiers?: ('public' | 'private' | 'static' | 'async' | 'const')[];
                    };
                    const { namePattern, returnType, paramCount, modifiers } = input;

                    try {
                        const response = await this.sendRequestWithRetry<FindBySignatureResponse>(
                            'codegraph.findBySignature',
                            { namePattern, returnType, paramCount, modifiers },
                            token,
                            { retries: 1 }
                        );

                        return new vscode.LanguageModelToolResult([
                            new vscode.LanguageModelTextPart(this.formatFindBySignature(response))
                        ]);
                    } catch (error) {
                        return this.handleToolError(error, 'find by signature', token);
                    }
                },
                prepareInvocation: async (options, _token) => {
                    const input = options.input as {
                        namePattern?: string;
                        returnType?: string;
                        modifiers?: string[];
                    };
                    const { namePattern, returnType, modifiers } = input;

                    const parts: string[] = [];
                    if (namePattern) {parts.push(`name: "${namePattern}"`);}
                    if (returnType) {parts.push(`returns: ${returnType}`);}
                    if (modifiers?.length) {parts.push(`modifiers: ${modifiers.join(', ')}`);}
                    const criteria = parts.length > 0 ? parts.join(', ') : 'all functions';

                    return {
                        invocationMessage: `Finding functions by signature (${criteria})...`
                    };
                }
            })
        );

        // ==========================================
        // Memory Layer Tools (Tools 18-24)
        // ==========================================

        // Tool 18: Memory Store
        this.disposables.push(
            vscode.lm.registerTool('codegraph_memory_store', {
                invoke: async (options, token) => {
                    const input = options.input as {
                        kind: string;
                        title: string;
                        content: string;
                        tags?: string[];
                        codeLinks?: Array<{ nodeId: string; nodeType: string }>;
                        confidence?: number;
                        problem?: string;
                        solution?: string;
                        decision?: string;
                        rationale?: string;
                        description?: string;
                        severity?: string;
                        name?: string;
                        topic?: string;
                    };

                    try {
                        const response = await this.sendRequestWithRetry<MemoryStoreResponse>(
                            'codegraph.memoryStore',
                            input,
                            token,
                            { retries: 1 }
                        );

                        return new vscode.LanguageModelToolResult([
                            new vscode.LanguageModelTextPart(this.formatMemoryStore(response, input.title))
                        ]);
                    } catch (error) {
                        return this.handleToolError(error, 'store memory', token);
                    }
                },
                prepareInvocation: async (options, _token) => {
                    const input = options.input as { kind: string; title: string };
                    return {
                        invocationMessage: `Storing ${input.kind} memory: "${input.title}"...`
                    };
                }
            })
        );

        // Tool 19: Memory Search
        this.disposables.push(
            vscode.lm.registerTool('codegraph_memory_search', {
                invoke: async (options, token) => {
                    const input = options.input as {
                        query: string;
                        limit?: number;
                        tags?: string[];
                        kinds?: string[];
                        currentOnly?: boolean;
                        codeContext?: string[];
                    };

                    try {
                        const response = await this.sendRequestWithRetry<MemorySearchResponse>(
                            'codegraph.memorySearch',
                            input,
                            token,
                            { retries: 1 }
                        );

                        return new vscode.LanguageModelToolResult([
                            new vscode.LanguageModelTextPart(this.formatMemorySearch(response, input.query))
                        ]);
                    } catch (error) {
                        return this.handleToolError(error, 'search memories', token);
                    }
                },
                prepareInvocation: async (options, _token) => {
                    const input = options.input as { query: string };
                    return {
                        invocationMessage: `Searching memories for "${input.query}"...`
                    };
                }
            })
        );

        // Tool 20: Memory Get
        this.disposables.push(
            vscode.lm.registerTool('codegraph_memory_get', {
                invoke: async (options, token) => {
                    const input = options.input as { id: string };

                    try {
                        const response = await this.sendRequestWithRetry<MemoryGetResponse>(
                            'codegraph.memoryGet',
                            input,
                            token,
                            { retries: 1 }
                        );

                        return new vscode.LanguageModelToolResult([
                            new vscode.LanguageModelTextPart(this.formatMemoryGet(response))
                        ]);
                    } catch (error) {
                        return this.handleToolError(error, 'get memory', token);
                    }
                },
                prepareInvocation: async (options, _token) => {
                    const input = options.input as { id: string };
                    return {
                        invocationMessage: `Retrieving memory ${input.id}...`
                    };
                }
            })
        );

        // Tool 21: Memory Context
        this.disposables.push(
            vscode.lm.registerTool('codegraph_memory_context', {
                invoke: async (options, token) => {
                    const input = options.input as {
                        uri: string;
                        position?: { line: number; character: number };
                        limit?: number;
                        kinds?: string[];
                    };

                    try {
                        const response = await this.sendRequestWithRetry<MemoryContextResponse>(
                            'codegraph.memoryContext',
                            input,
                            token,
                            { retries: 1 }
                        );

                        return new vscode.LanguageModelToolResult([
                            new vscode.LanguageModelTextPart(this.formatMemoryContext(response, input.uri))
                        ]);
                    } catch (error) {
                        return this.handleToolError(error, 'get memory context', token);
                    }
                },
                prepareInvocation: async (options, _token) => {
                    const input = options.input as { uri: string };
                    const filename = input.uri.split('/').pop() || input.uri;
                    return {
                        invocationMessage: `Finding relevant memories for ${filename}...`
                    };
                }
            })
        );

        // Tool 22: Memory Invalidate
        this.disposables.push(
            vscode.lm.registerTool('codegraph_memory_invalidate', {
                invoke: async (options, token) => {
                    const input = options.input as { id: string };

                    try {
                        const response = await this.sendRequestWithRetry<MemoryInvalidateResponse>(
                            'codegraph.memoryInvalidate',
                            input,
                            token,
                            { retries: 1 }
                        );

                        return new vscode.LanguageModelToolResult([
                            new vscode.LanguageModelTextPart(this.formatMemoryInvalidate(response, input.id))
                        ]);
                    } catch (error) {
                        return this.handleToolError(error, 'invalidate memory', token);
                    }
                },
                prepareInvocation: async (options, _token) => {
                    const input = options.input as { id: string };
                    return {
                        invocationMessage: `Invalidating memory ${input.id}...`
                    };
                }
            })
        );

        // Tool 23: Memory List
        this.disposables.push(
            vscode.lm.registerTool('codegraph_memory_list', {
                invoke: async (options, token) => {
                    const input = options.input as {
                        kinds?: string[];
                        tags?: string[];
                        currentOnly?: boolean;
                        limit?: number;
                        offset?: number;
                    };

                    try {
                        const response = await this.sendRequestWithRetry<MemoryListResponse>(
                            'codegraph.memoryList',
                            input,
                            token,
                            { retries: 1 }
                        );

                        return new vscode.LanguageModelToolResult([
                            new vscode.LanguageModelTextPart(this.formatMemoryList(response))
                        ]);
                    } catch (error) {
                        return this.handleToolError(error, 'list memories', token);
                    }
                },
                prepareInvocation: async (options, _token) => {
                    const input = options.input as { kinds?: string[]; tags?: string[] };
                    const filters: string[] = [];
                    if (input.kinds?.length) {filters.push(`kinds: ${input.kinds.join(', ')}`);}
                    if (input.tags?.length) {filters.push(`tags: ${input.tags.join(', ')}`);}
                    const filterStr = filters.length > 0 ? ` (${filters.join('; ')})` : '';
                    return {
                        invocationMessage: `Listing memories${filterStr}...`
                    };
                }
            })
        );

        // Tool 24: Memory Stats
        this.disposables.push(
            vscode.lm.registerTool('codegraph_memory_stats', {
                invoke: async (options, token) => {
                    try {
                        const response = await this.sendRequestWithRetry<MemoryStatsResponse>(
                            'codegraph.memoryStats',
                            {},
                            token,
                            { retries: 1 }
                        );

                        return new vscode.LanguageModelToolResult([
                            new vscode.LanguageModelTextPart(this.formatMemoryStats(response))
                        ]);
                    } catch (error) {
                        return this.handleToolError(error, 'get memory stats', token);
                    }
                },
                prepareInvocation: async (_options, _token) => {
                    return {
                        invocationMessage: 'Retrieving memory statistics...'
                    };
                }
            })
        );

        // Tool 28: Reindex Workspace
        this.disposables.push(
            vscode.lm.registerTool('codegraph_reindex_workspace', {
                invoke: async (_options, token) => {
                    try {
                        const response = await this.sendRequestWithRetry<{ status: string; message: string; files_indexed: number }>(
                            'codegraph.reindexWorkspace',
                            {},
                            token,
                            { retries: 1 }
                        );

                        return new vscode.LanguageModelToolResult([
                            new vscode.LanguageModelTextPart(`Workspace reindexed: ${response.files_indexed} files parsed.`)
                        ]);
                    } catch (error) {
                        return this.handleToolError(error, 'reindex workspace', token);
                    }
                },
                prepareInvocation: async (_options, _token) => {
                    return {
                        invocationMessage: 'Reindexing workspace...',
                        confirmationMessages: {
                            title: 'Reindex Workspace',
                            message: new vscode.MarkdownString('This will clear and rebuild the entire code graph. Continue?')
                        }
                    };
                }
            })
        );

        // Tool 26: Index Files
        this.disposables.push(
            vscode.lm.registerTool('codegraph_index_files', {
                invoke: async (options, token) => {
                    try {
                        const params = options.input as { files: string[] };
                        const response = await this.sendRequestWithRetry<any>(
                            'codegraph.indexFiles',
                            [params],
                            token
                        );
                        return new vscode.LanguageModelToolResult([
                            new vscode.LanguageModelTextPart(JSON.stringify(response, null, 2))
                        ]);
                    } catch (error) {
                        return this.handleToolError(error, 'index files', token);
                    }
                },
            })
        );

        // Tool 27: Index Directory
        this.disposables.push(
            vscode.lm.registerTool('codegraph_index_directory', {
                invoke: async (options, token) => {
                    try {
                        const params = options.input as { path: string; embed?: boolean };
                        const response = await this.sendRequestWithRetry<any>(
                            'codegraph.indexDirectory',
                            [params],
                            token
                        );
                        return new vscode.LanguageModelToolResult([
                            new vscode.LanguageModelTextPart(JSON.stringify(response, null, 2))
                        ]);
                    } catch (error) {
                        return this.handleToolError(error, 'index directory', token);
                    }
                },
            })
        );

        // Tool 28: Find Implementors
        this.disposables.push(
            vscode.lm.registerTool('codegraph_find_implementors', {
                invoke: async (options, token) => {
                    try {
                        const params = options.input as { structType?: string; field?: string; limit?: number };
                        const response = await this.sendRequestWithRetry<any>(
                            'codegraph.findImplementors',
                            [params],
                            token
                        );
                        return new vscode.LanguageModelToolResult([
                            new vscode.LanguageModelTextPart(JSON.stringify(response, null, 2))
                        ]);
                    } catch (error) {
                        return this.handleToolError(error, 'find implementors', token);
                    }
                },
            })
        );

        console.log(`[CodeGraph] Registered ${this.disposables.length} Language Model tools`);
    }

    /**
     * Format dependency graph for AI consumption
     */
    private formatDependencyGraph(response: DependencyGraphResponse, summary = false): string {
        const { nodes, edges } = response;
        const shouldSummarize = summary || nodes.length > 50 || edges.length > 80;

        let output = shouldSummarize ? '# Dependency Graph (summary)\n\n' : '# Dependency Graph\n\n';
        output += `Found ${nodes.length} files/modules with ${edges.length} dependencies.\n\n`;

        const imports = edges.filter(e => e.type === 'import' || e.type === 'require' || e.type === 'use');

        const edgeLimit = shouldSummarize ? 15 : imports.length;
        if (imports.length > 0) {
            output += `## Dependencies (${imports.length})\n`;
            imports.slice(0, edgeLimit).forEach(edge => {
                const fromNode = nodes.find(n => n.id === edge.from);
                const toNode = nodes.find(n => n.id === edge.to);
                output += `- ${fromNode?.label || edge.from} → ${toNode?.label || edge.to} (${edge.type})\n`;
            });
            if (imports.length > edgeLimit) {
                output += `... and ${imports.length - edgeLimit} more\n`;
            }
            output += '\n';
        }

        const nodeLimit = shouldSummarize ? 15 : nodes.length;
        output += `## Files/Modules${shouldSummarize ? ' (sample)' : ''}\n`;
        nodes.slice(0, nodeLimit).forEach(node => {
            output += `- **${node.label}** (${node.type}, ${node.language})\n`;
            if (node.uri) {
                output += `  Path: ${node.uri}\n`;
            }
        });
        if (nodes.length > nodeLimit) {
            output += `... and ${nodes.length - nodeLimit} more\n`;
        }

        return output;
    }

    /**
     * Format call graph for AI consumption
     */
    private formatCallGraph(response: CallGraphResponse, summary = false): string {
        const { root, nodes, edges } = response;

        let output = summary ? '# Call Graph (summary)\n\n' : '# Call Graph\n\n';

        if (!root) {
            output += '❌ No function found at the specified position.\n\n';
            output += 'This could mean:\n';
            output += '- The cursor is not on a function definition\n';
            output += '- The file has not been indexed yet\n';
            output += '- The position is in a comment or whitespace\n\n';
            output += 'Try:\n';
            output += '- Place cursor on a function name\n';
            output += '- Run "CodeGraph: Reindex Workspace" if the file is new\n';
            return output;
        }

        const shouldSummarize = summary || nodes.length > 50 || edges.length > 80;
        output += `Found ${nodes.length} functions with ${edges.length} call relationships.\n\n`;

        output += `## Target Function\n`;
        output += `**${root.name}** (${root.signature})\n`;
        output += `Location: ${root.uri}\n`;
        if (root.metrics) {
            output += `Complexity: ${root.metrics.complexity || 'N/A'}, Lines: ${root.metrics.linesOfCode || 'N/A'}\n`;
        }
        output += '\n';

        const callers = edges.filter(e => e.to === root.id);
        const callees = edges.filter(e => e.from === root.id);

        const callerLimit = shouldSummarize ? 15 : callers.length;
        if (callers.length > 0) {
            output += `## Callers (${callers.length})\n`;
            output += 'Functions that call this:\n';
            callers.slice(0, callerLimit).forEach(edge => {
                const caller = nodes.find(n => n.id === edge.from);
                if (caller) {
                    output += `- **${caller.name}** at ${caller.uri}\n`;
                }
            });
            if (callers.length > callerLimit) {
                output += `... and ${callers.length - callerLimit} more\n`;
            }
            output += '\n';
        }

        const calleeLimit = shouldSummarize ? 15 : callees.length;
        if (callees.length > 0) {
            output += `## Callees (${callees.length})\n`;
            output += 'Functions that this calls:\n';
            callees.slice(0, calleeLimit).forEach(edge => {
                const callee = nodes.find(n => n.id === edge.to);
                if (callee) {
                    output += `- **${callee.name}** at ${callee.uri}\n`;
                }
            });
            if (callees.length > calleeLimit) {
                output += `... and ${callees.length - calleeLimit} more\n`;
            }
            output += '\n';
        }

        return output;
    }

    /**
     * Format impact analysis for AI consumption
     */
    private formatImpactAnalysis(response: ImpactAnalysisResponse, summary = false): string {
        const shouldSummarize = summary || response.directImpact.length > 50 || response.indirectImpact.length > 50;

        let output = shouldSummarize ? '# Impact Analysis (summary)\n\n' : '# Impact Analysis\n\n';

        output += `## Summary\n`;
        output += `- Files Affected: ${response.summary.filesAffected}\n`;
        output += `- Breaking Changes: ${response.summary.breakingChanges}\n`;
        output += `- Warnings: ${response.summary.warnings}\n\n`;

        const directLimit = shouldSummarize ? 20 : response.directImpact.length;
        if (response.directImpact.length > 0) {
            output += `## Direct Impact (${response.directImpact.length})\n`;
            output += 'Immediate usages that will be affected:\n';
            response.directImpact.slice(0, directLimit).forEach(impact => {
                const severity = impact.severity === 'breaking' ? '🔴 BREAKING' :
                                impact.severity === 'warning' ? '🟡 WARNING' : '🔵 INFO';
                output += `${severity}: **${impact.type}** at ${impact.uri}:${impact.range.start.line + 1}\n`;
            });
            if (response.directImpact.length > directLimit) {
                output += `... and ${response.directImpact.length - directLimit} more\n`;
            }
            output += '\n';
        }

        const indirectLimit = shouldSummarize ? 15 : response.indirectImpact.length;
        if (response.indirectImpact.length > 0) {
            output += `## Indirect Impact (${response.indirectImpact.length})\n`;
            output += 'Transitive dependencies that will be affected:\n';
            response.indirectImpact.slice(0, indirectLimit).forEach(impact => {
                const severity = impact.severity === 'breaking' ? '🔴' :
                                impact.severity === 'warning' ? '🟡' : '🔵';
                output += `${severity} ${impact.uri}\n`;
                output += `  Dependency path: ${impact.path.join(' → ')}\n`;
            });
            if (response.indirectImpact.length > indirectLimit) {
                output += `... and ${response.indirectImpact.length - indirectLimit} more\n`;
            }
            output += '\n';
        }

        const testsLimit = shouldSummarize ? 10 : response.affectedTests.length;
        if (response.affectedTests.length > 0) {
            output += `## Affected Tests (${response.affectedTests.length})\n`;
            output += 'Tests that may need updating:\n';
            response.affectedTests.slice(0, testsLimit).forEach(test => {
                output += `🧪 **${test.testName}** at ${test.uri}\n`;
            });
            if (response.affectedTests.length > testsLimit) {
                output += `... and ${response.affectedTests.length - testsLimit} more\n`;
            }
        }

        return output;
    }

    private formatCuratedContext(response: CuratedContextResponse): string {
        let output = `# Curated Context: "${response.query}"\n\n`;

        if (response.symbols.length > 0) {
            output += `## Matching Symbols (${response.symbols.length})\n\n`;
            for (const sym of response.symbols) {
                output += `### ${sym.name} (${sym.kind})\n`;
                output += `${sym.file}:${sym.line} — relevance: ${(sym.score * 100).toFixed(0)}%\n`;
                if (sym.code) {
                    output += '```\n' + sym.code + '\n```\n';
                }
                output += '\n';
            }
        }

        if (response.dependencies.length > 0) {
            output += `## Related Code (${response.dependencies.length})\n\n`;
            for (const dep of response.dependencies) {
                output += `### ${dep.name} (${dep.relationship})\n`;
                output += `${dep.file}\n`;
                if (dep.code) {
                    output += '```\n' + dep.code + '\n```\n';
                }
                output += '\n';
            }
        }

        if (response.memories.length > 0) {
            output += `## Relevant Memories (${response.memories.length})\n\n`;
            for (const mem of response.memories) {
                output += `### [${mem.kind}] ${mem.title}\n`;
                output += mem.content + '\n\n';
            }
        }

        output += `_${response.metadata.totalTokens} tokens, ${response.metadata.queryTime}ms, ${response.metadata.symbolsFound} total matches_\n`;
        return output;
    }

    private formatEditContext(response: EditContextResponse): string {
        let output = '# Edit Context\n\n';

        // Symbol
        output += `## Symbol: ${response.symbol.name} (${response.symbol.type})\n`;
        output += `Language: ${response.symbol.language}\n\n`;
        output += '```' + response.symbol.language + '\n';
        output += response.symbol.code + '\n';
        output += '```\n\n';

        // Callers
        if (response.callers.length > 0) {
            output += `## Callers (${response.callers.length})\n\n`;
            for (const caller of response.callers) {
                output += `### ${caller.name}\n`;
                output += `${caller.file}:${caller.line}\n`;
                if (caller.code) {
                    output += '```\n' + caller.code + '\n```\n';
                }
                output += '\n';
            }
        }

        // Tests
        if (response.tests.length > 0) {
            output += `## Related Tests (${response.tests.length})\n\n`;
            for (const test of response.tests) {
                output += `- **${test.name}** (${test.relationship})`;
                if (test.code) {
                    output += '\n```\n' + test.code + '\n```';
                }
                output += '\n';
            }
            output += '\n';
        }

        // Memories
        if (response.memories.length > 0) {
            output += `## Relevant Memories (${response.memories.length})\n\n`;
            for (const mem of response.memories) {
                output += `### [${mem.kind}] ${mem.title}\n`;
                output += mem.content + '\n\n';
            }
        }

        // Recent changes
        if (response.recentChanges.length > 0) {
            output += `## Recent Changes\n\n`;
            for (const commit of response.recentChanges) {
                output += `- \`${commit.hash}\` ${commit.subject} (${commit.author}, ${commit.date})\n`;
            }
            output += '\n';
        }

        output += `_${response.metadata.totalTokens} tokens, ${response.metadata.queryTime}ms_\n`;
        return output;
    }

    private formatAIContext(response: AIContextResponse): string {
        let output = '# Code Context\n\n';

        output += `## Primary Code\n`;
        output += `**${response.primaryContext.type}: ${response.primaryContext.name}**\n`;
        output += `Language: ${response.primaryContext.language}\n`;
        output += `Location: ${response.primaryContext.location.uri}\n\n`;
        output += '```' + response.primaryContext.language + '\n';
        output += response.primaryContext.code + '\n';
        output += '```\n\n';

        if (response.relatedSymbols.length > 0) {
            output += `## Related Code (${response.relatedSymbols.length})\n\n`;
            response.relatedSymbols.slice(0, 5).forEach((symbol, i) => {
                output += `### ${i + 1}. ${symbol.relationship} (relevance: ${(symbol.relevanceScore * 100).toFixed(0)}%)\n`;
                output += `**${symbol.name}**\n`;
                output += '```\n';
                output += symbol.code + '\n';
                output += '```\n\n';
            });
        }

        if (response.architecture) {
            output += `## Architecture Context\n`;
            output += `- Module: ${response.architecture.module}\n`;
            output += `- Neighbors: ${response.architecture.neighbors.join(', ')}\n`;
        }

        return output;
    }

    /**
     * Format test context for AI consumption
     */
    private formatRelatedTests(response: RelatedTestsResponse): string {
        let output = '# Related Tests\n\n';

        if (!response.tests.length) {
            output += 'No related tests found in the codebase.\n';
            output += '\nThis could mean:\n';
            output += '- No tests exist for this code yet\n';
            output += '- Tests exist but are not directly connected in the dependency graph\n';
            output += '- Tests may use mocking or indirect references\n';
            return output;
        }

        output += `Found ${response.tests.length} related test(s):\n\n`;

        response.tests.forEach((test, i) => {
            output += `## ${i + 1}. ${test.testName}\n`;
            output += `Relationship: ${test.relationship}\n`;
            output += `Location: ${test.uri}:${test.range.start.line + 1}\n`;
            output += '\n';
        });

        if (response.truncated) {
            output += '_Results truncated; refine the selection or increase the limit for more tests._\n';
        }

        return output;
    }

    /**
     * Format symbol info for AI consumption
     */
    private formatSymbolInfo(data: {
        hovers?: vscode.Hover[];
        definitions?: vscode.Location[];
        references?: vscode.Location[];
        uri: string;
        line: number;
        character: number;
        referencesIncluded?: boolean;
        referencesTimedOut?: boolean;
    }): string {
        let output = '# Symbol Information\n\n';

        output += `Location: ${data.uri}:${data.line + 1}:${data.character + 1}\n\n`;

        if (data.hovers && data.hovers.length > 0) {
            output += '## Documentation & Type Information\n';
            data.hovers.forEach(hover => {
                hover.contents.forEach(content => {
                    if (typeof content === 'string') {
                        output += content + '\n';
                    } else if ('value' in content) {
                        output += content.value + '\n';
                    }
                });
            });
            output += '\n';
        }

        if (data.definitions && data.definitions.length > 0) {
            output += `## Definition${data.definitions.length > 1 ? 's' : ''}\n`;
            data.definitions.forEach(def => {
                if (def && def.uri && def.range) {
                    output += `- ${def.uri.fsPath}:${def.range.start.line + 1}\n`;
                }
            });
            output += '\n';
        }

        if (data.references && data.references.length > 0) {
            output += `## References (${data.references.length} usage${data.references.length > 1 ? 's' : ''})\n`;
            // Group by file
            const byFile = new Map<string, vscode.Location[]>();
            data.references.forEach(ref => {
                if (!ref || !ref.uri) { return; }
                const path = ref.uri.fsPath;
                if (!byFile.has(path)) {
                    byFile.set(path, []);
                }
                byFile.get(path)!.push(ref);
            });

            byFile.forEach((refs, path) => {
                const fileName = path.split('/').pop();
                output += `- **${fileName}** (${refs.length} reference${refs.length > 1 ? 's' : ''})\n`;
                refs.slice(0, 3).forEach(ref => {
                    output += `  Line ${ref.range.start.line + 1}\n`;
                });
                if (refs.length > 3) {
                    output += `  ... and ${refs.length - 3} more\n`;
                }
            });
        } else if (data.referencesIncluded) {
            // References were requested but none found or timed out
            if (data.referencesTimedOut) {
                output += '## References\n';
                output += '⏱️ Reference search timed out (>5s). The symbol may have many references.\n';
                output += 'Try narrowing your search or use `codegraph_analyze_impact` for dependency analysis.\n\n';
            } else {
                output += '## References\n';
                output += 'No references found for this symbol.\n\n';
            }
        } else {
            // References not requested
            output += '## References\n';
            output += '_References not included. Set `includeReferences: true` to find usages (may be slow)._\n\n';
        }

        if (!data.hovers?.length && !data.definitions?.length && !data.references?.length && !data.referencesIncluded) {
            output += 'No symbol information available at this location.\n';
        }

        return output;
    }

    /**
     * Format complexity analysis for AI consumption
     */
    private formatComplexityAnalysis(response: ComplexityResponse, summary = false): string {
        const { functions, fileSummary, recommendations } = response;
        const shouldSummarize = summary || functions.length > 30;

        let output = shouldSummarize ? '# Complexity Analysis (summary)\n\n' : '# Complexity Analysis\n\n';

        // File summary
        output += `## File Summary\n`;
        output += `- **Overall Grade**: ${fileSummary.overallGrade}\n`;
        output += `- Total Functions: ${fileSummary.totalFunctions}\n`;
        output += `- Average Complexity: ${fileSummary.averageComplexity.toFixed(1)}\n`;
        output += `- Max Complexity: ${fileSummary.maxComplexity}\n`;
        output += `- Functions Above Threshold: ${fileSummary.functionsAboveThreshold}\n\n`;

        // Function details
        const functionLimit = shouldSummarize ? 10 : functions.length;
        if (functions.length > 0) {
            output += `## Functions (${functions.length})\n`;
            output += 'Sorted by complexity (highest first):\n\n';

            functions.slice(0, functionLimit).forEach((func, i) => {
                const gradeEmoji = func.grade === 'A' ? '🟢' : func.grade === 'B' ? '🟡' : func.grade === 'C' ? '🟠' : '🔴';
                output += `### ${i + 1}. ${func.name} ${gradeEmoji}\n`;
                output += `- **Complexity**: ${func.complexity} (Grade: ${func.grade})\n`;
                output += `- Location: ${func.location.uri}:${func.location.range.start.line + 1}\n`;
                output += `- Branches: ${func.details.complexityBranches}, Loops: ${func.details.complexityLoops}, Conditions: ${func.details.complexityLogicalOps}\n`;
                output += `- Nesting Depth: ${func.details.complexityNesting}, Lines: ${func.details.linesOfCode}\n\n`;
            });

            if (functions.length > functionLimit) {
                output += `... and ${functions.length - functionLimit} more functions\n\n`;
            }
        }

        // Recommendations
        if (recommendations.length > 0) {
            output += `## Recommendations\n`;
            recommendations.forEach(rec => {
                output += `- ${rec}\n`;
            });
        }

        return output;
    }

    // ==========================================
    // AI Agent Query Primitives Formatters
    // ==========================================

    /**
     * Format symbol search results for AI consumption
     */
    private formatSymbolSearch(response: SymbolSearchResponse): string {
        let output = '# Symbol Search Results\n\n';
        output += `Found ${response.totalMatches} matches in ${response.queryTimeMs}ms.\n\n`;

        if (response.results.length === 0) {
            output += 'No symbols found matching the query.\n';
            return output;
        }

        response.results.forEach((match, i) => {
            const visibility = match.symbol.isPublic ? '🔓' : '🔒';
            output += `## ${i + 1}. ${match.symbol.name} ${visibility}\n`;
            output += `- **Kind**: ${match.symbol.kind}\n`;
            output += `- **Score**: ${match.score.toFixed(2)}\n`;
            output += `- **Location**: ${match.symbol.location.file}:${match.symbol.location.line}\n`;
            output += `- **Match Reason**: ${match.matchReason}\n`;
            if (match.symbol.signature) {
                output += `- **Signature**: \`${match.symbol.signature}\`\n`;
            }
            if (match.symbol.docstring) {
                output += `- **Documentation**: ${match.symbol.docstring.slice(0, 200)}${match.symbol.docstring.length > 200 ? '...' : ''}\n`;
            }
            output += '\n';
        });

        return output;
    }

    /**
     * Format find by imports results for AI consumption
     */
    private formatFindByImports(response: FindByImportsResponse, libraries: string[]): string {
        let output = '# Code Importing Libraries\n\n';
        output += `Libraries searched: ${libraries.join(', ')}\n`;
        output += `Found ${response.results.length} symbols in ${response.queryTimeMs}ms.\n\n`;

        if (response.results.length === 0) {
            output += 'No code found importing these libraries.\n';
            return output;
        }

        response.results.forEach((match, i) => {
            output += `## ${i + 1}. ${match.symbol.name}\n`;
            output += `- **Kind**: ${match.symbol.kind}\n`;
            output += `- **Location**: ${match.symbol.location.file}:${match.symbol.location.line}\n`;
            output += `- **Match Reason**: ${match.matchReason}\n`;
            output += '\n';
        });

        return output;
    }

    /**
     * Format entry points for AI consumption
     */
    private formatEntryPoints(response: FindEntryPointsResponse): string {
        let output = '# Entry Points\n\n';
        output += `Found ${response.totalFound} entry points.\n\n`;

        if (response.entryPoints.length === 0) {
            output += 'No entry points found.\n';
            return output;
        }

        response.entryPoints.forEach((ep, i) => {
            output += `## ${i + 1}. ${ep.symbol.name}\n`;
            output += `- **Type**: ${ep.entryType}\n`;
            output += `- **Location**: ${ep.symbol.location.file}:${ep.symbol.location.line}\n`;
            if (ep.route) {
                output += `- **Route**: ${ep.method || 'ANY'} ${ep.route}\n`;
            }
            if (ep.description) {
                output += `- **Description**: ${ep.description}\n`;
            }
            output += '\n';
        });

        return output;
    }

    /**
     * Format graph traversal results for AI consumption
     */
    private formatTraverseGraph(response: TraverseGraphResponse, direction: string): string {
        let output = `# Graph Traversal (${direction})\n\n`;
        output += `Found ${response.nodes.length} nodes in ${response.queryTimeMs}ms.\n\n`;

        if (response.nodes.length === 0) {
            output += 'No connected nodes found.\n';
            return output;
        }

        // Group by depth
        const byDepth = new Map<number, typeof response.nodes>();
        response.nodes.forEach(node => {
            if (!byDepth.has(node.depth)) {
                byDepth.set(node.depth, []);
            }
            byDepth.get(node.depth)!.push(node);
        });

        byDepth.forEach((nodes, depth) => {
            output += `## Depth ${depth}\n`;
            nodes.forEach(node => {
                output += `- **${node.symbol.name}** (${node.symbol.kind})\n`;
                output += `  Location: ${node.symbol.location.file}:${node.symbol.location.line}\n`;
                output += `  Edge type: ${node.edgeType}\n`;
            });
            output += '\n';
        });

        return output;
    }

    /**
     * Format callers/callees for AI consumption
     */
    private formatCallers(response: GetCallersResponse, type: 'callers' | 'callees'): string {
        const title = type === 'callers' ? 'Callers' : 'Callees';
        let output = `# ${title}\n\n`;
        output += `Found ${response.callers.length} ${type} in ${response.queryTimeMs}ms.\n\n`;

        if (response.callers.length === 0) {
            output += `No ${type} found.\n`;
            return output;
        }

        response.callers.forEach((call, i) => {
            output += `## ${i + 1}. ${call.symbol.name}\n`;
            output += `- **Kind**: ${call.symbol.kind}\n`;
            output += `- **Location**: ${call.symbol.location.file}:${call.symbol.location.line}\n`;
            output += `- **Call Site**: ${call.callSite.file}:${call.callSite.line}\n`;
            output += `- **Depth**: ${call.depth}\n`;
            output += '\n';
        });

        return output;
    }

    /**
     * Format detailed symbol info for AI consumption
     */
    private formatDetailedSymbol(response: DetailedSymbolResponse): string {
        let output = '# Detailed Symbol Information\n\n';

        const { symbol } = response;
        const visibility = response.isPublic ? '🔓 Public' : '🔒 Private';
        const deprecated = response.isDeprecated ? '⚠️ DEPRECATED' : '';

        output += `## ${symbol.name} ${visibility} ${deprecated}\n\n`;
        output += `- **Kind**: ${symbol.kind}\n`;
        output += `- **Location**: ${symbol.location.file}:${symbol.location.line}\n`;
        output += `- **Lines of Code**: ${response.linesOfCode}\n`;
        output += `- **Reference Count**: ${response.referenceCount}\n`;

        if (response.complexity !== undefined) {
            output += `- **Complexity**: ${response.complexity}\n`;
        }

        if (symbol.signature) {
            output += `- **Signature**: \`${symbol.signature}\`\n`;
        }

        if (symbol.docstring) {
            output += `\n### Documentation\n${symbol.docstring}\n`;
        }

        if (response.callers.length > 0) {
            output += `\n### Callers (${response.callers.length})\n`;
            response.callers.slice(0, 10).forEach(caller => {
                output += `- **${caller.symbol.name}** at ${caller.symbol.location.file}:${caller.symbol.location.line}\n`;
            });
            if (response.callers.length > 10) {
                output += `... and ${response.callers.length - 10} more\n`;
            }
        }

        if (response.callees.length > 0) {
            output += `\n### Callees (${response.callees.length})\n`;
            response.callees.slice(0, 10).forEach(callee => {
                output += `- **${callee.symbol.name}** at ${callee.symbol.location.file}:${callee.symbol.location.line}\n`;
            });
            if (response.callees.length > 10) {
                output += `... and ${response.callees.length - 10} more\n`;
            }
        }

        return output;
    }

    /**
     * Format find by signature results for AI consumption
     */
    private formatFindBySignature(response: FindBySignatureResponse): string {
        let output = '# Functions By Signature\n\n';
        output += `Found ${response.results.length} matches in ${response.queryTimeMs}ms.\n\n`;

        if (response.results.length === 0) {
            output += 'No functions found matching the signature criteria.\n';
            return output;
        }

        response.results.forEach((match, i) => {
            const visibility = match.symbol.isPublic ? '🔓' : '🔒';
            output += `## ${i + 1}. ${match.symbol.name} ${visibility}\n`;
            output += `- **Kind**: ${match.symbol.kind}\n`;
            output += `- **Score**: ${match.score.toFixed(2)}\n`;
            output += `- **Location**: ${match.symbol.location.file}:${match.symbol.location.line}\n`;
            output += `- **Match Reason**: ${match.matchReason}\n`;
            if (match.symbol.signature) {
                output += `- **Signature**: \`${match.symbol.signature}\`\n`;
            }
            if (match.symbol.docstring) {
                output += `- **Documentation**: ${match.symbol.docstring.slice(0, 200)}${match.symbol.docstring.length > 200 ? '...' : ''}\n`;
            }
            output += '\n';
        });

        return output;
    }

    // ==========================================
    // Memory Layer Formatters
    // ==========================================

    /**
     * Format memory store result for AI consumption
     */
    private formatMemoryStore(response: MemoryStoreResponse, title: string): string {
        let output = '# Memory Stored\n\n';

        if (response.success) {
            output += `✅ Successfully stored memory: "${title}"\n\n`;
            output += `- **Memory ID**: \`${response.id}\`\n`;
            output += '\nYou can retrieve this memory later using the ID, or it will be automatically surfaced when relevant.\n';
        } else {
            output += `❌ Failed to store memory: "${title}"\n`;
        }

        return output;
    }

    /**
     * Format memory search results for AI consumption
     */
    private formatMemorySearch(response: MemorySearchResponse, query: string): string {
        let output = '# Memory Search Results\n\n';
        output += `Query: "${query}"\n`;
        output += `Found ${response.total} memories.\n\n`;

        if (response.results.length === 0) {
            output += 'No memories found matching your query.\n\n';
            output += 'Tips:\n';
            output += '- Try broader search terms\n';
            output += '- Check if memories exist using `codegraph_memory_list`\n';
            output += '- Memories may have been invalidated\n';
            return output;
        }

        response.results.forEach((memory, i) => {
            const currentBadge = memory.isCurrent ? '✅' : '⚠️ invalidated';
            output += `## ${i + 1}. ${memory.title} ${currentBadge}\n`;
            output += `- **ID**: \`${memory.id}\`\n`;
            output += `- **Kind**: ${memory.kind}\n`;
            output += `- **Relevance**: ${(memory.score * 100).toFixed(1)}%\n`;
            if (memory.tags.length > 0) {
                output += `- **Tags**: ${memory.tags.join(', ')}\n`;
            }
            output += `\n${memory.content.slice(0, 300)}${memory.content.length > 300 ? '...' : ''}\n\n`;
        });

        return output;
    }

    /**
     * Format memory get result for AI consumption
     */
    private formatMemoryGet(response: MemoryGetResponse): string {
        let output = '# Memory Details\n\n';

        const currentBadge = response.isCurrent ? '✅ Current' : '⚠️ Invalidated';
        output += `## ${response.title} ${currentBadge}\n\n`;
        output += `- **ID**: \`${response.id}\`\n`;
        output += `- **Kind**: ${JSON.stringify(response.kind)}\n`;
        output += `- **Confidence**: ${(response.confidence * 100).toFixed(0)}%\n`;
        output += `- **Created**: ${response.createdAt}\n`;
        if (response.validFrom) {
            output += `- **Valid From**: ${response.validFrom}\n`;
        }
        if (response.tags.length > 0) {
            output += `- **Tags**: ${response.tags.join(', ')}\n`;
        }

        output += `\n### Content\n${response.content}\n`;

        if (response.codeLinks.length > 0) {
            output += `\n### Linked Code\n`;
            response.codeLinks.forEach(link => {
                output += `- ${link.nodeType}: \`${link.nodeId}\`\n`;
            });
        }

        return output;
    }

    /**
     * Format memory context results for AI consumption
     */
    private formatMemoryContext(response: MemoryContextResponse, uri: string): string {
        const filename = uri.split('/').pop() || uri;
        let output = `# Relevant Memories for ${filename}\n\n`;

        if (response.memories.length === 0) {
            output += 'No relevant memories found for this code context.\n\n';
            output += 'This means there are no stored:\n';
            output += '- Debug contexts from previous sessions\n';
            output += '- Architectural decisions related to this code\n';
            output += '- Known issues or conventions\n';
            return output;
        }

        output += `Found ${response.memories.length} relevant memories.\n\n`;

        response.memories.forEach((memory, i) => {
            output += `## ${i + 1}. ${memory.title}\n`;
            output += `- **Kind**: ${memory.kind}\n`;
            output += `- **Relevance**: ${(memory.relevanceScore * 100).toFixed(1)}%\n`;
            output += `- **Why relevant**: ${memory.relevanceReason}\n`;
            if (memory.tags.length > 0) {
                output += `- **Tags**: ${memory.tags.join(', ')}\n`;
            }
            output += `\n${memory.content.slice(0, 400)}${memory.content.length > 400 ? '...' : ''}\n\n`;
        });

        return output;
    }

    /**
     * Format memory invalidate result for AI consumption
     */
    private formatMemoryInvalidate(response: MemoryInvalidateResponse, id: string): string {
        let output = '# Memory Invalidation\n\n';

        if (response.success) {
            output += `✅ Successfully invalidated memory \`${id}\`\n\n`;
            output += 'The memory is now marked as no longer current and will be excluded from searches by default.\n';
            output += 'It can still be retrieved directly by ID if needed.\n';
        } else {
            output += `❌ Failed to invalidate memory \`${id}\`\n\n`;
            output += 'The memory may not exist or may already be invalidated.\n';
        }

        return output;
    }

    /**
     * Format memory list results for AI consumption
     */
    private formatMemoryList(response: MemoryListResponse): string {
        let output = '# Memory List\n\n';
        output += `Total: ${response.total} memories`;
        if (response.hasMore) {
            output += ` (showing ${response.memories.length}, more available)`;
        }
        output += '\n\n';

        if (response.memories.length === 0) {
            output += 'No memories found matching the criteria.\n';
            return output;
        }

        response.memories.forEach((memory, i) => {
            const currentBadge = memory.isCurrent ? '✅' : '⚠️';
            output += `${i + 1}. **${memory.title}** ${currentBadge}\n`;
            output += `   - ID: \`${memory.id}\`\n`;
            output += `   - Kind: ${memory.kind}\n`;
            if (memory.tags.length > 0) {
                output += `   - Tags: ${memory.tags.join(', ')}\n`;
            }
        });

        return output;
    }

    /**
     * Format memory stats for AI consumption
     */
    private formatMemoryStats(response: MemoryStatsResponse): string {
        let output = '# Memory Statistics\n\n';

        output += '## Overview\n';
        output += `- **Total Memories**: ${response.totalMemories}\n`;
        output += `- **Current (Valid)**: ${response.currentMemories}\n`;
        output += `- **Invalidated**: ${response.invalidatedMemories}\n`;
        output += '\n';

        if (Object.keys(response.byKind).length > 0) {
            output += '## By Kind\n';
            for (const [kind, count] of Object.entries(response.byKind)) {
                output += `- **${kind}**: ${count}\n`;
            }
            output += '\n';
        }

        if (Object.keys(response.byTag).length > 0) {
            output += '## By Tag (top 10)\n';
            const sortedTags = Object.entries(response.byTag)
                .sort(([, a], [, b]) => b - a)
                .slice(0, 10);
            for (const [tag, count] of sortedTags) {
                output += `- **${tag}**: ${count}\n`;
            }
        }

        return output;
    }

    /**
     * Dispose all tool registrations
     */
    dispose(): void {
        console.log('[CodeGraph] Disposing Language Model tools');
        this.disposables.forEach(d => d.dispose());
        this.disposables = [];
    }
}
