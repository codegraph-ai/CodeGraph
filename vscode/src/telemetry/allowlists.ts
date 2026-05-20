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
    'null_response',
    'rpc_error',
    'other',
] as const;
export type ErrorCategory = (typeof ERROR_CATEGORIES)[number];

/**
 * Map an arbitrary error/string to a categorical bucket. The raw message
 * is NEVER returned — only one of the enum values above.
 */
export function categorizeError(err: unknown): ErrorCategory {
    const message = String(err).toLowerCase();
    if (message.includes('cancel')) return 'cancelled';
    if (message.includes('timeout') || message.includes('timed out')) return 'timeout';
    if (message.includes('temporarily unavailable')) return 'server_unavailable';
    if (
        message.includes('returned null') ||
        message.includes('null —') ||
        message.includes('null -')
    )
        return 'null_response';
    if (message.includes('rpc') || message.includes('jsonrpc')) return 'rpc_error';
    return 'other';
}

/** Activation outcomes from `client.start()`. */
export const ACTIVATION_OUTCOMES = ['ok', 'spawn_fail', 'stdio_fail', 'timeout'] as const;
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

/** Server-health reasons. */
export const SERVER_RESTART_REASONS = ['crash', 'manual', 'setting_change'] as const;
export type ServerRestartReason = (typeof SERVER_RESTART_REASONS)[number];

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
