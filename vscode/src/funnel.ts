// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

/**
 * Onboarding-funnel repair.
 *
 * Telemetry (30-day window, ~2,540 active machines) showed two large leaks:
 *   1. ~445 machines produced a zero-file index and 84% of them never came
 *      back - the old "Indexed 0 files" toast was a dead end with no next step.
 *   2. Of ~2,394 machines that activate cleanly, only ~20% ever open a visible
 *      surface (tree views: 483, call graph: 116) and only ~9% invoke an agent
 *      tool - most activate and see nothing.
 *
 * This module owns the post-index UX that addresses both: it diagnoses why an
 * index came back empty and offers a concrete recovery, and - on the first
 * successful index - steers the user to the surfaces that already convert.
 *
 * The diagnosis is pure/observable so it can be unit-tested without a live
 * server; the notification wiring is a thin shell around it.
 */

import * as vscode from 'vscode';
import type { Reporter } from './telemetry/reporter';
import type { Language } from './telemetry/allowlists';
import type { ZeroFileReason } from './telemetry/allowlists';
import { refreshCodeLenses } from './views/codeLensRefresh';

/** globalState key: set once the first-index CTA has been shown. */
export const FIRST_INDEX_CTA_SHOWN_KEY = 'codegraph.funnel.firstIndexCtaShown';

/** Context key that gates the codegraphSymbols empty-state welcome. */
export const INDEXED_CONTEXT_KEY = 'codegraph.indexed';

const DOCS_ZERO_FILE_URL =
    'https://github.com/codegraph-ai/CodeGraph/blob/main/docs/troubleshooting.md#no-files-indexed';

/**
 * File extensions the community parsers understand, one flat set so a single
 * `findFiles` glob can answer "does this workspace contain anything we could
 * have parsed?". Kept deliberately broad so we never misdiagnose a real
 * workspace as `no_supported_files`.
 *
 * AUTHORITATIVE SOURCE: `crates/codegraph-server/src/parser_registry.rs`
 * (`supported_extensions()`, aggregated from each `codegraph-<lang>` parser's
 * `file_extensions()`). This list is a client-side mirror and must be kept in
 * sync when a parser is added or its extensions change. It is only used to
 * distinguish the `no_supported_files` vs `unknown` zero-file message, both of
 * which link to the same troubleshooting doc, so drift degrades the wording of
 * a recovery hint rather than breaking a feature. Follow-up: expose
 * `supported_extensions()` over LSP and consume it, with this list as the
 * offline fallback.
 */
export const SUPPORTED_EXTENSIONS: readonly string[] = [
    // scripting / dynamic
    'py', 'pyi', 'rb', 'php', 'pl', 'pm', 'lua', 'r', 'tcl', 'sh', 'bash',
    // systems
    'c', 'h', 'cpp', 'cc', 'cxx', 'hpp', 'hh', 'hxx', 'rs', 'go', 'zig',
    'swift', 'm', 'mm', 'v', 'sv', 'svh',
    // jvm
    'java', 'kt', 'kts', 'scala', 'sc', 'groovy', 'gradle', 'clj', 'cljs', 'cljc',
    // ml / functional
    'hs', 'ml', 'mli', 'ex', 'exs', 'erl', 'hrl', 'elm', 'jl',
    // web / .net
    'ts', 'tsx', 'js', 'jsx', 'mjs', 'cjs', 'cs', 'css', 'scss', 'sass', 'less', 'dart',
    // data / infra / legacy
    'toml', 'yaml', 'yml', 'tf', 'hcl', 'sol', 'cob', 'cbl', 'cpy',
    'f', 'f90', 'f95', 'f03', 'for',
];

/** The `findFiles` include-glob for any supported source file. */
export function supportedFilesGlob(): string {
    return `**/*.{${SUPPORTED_EXTENSIONS.join(',')}}`;
}

/**
 * Combine exclude patterns into a single `findFiles` exclude glob. A lone
 * pattern is passed through untouched: wrapping one pattern in `{...}` yields a
 * single-element brace whose nested `{a,b}` groups some glob engines mis-parse.
 * Multiple patterns are joined at the top level, where the separating commas
 * are unambiguous because each pattern's own braces balance.
 */
export function toExcludeGlob(patterns: string[]): string {
    return patterns.length === 1 ? patterns[0] : `{${patterns.join(',')}}`;
}

/** Read `result.files_indexed` from a reindex RPC response, defaulting to 0. */
export function filesIndexed(result: unknown): number {
    const n = (result as { files_indexed?: unknown } | null | undefined)?.files_indexed;
    return typeof n === 'number' ? n : 0;
}

export interface ZeroFileDiagnosis {
    reason: ZeroFileReason;
    hadWorkspace: boolean;
}

/**
 * Work out *why* an index produced no files, so the recovery prompt can offer
 * the one action that will actually help. Cheap and bounded: every scan is
 * capped and short-circuits on the first match.
 */
export async function diagnoseZeroFile(): Promise<ZeroFileDiagnosis> {
    const folders = vscode.workspace.workspaceFolders;
    if (!folders || folders.length === 0) {
        return { reason: 'no_workspace', hadWorkspace: false };
    }

    const config = vscode.workspace.getConfiguration('codegraph');
    const indexPaths = config.get<string[]>('indexPaths') ?? [];
    const excludePatterns = config.get<string[]>('excludePatterns') ?? [];

    // When indexPaths is set it defines the *effective* scope of indexing, so
    // "is there anything to index?" must be asked within that scope. A
    // whole-workspace scan would miss the common misconfiguration where the
    // configured paths are missing/empty but source lives elsewhere.
    if (indexPaths.length > 0) {
        const inScope = await indexPathsContainSource(folders[0], indexPaths);
        if (!inScope) {
            return { reason: 'index_paths_empty', hadWorkspace: true };
        }
    } else {
        const anySupported = await vscode.workspace.findFiles(supportedFilesGlob(), undefined, 1);
        if (anySupported.length === 0) {
            return { reason: 'no_supported_files', hadWorkspace: true };
        }
    }

    // Supported files exist in scope. If excludes filter every one of them out,
    // the excludes are the cause; otherwise it's an unexplained server-side gap
    // (files present, still zero indexed).
    if (excludePatterns.length > 0) {
        const anyIncluded = await vscode.workspace.findFiles(
            supportedFilesGlob(),
            toExcludeGlob(excludePatterns),
            1,
        );
        if (anyIncluded.length === 0) {
            return { reason: 'all_excluded', hadWorkspace: true };
        }
    }

    return { reason: 'unknown', hadWorkspace: true };
}

/** True if any configured index path contains at least one supported source file. */
async function indexPathsContainSource(
    folder: vscode.WorkspaceFolder,
    indexPaths: string[],
): Promise<boolean> {
    const suffix = `/**/*.{${SUPPORTED_EXTENSIONS.join(',')}}`;
    for (const raw of indexPaths) {
        const rel = raw.replace(/^\.\//, '').replace(/\/+$/, '');
        const pattern = new vscode.RelativePattern(folder, `${rel}${suffix}`);
        const hits = await vscode.workspace.findFiles(pattern, undefined, 1);
        if (hits.length > 0) return true;
    }
    return false;
}

/** What {@link handleIndexOutcome} did, so callers can decide any follow-up. */
export type IndexOutcomeAction = 'zero_file' | 'first_index_cta' | 'none';

/**
 * Route the outcome of an index run to the right onboarding UX, and keep the
 * `codegraph.indexed` context key (which gates the Symbols empty state and the
 * walkthrough's index step) in sync with the result.
 *
 * - `fileCount === 0` -> diagnose and offer a targeted recovery.
 * - first `fileCount > 0` on this install -> one-time steer to a converting
 *   surface, then never again (globalState-gated).
 * - subsequent successful indexes -> nothing (avoid nagging).
 *
 * Notifications are shown fire-and-forget: this function performs its
 * synchronous decisions (context key, globalState flag) and returns the chosen
 * action WITHOUT blocking on the user's button click, so it is safe to await
 * from an agent tool invocation. Callers use the returned action to decide
 * whether to add their own confirmation toast.
 */
export async function handleIndexOutcome(
    context: vscode.ExtensionContext,
    reporter: Reporter | undefined,
    fileCount: number,
    opts: { offerSurfaceCta: boolean } = { offerSurfaceCta: true },
): Promise<IndexOutcomeAction> {
    // Centralized so every index-completion caller keeps the empty-state and
    // walkthrough in sync without duplicating the setContext call.
    void vscode.commands.executeCommand('setContext', INDEXED_CONTEXT_KEY, fileCount > 0);

    // Counts behind CodeLens/hover just changed - drop the per-document cache
    // so the editor re-fetches fresh caller/test/complexity stats.
    refreshCodeLenses();

    if (fileCount === 0) {
        // Detached: an agent-triggered index must not hang awaiting a dialog
        // the agent can't answer. The recovery prompt still shows to the human.
        void showZeroFileRecovery(reporter);
        return 'zero_file';
    }

    // The surface-steer prompt is only appropriate when a human just indexed
    // (activation / command flow). On the agent-driven reindex path we suppress
    // it - popping "Explore Symbols" mid-agent-task is disruptive, not helpful.
    if (!opts.offerSurfaceCta) return 'none';

    if (!context.globalState.get<boolean>(FIRST_INDEX_CTA_SHOWN_KEY)) {
        // Persist the flag before showing (awaited) so a reload mid-prompt
        // can't replay it; the prompt itself is detached.
        await context.globalState.update(FIRST_INDEX_CTA_SHOWN_KEY, true);
        void showFirstIndexCta(reporter, fileCount);
        return 'first_index_cta';
    }

    return 'none';
}

async function showZeroFileRecovery(reporter: Reporter | undefined): Promise<void> {
    const diag = await diagnoseZeroFile();
    reporter?.funnelZeroFileIndex({ reason: diag.reason, hadWorkspace: diag.hadWorkspace });

    // Message + actions tailored to the diagnosis. Each action maps to a
    // bounded ZeroFileCta so we can measure which recovery users take.
    let message: string;
    const actions: { label: string; cta: 'open_folder' | 'configure_paths' | 'learn_more' }[] = [];

    switch (diag.reason) {
        case 'no_workspace':
            message = 'CodeGraph: no folder is open, so there was nothing to index. Open a folder to get code intelligence.';
            actions.push({ label: 'Open Folder', cta: 'open_folder' });
            break;
        case 'index_paths_empty':
            message = 'CodeGraph indexed 0 files: your codegraph.indexPaths setting points at locations with no source files. Update it or clear it to index the whole workspace.';
            actions.push({ label: 'Edit Settings', cta: 'configure_paths' });
            actions.push({ label: 'Learn More', cta: 'learn_more' });
            break;
        case 'all_excluded':
            message = 'CodeGraph indexed 0 files: every source file is matched by codegraph.excludePatterns. Loosen the excludes to index your code.';
            actions.push({ label: 'Edit Settings', cta: 'configure_paths' });
            actions.push({ label: 'Learn More', cta: 'learn_more' });
            break;
        case 'no_supported_files':
            message = 'CodeGraph indexed 0 files: no files in a supported language were found in this workspace.';
            actions.push({ label: 'Learn More', cta: 'learn_more' });
            break;
        default:
            message = 'CodeGraph indexed 0 files even though supported source files are present. This may be a bug - see troubleshooting.';
            actions.push({ label: 'Learn More', cta: 'learn_more' });
            break;
    }

    const choice = await vscode.window.showWarningMessage(message, ...actions.map((a) => a.label));
    const picked = actions.find((a) => a.label === choice);
    reporter?.funnelZeroFileCta({ reason: diag.reason, action: picked?.cta ?? 'dismissed' });

    switch (picked?.cta) {
        case 'open_folder':
            await vscode.commands.executeCommand('workbench.action.files.openFolder');
            break;
        case 'configure_paths':
            await vscode.commands.executeCommand(
                'workbench.action.openSettings',
                'codegraph.indexPaths',
            );
            break;
        case 'learn_more':
            await vscode.env.openExternal(vscode.Uri.parse(DOCS_ZERO_FILE_URL));
            break;
        default:
            break;
    }
}

async function showFirstIndexCta(reporter: Reporter | undefined, fileCount: number): Promise<void> {
    const EXPLORE = 'Explore Symbols';
    const CALL_GRAPH = 'Show Call Graph';
    const message = `CodeGraph indexed ${fileCount.toLocaleString()} file${fileCount === 1 ? '' : 's'}. Explore your code as a graph:`;

    const choice = await vscode.window.showInformationMessage(message, EXPLORE, CALL_GRAPH);

    if (choice === EXPLORE) {
        reporter?.funnelFirstIndexCta({ action: 'explore_symbols', fileCount });
        // Reveal the Symbols tree in the CodeGraph activity-bar container.
        await vscode.commands.executeCommand('codegraphSymbols.focus');
    } else if (choice === CALL_GRAPH) {
        reporter?.funnelFirstIndexCta({ action: 'show_call_graph', fileCount });
        await vscode.commands.executeCommand('codegraph.showCallGraph');
    } else {
        reporter?.funnelFirstIndexCta({ action: 'dismissed', fileCount });
    }
}

/**
 * Map a reindex-RPC response to the `index.completed` + `index.languageBreakdown`
 * telemetry events. Shared by every index-completion site (activation, the
 * reindex command, and the agent tool path) so the response-shape coupling
 * lives in exactly one place. The server-side `duration_ms` is preferred when
 * present (it excludes network RTT); otherwise the local wall-clock is used.
 */
export function reportIndexTelemetry(
    reporter: Reporter | undefined,
    localStartedAt: number,
    result: unknown,
): void {
    if (!reporter) return;
    const r = result as { duration_ms?: unknown; by_language?: unknown } | null | undefined;
    const durationMs =
        typeof r?.duration_ms === 'number' ? Number(r.duration_ms) : Date.now() - localStartedAt;
    reporter.indexCompleted({ outcome: 'ok', durationMs, fileCount: filesIndexed(result) });

    const byLanguage = r?.by_language;
    if (byLanguage && typeof byLanguage === 'object') {
        const map = new Map<Language, number>();
        for (const [lang, count] of Object.entries(byLanguage)) {
            if (typeof count === 'number') map.set(lang as Language, count);
        }
        if (map.size > 0) reporter.indexLanguageBreakdown(map);
    }
}
