// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

import { describe, it, expect, vi } from 'vitest';

// Minimal `vscode` mock - the real module only exists in the VS Code runtime.
// Nothing in this module's import chain touches the API at load time, and the
// function under test takes its configuration as an argument, so an empty
// namespace is enough.
vi.mock('vscode', () => ({}));

import { engineSpawnEnv } from './server';

/** Stands in for a `WorkspaceConfiguration` over the given settings. */
function config(settings: Record<string, string | undefined>) {
    return { get: <T>(key: string) => settings[key] as T | undefined };
}

describe('engineSpawnEnv', () => {
    it('passes the base environment through untouched', () => {
        const env = engineSpawnEnv(config({}), { PATH: '/usr/bin' });

        expect(env.PATH).toBe('/usr/bin');
    });

    it('does not point CODEGRAPH_STATIC_MODEL at a bundled path the VSIX no longer ships', () => {
        // The regression: with `bin/**` excluded from the VSIX, defaulting to
        // <extensionPath>/bin/jina-code-static-256 left the engine pointed at
        // a directory that no longer existed and its vector engine failed to
        // start. Unset, the engine finds the shared ~/.codegraph copy itself.
        const env = engineSpawnEnv(config({ embeddingModel: 'static' }), {});

        expect(env.CODEGRAPH_STATIC_MODEL).toBeUndefined();
    });

    it('honours a static model directory the user named', () => {
        const env = engineSpawnEnv(
            config({ embeddingModel: 'static', staticModelPath: '/models/jina' }),
            {},
        );

        expect(env.CODEGRAPH_STATIC_MODEL).toBe('/models/jina');
    });

    it('ignores a static model directory when the static embedder is not selected', () => {
        const env = engineSpawnEnv(
            config({ embeddingModel: 'onnx', staticModelPath: '/models/jina' }),
            {},
        );

        expect(env.CODEGRAPH_STATIC_MODEL).toBeUndefined();
    });

    it('leaves an inherited CODEGRAPH_STATIC_MODEL alone rather than clearing it', () => {
        const env = engineSpawnEnv(config({ embeddingModel: 'static' }), {
            CODEGRAPH_STATIC_MODEL: '/from/the/users/shell',
        });

        expect(env.CODEGRAPH_STATIC_MODEL).toBe('/from/the/users/shell');
    });
});
