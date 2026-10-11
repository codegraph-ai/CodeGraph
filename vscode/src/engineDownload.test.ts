// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

import { describe, it, expect, vi } from 'vitest';

vi.mock('vscode', () => ({}));

import { shouldSkipAutomaticUpdate, UPDATE_RETRY_BACKOFF_MS } from './engineDownload';

describe('shouldSkipAutomaticUpdate', () => {
    const now = 1_000_000_000_000;

    it('attempts the update when no earlier failure is recorded', () => {
        expect(shouldSkipAutomaticUpdate(undefined, '0.21.0', now)).toBe(false);
    });

    it('holds off a version that failed less than a day ago', () => {
        const failure = { version: '0.21.0', at: now - UPDATE_RETRY_BACKOFF_MS + 1 };
        expect(shouldSkipAutomaticUpdate(failure, '0.21.0', now)).toBe(true);
    });

    it('tries the same version again once a day has passed', () => {
        const failure = { version: '0.21.0', at: now - UPDATE_RETRY_BACKOFF_MS };
        expect(shouldSkipAutomaticUpdate(failure, '0.21.0', now)).toBe(false);
    });

    it('does not hold off a different version', () => {
        const failure = { version: '0.20.0', at: now - 1 };
        expect(shouldSkipAutomaticUpdate(failure, '0.21.0', now)).toBe(false);
    });
});
