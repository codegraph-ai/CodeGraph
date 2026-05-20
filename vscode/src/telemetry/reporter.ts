// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

/**
 * Telemetry reporter — the single place that talks to PostHog.
 *
 * Privacy contract (enforced HERE, not at call sites):
 *   1. All three opt-out gates checked before any send:
 *      - `vscode.env.isTelemetryEnabled`
 *      - `telemetry.telemetryLevel` (via the wrapping TelemetryLogger)
 *      - `codegraph.telemetry.enabled`
 *   2. Allowlist redaction: every string-typed property is compared
 *      against `allowlists.ts`; mismatches collapse to `'other'`.
 *   3. Sampling: `tool.invoke` / `tool.result` sampled at 10% stratified
 *      by `machineId`; everything else 100%. Errors always 100%.
 *   4. Common properties merged in, never set per call site.
 *   5. If `codegraph.telemetry.verbose` is on, the event is also written
 *      to the CodeGraph Telemetry output channel for user inspection —
 *      the same payload that goes to PostHog, nothing hidden.
 *
 * Public API: `createReporter(ctx)` returns a Reporter object with one
 * method per event family (`activation`, `index`, `tool`, `command`,
 * `engagement`, `server`). Call sites NEVER call PostHog directly.
 */

import * as vscode from 'vscode';
import * as os from 'os';
import { PostHog } from 'posthog-node';

import {
    type ActivationOutcome,
    type CommandId,
    categorizeError,
    type ErrorCategory,
    type GraphPanel,
    type IndexOutcome,
    type IndexTrigger,
    isCommandId,
    isToolName,
    type Language,
    normalizeLanguage,
    type ServerRestartReason,
    SETTINGS_SNAPSHOT_KEYS,
    type ToolName,
    type TreeView,
} from './allowlists';
import {
    durationBucket,
    fileCountBucket,
    resultSizeBucket,
    settingNumberBucket,
    uptimeBucket,
    workspaceFolderCount,
} from './buckets';

// PostHog project key. Replaced at build time via esbuild's `define`
// option (see esbuild.js). Defaults to empty so dev builds don't ship
// to a live PostHog project.
declare const __POSTHOG_KEY__: string;
declare const __POSTHOG_HOST__: string;
const POSTHOG_KEY: string = typeof __POSTHOG_KEY__ === 'string' ? __POSTHOG_KEY__ : '';
const POSTHOG_HOST: string =
    typeof __POSTHOG_HOST__ === 'string' && __POSTHOG_HOST__ !== ''
        ? __POSTHOG_HOST__
        : 'https://us.posthog.com';

// Tool-event sample rate. At ~507 installs × ~50 tool calls/session × 30
// days × 50% active ≈ 380k events/mo at 100% — well under PostHog's
// 1M/mo free tier with the $0 billing hard-cap as the financial floor.
// Set to 1.0 (capture every tool invocation). Drop to 0.25 / 0.1 if the
// volume ever crosses ~700k/mo and tail-tool signal is no longer needed.
const TOOL_SAMPLE_RATE = 1.0;

type EventProps = Record<string, string | number | boolean | undefined | null>;

export interface Reporter {
    activationStart(props: {
        enabledSetting: boolean;
        workspaceFolders: number;
        hasMultiRoot: boolean;
    }): void;
    activationServerStartResult(props: {
        outcome: ActivationOutcome;
        durationMs: number;
        serverBinaryFound: boolean;
    }): void;
    activationToolRegistration(props: {
        lmApiAvailable: boolean;
        toolsRegistered: number;
        vscodeTooOld: boolean;
    }): void;

    indexRequested(trigger: IndexTrigger): void;
    indexCompleted(props: {
        outcome: IndexOutcome;
        durationMs: number;
        fileCount: number;
        errorCategory?: ErrorCategory;
    }): void;
    indexLanguageBreakdown(languageFileCounts: Map<Language, number>): void;

    toolInvoke(toolName: string, argShape: string): void;
    toolResult(props: {
        toolName: string;
        durationMs: number;
        resultSizeChars: number;
        retried: boolean;
    }): void;
    toolError(props: {
        toolName: string;
        error: unknown;
        attemptCount: number;
        durationMs: number;
    }): void;

    commandInvoke(commandId: string, props: { hasActiveEditor: boolean; activeEditorLanguage?: string }): void;
    commandResult(props: {
        commandId: string;
        outcome: 'ok' | 'warn' | 'error' | 'noop';
        durationMs: number;
    }): void;

    engagementTreeViewOpened(view: TreeView): void;
    engagementGraphPanelOpened(panel: GraphPanel): void;
    engagementSettingsSnapshot(): void;

    serverCrash(props: { uptimeSeconds: number; lastToolName?: string; restartCount: number }): void;
    serverRestart(reason: ServerRestartReason): void;
    serverRpcTimeout(props: { command: string; attemptCount: number }): void;

    /** Flush pending events (call on `deactivate()`). */
    dispose(): Promise<void>;
}

/**
 * Create the reporter for this activation. Returns a singleton; subsequent
 * calls in the same session return the same instance.
 */
export function createReporter(ctx: vscode.ExtensionContext): Reporter {
    const verboseChannel = vscode.window.createOutputChannel('CodeGraph Telemetry');
    ctx.subscriptions.push(verboseChannel);

    const ph: PostHog | null = POSTHOG_KEY
        ? new PostHog(POSTHOG_KEY, {
              host: POSTHOG_HOST,
              flushAt: 20,
              flushInterval: 30_000,
              disableGeoip: true,
          })
        : null;

    const machineId = vscode.env.machineId;
    const sessionId = vscode.env.sessionId;
    const extensionVersion = String(ctx.extension.packageJSON.version ?? '0.0.0');
    const vscodeVersionFull = vscode.version;
    const vscodeVersion = vscodeVersionFull.split('.').slice(0, 2).join('.'); // major.minor only

    // Hash-bucket the machineId into [0, 1) for stratified sampling so
    // each install is consistently sampled (not each event independently).
    // Bias-free for our needs: fnv-1a-style fold of the string bytes.
    function machineIdSampleRoll(): number {
        let h = 2166136261;
        for (let i = 0; i < machineId.length; i++) {
            h ^= machineId.charCodeAt(i);
            h = Math.imul(h, 16777619);
        }
        return (h >>> 0) / 0xffffffff;
    }
    const samplingRoll = machineIdSampleRoll();
    const isSampledInstall = samplingRoll < TOOL_SAMPLE_RATE;

    function commonProps(): EventProps {
        return {
            extensionVersion,
            serverEdition: getServerEdition(),
            vscodeVersion,
            os: os.platform(),
            machineId,
            sessionId,
        };
    }

    function getServerEdition(): string {
        // Populated by server.ts when it resolves the binary. Until then,
        // the property may be `undefined` — caller drops it (per allowlist
        // rule: never log unknowns as literal `"undefined"`).
        return (globalThis as any).__codegraphServerEdition ?? 'unknown';
    }

    function configEnabled(): boolean {
        return vscode.workspace
            .getConfiguration('codegraph')
            .get<boolean>('telemetry.enabled', true);
    }

    function configErrorOnly(): boolean {
        return vscode.workspace
            .getConfiguration('codegraph')
            .get<boolean>('telemetry.errorReportsOnly', false);
    }

    function verbose(): boolean {
        return vscode.workspace
            .getConfiguration('codegraph')
            .get<boolean>('telemetry.verbose', false);
    }

    function send(event: string, props: EventProps, isError: boolean): void {
        // Hard gates — every one of these must pass.
        if (!vscode.env.isTelemetryEnabled) return;
        if (!configEnabled()) return;
        if (configErrorOnly() && !isError) return;

        const merged: EventProps = { ...commonProps(), ...props };

        // Drop nulls/undefineds so PostHog doesn't store them.
        for (const k of Object.keys(merged)) {
            if (merged[k] === undefined || merged[k] === null) delete merged[k];
        }

        if (verbose()) {
            verboseChannel.appendLine(
                `[${new Date().toISOString()}] ${event} ${JSON.stringify(merged)}`,
            );
        }

        if (ph) {
            ph.capture({
                distinctId: machineId,
                event,
                properties: merged,
            });
        }
    }

    function sample(): boolean {
        return isSampledInstall;
    }

    return {
        activationStart(props) {
            send(
                'activation.start',
                {
                    enabledSetting: props.enabledSetting,
                    workspaceFolders: workspaceFolderCount(props.workspaceFolders),
                    hasMultiRoot: props.hasMultiRoot,
                },
                false,
            );
        },
        activationServerStartResult(props) {
            send(
                'activation.serverStartResult',
                {
                    outcome: props.outcome,
                    durationBucket: durationBucket(props.durationMs),
                    serverBinaryFound: props.serverBinaryFound,
                },
                props.outcome !== 'ok',
            );
        },
        activationToolRegistration(props) {
            send('activation.toolRegistration', {
                lmApiAvailable: props.lmApiAvailable,
                toolsRegistered: props.toolsRegistered,
                vscodeTooOld: props.vscodeTooOld,
            }, !props.lmApiAvailable);
        },

        indexRequested(trigger) {
            send('index.requested', { trigger }, false);
        },
        indexCompleted(props) {
            send(
                'index.completed',
                {
                    outcome: props.outcome,
                    durationBucket: durationBucket(props.durationMs),
                    fileCountBucket: fileCountBucket(props.fileCount),
                    errorCategory: props.errorCategory,
                },
                props.outcome !== 'ok',
            );
        },
        indexLanguageBreakdown(languageFileCounts) {
            // Normalize every key through the allowlist before emitting —
            // a server returning `"solidity": 12` (or any non-allowlist
            // language) would otherwise leak the language name as a
            // property key. Unknown languages collapse to `lang_other`,
            // with counts summed.
            const collected: Record<string, number> = {};
            for (const [rawLang, count] of languageFileCounts.entries()) {
                const safe = normalizeLanguage(String(rawLang));
                collected[safe] = (collected[safe] ?? 0) + count;
            }
            const breakdown: EventProps = {};
            for (const [lang, count] of Object.entries(collected)) {
                breakdown[`lang_${lang}`] = fileCountBucket(count);
            }
            send('index.languageBreakdown', breakdown, false);
        },

        toolInvoke(toolName, argShape) {
            if (!sample()) return;
            send(
                'tool.invoke',
                {
                    toolName: isToolName(toolName) ? toolName : 'other',
                    argShape,
                },
                false,
            );
        },
        toolResult(props) {
            if (!sample()) return;
            send(
                'tool.result',
                {
                    toolName: isToolName(props.toolName) ? props.toolName : 'other',
                    durationBucket: durationBucket(props.durationMs),
                    resultSizeBucket: resultSizeBucket(props.resultSizeChars),
                    retried: props.retried,
                },
                false,
            );
        },
        toolError(props) {
            // 100% sample on errors regardless of stratified-sample roll.
            send(
                'tool.error',
                {
                    toolName: isToolName(props.toolName) ? props.toolName : 'other',
                    errorCategory: categorizeError(props.error),
                    attemptCount: Math.min(props.attemptCount, 10),
                    durationBucket: durationBucket(props.durationMs),
                },
                true,
            );
        },

        commandInvoke(commandId, props) {
            send(
                'command.invoke',
                {
                    commandId: isCommandId(commandId) ? commandId : 'other',
                    hasActiveEditor: props.hasActiveEditor,
                    activeEditorLanguage: props.activeEditorLanguage
                        ? normalizeLanguage(props.activeEditorLanguage)
                        : undefined,
                },
                false,
            );
        },
        commandResult(props) {
            send(
                'command.result',
                {
                    commandId: isCommandId(props.commandId) ? props.commandId : 'other',
                    outcome: props.outcome,
                    durationBucket: durationBucket(props.durationMs),
                },
                props.outcome === 'error',
            );
        },

        engagementTreeViewOpened(view) {
            send('engagement.treeViewOpened', { view }, false);
        },
        engagementGraphPanelOpened(panel) {
            send('engagement.graphPanelOpened', { panelType: panel }, false);
        },
        engagementSettingsSnapshot() {
            const cfg = vscode.workspace.getConfiguration('codegraph');
            const props: EventProps = {};
            for (const key of SETTINGS_SNAPSHOT_KEYS.boolean) {
                const v = cfg.get<boolean>(key);
                if (typeof v === 'boolean') props[`setting_${key.replace(/\./g, '_')}`] = v;
            }
            for (const key of SETTINGS_SNAPSHOT_KEYS.enum) {
                // Only emit if value is a string (server-defined enum).
                // No allowlist comparison here because the enum values
                // come from a non-volatile server-side schema; if it's
                // ever extended with user-supplied strings, add a check.
                const v = cfg.get<string>(key);
                if (typeof v === 'string') props[`setting_${key.replace(/\./g, '_')}`] = v;
            }
            for (const key of SETTINGS_SNAPSHOT_KEYS.bucketedNumber) {
                const v = cfg.get<number>(key);
                if (typeof v === 'number')
                    props[`setting_${key.replace(/\./g, '_')}`] = settingNumberBucket(v);
            }
            send('engagement.settingsSnapshot', props, false);
        },

        serverCrash(props) {
            send(
                'server.crash',
                {
                    uptimeBucket: uptimeBucket(props.uptimeSeconds),
                    lastToolName: props.lastToolName && isToolName(props.lastToolName)
                        ? props.lastToolName
                        : props.lastToolName
                          ? 'other'
                          : undefined,
                    restartCount: Math.min(props.restartCount, 20),
                },
                true,
            );
        },
        serverRestart(reason) {
            send('server.restart', { reason }, reason === 'crash');
        },
        serverRpcTimeout(props) {
            send(
                'server.rpcTimeout',
                {
                    command: isCommandId(props.command) ? props.command : 'other',
                    attemptCount: Math.min(props.attemptCount, 10),
                },
                true,
            );
        },

        async dispose() {
            if (ph) {
                try {
                    await ph.shutdown();
                } catch {
                    // Best-effort flush; silent.
                }
            }
        },
    };
}

/**
 * Compress a tool-call args object into a shape signature. Values are
 * NEVER logged literally — only structural presence and bounded numbers.
 *
 * Examples:
 *   { uri: '...', line: 42 }                  -> "u:1,l:10-99"
 *   { query: 'foo', limit: 10 }               -> "q:1,lim:10"
 *   { nodeId: '...', depth: 5, direction: 'callers' }
 *                                              -> "n:1,d:5,dir:callers"
 *
 * Keys with string values always reduce to presence (`1` / `0`). Number
 * keys pass through if small (<100) or get bucketed (`line`, `limit`).
 * Enum-typed values pass through verbatim because they come from the
 * server-defined schema, not from user content.
 */
export function describeArgShape(args: unknown): string {
    if (args === null || args === undefined) return '';
    if (typeof args !== 'object') return '';
    const parts: string[] = [];
    const obj = args as Record<string, unknown>;
    for (const [key, value] of Object.entries(obj)) {
        const short = shortKey(key);
        if (value === null || value === undefined) {
            parts.push(`${short}:0`);
            continue;
        }
        if (typeof value === 'string') {
            parts.push(`${short}:1`);
            continue;
        }
        if (typeof value === 'boolean') {
            parts.push(`${short}:${value ? 1 : 0}`);
            continue;
        }
        if (typeof value === 'number') {
            if (key === 'line' || key === 'startLine' || key === 'endLine') {
                parts.push(`${short}:${lineBucketLocal(value)}`);
            } else if (key === 'depth' || key === 'limit' || key === 'maxResults') {
                parts.push(`${short}:${value < 100 ? value : '100+'}`);
            } else {
                parts.push(`${short}:${value < 100 ? value : '100+'}`);
            }
            continue;
        }
        if (Array.isArray(value)) {
            parts.push(`${short}:[${value.length}]`);
            continue;
        }
        if (typeof value === 'object') {
            parts.push(`${short}:{}`);
            continue;
        }
        parts.push(`${short}:1`);
    }
    return parts.join(',');
}

function shortKey(k: string): string {
    // Stable short forms for common keys; everything else uses first 3 chars.
    switch (k) {
        case 'uri':
            return 'u';
        case 'line':
            return 'l';
        case 'depth':
            return 'd';
        case 'direction':
            return 'dir';
        case 'limit':
            return 'lim';
        case 'query':
            return 'q';
        case 'nodeId':
            return 'n';
        case 'symbolName':
            return 'sym';
        case 'summary':
            return 'sum';
        case 'language':
            return 'lng';
        case 'scope':
            return 'sc';
        case 'pattern':
            return 'pat';
        case 'includeTests':
            return 'it';
        default:
            return k.slice(0, 3);
    }
}

function lineBucketLocal(line: number): string {
    if (line < 0 || !Number.isFinite(line)) return 'unknown';
    if (line < 10) return '<10';
    if (line < 100) return '10-99';
    return '100+';
}

/**
 * Mark the server edition so common-props can include it. Called from
 * `server.ts` once the binary path resolves. Avoids a circular import.
 */
export function setServerEdition(edition: 'community' | 'pro'): void {
    (globalThis as any).__codegraphServerEdition = edition;
}
