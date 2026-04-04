// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

import { describe, it, expect, beforeEach, vi } from 'vitest';
import {
  setMock,
  reset,
  clearAllMocks,
} from '@vsforge/shim';
import {
  mockConfiguration,
  createTextDocument,
} from '@vsforge/test';

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

describe('CodeGraphAIProvider', () => {
  // Mock LanguageClient
  const mockClient = {
    sendRequest: vi.fn(),
  };

  beforeEach(() => {
    reset();
    clearAllMocks();
    vi.clearAllMocks();
    mockClient.sendRequest.mockReset();
  });

  describe('provideCodeContext', () => {
    it('should request AI context with correct parameters', async () => {
      // Set up configuration
      mockConfiguration({
        section: 'codegraph',
        values: {
          'ai.maxContextTokens': 4000,
        },
      });

      // Mock LSP response
      mockClient.sendRequest.mockResolvedValue({
        primaryContext: {
          code: 'function hello() { return "world"; }',
          language: 'typescript',
          type: 'function',
          name: 'hello',
        },
        relatedSymbols: [
          {
            code: 'const greeting = hello();',
            relationship: 'caller',
            relevanceScore: 0.9,
          },
        ],
        architecture: {
          module: 'greetings',
          neighbors: ['utils', 'core'],
        },
        tokenCount: 150,
      });

      const { CodeGraphAIProvider } = await import('./contextProvider');
      const provider = new CodeGraphAIProvider(mockClient as any);

      // Create a mock document
      const doc = createTextDocument({
        uri: 'file:///test/file.ts',
        fileName: '/test/file.ts',
        languageId: 'typescript',
        content: 'function hello() { return "world"; }',
      });

      const position = { line: 0, character: 10 };

      const context = await provider.provideCodeContext(
        doc as any,
        position as any,
        'explain'
      );

      // Verify LSP request was made
      expect(mockClient.sendRequest).toHaveBeenCalledWith(
        'workspace/executeCommand',
        expect.objectContaining({
          command: 'codegraph.getAIContext',
          arguments: expect.arrayContaining([
            expect.objectContaining({
              contextType: 'explain',
              maxTokens: 4000,
            }),
          ]),
        })
      );

      // Verify formatted response
      expect(context.primary.code).toBe('function hello() { return "world"; }');
      expect(context.primary.language).toBe('typescript');
      expect(context.primary.description).toBe('function: hello');
      expect(context.related).toHaveLength(1);
      expect(context.related[0].relationship).toBe('caller');
      expect(context.architecture?.module).toBe('greetings');
    });

    it('should use custom maxTokens from configuration', async () => {
      mockConfiguration({
        section: 'codegraph',
        values: {
          'ai.maxContextTokens': 8000,
        },
      });

      mockClient.sendRequest.mockResolvedValue({
        primaryContext: {
          code: 'test',
          language: 'typescript',
          type: 'variable',
          name: 'test',
        },
        relatedSymbols: [],
        tokenCount: 10,
      });

      const { CodeGraphAIProvider } = await import('./contextProvider');
      const provider = new CodeGraphAIProvider(mockClient as any);

      const doc = createTextDocument({
        uri: 'file:///test/file.ts',
        content: 'const test = 1;',
      });

      await provider.provideCodeContext(doc as any, { line: 0, character: 6 } as any, 'modify');

      expect(mockClient.sendRequest).toHaveBeenCalledWith(
        'workspace/executeCommand',
        expect.objectContaining({
          arguments: expect.arrayContaining([
            expect.objectContaining({
              maxTokens: 8000,
            }),
          ]),
        })
      );
    });

    it('should handle different intent types', async () => {
      mockConfiguration({
        section: 'codegraph',
        values: { 'ai.maxContextTokens': 4000 },
      });

      mockClient.sendRequest.mockResolvedValue({
        primaryContext: { code: 'test', language: 'typescript', type: 'function', name: 'test' },
        relatedSymbols: [],
        tokenCount: 10,
      });

      const { CodeGraphAIProvider } = await import('./contextProvider');
      const provider = new CodeGraphAIProvider(mockClient as any);

      const doc = createTextDocument({
        uri: 'file:///test/file.ts',
        content: 'function test() {}',
      });

      const intents = ['explain', 'modify', 'debug', 'test'] as const;

      for (const intent of intents) {
        mockClient.sendRequest.mockClear();
        await provider.provideCodeContext(doc as any, { line: 0, character: 9 } as any, intent);

        expect(mockClient.sendRequest).toHaveBeenCalledWith(
          'workspace/executeCommand',
          expect.objectContaining({
            arguments: expect.arrayContaining([
              expect.objectContaining({
                contextType: intent,
              }),
            ]),
          })
        );
      }
    });

    it('should handle response without architecture', async () => {
      mockConfiguration({
        section: 'codegraph',
        values: { 'ai.maxContextTokens': 4000 },
      });

      mockClient.sendRequest.mockResolvedValue({
        primaryContext: {
          code: 'const x = 1;',
          language: 'typescript',
          type: 'variable',
          name: 'x',
        },
        relatedSymbols: [],
        tokenCount: 5,
        // No architecture field
      });

      const { CodeGraphAIProvider } = await import('./contextProvider');
      const provider = new CodeGraphAIProvider(mockClient as any);

      const doc = createTextDocument({
        uri: 'file:///test/file.ts',
        content: 'const x = 1;',
      });

      const context = await provider.provideCodeContext(
        doc as any,
        { line: 0, character: 6 } as any,
        'explain'
      );

      expect(context.architecture).toBeUndefined();
    });
  });

  describe('buildEnhancedPrompt', () => {
    it('should build prompt with primary code only', async () => {
      const { CodeGraphAIProvider } = await import('./contextProvider');
      const provider = new CodeGraphAIProvider(mockClient as any);

      const context = {
        primary: {
          code: 'function hello() { return "world"; }',
          language: 'typescript',
          description: 'function: hello',
        },
        related: [],
      };

      const prompt = provider.buildEnhancedPrompt('What does this function do?', context);

      expect(prompt).toContain('function hello() { return "world"; }');
      expect(prompt).toContain('typescript');
      expect(prompt).toContain('What does this function do?');
      expect(prompt).not.toContain('## Related Code');
      expect(prompt).not.toContain('## Architecture Context');
    });

    it('should build prompt with related code', async () => {
      const { CodeGraphAIProvider } = await import('./contextProvider');
      const provider = new CodeGraphAIProvider(mockClient as any);

      const context = {
        primary: {
          code: 'function hello() { return "world"; }',
          language: 'typescript',
          description: 'function: hello',
        },
        related: [
          {
            code: 'const result = hello();',
            relationship: 'caller',
            relevance: 0.95,
          },
          {
            code: 'export { hello };',
            relationship: 'export',
            relevance: 0.8,
          },
        ],
      };

      const prompt = provider.buildEnhancedPrompt('Explain this', context);

      expect(prompt).toContain('## Related Code');
      expect(prompt).toContain('caller');
      expect(prompt).toContain('95%');
      expect(prompt).toContain('const result = hello();');
    });

    it('should build prompt with architecture context', async () => {
      const { CodeGraphAIProvider } = await import('./contextProvider');
      const provider = new CodeGraphAIProvider(mockClient as any);

      const context = {
        primary: {
          code: 'class Service {}',
          language: 'typescript',
          description: 'class: Service',
        },
        related: [],
        architecture: {
          module: 'services/auth',
          neighbors: ['utils/crypto', 'models/user', 'config'],
        },
      };

      const prompt = provider.buildEnhancedPrompt('How does this service work?', context);

      expect(prompt).toContain('## Architecture Context');
      expect(prompt).toContain('Module: services/auth');
      expect(prompt).toContain('utils/crypto');
      expect(prompt).toContain('models/user');
    });

    it('should limit related code to 5 items', async () => {
      const { CodeGraphAIProvider } = await import('./contextProvider');
      const provider = new CodeGraphAIProvider(mockClient as any);

      const context = {
        primary: {
          code: 'function main() {}',
          language: 'typescript',
          description: 'function: main',
        },
        related: Array.from({ length: 10 }, (_, i) => ({
          code: `function related${i}() {}`,
          relationship: `relation${i}`,
          relevance: 0.9 - i * 0.05,
        })),
      };

      const prompt = provider.buildEnhancedPrompt('Explain', context);

      // Should only include first 5 related items
      expect(prompt).toContain('related0');
      expect(prompt).toContain('related4');
      expect(prompt).not.toContain('related5');
      expect(prompt).not.toContain('related9');
    });
  });
});
