// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Inline CodeLens (and hover) surfacing graph intelligence directly in the
//! editor: callers, related tests, and cyclomatic complexity above every
//! function. Telemetry showed humans engage the visible surfaces (tree views,
//! call graph) far more than the agent tools, so this puts the graph where
//! people already read code. One batched `codegraph/getDocumentCodeLens`
//! request per document backs both the lenses and the hovers.

import * as vscode from 'vscode';
import { LanguageClient, RequestType } from 'vscode-languageclient/node';
import type { Reporter } from '../telemetry/reporter';
import { onDidRefreshCodeLenses, refreshCodeLenses } from './codeLensRefresh';

/** Per-symbol stats returned by the server for one document. */
interface CodeLensSymbol {
    name: string;
    /** 0-based start line. */
    line: number;
    callerCount: number;
    testCount: number;
    complexity: number;
}

interface DocumentCodeLensResponse {
    symbols: CodeLensSymbol[];
}

namespace GetDocumentCodeLensRequest {
    export const type = new RequestType<{ uri: string }, DocumentCodeLensResponse, void>(
        'codegraph/getDocumentCodeLens',
    );
}

// Register for all on-disk files rather than an enumerated language list (which
// would be a fourth place to update per new parser, and would drift silently).
// The server returns no symbols for a file it didn't index, so an unsupported
// file simply yields no lenses/hover - no list to maintain, no feature gap.
const SELECTOR: vscode.DocumentSelector = { scheme: 'file' };

/** True when the CodeLens surface is enabled in settings (default on). */
function codeLensEnabled(): boolean {
    return vscode.workspace.getConfiguration('codegraph').get<boolean>('codeLens.enabled', true);
}

/** True when the hover surface is enabled in settings (default on). */
function hoverEnabled(): boolean {
    return vscode.workspace.getConfiguration('codegraph').get<boolean>('hover.enabled', true);
}

/**
 * Fetch per-document symbol stats, cached by document URI + version so
 * scrolling or re-render doesn't re-hit the server; a new edit (version bump)
 * or an explicit {@link refreshCodeLenses} invalidates the entry.
 */
class DocumentStatsCache {
    private entries = new Map<string, { version: number; symbols: CodeLensSymbol[] }>();

    constructor(private client: LanguageClient) {}

    invalidate(): void {
        this.entries.clear();
    }

    /** Drop one document's entry (call when its editor closes) to bound memory. */
    evict(uri: vscode.Uri): void {
        this.entries.delete(uri.toString());
    }

    async get(document: vscode.TextDocument): Promise<CodeLensSymbol[]> {
        const key = document.uri.toString();
        const cached = this.entries.get(key);
        if (cached && cached.version === document.version) {
            return cached.symbols;
        }
        try {
            const response = await this.client.sendRequest(GetDocumentCodeLensRequest.type, {
                uri: document.uri.toString(),
            });
            const symbols = response?.symbols ?? [];
            this.entries.set(key, { version: document.version, symbols });
            return symbols;
        } catch {
            // Server not ready / not indexed / unsupported file - no lenses.
            return [];
        }
    }
}

function formatLensTitle(s: CodeLensSymbol): string {
    const parts: string[] = [];
    parts.push(`$(references) ${s.callerCount} caller${s.callerCount === 1 ? '' : 's'}`);
    parts.push(`$(beaker) ${s.testCount} test${s.testCount === 1 ? '' : 's'}`);
    if (s.complexity > 0) {
        parts.push(`$(pulse) complexity ${s.complexity}`);
    }
    return parts.join('  ·  ');
}

class CodeGraphCodeLensProvider implements vscode.CodeLensProvider {
    readonly onDidChangeCodeLenses = onDidRefreshCodeLenses;

    constructor(private cache: DocumentStatsCache) {}

    async provideCodeLenses(
        document: vscode.TextDocument,
        token: vscode.CancellationToken,
    ): Promise<vscode.CodeLens[]> {
        if (!codeLensEnabled()) return [];
        const symbols = await this.cache.get(document);
        if (token.isCancellationRequested) return [];

        const lenses: vscode.CodeLens[] = [];
        for (const s of symbols) {
            if (s.line < 0 || s.line >= document.lineCount) continue;
            const range = document.lineAt(s.line).range;
            lenses.push(
                new vscode.CodeLens(range, {
                    title: formatLensTitle(s),
                    command: 'codegraph.revealCallGraphAt',
                    arguments: [document.uri, s.line],
                }),
            );
        }
        return lenses;
    }
}

class CodeGraphHoverProvider implements vscode.HoverProvider {
    constructor(private cache: DocumentStatsCache) {}

    async provideHover(
        document: vscode.TextDocument,
        position: vscode.Position,
    ): Promise<vscode.Hover | undefined> {
        if (!hoverEnabled()) return undefined;
        const symbols = await this.cache.get(document);
        // Match the symbol whose declaration line the hover is on.
        const s = symbols.find((sym) => sym.line === position.line);
        if (!s) return undefined;

        const md = new vscode.MarkdownString(undefined, true);
        md.appendMarkdown(`**${s.name}** · CodeGraph\n\n`);
        md.appendMarkdown(
            `$(references) ${s.callerCount} caller${s.callerCount === 1 ? '' : 's'}  ·  ` +
                `$(beaker) ${s.testCount} test${s.testCount === 1 ? '' : 's'}` +
                (s.complexity > 0 ? `  ·  $(pulse) complexity ${s.complexity}` : ''),
        );
        return new vscode.Hover(md, document.lineAt(s.line).range);
    }
}

/**
 * Register the CodeLens provider, the matching hover, and the click command
 * that opens the call graph at a symbol. Returns disposables via `context`.
 */
export function registerCodeLens(
    context: vscode.ExtensionContext,
    client: LanguageClient,
    reporter?: Reporter,
): void {
    const cache = new DocumentStatsCache(client);

    // Clear the cache whenever a refresh is requested (post-reindex) so the
    // next provideCodeLenses fetches fresh counts.
    context.subscriptions.push(onDidRefreshCodeLenses(() => cache.invalidate()));

    // Bound memory: drop a document's cached stats when its editor closes.
    context.subscriptions.push(
        vscode.workspace.onDidCloseTextDocument((doc) => cache.evict(doc.uri)),
    );

    context.subscriptions.push(
        vscode.languages.registerCodeLensProvider(SELECTOR, new CodeGraphCodeLensProvider(cache)),
        vscode.languages.registerHoverProvider(SELECTOR, new CodeGraphHoverProvider(cache)),
    );

    // Re-render lenses when the toggles change.
    context.subscriptions.push(
        vscode.workspace.onDidChangeConfiguration((e) => {
            if (
                e.affectsConfiguration('codegraph.codeLens.enabled') ||
                e.affectsConfiguration('codegraph.hover.enabled')
            ) {
                refreshCodeLenses();
            }
        }),
    );

    // CodeLens click: reveal the symbol's line, then open its call graph.
    context.subscriptions.push(
        vscode.commands.registerCommand(
            'codegraph.revealCallGraphAt',
            async (uri: vscode.Uri, line: number) => {
                reporter?.engagementCodeLensClicked();
                const editor = await vscode.window.showTextDocument(uri);
                const pos = new vscode.Position(line, 0);
                editor.selection = new vscode.Selection(pos, pos);
                editor.revealRange(new vscode.Range(pos, pos));
                await vscode.commands.executeCommand('codegraph.showCallGraph');
            },
        ),
    );
}
