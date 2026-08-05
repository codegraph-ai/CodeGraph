// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

import { describe, it, expect, vi, beforeEach } from 'vitest';

// Minimal, controllable `vscode` mock - the real module only exists in the VS
// Code runtime. The registered providers are captured out of the mock so the
// test drives the same objects the editor would.
const mockState = {
    config: {} as Record<string, unknown>,
    symbols: [] as unknown[],
    codeLensProvider: undefined as
        | { provideCodeLenses(doc: unknown, token: unknown): Promise<unknown[]> }
        | undefined,
    hoverProvider: undefined as
        | { provideHover(doc: unknown, pos: unknown): Promise<unknown> }
        | undefined,
};

vi.mock('vscode', () => {
    class CodeLens {
        constructor(
            public range: unknown,
            public command: { title: string; command: string; arguments: unknown[] },
        ) {}
    }
    class MarkdownString {
        value = '';
        constructor(
            _value?: string,
            public supportThemeIcons?: boolean,
        ) {}
        appendMarkdown(text: string) {
            this.value += text;
            return this;
        }
    }
    return {
        workspace: {
            getConfiguration: () => ({
                get: (key: string, fallback: unknown) =>
                    key in mockState.config ? mockState.config[key] : fallback,
            }),
            onDidCloseTextDocument: () => ({ dispose() {} }),
            onDidChangeConfiguration: () => ({ dispose() {} }),
        },
        languages: {
            registerCodeLensProvider: (_selector: unknown, provider: never) => {
                mockState.codeLensProvider = provider;
                return { dispose() {} };
            },
            registerHoverProvider: (_selector: unknown, provider: never) => {
                mockState.hoverProvider = provider;
                return { dispose() {} };
            },
        },
        commands: { registerCommand: () => ({ dispose() {} }), executeCommand: vi.fn() },
        window: { showTextDocument: vi.fn() },
        // codeLensRefresh.ts constructs an EventEmitter at module load.
        EventEmitter: class {
            fire() {}
            get event() {
                return () => ({ dispose() {} });
            }
            dispose() {}
        },
        CodeLens,
        MarkdownString,
        Hover: class {
            constructor(
                public contents: unknown,
                public range: unknown,
            ) {}
        },
        Position: class {
            constructor(
                public line: number,
                public character: number,
            ) {}
        },
        Selection: class {},
        Range: class {},
    };
});

import { registerCodeLens } from './codeLensProvider';

/** A document whose every line is its own range, as far as the provider cares. */
function documentWith(lineCount: number) {
    return {
        uri: { toString: () => 'file:///repo/src/lib.rs' },
        version: 1,
        lineCount,
        lineAt: (line: number) => ({ range: { line } }),
    };
}

function lensTitles(lenses: unknown[]): string[] {
    return (lenses as { command: { title: string } }[]).map((l) => l.command.title);
}

beforeEach(() => {
    mockState.config = {};
    mockState.symbols = [];
    mockState.codeLensProvider = undefined;
    mockState.hoverProvider = undefined;

    const client = {
        sendRequest: vi.fn(async () => ({ symbols: mockState.symbols })),
    };
    registerCodeLens({ subscriptions: [] } as never, client as never, undefined);
});

const token = { isCancellationRequested: false };

describe('CodeLens titles', () => {
    it('reports the counts a symbol actually has', async () => {
        mockState.symbols = [
            { name: 'parse_config', line: 5, callerCount: 1, testCount: 1, complexity: 1 },
        ];
        const lenses = await mockState.codeLensProvider!.provideCodeLenses(
            documentWith(30),
            token,
        );
        expect(lensTitles(lenses)).toEqual(['$(references) 1 caller  ·  $(beaker) 1 test']);
    });

    it('pluralises counts above one', async () => {
        mockState.symbols = [
            { name: 'load', line: 2, callerCount: 3, testCount: 2, complexity: 0 },
        ];
        const lenses = await mockState.codeLensProvider!.provideCodeLenses(
            documentWith(30),
            token,
        );
        expect(lensTitles(lenses)).toEqual(['$(references) 3 callers  ·  $(beaker) 2 tests']);
    });

    it('omits a zero count rather than rendering it', async () => {
        mockState.symbols = [
            { name: 'helper', line: 4, callerCount: 2, testCount: 0, complexity: 0 },
        ];
        const lenses = await mockState.codeLensProvider!.provideCodeLenses(
            documentWith(30),
            token,
        );
        expect(lensTitles(lenses)).toEqual(['$(references) 2 callers']);
    });

    it('renders no lens at all for a symbol with nothing to report', async () => {
        mockState.symbols = [
            // The uncalled, untested, trivial function that used to get
            // "0 callers · 0 tests · complexity 1" above it.
            { name: 'load_settings', line: 11, callerCount: 0, testCount: 0, complexity: 1 },
        ];
        const lenses = await mockState.codeLensProvider!.provideCodeLenses(
            documentWith(30),
            token,
        );
        expect(lenses).toEqual([]);
    });

    it('shows complexity only once it is worth the chrome', async () => {
        mockState.symbols = [
            { name: 'below', line: 1, callerCount: 0, testCount: 0, complexity: 4 },
            { name: 'atFloor', line: 2, callerCount: 0, testCount: 0, complexity: 5 },
        ];
        const lenses = await mockState.codeLensProvider!.provideCodeLenses(
            documentWith(30),
            token,
        );
        expect(lensTitles(lenses)).toEqual(['$(pulse) complexity 5']);
    });

    it('drops symbols the document no longer has a line for', async () => {
        mockState.symbols = [
            { name: 'stale', line: 99, callerCount: 1, testCount: 1, complexity: 1 },
        ];
        const lenses = await mockState.codeLensProvider!.provideCodeLenses(
            documentWith(30),
            token,
        );
        expect(lenses).toEqual([]);
    });

    it('renders nothing when the surface is switched off', async () => {
        mockState.config['codeLens.enabled'] = false;
        mockState.symbols = [
            { name: 'parse_config', line: 5, callerCount: 1, testCount: 1, complexity: 1 },
        ];
        const lenses = await mockState.codeLensProvider!.provideCodeLenses(
            documentWith(30),
            token,
        );
        expect(lenses).toEqual([]);
    });
});

describe('hover', () => {
    it('still shows every stat, including the zeroes the lens drops', async () => {
        mockState.symbols = [
            { name: 'load_settings', line: 11, callerCount: 0, testCount: 0, complexity: 1 },
        ];
        const hover = (await mockState.hoverProvider!.provideHover(documentWith(30), {
            line: 11,
        })) as { contents: { value: string } };
        expect(hover.contents.value).toContain('**load_settings**');
        expect(hover.contents.value).toContain('0 callers');
        expect(hover.contents.value).toContain('0 tests');
        expect(hover.contents.value).toContain('complexity 1');
    });
});
