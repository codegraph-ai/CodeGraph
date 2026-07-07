// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

/**
 * Single source of truth for "what's safe to log" — every string-typed
 * value that reaches the reporter must come from one of these enums.
 *
 * If a value isn't in the matching allowlist, the reporter substitutes
 * `'other'` so we never emit a free-form identifier that could leak
 * workspace identity (e.g. an unusual language like `solidity` indexed
 * by 4 users in a 507-install dataset would re-identify the cohort).
 */

/** All 32 language-model tools registered by the community extension. */
export const TOOL_NAMES = [
    'codegraph_get_dependency_graph',
    'codegraph_get_call_graph',
    'codegraph_analyze_impact',
    'codegraph_get_ai_context',
    'codegraph_get_edit_context',
    'codegraph_get_curated_context',
    'codegraph_find_related_tests',
    'codegraph_get_symbol_info',
    'codegraph_analyze_complexity',
    'codegraph_symbol_search',
    'codegraph_find_by_imports',
    'codegraph_find_entry_points',
    'codegraph_traverse_graph',
    'codegraph_get_callers',
    'codegraph_get_callees',
    'codegraph_get_detailed_symbol',
    'codegraph_find_by_signature',
    'codegraph_memory_store',
    'codegraph_memory_search',
    'codegraph_memory_get',
    'codegraph_memory_context',
    'codegraph_memory_invalidate',
    'codegraph_memory_list',
    'codegraph_memory_stats',
    'codegraph_reindex_workspace',
    'codegraph_find_implementors',
    'codegraph_index_files',
    'codegraph_index_directory',
] as const;
export type ToolName = (typeof TOOL_NAMES)[number];
const TOOL_NAME_SET = new Set<string>(TOOL_NAMES);
export function isToolName(s: string): s is ToolName {
    return TOOL_NAME_SET.has(s);
}

/** Command palette commands the extension registers. */
export const COMMAND_IDS = [
    'codegraph.showDependencyGraph',
    'codegraph.showCallGraph',
    'codegraph.analyzeImpact',
    'codegraph.showMetrics',
    'codegraph.openAIChat',
    'codegraph.reindex',
    'codegraph.reindexWorkspace',
    'codegraph.debugTools',
    'codegraph.storeMemory',
    'codegraph.searchMemories',
    'codegraph.showMemory',
    'codegraph.invalidateMemory',
    'codegraph.memoryStats',
    'codegraph.mineGitHistory',
    'codegraph.refreshMemories',
    'codegraph.refreshSymbols',
    'codegraph.openSymbol',
    'codegraph.findReferences',
] as const;
export type CommandId = (typeof COMMAND_IDS)[number];
const COMMAND_ID_SET = new Set<string>(COMMAND_IDS);
export function isCommandId(s: string): s is CommandId {
    return COMMAND_ID_SET.has(s);
}

/**
 * Languages the extension's documentSelector activates for, plus the
 * server-supported set. Anything else collapses to `'other'`.
 */
export const LANGUAGES = [
    'python',
    'rust',
    'typescript',
    'javascript',
    'typescriptreact',
    'javascriptreact',
    'go',
    'c',
    'java',
    'cpp',
    'kotlin',
    'csharp',
    'other',
] as const;
export type Language = (typeof LANGUAGES)[number];
const LANGUAGE_SET = new Set<string>(LANGUAGES);
export function normalizeLanguage(s: string | undefined): Language {
    if (!s) return 'other';
    return (LANGUAGE_SET.has(s) ? s : 'other') as Language;
}

/** Error categories surfaced from tool / RPC failures — never raw text. */
export const ERROR_CATEGORIES = [
    'timeout',
    'cancelled',
    'server_unavailable',
    'connection_lost',
    'null_response',
    'not_indexed',
    'not_found',
    'invalid_params',
    'internal_error',
    'parse_error',
    'rpc_error',
    'other',
] as const;
export type ErrorCategory = (typeof ERROR_CATEGORIES)[number];

/**
 * Map an arbitrary error/string to a categorical bucket. The raw message
 * is NEVER returned — only one of the enum values above.
 *
 * Prefers the structured JSON-RPC `code` (vscode `ResponseError` carries a
 * numeric `.code`) since it's stable across locales; falls back to message
 * matching. The previous version collapsed almost everything to `'other'`
 * (telemetry showed 100% `other`), which made tool.error untriageable.
 */
export function categorizeError(err: unknown): ErrorCategory {
    const code =
        typeof err === 'object' && err !== null && typeof (err as { code?: unknown }).code === 'number'
            ? ((err as { code: number }).code)
            : undefined;
    if (code !== undefined) {
        switch (code) {
            case -32700:
                return 'parse_error';
            case -32602:
                return 'invalid_params';
            case -32603:
                return 'internal_error';
            case -32800: // RequestCancelled
            case -32801: // ContentModified
            case -32802: // ServerCancelled
                return 'cancelled';
        }
    }

    const message = String((err as { message?: unknown } | undefined)?.message ?? err).toLowerCase();

    // Connection / pipe death — the server process died mid-request. Same
    // failure family as the dominant spawn_fail hints (EPIPE, stream
    // destroyed, connection disposed); see extension.ts errorHint.
    if (
        message.includes('epipe') ||
        message.includes('write after') ||
        message.includes('stream was destroyed') ||
        message.includes('connection got disposed') ||
        message.includes('connection is disposed') ||
        message.includes('pending response rejected') ||
        message.includes('socket hang up')
    )
        return 'connection_lost';

    if (message.includes('cancel')) return 'cancelled';
    if (message.includes('timeout') || message.includes('timed out')) return 'timeout';
    if (
        message.includes('temporarily unavailable') ||
        message.includes('restarting') ||
        message.includes('not running')
    )
        return 'server_unavailable';
    if (
        message.includes('returned null') ||
        message.includes('null —') ||
        message.includes('null -')
    )
        return 'null_response';
    if (
        message.includes('not indexed') ||
        message.includes('no index') ||
        message.includes('index is empty')
    )
        return 'not_indexed';
    if (
        message.includes('not found') ||
        message.includes('no such') ||
        message.includes('no results') ||
        message.includes('does not exist') ||
        message.includes('unknown symbol')
    )
        return 'not_found';
    if (
        message.includes('invalid param') ||
        message.includes('missing required') ||
        message.includes('bad request') ||
        message.includes('invalid argument')
    )
        return 'invalid_params';
    if (message.includes('panic') || message.includes('internal error')) return 'internal_error';
    if (message.includes('parse error') || message.includes('failed to parse')) return 'parse_error';
    if (message.includes('rpc') || message.includes('jsonrpc')) return 'rpc_error';
    return 'other';
}

/** Activation outcomes from `client.start()`. */
export const ACTIVATION_OUTCOMES = ['ok', 'spawn_fail', 'stdio_fail', 'timeout', 'pro_detected'] as const;
export type ActivationOutcome = (typeof ACTIVATION_OUTCOMES)[number];

/** Index outcomes. */
export const INDEX_OUTCOMES = ['ok', 'error', 'cancelled'] as const;
export type IndexOutcome = (typeof INDEX_OUTCOMES)[number];

/** Why the user triggered an index. */
export const INDEX_TRIGGERS = [
    'activation_prompt',
    'command',
    'setting_change',
    'tool_invocation',
] as const;
export type IndexTrigger = (typeof INDEX_TRIGGERS)[number];

/** Tree views that fire visibility telemetry. */
export const TREE_VIEWS = ['symbols', 'memories'] as const;
export type TreeView = (typeof TREE_VIEWS)[number];

/** Graph panels. */
export const GRAPH_PANELS = ['dependency', 'call', 'impact'] as const;
export type GraphPanel = (typeof GRAPH_PANELS)[number];

/**
 * Why an index run produced zero files - the funnel dead-end that ~445 of
 * ~2,540 active machines hit, of which only ~16% ever recovered. Bounded so
 * we can measure which recovery hint to invest in without logging paths.
 */
export const ZERO_FILE_REASONS = [
    'no_workspace', // no folder open at all
    'no_supported_files', // folder open, but no files in a language we parse
    'index_paths_empty', // codegraph.indexPaths points only at missing/empty dirs
    'all_excluded', // matches exist but excludePatterns filtered every one
    'unknown', // files present, count still 0 (server-side gap)
] as const;
export type ZeroFileReason = (typeof ZERO_FILE_REASONS)[number];
const ZERO_FILE_REASON_SET = new Set<string>(ZERO_FILE_REASONS);
export function normalizeZeroFileReason(s: string | undefined): ZeroFileReason {
    if (!s) return 'unknown';
    return (ZERO_FILE_REASON_SET.has(s) ? s : 'unknown') as ZeroFileReason;
}

/**
 * Actions offered on the one-time post-first-index prompt that steers users
 * toward the surfaces telemetry shows already convert best (tree views: 483
 * machines; call graph: 116 - vs 238 that ever invoke an agent tool).
 * `dismissed` covers closing the toast without choosing.
 */
export const FIRST_INDEX_CTAS = [
    'explore_symbols',
    'show_call_graph',
    'open_walkthrough',
    'dismissed',
] as const;
export type FirstIndexCta = (typeof FIRST_INDEX_CTAS)[number];

/** Actions offered on the zero-file recovery prompt. */
export const ZERO_FILE_CTAS = ['open_folder', 'configure_paths', 'learn_more', 'dismissed'] as const;
export type ZeroFileCta = (typeof ZERO_FILE_CTAS)[number];

/** Server-health reasons. */
export const SERVER_RESTART_REASONS = ['crash', 'manual', 'setting_change'] as const;
export type ServerRestartReason = (typeof SERVER_RESTART_REASONS)[number];

/**
 * Crash-cause classes. `panic_*` come from the server's local crash
 * breadcrumb (`~/.codegraph/last-crash.*.json`, written by the panic hook);
 * `signal`/`hard_crash` are inferred by the extension when no breadcrumb
 * exists (SIGSEGV/SIGABRT/OOM-kill can't run the hook). Never raw text.
 */
export const CRASH_CAUSES = [
    'rocksdb_lock',
    'utf8_parse',
    'mutex_poison',
    'oom',
    'bounds',
    'panic_other',
    'signal',
    'hard_crash',
    'unknown',
] as const;
export type CrashCause = (typeof CRASH_CAUSES)[number];
const CRASH_CAUSE_SET = new Set<string>(CRASH_CAUSES);
export function normalizeCrashCause(s: string | undefined): CrashCause {
    if (!s) return 'unknown';
    return (CRASH_CAUSE_SET.has(s) ? s : 'panic_other') as CrashCause;
}

/**
 * Init phase the server reached before death — from `~/.codegraph/last-phase.*`,
 * stamped by the server as init progresses. Pairs with `crashCause`: a
 * `hard_crash` at phase `onnx_load` means the native ONNX load killed the
 * process (OOM/segfault) where the panic hook can't fire.
 */
export const CRASH_PHASES = [
    'startup',
    'rocksdb_open',
    'onnx_load',
    'post_onnx',
    'onnx_skipped_lowmem',
    'indexing',
    // Fine-grained index sub-phases (server >= 0.18.2) — split the old coarse
    // `serving`/`indexing` window so a startup crash localizes to parsing
    // (native tree-sitter), persistence (RocksDB), or embedding (native ONNX).
    'index_parse',
    'index_persist',
    'index_embed',
    // Pre-index startup (server >= 0.18.3): loading the persisted graph from
    // RocksDB (`graph_load` — the native 0xC0000005 suspect) and building the
    // in-memory indexes from it (`graph_build`).
    'graph_load',
    'graph_build',
    'serving',
    'ready',
    'unknown',
] as const;
export type CrashPhase = (typeof CRASH_PHASES)[number];
const CRASH_PHASE_SET = new Set<string>(CRASH_PHASES);
export function normalizeCrashPhase(s: string | undefined): CrashPhase {
    if (!s) return 'unknown';
    return (CRASH_PHASE_SET.has(s) ? s : 'unknown') as CrashPhase;
}

/**
 * Process exit signal of the crashed server child (unix). Distinguishes our
 * own crash (SIGSEGV/SIGILL/SIGABRT/SIGBUS/SIGFPE) from an external kill
 * (SIGKILL = OOM-killer or `kill -9`; SIGTERM = AV / shutdown). Windows has
 * no signals — it reports the exit code instead (see `exitCode`).
 */
export const EXIT_SIGNALS = [
    'SIGSEGV',
    'SIGABRT',
    'SIGKILL',
    'SIGILL',
    'SIGBUS',
    'SIGFPE',
    'SIGTERM',
    'SIGINT',
    'SIGHUP',
    'other',
] as const;
/**
 * Machine-profile enums (engagement.machineProfile) — bucketed, never PII.
 * Used to fingerprint the graph_load crash cohort: which classes of machine
 * corrupt their graph.db. The DETECTOR may read a path / MAC / AV name to
 * classify, but only ever emits the enum here — never the raw value.
 */

/** Where ~/.codegraph resolves — the top corruption suspect on Windows. */
export const DATA_DIR_KINDS = [
    'local',
    'cloud_onedrive',
    'cloud_other',
    'unc_network',
    'unknown',
] as const;
export type DataDirKind = (typeof DATA_DIR_KINDS)[number];
const DATA_DIR_KIND_SET = new Set<string>(DATA_DIR_KINDS);
export function normalizeDataDirKind(s: string | undefined): DataDirKind {
    if (!s) return 'unknown';
    return (DATA_DIR_KIND_SET.has(s) ? s : 'unknown') as DataDirKind;
}

/** Physical vs virtualized — affects disk/fsync durability semantics. */
export const MACHINE_KINDS = ['physical', 'vm', 'wsl', 'container', 'unknown'] as const;
export type MachineKind = (typeof MACHINE_KINDS)[number];
const MACHINE_KIND_SET = new Set<string>(MACHINE_KINDS);
export function normalizeMachineKind(s: string | undefined): MachineKind {
    if (!s) return 'unknown';
    return (MACHINE_KIND_SET.has(s) ? s : 'unknown') as MachineKind;
}

/** AV class (Windows) — third-party AV holding file handles is a corruption
 * suspect. Coarse class only; never the product name. */
export const ANTIVIRUS_KINDS = ['defender_only', 'third_party', 'none', 'unknown'] as const;
export type AntivirusKind = (typeof ANTIVIRUS_KINDS)[number];
const ANTIVIRUS_KIND_SET = new Set<string>(ANTIVIRUS_KINDS);
export function normalizeAntivirusKind(s: string | undefined): AntivirusKind {
    if (!s) return 'unknown';
    return (ANTIVIRUS_KIND_SET.has(s) ? s : 'unknown') as AntivirusKind;
}

export type ExitSignal = (typeof EXIT_SIGNALS)[number];
const EXIT_SIGNAL_SET = new Set<string>(EXIT_SIGNALS);
export function normalizeExitSignal(s: string | undefined): ExitSignal {
    if (!s) return 'other';
    return (EXIT_SIGNAL_SET.has(s) ? s : 'other') as ExitSignal;
}

/**
 * Settings included in `engagement.settingsSnapshot`. Only booleans,
 * server-defined enums, and bucketed numbers — NEVER free-form strings
 * (`excludePatterns`, `indexPaths`, custom `languages`) or path-bearing
 * values (`trace.server`).
 */
export const SETTINGS_SNAPSHOT_KEYS = {
    boolean: [
        'enabled',
        'indexOnStartup',
        'includePrivate',
        'includeTests',
        'parallelParsing',
        'cache.enabled',
        'fullBodyEmbedding',
        'memory.enabled',
        'memory.autoInvalidate',
        'memory.gitMining.enabled',
        'codeLens.enabled',
        'hover.enabled',
    ] as const,
    enum: ['embeddingModel', 'ai.contextStrategy'] as const,
    bucketedNumber: [
        'maxFileSizeKB',
        'visualization.defaultDepth',
        'ai.maxContextTokens',
        'cache.maxSizeMB',
        'memory.gitMining.maxCommits',
    ] as const,
} as const;
