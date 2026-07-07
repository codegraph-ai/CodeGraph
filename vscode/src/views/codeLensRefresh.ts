// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Standalone refresh signal for the CodeLens/hover surfaces. Kept in its own
//! module (depending only on `vscode`, not `vscode-languageclient`) so that
//! index-completion code - which is unit-tested with a mocked `vscode` - can
//! fire a refresh without pulling the language-client runtime into the test.

import * as vscode from 'vscode';

const refreshEmitter = new vscode.EventEmitter<void>();

/** Fires when CodeLens/hover data should be re-fetched (e.g. after a reindex). */
export const onDidRefreshCodeLenses = refreshEmitter.event;

/** Invalidate all CodeLens/hover data so the editor re-requests fresh stats. */
export function refreshCodeLenses(): void {
    refreshEmitter.fire();
}
