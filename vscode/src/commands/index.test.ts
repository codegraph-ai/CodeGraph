// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

import { describe, it, expect, beforeEach, vi } from 'vitest';
import {
  setMock,
  reset,
  getCalls,
  clearAllMocks,
} from '@vsforge/shim';
import {
  mockActiveTextEditor,
  mockConfiguration,
  mockQuickPickSelection,
} from '@vsforge/test';

// Mock the vscode module before importing the commands
vi.mock('vscode', async () => {
  const shim = await import('@vsforge/shim');
  return shim.vscode;
});

// Mock vscode-languageclient
vi.mock('vscode-languageclient/node', () => ({
  LanguageClient: vi.fn(),
  RequestType: vi.fn().mockImplementation((method: string) => ({ method })),
}));

// Mock the GraphVisualizationPanel
vi.mock('../views/graphPanel', () => ({
  GraphVisualizationPanel: {
    createOrShow: vi.fn(),
  },
}));

// Mock the AI provider
vi.mock('../ai/contextProvider', () => ({
  CodeGraphAIProvider: vi.fn().mockImplementation(() => ({
    provideCodeContext: vi.fn().mockResolvedValue({
      primary: { code: 'test code', language: 'typescript', description: 'Test' },
      related: [],
    }),
  })),
}));

describe('CodeGraph Commands', () => {
  // Create a mock LanguageClient
  const mockClient = {
    sendRequest: vi.fn(),
  };

  // Create a mock ExtensionContext
  const mockContext = {
    subscriptions: [] as { dispose: () => void }[],
    extensionUri: { fsPath: '/test/extension' },
  };

  // Create a mock AI provider
  const mockAIProvider = {
    provideCodeContext: vi.fn(),
  };

  beforeEach(() => {
    // Reset mocks but not modules (commands stay registered)
    reset();
    clearAllMocks();
    vi.clearAllMocks();
    mockClient.sendRequest.mockReset();
    mockAIProvider.provideCodeContext.mockReset();
  });

  it('should register commands without throwing', async () => {
    const { registerCommands } = await import('./index');

    // Should not throw even if called multiple times
    expect(() => {
      registerCommands(mockContext as any, mockClient as any, mockAIProvider as any);
    }).not.toThrow();
  });

  it('should set up mocks correctly with VSForge shim', async () => {
    // Set up mock for activeTextEditor (using setMock directly)
    const mockEditor = {
      document: {
        uri: { toString: () => 'file:///test/file.ts' },
        fileName: '/test/file.ts',
        languageId: 'typescript',
      },
      selection: { active: { line: 0, character: 0 } },
    };

    setMock('window.activeTextEditor', {
      returnValue: mockEditor,
    });

    mockConfiguration({
      section: 'codegraph',
      values: {
        'visualization.defaultDepth': 5,
      },
    });

    // The configuration mock should work
    const { vscode } = await import('@vsforge/shim');
    const config = vscode.workspace.getConfiguration('codegraph');
    expect(config.get('visualization.defaultDepth')).toBe(5);
  });

  it('should mock quick pick selections', async () => {
    mockQuickPickSelection('Modify');

    // The mock should be registered
    const { vscode } = await import('@vsforge/shim');

    // Test that showQuickPick works with the mock
    const result = await vscode.window.showQuickPick([
      { label: 'Modify', value: 'modify' },
      { label: 'Delete', value: 'delete' },
    ]);

    expect(result).toEqual({ label: 'Modify', value: 'modify' });
  });

  it('should record API calls for verification', async () => {
    // Set up mocks first (shim requires mocks for all calls)
    setMock('window.showInformationMessage', { resolvedValue: undefined });
    setMock('window.showWarningMessage', { resolvedValue: undefined });

    const { vscode } = await import('@vsforge/shim');

    // Make some API calls
    await vscode.window.showInformationMessage('Test message');
    await vscode.window.showWarningMessage('Warning');

    // Verify calls were recorded
    const infoCalls = getCalls({ namespace: 'window', method: 'showInformationMessage' });
    const warningCalls = getCalls({ namespace: 'window', method: 'showWarningMessage' });

    expect(infoCalls.length).toBeGreaterThan(0);
    expect(warningCalls.length).toBeGreaterThan(0);
    expect(infoCalls[0].args[0]).toBe('Test message');
    expect(warningCalls[0].args[0]).toBe('Warning');
  });

  it('should mock configuration values', async () => {
    mockConfiguration({
      section: 'codegraph',
      values: {
        'visualization.defaultDepth': 10,
        'ai.maxContextTokens': 8000,
      },
    });

    const { vscode } = await import('@vsforge/shim');
    const config = vscode.workspace.getConfiguration('codegraph');

    expect(config.get('visualization.defaultDepth')).toBe(10);
    expect(config.get('ai.maxContextTokens')).toBe(8000);
  });
});
