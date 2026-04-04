// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

import * as vscode from 'vscode';
import { LanguageClient } from 'vscode-languageclient/node';
import {
    MemoryKind,
    MemorySearchResult,
    MemoryListResponse,
    MemoryGetResponse,
    MemoryStatsResponse,
} from '../types';

/**
 * Tree item representing a memory entry.
 */
export class MemoryTreeItem extends vscode.TreeItem {
    constructor(
        public readonly memory: MemorySearchResult,
        public readonly collapsibleState: vscode.TreeItemCollapsibleState
    ) {
        super(memory.title, collapsibleState);

        this.description = this.getKindLabel(memory.kind);
        this.tooltip = this.buildTooltip();

        // Set icon based on memory kind
        this.iconPath = this.getIcon(memory.kind);

        // Make it clickable to show memory details
        this.command = {
            command: 'codegraph.showMemory',
            title: 'Show Memory',
            arguments: [memory.id],
        };

        // Add context value for inline actions
        this.contextValue = memory.isCurrent ? 'memory-current' : 'memory-invalidated';
    }

    private getKindLabel(kind: string): string {
        const labels: Record<string, string> = {
            debug_context: 'Debug',
            architectural_decision: 'Architecture',
            known_issue: 'Issue',
            convention: 'Convention',
            project_context: 'Context',
        };
        return labels[kind] || kind;
    }

    private buildTooltip(): string {
        const lines = [
            this.memory.title,
            `Kind: ${this.getKindLabel(this.memory.kind)}`,
            `Status: ${this.memory.isCurrent ? 'Current' : 'Invalidated'}`,
        ];

        if (this.memory.tags.length > 0) {
            lines.push(`Tags: ${this.memory.tags.join(', ')}`);
        }

        if (this.memory.score !== undefined) {
            lines.push(`Relevance: ${(this.memory.score * 100).toFixed(0)}%`);
        }

        return lines.join('\n');
    }

    private getIcon(kind: string): vscode.ThemeIcon {
        const iconMap: Record<string, string> = {
            debug_context: 'bug',
            architectural_decision: 'symbol-structure',
            known_issue: 'warning',
            convention: 'book',
            project_context: 'info',
        };

        return new vscode.ThemeIcon(iconMap[kind] || 'note');
    }
}

/**
 * Tree item representing a category of memories (grouped by kind).
 */
class MemoryCategoryItem extends vscode.TreeItem {
    constructor(
        public readonly kind: MemoryKind,
        public readonly count: number
    ) {
        super(MemoryCategoryItem.getLabel(kind), vscode.TreeItemCollapsibleState.Collapsed);

        this.description = `${count} memories`;
        this.iconPath = this.getIcon(kind);
        this.contextValue = 'memory-category';
    }

    private static getLabel(kind: MemoryKind): string {
        const labels: Record<MemoryKind, string> = {
            debug_context: 'Debug Context',
            architectural_decision: 'Architectural Decisions',
            known_issue: 'Known Issues',
            convention: 'Conventions',
            project_context: 'Project Context',
        };
        return labels[kind];
    }

    private getIcon(kind: MemoryKind): vscode.ThemeIcon {
        const iconMap: Record<MemoryKind, string> = {
            debug_context: 'bug',
            architectural_decision: 'symbol-structure',
            known_issue: 'warning',
            convention: 'book',
            project_context: 'info',
        };

        return new vscode.ThemeIcon(iconMap[kind]);
    }
}

type MemoryTreeElement = MemoryTreeItem | MemoryCategoryItem;

/**
 * Tree data provider for CodeGraph memories.
 * Shows memories grouped by kind with filtering support.
 */
export class MemoryTreeProvider implements vscode.TreeDataProvider<MemoryTreeElement> {
    private _onDidChangeTreeData = new vscode.EventEmitter<MemoryTreeElement | undefined | null>();
    readonly onDidChangeTreeData = this._onDidChangeTreeData.event;

    private memoriesByKind: Map<MemoryKind, MemorySearchResult[]> = new Map();
    private showInvalidated: boolean = false;
    private searchQuery: string = '';

    constructor(private client: LanguageClient) {}

    refresh(): void {
        this._onDidChangeTreeData.fire(undefined);
    }

    setShowInvalidated(show: boolean): void {
        this.showInvalidated = show;
        this.refresh();
    }

    setSearchQuery(query: string): void {
        this.searchQuery = query;
        this.refresh();
    }

    getTreeItem(element: MemoryTreeElement): vscode.TreeItem {
        return element;
    }

    async getChildren(element?: MemoryTreeElement): Promise<MemoryTreeElement[]> {
        if (element instanceof MemoryCategoryItem) {
            // Return memories for this category
            const memories = this.memoriesByKind.get(element.kind) || [];
            return memories.map(
                memory =>
                    new MemoryTreeItem(memory, vscode.TreeItemCollapsibleState.None)
            );
        }

        // Root level - fetch and group memories
        try {
            if (this.searchQuery) {
                // Use search endpoint
                const response = await this.client.sendRequest<{
                    results: MemorySearchResult[];
                    total: number;
                }>('workspace/executeCommand', {
                    command: 'codegraph.memorySearch',
                    arguments: [{
                        query: this.searchQuery,
                        currentOnly: !this.showInvalidated,
                        limit: 100,
                    }],
                });

                // Group by kind
                this.memoriesByKind.clear();
                for (const memory of response.results) {
                    const kind = memory.kind as MemoryKind;
                    if (!this.memoriesByKind.has(kind)) {
                        this.memoriesByKind.set(kind, []);
                    }
                    this.memoriesByKind.get(kind)!.push(memory);
                }
            } else {
                // Use list endpoint
                const response = await this.client.sendRequest<MemoryListResponse>(
                    'workspace/executeCommand',
                    {
                        command: 'codegraph.memoryList',
                        arguments: [{
                            currentOnly: !this.showInvalidated,
                            limit: 100,
                        }],
                    }
                );

                // Group by kind
                this.memoriesByKind.clear();
                for (const memory of response.memories) {
                    const kind = memory.kind as MemoryKind;
                    if (!this.memoriesByKind.has(kind)) {
                        this.memoriesByKind.set(kind, []);
                    }
                    this.memoriesByKind.get(kind)!.push(memory);
                }
            }

            // Return category items
            const categories: MemoryCategoryItem[] = [];
            const kindOrder: MemoryKind[] = [
                'debug_context',
                'architectural_decision',
                'known_issue',
                'convention',
                'project_context',
            ];

            for (const kind of kindOrder) {
                const memories = this.memoriesByKind.get(kind);
                if (memories && memories.length > 0) {
                    categories.push(new MemoryCategoryItem(kind, memories.length));
                }
            }

            return categories;
        } catch (error) {
            console.error('Failed to get memories:', error);
            return [];
        }
    }
}

/**
 * Register memory tree view and related commands.
 */
export function registerMemoryTreeView(
    context: vscode.ExtensionContext,
    client: LanguageClient
): void {
    const memoryProvider = new MemoryTreeProvider(client);

    // Register tree view
    const treeView = vscode.window.createTreeView('codegraphMemories', {
        treeDataProvider: memoryProvider,
        showCollapseAll: true,
    });

    context.subscriptions.push(treeView);

    // Register refresh command
    context.subscriptions.push(
        vscode.commands.registerCommand('codegraph.refreshMemories', () => {
            memoryProvider.refresh();
        })
    );

    // Register search memories command
    context.subscriptions.push(
        vscode.commands.registerCommand('codegraph.searchMemories', async () => {
            const query = await vscode.window.showInputBox({
                prompt: 'Search memories',
                placeHolder: 'Enter search query...',
            });
            if (query !== undefined) {
                memoryProvider.setSearchQuery(query);
            }
        })
    );

    // Register clear search command
    context.subscriptions.push(
        vscode.commands.registerCommand('codegraph.clearMemorySearch', () => {
            memoryProvider.setSearchQuery('');
        })
    );

    // Register toggle invalidated command
    context.subscriptions.push(
        vscode.commands.registerCommand('codegraph.toggleInvalidatedMemories', async () => {
            const items: vscode.QuickPickItem[] = [
                { label: 'Show current only', description: 'Hide invalidated memories' },
                { label: 'Show all', description: 'Include invalidated memories' },
            ];

            const selection = await vscode.window.showQuickPick(items, {
                placeHolder: 'Select which memories to show',
            });

            if (selection) {
                memoryProvider.setShowInvalidated(selection.label === 'Show all');
            }
        })
    );

    // Register show memory command
    context.subscriptions.push(
        vscode.commands.registerCommand('codegraph.showMemory', async (memoryId: string) => {
            try {
                const response = await client.sendRequest<MemoryGetResponse>(
                    'workspace/executeCommand',
                    {
                        command: 'codegraph.memoryGet',
                        arguments: [{ id: memoryId }],
                    }
                );

                // Create a virtual document to show memory details
                const content = formatMemoryDetails(response);
                const doc = await vscode.workspace.openTextDocument({
                    content,
                    language: 'markdown',
                });
                await vscode.window.showTextDocument(doc, { preview: true });
            } catch (error) {
                vscode.window.showErrorMessage(`Failed to get memory: ${error}`);
            }
        })
    );

    // Register invalidate memory command
    context.subscriptions.push(
        vscode.commands.registerCommand('codegraph.invalidateMemory', async (item: MemoryTreeItem) => {
            const confirm = await vscode.window.showWarningMessage(
                `Invalidate memory "${item.memory.title}"?`,
                { modal: true },
                'Invalidate'
            );

            if (confirm === 'Invalidate') {
                try {
                    await client.sendRequest('workspace/executeCommand', {
                        command: 'codegraph.memoryInvalidate',
                        arguments: [{ id: item.memory.id }],
                    });
                    memoryProvider.refresh();
                    vscode.window.showInformationMessage('Memory invalidated');
                } catch (error) {
                    vscode.window.showErrorMessage(`Failed to invalidate memory: ${error}`);
                }
            }
        })
    );

    // Register store memory command
    context.subscriptions.push(
        vscode.commands.registerCommand('codegraph.storeMemory', async () => {
            // Select memory kind
            const kindItems: vscode.QuickPickItem[] = [
                { label: 'Debug Context', description: 'Bug fix or debugging information' },
                { label: 'Architectural Decision', description: 'Design decision and rationale' },
                { label: 'Known Issue', description: 'Known problem or limitation' },
                { label: 'Convention', description: 'Coding convention or pattern' },
                { label: 'Project Context', description: 'General project knowledge' },
            ];

            const kindSelection = await vscode.window.showQuickPick(kindItems, {
                placeHolder: 'Select memory type',
            });

            if (!kindSelection) {
                return;
            }

            const kindMap: Record<string, MemoryKind> = {
                'Debug Context': 'debug_context',
                'Architectural Decision': 'architectural_decision',
                'Known Issue': 'known_issue',
                'Convention': 'convention',
                'Project Context': 'project_context',
            };

            const kind = kindMap[kindSelection.label];

            // Get title
            const title = await vscode.window.showInputBox({
                prompt: 'Memory title',
                placeHolder: 'Enter a descriptive title...',
            });

            if (!title) {
                return;
            }

            // Get content
            const content = await vscode.window.showInputBox({
                prompt: 'Memory content',
                placeHolder: 'Enter the memory content...',
            });

            if (!content) {
                return;
            }

            // Get tags (optional)
            const tagsInput = await vscode.window.showInputBox({
                prompt: 'Tags (optional)',
                placeHolder: 'Enter comma-separated tags...',
            });

            const tags = tagsInput
                ? tagsInput.split(',').map(t => t.trim()).filter(t => t)
                : [];

            try {
                const response = await client.sendRequest<{ id: string; success: boolean }>(
                    'workspace/executeCommand',
                    {
                        command: 'codegraph.memoryStore',
                        arguments: [{
                            kind,
                            title,
                            content,
                            tags,
                        }],
                    }
                );

                if (response.success) {
                    memoryProvider.refresh();
                    vscode.window.showInformationMessage(`Memory stored with ID: ${response.id}`);
                } else {
                    vscode.window.showErrorMessage('Failed to store memory');
                }
            } catch (error) {
                vscode.window.showErrorMessage(`Failed to store memory: ${error}`);
            }
        })
    );

    // Register memory stats command (wrapped in try-catch for dev reloads)
    try {
        context.subscriptions.push(
            vscode.commands.registerCommand('codegraph.memoryStats', async () => {
            try {
                const stats = await client.sendRequest<MemoryStatsResponse>(
                    'workspace/executeCommand',
                    {
                        command: 'codegraph.memoryStats',
                        arguments: [],
                    }
                );

                const message = [
                    '📊 Memory Statistics:',
                    `Total: ${stats.totalMemories}`,
                    `Current: ${stats.currentMemories}`,
                    `Invalidated: ${stats.invalidatedMemories}`,
                    '',
                    'By Kind:',
                    ...Object.entries(stats.byKind).map(([k, v]) => `  ${k}: ${v}`),
                ].join('\n');

                vscode.window.showInformationMessage(message, { modal: true });
            } catch (error) {
                vscode.window.showErrorMessage(`Failed to get memory stats: ${error}`);
            }
        })
    );
    } catch {
        // Command may already be registered from previous activation
        console.log('[CodeGraph] memoryStats command already registered');
    }

    // Auto-refresh when files change
    context.subscriptions.push(
        vscode.workspace.onDidSaveTextDocument(() => {
            // Refresh after a short delay to allow auto-invalidation to complete
            setTimeout(() => memoryProvider.refresh(), 500);
        })
    );
}

/**
 * Format memory details as markdown.
 */
function formatMemoryDetails(memory: MemoryGetResponse): string {
    const lines = [
        `# ${memory.title}`,
        '',
        `**Kind:** ${formatKind(memory.kind)}`,
        `**Status:** ${memory.isCurrent ? '✓ Current' : '✗ Invalidated'}`,
        `**Confidence:** ${(memory.confidence * 100).toFixed(0)}%`,
        `**Created:** ${memory.createdAt}`,
    ];

    if (memory.validFrom) {
        lines.push(`**Valid From:** ${memory.validFrom}`);
    }

    if (memory.tags.length > 0) {
        lines.push(`**Tags:** ${memory.tags.join(', ')}`);
    }

    lines.push('', '---', '', '## Content', '', memory.content);

    if (memory.codeLinks.length > 0) {
        lines.push('', '## Code Links', '');
        for (const link of memory.codeLinks) {
            lines.push(`- ${link.nodeType}: \`${link.nodeId}\``);
        }
    }

    return lines.join('\n');
}

function formatKind(kind: Record<string, unknown>): string {
    // The kind is returned as an object with the variant name as key
    const kindName = Object.keys(kind)[0];
    const kindLabels: Record<string, string> = {
        DebugContext: 'Debug Context',
        ArchitecturalDecision: 'Architectural Decision',
        KnownIssue: 'Known Issue',
        Convention: 'Convention',
        ProjectContext: 'Project Context',
    };
    return kindLabels[kindName] || kindName;
}
