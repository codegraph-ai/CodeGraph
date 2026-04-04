// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

import { describe, it, expect, beforeEach, vi } from 'vitest';
import {
  setMock,
  reset,
  getCalls,
  clearAllMocks,
} from '@vsforge/shim';

// Mock vscode module
vi.mock('vscode', async () => {
  const shim = await import('@vsforge/shim');
  return shim.vscode;
});

// Mock vscode-languageclient
vi.mock('vscode-languageclient/node', () => ({
  LanguageClient: vi.fn(),
  RequestType: vi.fn().mockImplementation((method: string) => ({ method })),
}));

// Helper to get vscode asynchronously
const getVscode = async () => {
  const shim = await import('@vsforge/shim');
  return shim.vscode;
};

describe('SymbolTreeProvider', () => {
  // Mock language client
  const mockClient = {
    sendRequest: vi.fn(),
  };

  // Mock extension context
  const mockContext = {
    subscriptions: [] as { dispose: () => void }[],
    extensionUri: { fsPath: '/test/extension' },
  };

  beforeEach(() => {
    reset();
    clearAllMocks();
    vi.clearAllMocks();
    mockContext.subscriptions = [];
    mockClient.sendRequest.mockReset();

    // Mock tree view creation
    setMock('window.createTreeView', {
      implementation: (viewId: string, options: any) => ({
        viewId,
        treeDataProvider: options.treeDataProvider,
        dispose: vi.fn(),
        reveal: vi.fn(),
        selection: [],
        visible: true,
        message: undefined,
        title: undefined,
        description: undefined,
        badge: undefined,
      }),
    });

    // Mock command registration
    setMock('commands.registerCommand', {
      implementation: (command: string, callback: Function) => ({
        dispose: vi.fn(),
      }),
    });

    // Mock input box
    setMock('window.showInputBox', {
      resolvedValue: '',
    });

    // Mock document operations
    setMock('workspace.openTextDocument', {
      implementation: async (uri: any) => ({
        uri,
        getText: () => '',
        languageId: 'typescript',
      }),
    });

    setMock('window.showTextDocument', {
      resolvedValue: {},
    });

    setMock('window.showErrorMessage', {
      resolvedValue: undefined,
    });

    // Mock file save event
    setMock('workspace.onDidSaveTextDocument', {
      implementation: (callback: Function) => ({
        dispose: vi.fn(),
      }),
    });
  });

  describe('constructor', () => {
    it('should create a symbol tree provider with client', async () => {
      const { SymbolTreeProvider } = await import('./treeProviders');
      const provider = new SymbolTreeProvider(mockClient as any);
      expect(provider).toBeDefined();
    });
  });

  describe('getTreeItem', () => {
    it('should return the tree item as-is', async () => {
      const vscode = await getVscode();
      const { SymbolTreeProvider } = await import('./treeProviders');
      const provider = new SymbolTreeProvider(mockClient as any);

      // Create a tree item manually using vscode API
      const treeItem = new vscode.TreeItem('TestFunction', vscode.TreeItemCollapsibleState.None);

      const result = provider.getTreeItem(treeItem as any);
      expect(result).toBe(treeItem);
    });
  });

  describe('getChildren', () => {
    it('should fetch root symbols from server', async () => {
      mockClient.sendRequest.mockResolvedValue({
        symbols: [
          {
            id: 'sym-1',
            name: 'MyClass',
            kind: 'class',
            language: 'typescript',
            uri: 'file:///test/file.ts',
            range: { start: { line: 0, character: 0 }, end: { line: 10, character: 0 } },
          },
          {
            id: 'sym-2',
            name: 'myFunction',
            kind: 'function',
            language: 'typescript',
            uri: 'file:///test/file.ts',
            range: { start: { line: 12, character: 0 }, end: { line: 15, character: 0 } },
          },
        ],
      });

      const { SymbolTreeProvider } = await import('./treeProviders');
      const provider = new SymbolTreeProvider(mockClient as any);
      const children = await provider.getChildren();

      expect(mockClient.sendRequest).toHaveBeenCalled();
      expect(children.length).toBe(2);
    });

    it('should return empty array on error', async () => {
      mockClient.sendRequest.mockRejectedValue(new Error('Network error'));

      const { SymbolTreeProvider } = await import('./treeProviders');
      const provider = new SymbolTreeProvider(mockClient as any);
      const children = await provider.getChildren();

      expect(children).toEqual([]);
    });

    it('should return children of a symbol with nested children', async () => {
      const vscode = await getVscode();
      mockClient.sendRequest.mockResolvedValue({
        symbols: [
          {
            id: 'sym-1',
            name: 'MyClass',
            kind: 'class',
            language: 'typescript',
            uri: 'file:///test/file.ts',
            range: { start: { line: 0, character: 0 }, end: { line: 20, character: 0 } },
            children: [
              {
                id: 'method-1',
                name: 'myMethod',
                kind: 'method',
                language: 'typescript',
                uri: 'file:///test/file.ts',
                range: { start: { line: 5, character: 2 }, end: { line: 10, character: 2 } },
              },
            ],
          },
        ],
      });

      const { SymbolTreeProvider } = await import('./treeProviders');
      const provider = new SymbolTreeProvider(mockClient as any);
      const rootChildren = await provider.getChildren();

      expect(rootChildren.length).toBe(1);
      expect(rootChildren[0].collapsibleState).toBe(vscode.TreeItemCollapsibleState.Collapsed);

      // Get children of the class
      const classChildren = await provider.getChildren(rootChildren[0]);
      expect(classChildren.length).toBe(1);
    });

    it('should apply filter when set', async () => {
      mockClient.sendRequest.mockResolvedValue({
        symbols: [],
      });

      const { SymbolTreeProvider } = await import('./treeProviders');
      const provider = new SymbolTreeProvider(mockClient as any);
      provider.setFilter('test');

      await provider.getChildren();

      expect(mockClient.sendRequest).toHaveBeenCalledWith(
        expect.anything(),
        { query: 'test' }
      );
    });
  });

  describe('refresh', () => {
    it('should fire tree data change event', async () => {
      const { SymbolTreeProvider } = await import('./treeProviders');
      const provider = new SymbolTreeProvider(mockClient as any);

      let eventFired = false;
      provider.onDidChangeTreeData(() => {
        eventFired = true;
      });

      provider.refresh();

      expect(eventFired).toBe(true);
    });
  });

  describe('setFilter', () => {
    it('should set filter and refresh', async () => {
      const { SymbolTreeProvider } = await import('./treeProviders');
      const provider = new SymbolTreeProvider(mockClient as any);

      let refreshCount = 0;
      provider.onDidChangeTreeData(() => {
        refreshCount++;
      });

      provider.setFilter('myFilter');

      expect(refreshCount).toBe(1);
    });
  });
});

describe('registerTreeDataProviders', () => {
  const mockClient = {
    sendRequest: vi.fn(),
  };

  const mockContext = {
    subscriptions: [] as { dispose: () => void }[],
  };

  beforeEach(() => {
    reset();
    clearAllMocks();
    vi.clearAllMocks();
    mockContext.subscriptions = [];

    setMock('window.createTreeView', {
      implementation: (viewId: string, options: any) => ({
        viewId,
        dispose: vi.fn(),
      }),
    });

    setMock('commands.registerCommand', {
      implementation: (command: string, callback: Function) => ({
        dispose: vi.fn(),
        command,
        callback,
      }),
    });

    setMock('window.showInputBox', {
      resolvedValue: 'filter-text',
    });

    setMock('workspace.openTextDocument', {
      resolvedValue: { uri: {} },
    });

    setMock('window.showTextDocument', {
      resolvedValue: {},
    });

    setMock('window.showErrorMessage', {
      resolvedValue: undefined,
    });

    setMock('workspace.onDidSaveTextDocument', {
      implementation: (callback: Function) => ({
        dispose: vi.fn(),
      }),
    });
  });

  it('should create tree view for codegraphSymbols', async () => {
    const { registerTreeDataProviders } = await import('./treeProviders');
    registerTreeDataProviders(mockContext as any, mockClient as any);

    const calls = getCalls({ namespace: 'window', method: 'createTreeView' });
    expect(calls.some(c => c.args[0] === 'codegraphSymbols')).toBe(true);
  });

  it('should register refresh command', async () => {
    const { registerTreeDataProviders } = await import('./treeProviders');
    registerTreeDataProviders(mockContext as any, mockClient as any);

    const calls = getCalls({ namespace: 'commands', method: 'registerCommand' });
    expect(calls.some(c => c.args[0] === 'codegraph.refreshSymbols')).toBe(true);
  });

  it('should register filter command', async () => {
    const { registerTreeDataProviders } = await import('./treeProviders');
    registerTreeDataProviders(mockContext as any, mockClient as any);

    const calls = getCalls({ namespace: 'commands', method: 'registerCommand' });
    expect(calls.some(c => c.args[0] === 'codegraph.filterSymbols')).toBe(true);
  });

  it('should register goToSymbol command', async () => {
    const { registerTreeDataProviders } = await import('./treeProviders');
    registerTreeDataProviders(mockContext as any, mockClient as any);

    const calls = getCalls({ namespace: 'commands', method: 'registerCommand' });
    expect(calls.some(c => c.args[0] === 'codegraph.goToSymbol')).toBe(true);
  });

  it('should register file save listener', async () => {
    const { registerTreeDataProviders } = await import('./treeProviders');
    registerTreeDataProviders(mockContext as any, mockClient as any);

    const calls = getCalls({ namespace: 'workspace', method: 'onDidSaveTextDocument' });
    expect(calls.length).toBeGreaterThan(0);
  });

  it('should add disposables to context subscriptions', async () => {
    const { registerTreeDataProviders } = await import('./treeProviders');
    registerTreeDataProviders(mockContext as any, mockClient as any);

    // Tree view + 3 commands + file watcher = 5 disposables
    expect(mockContext.subscriptions.length).toBeGreaterThanOrEqual(4);
  });
});
