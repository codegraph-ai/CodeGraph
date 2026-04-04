// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

import * as vscode from 'vscode';
import { LanguageClient, RequestType } from 'vscode-languageclient/node';
import { registerMemoryTreeView } from './memoryProvider';

interface SymbolInfo {
    id: string;
    name: string;
    kind: string;
    language: string;
    uri: string;
    range: {
        start: { line: number; character: number };
        end: { line: number; character: number };
    };
    children?: SymbolInfo[];
}

interface WorkspaceSymbolsResponse {
    symbols: SymbolInfo[];
}

namespace GetWorkspaceSymbolsRequest {
    export const type = new RequestType<{ query?: string }, WorkspaceSymbolsResponse, void>(
        'codegraph/getWorkspaceSymbols'
    );
}

/**
 * Tree item for CodeGraph symbols view.
 */
class SymbolTreeItem extends vscode.TreeItem {
    constructor(
        public readonly symbol: SymbolInfo,
        public readonly collapsibleState: vscode.TreeItemCollapsibleState
    ) {
        super(symbol.name, collapsibleState);

        this.description = `${symbol.language} ${symbol.kind}`;
        this.tooltip = `${symbol.name} (${symbol.kind})\n${symbol.uri}`;

        // Set icon based on symbol kind
        this.iconPath = this.getIcon(symbol.kind);

        // Make it clickable to navigate to the symbol
        this.command = {
            command: 'codegraph.goToSymbol',
            title: 'Go to Symbol',
            arguments: [symbol],
        };

        this.contextValue = symbol.kind.toLowerCase();
    }

    private getIcon(kind: string): vscode.ThemeIcon {
        const iconMap: Record<string, string> = {
            function: 'symbol-function',
            method: 'symbol-method',
            class: 'symbol-class',
            struct: 'symbol-struct',
            interface: 'symbol-interface',
            trait: 'symbol-interface',
            module: 'symbol-namespace',
            file: 'symbol-file',
            variable: 'symbol-variable',
            constant: 'symbol-constant',
            enum: 'symbol-enum',
            property: 'symbol-property',
            field: 'symbol-field',
        };

        return new vscode.ThemeIcon(iconMap[kind.toLowerCase()] || 'symbol-misc');
    }
}

/**
 * Tree data provider for CodeGraph symbols.
 */
export class SymbolTreeProvider implements vscode.TreeDataProvider<SymbolTreeItem> {
    private _onDidChangeTreeData = new vscode.EventEmitter<SymbolTreeItem | undefined | null>();
    readonly onDidChangeTreeData = this._onDidChangeTreeData.event;

    private symbols: SymbolInfo[] = [];
    private filter: string = '';

    constructor(private client: LanguageClient) {}

    refresh(): void {
        this._onDidChangeTreeData.fire(undefined);
    }

    setFilter(filter: string): void {
        this.filter = filter;
        this.refresh();
    }

    getTreeItem(element: SymbolTreeItem): vscode.TreeItem {
        return element;
    }

    async getChildren(element?: SymbolTreeItem): Promise<SymbolTreeItem[]> {
        if (element) {
            // Return children of this symbol
            if (element.symbol.children) {
                return element.symbol.children.map(child => {
                    const hasChildren = child.children && child.children.length > 0;
                    return new SymbolTreeItem(
                        child,
                        hasChildren
                            ? vscode.TreeItemCollapsibleState.Collapsed
                            : vscode.TreeItemCollapsibleState.None
                    );
                });
            }
            return [];
        }

        // Root level - fetch symbols from server
        try {
            const response = await this.client.sendRequest(
                GetWorkspaceSymbolsRequest.type,
                { query: this.filter || undefined }
            );

            this.symbols = response.symbols;

            return this.symbols.map(symbol => {
                const hasChildren = symbol.children && symbol.children.length > 0;
                return new SymbolTreeItem(
                    symbol,
                    hasChildren
                        ? vscode.TreeItemCollapsibleState.Collapsed
                        : vscode.TreeItemCollapsibleState.None
                );
            });
        } catch (error) {
            console.error('Failed to get workspace symbols:', error);
            return [];
        }
    }
}

/**
 * Register tree data providers and related commands.
 */
export function registerTreeDataProviders(
    context: vscode.ExtensionContext,
    client: LanguageClient
): void {
    const symbolProvider = new SymbolTreeProvider(client);

    // Register tree view
    const treeView = vscode.window.createTreeView('codegraphSymbols', {
        treeDataProvider: symbolProvider,
        showCollapseAll: true,
    });

    context.subscriptions.push(treeView);

    // Register refresh command
    context.subscriptions.push(
        vscode.commands.registerCommand('codegraph.refreshSymbols', () => {
            symbolProvider.refresh();
        })
    );

    // Register filter command
    context.subscriptions.push(
        vscode.commands.registerCommand('codegraph.filterSymbols', async () => {
            const filter = await vscode.window.showInputBox({
                prompt: 'Filter symbols',
                placeHolder: 'Enter filter text...',
            });
            if (filter !== undefined) {
                symbolProvider.setFilter(filter);
            }
        })
    );

    // Register go to symbol command
    context.subscriptions.push(
        vscode.commands.registerCommand('codegraph.goToSymbol', async (symbol: SymbolInfo) => {
            try {
                const uri = vscode.Uri.parse(symbol.uri);
                const range = new vscode.Range(
                    symbol.range.start.line,
                    symbol.range.start.character,
                    symbol.range.end.line,
                    symbol.range.end.character
                );

                const doc = await vscode.workspace.openTextDocument(uri);
                await vscode.window.showTextDocument(doc, {
                    selection: range,
                });
            } catch (error) {
                vscode.window.showErrorMessage(`Failed to navigate to symbol: ${error}`);
            }
        })
    );

    // Auto-refresh on file changes
    context.subscriptions.push(
        vscode.workspace.onDidSaveTextDocument(() => {
            symbolProvider.refresh();
        })
    );

    // Register memory tree view
    registerMemoryTreeView(context, client);
}
