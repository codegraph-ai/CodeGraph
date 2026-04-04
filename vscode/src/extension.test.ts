// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

import { describe, it, expect, beforeEach, vi, afterEach } from 'vitest';
import {
  setMock,
  reset,
  getCalls,
  clearAllMocks,
  vscode,
} from '@vsforge/shim';
import { mockConfiguration } from '@vsforge/test';

// Mock vscode module
vi.mock('vscode', async () => {
  const shim = await import('@vsforge/shim');
  return shim.vscode;
});

// Mock vscode-languageclient
const mockClientStart = vi.fn().mockResolvedValue(undefined);
const mockClientStop = vi.fn().mockResolvedValue(undefined);
const mockClientInstance = {
  start: mockClientStart,
  stop: mockClientStop,
  sendRequest: vi.fn(),
};

vi.mock('vscode-languageclient/node', () => ({
  LanguageClient: vi.fn().mockImplementation(() => mockClientInstance),
  LanguageClientOptions: vi.fn(),
  ServerOptions: vi.fn(),
  TransportKind: { stdio: 0 },
}));

// Mock registerCommands and registerTreeDataProviders
vi.mock('./commands', () => ({
  registerCommands: vi.fn(),
}));

vi.mock('./views/treeProviders', () => ({
  registerTreeDataProviders: vi.fn(),
}));

// Mock getServerPath
vi.mock('./server', () => ({
  getServerPath: vi.fn().mockReturnValue('/mock/server/binary'),
}));

// Mock CodeGraphAIProvider
vi.mock('./ai/contextProvider', () => ({
  CodeGraphAIProvider: vi.fn().mockImplementation(() => ({
    provideCodeContext: vi.fn(),
  })),
}));

describe('Extension Lifecycle', () => {
  // Create mock ExtensionContext
  const mockContext = {
    subscriptions: [] as { dispose: () => void }[],
    extensionUri: { fsPath: '/test/extension' },
    asAbsolutePath: vi.fn((p: string) => `/test/extension/${p}`),
    storagePath: '/test/storage',
    globalStoragePath: '/test/global-storage',
    logPath: '/test/logs',
    extensionPath: '/test/extension',
    extensionMode: 1,
    globalState: {
      get: vi.fn(),
      update: vi.fn(),
      keys: vi.fn().mockReturnValue([]),
    },
    workspaceState: {
      get: vi.fn(),
      update: vi.fn(),
      keys: vi.fn().mockReturnValue([]),
    },
    secrets: {
      get: vi.fn(),
      store: vi.fn(),
      delete: vi.fn(),
    },
  };

  beforeEach(() => {
    reset();
    clearAllMocks();
    vi.clearAllMocks();
    mockContext.subscriptions = [];
    mockClientStart.mockReset().mockResolvedValue(undefined);
    mockClientStop.mockReset().mockResolvedValue(undefined);

    // Default: extension is enabled
    mockConfiguration({
      section: 'codegraph',
      values: {
        enabled: true,
      },
    });

    // Mock output channels
    setMock('window.createOutputChannel', {
      implementation: (name: string) => ({
        name,
        append: vi.fn(),
        appendLine: vi.fn(),
        clear: vi.fn(),
        show: vi.fn(),
        hide: vi.fn(),
        dispose: vi.fn(),
      }),
    });

    // Mock file system watcher
    setMock('workspace.createFileSystemWatcher', {
      implementation: () => ({
        onDidCreate: vi.fn(() => ({ dispose: () => {} })),
        onDidChange: vi.fn(() => ({ dispose: () => {} })),
        onDidDelete: vi.fn(() => ({ dispose: () => {} })),
        dispose: vi.fn(),
      }),
    });

    // Mock executeCommand
    setMock('commands.executeCommand', {
      resolvedValue: undefined,
    });

    // Mock messages
    setMock('window.showInformationMessage', { resolvedValue: undefined });
    setMock('window.showErrorMessage', { resolvedValue: undefined });
  });

  afterEach(() => {
    vi.resetModules();
  });

  describe('activate', () => {
    it('should start language server when extension is enabled', async () => {
      const { activate } = await import('./extension');

      await activate(mockContext as any);

      expect(mockClientStart).toHaveBeenCalled();
    });

    it('should not start when extension is disabled', async () => {
      mockConfiguration({
        section: 'codegraph',
        values: {
          enabled: false,
        },
      });

      const { activate } = await import('./extension');
      await activate(mockContext as any);

      expect(mockClientStart).not.toHaveBeenCalled();
    });

    it('should show success message when server starts', async () => {
      const { activate } = await import('./extension');
      await activate(mockContext as any);

      const calls = getCalls({ namespace: 'window', method: 'showInformationMessage' });
      expect(calls.some(c => c.args[0].includes('Language server started'))).toBe(true);
    });

    it('should show error message when server fails to start', async () => {
      mockClientStart.mockRejectedValueOnce(new Error('Connection failed'));

      const { activate } = await import('./extension');
      await activate(mockContext as any);

      const calls = getCalls({ namespace: 'window', method: 'showErrorMessage' });
      expect(calls.some(c => c.args[0].includes('Failed to start'))).toBe(true);
    });

    it('should create output channels', async () => {
      const { activate } = await import('./extension');
      await activate(mockContext as any);

      const calls = getCalls({ namespace: 'window', method: 'createOutputChannel' });
      expect(calls.length).toBeGreaterThanOrEqual(2);
      expect(calls.some(c => c.args[0] === 'CodeGraph')).toBe(true);
      expect(calls.some(c => c.args[0] === 'CodeGraph Trace')).toBe(true);
    });

    it('should register commands after successful start', async () => {
      const { registerCommands } = await import('./commands');
      const { activate } = await import('./extension');

      await activate(mockContext as any);

      expect(registerCommands).toHaveBeenCalled();
    });

    it('should register tree data providers', async () => {
      const { registerTreeDataProviders } = await import('./views/treeProviders');
      const { activate } = await import('./extension');

      await activate(mockContext as any);

      expect(registerTreeDataProviders).toHaveBeenCalled();
    });

    it('should set codegraph.enabled context', async () => {
      const { activate } = await import('./extension');
      await activate(mockContext as any);

      const calls = getCalls({ namespace: 'commands', method: 'executeCommand' });
      expect(calls.some(c =>
        c.args[0] === 'setContext' &&
        c.args[1] === 'codegraph.enabled' &&
        c.args[2] === true
      )).toBe(true);
    });

    it('should add client to subscriptions for cleanup', async () => {
      const { activate } = await import('./extension');
      await activate(mockContext as any);

      expect(mockContext.subscriptions.length).toBeGreaterThan(0);
    });
  });

  describe('deactivate', () => {
    it('should stop client when deactivating', async () => {
      const { activate, deactivate } = await import('./extension');

      await activate(mockContext as any);
      await deactivate();

      expect(mockClientStop).toHaveBeenCalled();
    });
  });
});
