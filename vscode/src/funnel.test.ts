// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

import { describe, it, expect, vi, beforeEach } from 'vitest';

// Minimal, controllable `vscode` mock - the real module only exists in the
// VS Code runtime. Test state is driven through `mockState`.
//
// findFiles is dispatched by argument shape to mirror the three distinct
// scans diagnoseZeroFile performs:
//   - RelativePattern include            -> an indexPaths-scoped scan
//   - string include, no exclude         -> the whole-workspace scan
//   - string include, with exclude glob  -> the excludes scan
const mockState = {
    folders: undefined as { uri: unknown }[] | undefined,
    config: {} as Record<string, unknown>,
    indexPathScoped: [] as unknown[],
    supportedNoExclude: [] as unknown[],
    supportedWithExclude: [] as unknown[],
};

vi.mock('vscode', () => {
    // Defined inside the (hoisted) factory: vitest only lets `mock`-prefixed
    // outer variables be referenced here, so the class must live in-closure.
    class RelativePattern {
        constructor(
            public base: unknown,
            public pattern: string,
        ) {}
    }
    return {
        workspace: {
            get workspaceFolders() {
                return mockState.folders;
            },
            getConfiguration: () => ({
                get: (key: string) => mockState.config[key],
            }),
            findFiles: vi.fn(async (include: unknown, exclude: unknown) => {
                if (include instanceof RelativePattern) return mockState.indexPathScoped;
                return exclude === undefined
                    ? mockState.supportedNoExclude
                    : mockState.supportedWithExclude;
            }),
        },
        RelativePattern,
        // codeLensRefresh.ts constructs an EventEmitter at module load.
        EventEmitter: class {
            fire() {}
            get event() {
                return () => ({ dispose() {} });
            }
            dispose() {}
        },
        Uri: { parse: (s: string) => ({ toString: () => s }) },
        commands: { executeCommand: vi.fn() },
        window: { showWarningMessage: vi.fn(), showInformationMessage: vi.fn() },
        env: { openExternal: vi.fn() },
    };
});

import {
    diagnoseZeroFile,
    supportedFilesGlob,
    toExcludeGlob,
    filesIndexed,
    SUPPORTED_EXTENSIONS,
} from './funnel';

beforeEach(() => {
    mockState.folders = [{ uri: {} }];
    mockState.config = {};
    mockState.indexPathScoped = [];
    mockState.supportedNoExclude = [];
    mockState.supportedWithExclude = [];
});

describe('supportedFilesGlob', () => {
    it('covers the common languages seen in telemetry', () => {
        for (const ext of ['ts', 'py', 'rs', 'c', 'cpp', 'java', 'cs', 'go', 'kt']) {
            expect(SUPPORTED_EXTENSIONS).toContain(ext);
        }
    });

    it('produces a single brace-expansion glob', () => {
        const glob = supportedFilesGlob();
        expect(glob.startsWith('**/*.{')).toBe(true);
        expect(glob.endsWith('}')).toBe(true);
        expect(glob).toContain('ts,');
    });
});

describe('toExcludeGlob', () => {
    it('passes a lone pattern through without wrapping braces', () => {
        // Wrapping one pattern that itself contains a nested {a,b} group in an
        // outer single-element brace is what some glob engines mis-parse.
        expect(toExcludeGlob(['**/{test,spec}/**'])).toBe('**/{test,spec}/**');
    });

    it('brace-joins multiple patterns', () => {
        expect(toExcludeGlob(['**/node_modules/**', '**/dist/**'])).toBe(
            '{**/node_modules/**,**/dist/**}',
        );
    });
});

describe('filesIndexed', () => {
    it('reads a numeric files_indexed and defaults everything else to 0', () => {
        expect(filesIndexed({ files_indexed: 42 })).toBe(42);
        expect(filesIndexed({ files_indexed: '42' })).toBe(0);
        expect(filesIndexed({})).toBe(0);
        expect(filesIndexed(null)).toBe(0);
        expect(filesIndexed(undefined)).toBe(0);
    });
});

describe('diagnoseZeroFile', () => {
    it('reports no_workspace when no folder is open', async () => {
        mockState.folders = undefined;
        const d = await diagnoseZeroFile();
        expect(d).toEqual({ reason: 'no_workspace', hadWorkspace: false });
    });

    it('reports no_supported_files when the folder has nothing we parse', async () => {
        mockState.supportedNoExclude = []; // no supported source found
        const d = await diagnoseZeroFile();
        expect(d).toEqual({ reason: 'no_supported_files', hadWorkspace: true });
    });

    it('reports index_paths_empty when indexPaths yields nothing IN SCOPE, even if source exists elsewhere', async () => {
        mockState.config['indexPaths'] = ['does/not/exist'];
        mockState.indexPathScoped = []; // configured paths hold no source
        mockState.supportedNoExclude = [{ path: 'src/a.ts' }]; // ...but the workspace does
        const d = await diagnoseZeroFile();
        // The whole-workspace source must NOT mask the misconfigured indexPaths.
        expect(d).toEqual({ reason: 'index_paths_empty', hadWorkspace: true });
    });

    it('does not report index_paths_empty when the configured paths do contain source', async () => {
        mockState.config['indexPaths'] = ['src'];
        mockState.indexPathScoped = [{ path: 'src/a.ts' }];
        const d = await diagnoseZeroFile();
        expect(d.reason).toBe('unknown'); // source in scope, no excludes -> server-side gap
    });

    it('reports all_excluded when excludes filter out every source file', async () => {
        mockState.config['excludePatterns'] = ['**/*'];
        mockState.supportedNoExclude = [{ path: 'a.ts' }]; // source exists
        mockState.supportedWithExclude = []; // ...but all excluded
        const d = await diagnoseZeroFile();
        expect(d).toEqual({ reason: 'all_excluded', hadWorkspace: true });
    });

    it('reports unknown when source is present and not excluded (server-side gap)', async () => {
        mockState.supportedNoExclude = [{ path: 'a.ts' }];
        mockState.supportedWithExclude = [{ path: 'a.ts' }];
        const d = await diagnoseZeroFile();
        expect(d).toEqual({ reason: 'unknown', hadWorkspace: true });
    });

    it('treats an empty indexPaths array as "scan whole workspace"', async () => {
        mockState.config['indexPaths'] = [];
        mockState.supportedNoExclude = [];
        const d = await diagnoseZeroFile();
        expect(d.reason).toBe('no_supported_files');
    });
});
