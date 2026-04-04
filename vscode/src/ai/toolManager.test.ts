// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

import { describe, it, expect, beforeEach, vi, Mock } from 'vitest';
import {
    reset,
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
}));

describe('CodeGraphToolManager', () => {
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

    describe('formatDependencyGraph', () => {
        it('should format dependency graph with nodes and edges', async () => {
            const { CodeGraphToolManager } = await import('./toolManager');
            const manager = new CodeGraphToolManager(mockClient as any);

            // Access private method via any cast
            const format = (manager as any).formatDependencyGraph.bind(manager);

            const response = {
                nodes: [
                    { id: '1', label: 'main.ts', type: 'file', language: 'typescript', uri: 'file:///src/main.ts' },
                    { id: '2', label: 'utils.ts', type: 'file', language: 'typescript', uri: 'file:///src/utils.ts' },
                ],
                edges: [
                    { from: '1', to: '2', type: 'import' },
                ],
            };

            const result = format(response);

            expect(result).toContain('# Dependency Graph');
            expect(result).toContain('2 files/modules');
            expect(result).toContain('1 dependencies');
            expect(result).toContain('main.ts');
            expect(result).toContain('utils.ts');
            expect(result).toContain('import');
        });

        it('should handle empty dependency graph', async () => {
            const { CodeGraphToolManager } = await import('./toolManager');
            const manager = new CodeGraphToolManager(mockClient as any);
            const format = (manager as any).formatDependencyGraph.bind(manager);

            const response = { nodes: [], edges: [] };
            const result = format(response);

            expect(result).toContain('0 files/modules');
            expect(result).toContain('0 dependencies');
        });

        it('should group edges by type', async () => {
            const { CodeGraphToolManager } = await import('./toolManager');
            const manager = new CodeGraphToolManager(mockClient as any);
            const format = (manager as any).formatDependencyGraph.bind(manager);

            const response = {
                nodes: [
                    { id: '1', label: 'main.ts', type: 'file', language: 'typescript', uri: '' },
                    { id: '2', label: 'lib.ts', type: 'file', language: 'typescript', uri: '' },
                    { id: '3', label: 'pkg', type: 'package', language: 'typescript', uri: '' },
                ],
                edges: [
                    { from: '1', to: '2', type: 'import' },
                    { from: '1', to: '3', type: 'require' },
                    { from: '2', to: '3', type: 'use' },
                ],
            };

            const result = format(response);

            expect(result).toContain('## Dependencies (3)');
            expect(result).toContain('import');
            expect(result).toContain('require');
            expect(result).toContain('use');
        });

        it('should display node metadata', async () => {
            const { CodeGraphToolManager } = await import('./toolManager');
            const manager = new CodeGraphToolManager(mockClient as any);
            const format = (manager as any).formatDependencyGraph.bind(manager);

            const response = {
                nodes: [
                    { id: '1', label: 'auth.rs', type: 'module', language: 'rust', uri: 'file:///src/auth.rs' },
                ],
                edges: [],
            };

            const result = format(response);

            expect(result).toContain('**auth.rs**');
            expect(result).toContain('module');
            expect(result).toContain('rust');
            expect(result).toContain('Path:');
        });
    });

    describe('formatCallGraph', () => {
        it('should format call graph with root function', async () => {
            const { CodeGraphToolManager } = await import('./toolManager');
            const manager = new CodeGraphToolManager(mockClient as any);
            const format = (manager as any).formatCallGraph.bind(manager);

            const response = {
                root: {
                    id: '1',
                    name: 'processData',
                    signature: 'fn processData(data: Vec<u8>) -> Result<(), Error>',
                    uri: 'file:///src/lib.rs',
                    range: { start: { line: 10, character: 0 }, end: { line: 20, character: 1 } },
                    language: 'rust',
                    metrics: { complexity: 5, linesOfCode: 10 },
                },
                nodes: [
                    { id: '1', name: 'processData', signature: '', uri: 'file:///src/lib.rs', range: {}, language: 'rust' },
                    { id: '2', name: 'validateInput', signature: '', uri: 'file:///src/validate.rs', range: {}, language: 'rust' },
                ],
                edges: [
                    { from: '1', to: '2', callSites: [] },
                ],
            };

            const result = format(response);

            expect(result).toContain('# Call Graph');
            expect(result).toContain('processData');
            expect(result).toContain('## Target Function');
            expect(result).toContain('Complexity: 5');
            expect(result).toContain('Lines: 10');
        });

        it('should handle missing root function', async () => {
            const { CodeGraphToolManager } = await import('./toolManager');
            const manager = new CodeGraphToolManager(mockClient as any);
            const format = (manager as any).formatCallGraph.bind(manager);

            const response = {
                root: null,
                nodes: [],
                edges: [],
            };

            const result = format(response);

            expect(result).toContain('No function found at the specified position');
            expect(result).toContain('not on a function definition');
        });

        it('should group callers and callees', async () => {
            const { CodeGraphToolManager } = await import('./toolManager');
            const manager = new CodeGraphToolManager(mockClient as any);
            const format = (manager as any).formatCallGraph.bind(manager);

            const response = {
                root: { id: 'main', name: 'main', signature: '', uri: '', range: {}, language: 'typescript' },
                nodes: [
                    { id: 'main', name: 'main', signature: '', uri: '', range: {}, language: 'typescript' },
                    { id: 'caller1', name: 'init', signature: '', uri: 'file:///init.ts', range: {}, language: 'typescript' },
                    { id: 'callee1', name: 'helper', signature: '', uri: 'file:///helper.ts', range: {}, language: 'typescript' },
                ],
                edges: [
                    { from: 'caller1', to: 'main', callSites: [] },
                    { from: 'main', to: 'callee1', callSites: [] },
                ],
            };

            const result = format(response);

            expect(result).toContain('## Callers (1)');
            expect(result).toContain('## Callees (1)');
            expect(result).toContain('init');
            expect(result).toContain('helper');
        });

        it('should handle functions without metrics', async () => {
            const { CodeGraphToolManager } = await import('./toolManager');
            const manager = new CodeGraphToolManager(mockClient as any);
            const format = (manager as any).formatCallGraph.bind(manager);

            const response = {
                root: { id: '1', name: 'simpleFunc', signature: '() -> void', uri: '', range: {}, language: 'typescript' },
                nodes: [],
                edges: [],
            };

            const result = format(response);

            expect(result).toContain('simpleFunc');
            expect(result).not.toContain('Complexity:');
        });
    });

    describe('formatImpactAnalysis', () => {
        it('should format impact analysis summary', async () => {
            const { CodeGraphToolManager } = await import('./toolManager');
            const manager = new CodeGraphToolManager(mockClient as any);
            const format = (manager as any).formatImpactAnalysis.bind(manager);

            const response = {
                directImpact: [],
                indirectImpact: [],
                affectedTests: [],
                summary: {
                    filesAffected: 5,
                    breakingChanges: 2,
                    warnings: 3,
                },
            };

            const result = format(response);

            expect(result).toContain('# Impact Analysis');
            expect(result).toContain('Files Affected: 5');
            expect(result).toContain('Breaking Changes: 2');
            expect(result).toContain('Warnings: 3');
        });

        it('should format direct impact with severity icons', async () => {
            const { CodeGraphToolManager } = await import('./toolManager');
            const manager = new CodeGraphToolManager(mockClient as any);
            const format = (manager as any).formatImpactAnalysis.bind(manager);

            const response = {
                directImpact: [
                    { uri: 'file:///a.ts', range: { start: { line: 10 } }, type: 'caller', severity: 'breaking' },
                    { uri: 'file:///b.ts', range: { start: { line: 20 } }, type: 'reference', severity: 'warning' },
                    { uri: 'file:///c.ts', range: { start: { line: 30 } }, type: 'subclass', severity: 'info' },
                ],
                indirectImpact: [],
                affectedTests: [],
                summary: { filesAffected: 3, breakingChanges: 1, warnings: 1 },
            };

            const result = format(response);

            expect(result).toContain('## Direct Impact (3)');
            expect(result).toContain('🔴 BREAKING');
            expect(result).toContain('🟡 WARNING');
            expect(result).toContain('🔵 INFO');
        });

        it('should format indirect impact with dependency paths', async () => {
            const { CodeGraphToolManager } = await import('./toolManager');
            const manager = new CodeGraphToolManager(mockClient as any);
            const format = (manager as any).formatImpactAnalysis.bind(manager);

            const response = {
                directImpact: [],
                indirectImpact: [
                    { uri: 'file:///z.ts', path: ['a.ts', 'b.ts', 'z.ts'], severity: 'breaking' },
                ],
                affectedTests: [],
                summary: { filesAffected: 1, breakingChanges: 1, warnings: 0 },
            };

            const result = format(response);

            expect(result).toContain('## Indirect Impact (1)');
            expect(result).toContain('a.ts → b.ts → z.ts');
        });

        it('should format affected tests', async () => {
            const { CodeGraphToolManager } = await import('./toolManager');
            const manager = new CodeGraphToolManager(mockClient as any);
            const format = (manager as any).formatImpactAnalysis.bind(manager);

            const response = {
                directImpact: [],
                indirectImpact: [],
                affectedTests: [
                    { uri: 'file:///test/unit.test.ts', testName: 'should validate input' },
                    { uri: 'file:///test/integration.test.ts', testName: 'should handle API response' },
                ],
                summary: { filesAffected: 0, breakingChanges: 0, warnings: 0 },
            };

            const result = format(response);

            expect(result).toContain('## Affected Tests (2)');
            expect(result).toContain('🧪');
            expect(result).toContain('should validate input');
            expect(result).toContain('should handle API response');
        });
    });

    describe('formatAIContext', () => {
        it('should format primary context with code block', async () => {
            const { CodeGraphToolManager } = await import('./toolManager');
            const manager = new CodeGraphToolManager(mockClient as any);
            const format = (manager as any).formatAIContext.bind(manager);

            const response = {
                primaryContext: {
                    type: 'function',
                    name: 'calculateTotal',
                    code: 'function calculateTotal(items) { return items.reduce((a, b) => a + b, 0); }',
                    language: 'javascript',
                    location: { uri: 'file:///src/calc.js' },
                },
                relatedSymbols: [],
                dependencies: [],
                metadata: { totalTokens: 50, queryTime: 100 },
            };

            const result = format(response);

            expect(result).toContain('# Code Context');
            expect(result).toContain('function: calculateTotal');
            expect(result).toContain('Language: javascript');
            expect(result).toContain('```javascript');
            expect(result).toContain('calculateTotal');
        });

        it('should format related symbols with relevance scores', async () => {
            const { CodeGraphToolManager } = await import('./toolManager');
            const manager = new CodeGraphToolManager(mockClient as any);
            const format = (manager as any).formatAIContext.bind(manager);

            const response = {
                primaryContext: {
                    type: 'function',
                    name: 'main',
                    code: 'fn main() {}',
                    language: 'rust',
                    location: { uri: '' },
                },
                relatedSymbols: [
                    { name: 'helper', relationship: 'calls', code: 'fn helper() {}', relevanceScore: 0.95, location: {} },
                    { name: 'util', relationship: 'uses', code: 'mod util;', relevanceScore: 0.75, location: {} },
                ],
                dependencies: [],
                metadata: { totalTokens: 100, queryTime: 50 },
            };

            const result = format(response);

            expect(result).toContain('## Related Code (2)');
            expect(result).toContain('calls');
            expect(result).toContain('95%');
            expect(result).toContain('helper');
            expect(result).toContain('75%');
        });

        it('should limit related symbols to 5', async () => {
            const { CodeGraphToolManager } = await import('./toolManager');
            const manager = new CodeGraphToolManager(mockClient as any);
            const format = (manager as any).formatAIContext.bind(manager);

            const relatedSymbols = Array.from({ length: 10 }, (_, i) => ({
                name: `symbol${i}`,
                relationship: 'related',
                code: `code${i}`,
                relevanceScore: 0.9 - i * 0.05,
                location: {},
            }));

            const response = {
                primaryContext: { type: 'function', name: 'test', code: '', language: 'typescript', location: {} },
                relatedSymbols,
                dependencies: [],
                metadata: { totalTokens: 200, queryTime: 100 },
            };

            const result = format(response);

            expect(result).toContain('symbol0');
            expect(result).toContain('symbol4');
            expect(result).not.toContain('symbol5');
        });

        it('should format architecture context', async () => {
            const { CodeGraphToolManager } = await import('./toolManager');
            const manager = new CodeGraphToolManager(mockClient as any);
            const format = (manager as any).formatAIContext.bind(manager);

            const response = {
                primaryContext: { type: 'class', name: 'UserService', code: 'class UserService {}', language: 'typescript', location: {} },
                relatedSymbols: [],
                dependencies: [],
                architecture: {
                    module: 'services/user',
                    neighbors: ['repositories/user', 'utils/validation', 'config'],
                },
                metadata: { totalTokens: 80, queryTime: 75 },
            };

            const result = format(response);

            expect(result).toContain('## Architecture Context');
            expect(result).toContain('Module: services/user');
            expect(result).toContain('repositories/user');
            expect(result).toContain('utils/validation');
        });
    });

    describe('formatRelatedTests', () => {
        it('should format related tests response', async () => {
            const { CodeGraphToolManager } = await import('./toolManager');
            const manager = new CodeGraphToolManager(mockClient as any);
            const format = (manager as any).formatRelatedTests.bind(manager);

            const response = {
                tests: [
                    {
                        uri: 'file:///project/src/processData.test.ts',
                        testName: 'testProcessData',
                        relationship: 'direct',
                        range: { start: { line: 10, character: 0 }, end: { line: 20, character: 0 } }
                    },
                    {
                        uri: 'file:///project/src/processData.test.ts',
                        testName: 'processDataIntegrationTest',
                        relationship: 'indirect',
                        range: { start: { line: 30, character: 0 }, end: { line: 40, character: 0 } }
                    },
                ],
                truncated: false,
            };

            const result = format(response);

            expect(result).toContain('# Related Tests');
            expect(result).toContain('testProcessData');
            expect(result).toContain('processDataIntegrationTest');
            expect(result).toContain('Found 2 related test(s)');
        });

        it('should handle no related tests found', async () => {
            const { CodeGraphToolManager } = await import('./toolManager');
            const manager = new CodeGraphToolManager(mockClient as any);
            const format = (manager as any).formatRelatedTests.bind(manager);

            const response = {
                tests: [],
                truncated: false,
            };

            const result = format(response);

            expect(result).toContain('No related tests found');
            expect(result).toContain('No tests exist for this code yet');
        });
    });

    describe('formatSymbolInfo', () => {
        it('should format hover information', async () => {
            const { CodeGraphToolManager } = await import('./toolManager');
            const manager = new CodeGraphToolManager(mockClient as any);
            const format = (manager as any).formatSymbolInfo.bind(manager);

            const data = {
                hovers: [
                    { contents: [{ value: '```typescript\nfunction test(): void\n```' }] },
                ],
                definitions: [],
                references: [],
                uri: 'file:///src/test.ts',
                line: 10,
                character: 5,
            };

            const result = format(data);

            expect(result).toContain('# Symbol Information');
            expect(result).toContain('Location: file:///src/test.ts:11:6');
            expect(result).toContain('## Documentation & Type Information');
            expect(result).toContain('function test(): void');
        });

        it('should format definitions', async () => {
            const { CodeGraphToolManager } = await import('./toolManager');
            const manager = new CodeGraphToolManager(mockClient as any);
            const format = (manager as any).formatSymbolInfo.bind(manager);

            const data = {
                hovers: [],
                definitions: [
                    { uri: { fsPath: '/src/lib.ts' }, range: { start: { line: 20 } } },
                    { uri: { fsPath: '/src/utils.ts' }, range: { start: { line: 5 } } },
                ],
                references: [],
                uri: 'file:///src/test.ts',
                line: 0,
                character: 0,
            };

            const result = format(data);

            expect(result).toContain('## Definitions');
            expect(result).toContain('/src/lib.ts:21');
            expect(result).toContain('/src/utils.ts:6');
        });

        it('should format references grouped by file', async () => {
            const { CodeGraphToolManager } = await import('./toolManager');
            const manager = new CodeGraphToolManager(mockClient as any);
            const format = (manager as any).formatSymbolInfo.bind(manager);

            const data = {
                hovers: [],
                definitions: [],
                references: [
                    { uri: { fsPath: '/src/a.ts' }, range: { start: { line: 10 } } },
                    { uri: { fsPath: '/src/a.ts' }, range: { start: { line: 20 } } },
                    { uri: { fsPath: '/src/b.ts' }, range: { start: { line: 5 } } },
                ],
                uri: 'file:///src/test.ts',
                line: 0,
                character: 0,
            };

            const result = format(data);

            expect(result).toContain('## References (3 usages)');
            expect(result).toContain('**a.ts** (2 references)');
            expect(result).toContain('Line 11');
            expect(result).toContain('Line 21');
            expect(result).toContain('**b.ts** (1 reference)');
        });

        it('should truncate references after 3 per file', async () => {
            const { CodeGraphToolManager } = await import('./toolManager');
            const manager = new CodeGraphToolManager(mockClient as any);
            const format = (manager as any).formatSymbolInfo.bind(manager);

            const data = {
                hovers: [],
                definitions: [],
                references: [
                    { uri: { fsPath: '/src/a.ts' }, range: { start: { line: 1 } } },
                    { uri: { fsPath: '/src/a.ts' }, range: { start: { line: 2 } } },
                    { uri: { fsPath: '/src/a.ts' }, range: { start: { line: 3 } } },
                    { uri: { fsPath: '/src/a.ts' }, range: { start: { line: 4 } } },
                    { uri: { fsPath: '/src/a.ts' }, range: { start: { line: 5 } } },
                ],
                uri: 'file:///src/test.ts',
                line: 0,
                character: 0,
            };

            const result = format(data);

            expect(result).toContain('... and 2 more');
        });

        it('should handle no symbol information', async () => {
            const { CodeGraphToolManager } = await import('./toolManager');
            const manager = new CodeGraphToolManager(mockClient as any);
            const format = (manager as any).formatSymbolInfo.bind(manager);

            const data = {
                hovers: [],
                definitions: [],
                references: [],
                uri: 'file:///src/test.ts',
                line: 0,
                character: 0,
            };

            const result = format(data);

            expect(result).toContain('No symbol information available');
        });
    });

    describe('dispose', () => {
        it('should dispose all registered tools', async () => {
            const { CodeGraphToolManager } = await import('./toolManager');
            const manager = new CodeGraphToolManager(mockClient as any);

            // Access internal disposables array
            const disposables = (manager as any).disposables;

            // Simulate having some disposables
            const mockDisposable1 = { dispose: vi.fn() };
            const mockDisposable2 = { dispose: vi.fn() };
            disposables.push(mockDisposable1, mockDisposable2);

            manager.dispose();

            expect(mockDisposable1.dispose).toHaveBeenCalled();
            expect(mockDisposable2.dispose).toHaveBeenCalled();
            expect((manager as any).disposables).toHaveLength(0);
        });
    });
});
