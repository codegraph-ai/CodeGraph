// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

import { describe, it, expect, beforeEach, vi, afterEach } from 'vitest';
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

describe('GraphVisualizationPanel', () => {
  // Mock language client
  const mockClient = {
    sendRequest: vi.fn(),
  };

  // Mock webview
  let mockWebview: any;
  let postedMessages: any[] = [];
  let messageHandler: ((message: any) => void) | null = null;

  // Mock panel
  let mockPanel: any;
  let disposeHandler: (() => void) | null = null;

  // Helper to get vscode
  const getVscode = async () => {
    const shim = await import('@vsforge/shim');
    return shim.vscode;
  };

  beforeEach(async () => {
    reset();
    clearAllMocks();
    vi.clearAllMocks();
    postedMessages = [];
    messageHandler = null;
    disposeHandler = null;

    const vscode = await getVscode();

    // Create mock webview
    mockWebview = {
      html: '',
      postMessage: vi.fn((msg: any) => {
        postedMessages.push(msg);
        return Promise.resolve(true);
      }),
      onDidReceiveMessage: vi.fn((handler: (msg: any) => void) => {
        messageHandler = handler;
        return { dispose: vi.fn() };
      }),
      asWebviewUri: vi.fn((uri: any) => uri),
      cspSource: 'self',
    };

    // Create mock panel
    mockPanel = {
      webview: mockWebview,
      viewColumn: vscode.ViewColumn.One,
      active: true,
      visible: true,
      reveal: vi.fn(),
      dispose: vi.fn(),
      onDidDispose: vi.fn((handler: () => void) => {
        disposeHandler = handler;
        return { dispose: vi.fn() };
      }),
      onDidChangeViewState: vi.fn(() => ({ dispose: vi.fn() })),
    };

    // Mock window.activeTextEditor
    setMock('window.activeTextEditor', {
      returnValue: {
        viewColumn: vscode.ViewColumn.One,
      },
    });

    // Mock createWebviewPanel
    setMock('window.createWebviewPanel', {
      implementation: (viewType: string, title: string, column: any, options: any) => {
        mockPanel.viewType = viewType;
        mockPanel.title = title;
        return mockPanel;
      },
    });

    // Mock workspace.openTextDocument
    setMock('workspace.openTextDocument', {
      implementation: async (uri: any) => ({
        uri,
        getText: () => 'mock content',
      }),
    });

    // Mock window.showTextDocument
    setMock('window.showTextDocument', {
      resolvedValue: {},
    });
  });

  afterEach(() => {
    vi.resetModules();
  });

  describe('createOrShow', () => {
    it('should create a new panel when none exists', async () => {
      const vscode = await getVscode();
      const { GraphVisualizationPanel } = await import('./graphPanel');

      // Reset static state
      (GraphVisualizationPanel as any).currentPanel = undefined;

      const extensionUri = vscode.Uri.file('/test/extension');
      const mockData = {
        nodes: [{ id: 'n1', label: 'Node 1', type: 'module', language: 'typescript' }],
        edges: [{ from: 'n1', to: 'n2', type: 'import' }],
      };

      GraphVisualizationPanel.createOrShow(
        extensionUri,
        mockClient as any,
        'dependency',
        mockData
      );

      const calls = getCalls({ namespace: 'window', method: 'createWebviewPanel' });
      expect(calls.length).toBe(1);
      expect(calls[0].args[0]).toBe('codegraphVisualization');
      expect(calls[0].args[1]).toContain('Dependencies');
    });

    it('should reveal existing panel instead of creating new one', async () => {
      const vscode = await getVscode();
      const { GraphVisualizationPanel } = await import('./graphPanel');

      // Reset static state
      (GraphVisualizationPanel as any).currentPanel = undefined;

      const extensionUri = vscode.Uri.file('/test/extension');
      const mockData = {
        nodes: [{ id: 'n1', label: 'Node 1', type: 'module', language: 'typescript' }],
        edges: [],
      };

      // Create first panel
      GraphVisualizationPanel.createOrShow(extensionUri, mockClient as any, 'dependency', mockData);

      // Create second panel - should reuse
      GraphVisualizationPanel.createOrShow(extensionUri, mockClient as any, 'call', mockData);

      const createCalls = getCalls({ namespace: 'window', method: 'createWebviewPanel' });
      expect(createCalls.length).toBe(1); // Only one panel created
      expect(mockPanel.reveal).toHaveBeenCalled();
    });

    it('should set webview HTML content', async () => {
      const vscode = await getVscode();
      const { GraphVisualizationPanel } = await import('./graphPanel');
      (GraphVisualizationPanel as any).currentPanel = undefined;

      const extensionUri = vscode.Uri.file('/test/extension');
      const mockData = { nodes: [], edges: [] };

      GraphVisualizationPanel.createOrShow(extensionUri, mockClient as any, 'dependency', mockData);

      expect(mockWebview.html).toContain('<!DOCTYPE html>');
      expect(mockWebview.html).toContain('CodeGraph Visualization');
    });

    it('should post initial graph data to webview', async () => {
      const vscode = await getVscode();
      const { GraphVisualizationPanel } = await import('./graphPanel');
      (GraphVisualizationPanel as any).currentPanel = undefined;

      const extensionUri = vscode.Uri.file('/test/extension');
      const mockData = {
        nodes: [{ id: 'n1', label: 'Test', type: 'module', language: 'typescript' }],
        edges: [],
      };

      GraphVisualizationPanel.createOrShow(extensionUri, mockClient as any, 'dependency', mockData);

      expect(postedMessages.some(m => m.command === 'renderGraph')).toBe(true);
      const renderMsg = postedMessages.find(m => m.command === 'renderGraph');
      expect(renderMsg.graphType).toBe('dependency');
    });

    it('should use correct title for call graph', async () => {
      const vscode = await getVscode();
      const { GraphVisualizationPanel } = await import('./graphPanel');
      (GraphVisualizationPanel as any).currentPanel = undefined;

      const extensionUri = vscode.Uri.file('/test/extension');
      const mockData = {
        nodes: [{ id: 'f1', name: 'myFunc', language: 'typescript' }],
        edges: [],
      };

      GraphVisualizationPanel.createOrShow(extensionUri, mockClient as any, 'call', mockData);

      const calls = getCalls({ namespace: 'window', method: 'createWebviewPanel' });
      expect(calls[0].args[1]).toContain('Call Graph');
    });

    it('should enable scripts in webview options', async () => {
      const vscode = await getVscode();
      const { GraphVisualizationPanel } = await import('./graphPanel');
      (GraphVisualizationPanel as any).currentPanel = undefined;

      const extensionUri = vscode.Uri.file('/test/extension');

      GraphVisualizationPanel.createOrShow(extensionUri, mockClient as any, 'dependency', { nodes: [], edges: [] });

      const calls = getCalls({ namespace: 'window', method: 'createWebviewPanel' });
      expect(calls[0].args[3].enableScripts).toBe(true);
    });

    it('should retain context when hidden', async () => {
      const vscode = await getVscode();
      const { GraphVisualizationPanel } = await import('./graphPanel');
      (GraphVisualizationPanel as any).currentPanel = undefined;

      const extensionUri = vscode.Uri.file('/test/extension');

      GraphVisualizationPanel.createOrShow(extensionUri, mockClient as any, 'dependency', { nodes: [], edges: [] });

      const calls = getCalls({ namespace: 'window', method: 'createWebviewPanel' });
      expect(calls[0].args[3].retainContextWhenHidden).toBe(true);
    });
  });

  describe('message handling', () => {
    it('should handle nodeClick message', async () => {
      const vscode = await getVscode();
      const { GraphVisualizationPanel } = await import('./graphPanel');
      (GraphVisualizationPanel as any).currentPanel = undefined;

      mockClient.sendRequest.mockResolvedValue({
        uri: 'file:///test/file.ts',
        range: {
          start: { line: 10, character: 0 },
          end: { line: 20, character: 0 },
        },
      });

      const extensionUri = vscode.Uri.file('/test/extension');
      GraphVisualizationPanel.createOrShow(extensionUri, mockClient as any, 'dependency', { nodes: [], edges: [] });

      // Simulate nodeClick message from webview
      if (messageHandler) {
        await messageHandler({ command: 'nodeClick', nodeId: 'node-1' });
      }

      // Should open document and show it
      const openCalls = getCalls({ namespace: 'workspace', method: 'openTextDocument' });
      expect(openCalls.length).toBeGreaterThan(0);
    });

    it('should handle expandNode message', async () => {
      const vscode = await getVscode();
      const { GraphVisualizationPanel } = await import('./graphPanel');
      (GraphVisualizationPanel as any).currentPanel = undefined;

      const extensionUri = vscode.Uri.file('/test/extension');
      GraphVisualizationPanel.createOrShow(extensionUri, mockClient as any, 'dependency', { nodes: [], edges: [] });

      // Clear previous messages
      postedMessages = [];

      // Simulate expandNode message
      if (messageHandler) {
        await messageHandler({ command: 'expandNode', nodeId: 'node-1' });
      }

      // Should post 'expanding' message back
      expect(postedMessages.some(m => m.command === 'expanding')).toBe(true);
    });

    it('should handle refresh message for dependency graph', async () => {
      const vscode = await getVscode();
      const { GraphVisualizationPanel } = await import('./graphPanel');
      (GraphVisualizationPanel as any).currentPanel = undefined;

      mockClient.sendRequest.mockResolvedValue({
        nodes: [{ id: 'n1', label: 'Refreshed', type: 'module', language: 'typescript' }],
        edges: [],
      });

      const extensionUri = vscode.Uri.file('/test/extension');
      GraphVisualizationPanel.createOrShow(extensionUri, mockClient as any, 'dependency', { nodes: [], edges: [] });

      postedMessages = [];

      // Simulate refresh message (no 'position' means dependency graph)
      if (messageHandler) {
        await messageHandler({ command: 'refresh', params: { uri: 'file:///test.ts' } });
      }

      expect(postedMessages.some(m => m.command === 'renderGraph')).toBe(true);
    });

    it('should handle refresh message for call graph', async () => {
      const vscode = await getVscode();
      const { GraphVisualizationPanel } = await import('./graphPanel');
      (GraphVisualizationPanel as any).currentPanel = undefined;

      mockClient.sendRequest.mockResolvedValue({
        nodes: [{ id: 'f1', name: 'func', language: 'typescript' }],
        edges: [],
      });

      const extensionUri = vscode.Uri.file('/test/extension');
      GraphVisualizationPanel.createOrShow(extensionUri, mockClient as any, 'call', { nodes: [], edges: [] });

      postedMessages = [];

      // Simulate refresh for call graph (has 'position')
      if (messageHandler) {
        await messageHandler({
          command: 'refresh',
          params: { uri: 'file:///test.ts', position: { line: 10, character: 5 } }
        });
      }

      const renderMsg = postedMessages.find(m => m.command === 'renderGraph');
      expect(renderMsg?.graphType).toBe('call');
    });

    it('should handle refresh errors', async () => {
      const vscode = await getVscode();
      const { GraphVisualizationPanel } = await import('./graphPanel');
      (GraphVisualizationPanel as any).currentPanel = undefined;

      mockClient.sendRequest.mockRejectedValue(new Error('Server error'));

      const extensionUri = vscode.Uri.file('/test/extension');
      GraphVisualizationPanel.createOrShow(extensionUri, mockClient as any, 'dependency', { nodes: [], edges: [] });

      postedMessages = [];

      if (messageHandler) {
        await messageHandler({ command: 'refresh', params: { uri: 'file:///test.ts' } });
      }

      expect(postedMessages.some(m => m.command === 'error')).toBe(true);
    });
  });

  describe('data conversion', () => {
    it('should convert dependency data correctly', async () => {
      const vscode = await getVscode();
      const { GraphVisualizationPanel } = await import('./graphPanel');
      (GraphVisualizationPanel as any).currentPanel = undefined;

      const extensionUri = vscode.Uri.file('/test/extension');
      const mockData = {
        nodes: [
          { id: 'n1', label: 'module1', type: 'module', language: 'typescript' },
          { id: 'n2', label: 'module2', type: 'file', language: 'javascript' },
        ],
        edges: [
          { from: 'n1', to: 'n2', type: 'import' },
        ],
      };

      GraphVisualizationPanel.createOrShow(extensionUri, mockClient as any, 'dependency', mockData);

      const renderMsg = postedMessages.find(m => m.command === 'renderGraph');
      expect(renderMsg.data.nodes.length).toBe(2);
      expect(renderMsg.data.edges.length).toBe(1);
    });

    it('should convert call graph data correctly', async () => {
      const vscode = await getVscode();
      const { GraphVisualizationPanel } = await import('./graphPanel');
      (GraphVisualizationPanel as any).currentPanel = undefined;

      const extensionUri = vscode.Uri.file('/test/extension');
      const mockData = {
        nodes: [
          { id: 'f1', name: 'funcA', language: 'typescript' },
          { id: 'f2', name: 'funcB', language: 'typescript' },
        ],
        edges: [
          { from: 'f1', to: 'f2', isRecursive: false },
          { from: 'f2', to: 'f2', isRecursive: true },
        ],
      };

      GraphVisualizationPanel.createOrShow(extensionUri, mockClient as any, 'call', mockData);

      const renderMsg = postedMessages.find(m => m.command === 'renderGraph');
      expect(renderMsg.data.nodes[0].type).toBe('function');
      expect(renderMsg.data.edges[0].type).toBe('call');
      expect(renderMsg.data.edges[1].type).toBe('recursive');
    });
  });

  describe('dispose', () => {
    it('should clear currentPanel on dispose', async () => {
      const vscode = await getVscode();
      const { GraphVisualizationPanel } = await import('./graphPanel');
      (GraphVisualizationPanel as any).currentPanel = undefined;

      const extensionUri = vscode.Uri.file('/test/extension');
      GraphVisualizationPanel.createOrShow(extensionUri, mockClient as any, 'dependency', { nodes: [], edges: [] });

      expect(GraphVisualizationPanel.currentPanel).toBeDefined();

      // Trigger dispose
      if (disposeHandler) {
        disposeHandler();
      }

      expect(GraphVisualizationPanel.currentPanel).toBeUndefined();
    });
  });
});
