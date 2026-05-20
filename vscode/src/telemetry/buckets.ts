// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

/**
 * Coarse log-ish bucketing for numeric telemetry values.
 *
 * All numbers go through these before leaving the extension. The goal is
 * decision-grade signal — fine enough to compare orders of magnitude,
 * coarse enough that the bucket boundaries do not re-identify specific
 * workspaces (e.g. "the user with the 247,381-file monorepo").
 *
 * Pure functions, unit-testable. No side effects.
 */

/** Duration in milliseconds → string bucket. */
export function durationBucket(ms: number): string {
    if (ms < 0 || !Number.isFinite(ms)) return 'unknown';
    if (ms < 50) return '<50ms';
    if (ms < 100) return '50-100ms';
    if (ms < 250) return '100-250ms';
    if (ms < 500) return '250-500ms';
    if (ms < 1_000) return '500-1000ms';
    if (ms < 2_000) return '1-2s';
    if (ms < 5_000) return '2-5s';
    if (ms < 10_000) return '5-10s';
    if (ms < 30_000) return '10-30s';
    if (ms < 60_000) return '30-60s';
    return '>60s';
}

/** File count → string bucket. Used for indexed files, lines, etc. */
export function fileCountBucket(n: number): string {
    if (n < 0 || !Number.isFinite(n)) return 'unknown';
    if (n === 0) return '0';
    if (n < 10) return '1-9';
    if (n < 100) return '10-99';
    if (n < 500) return '100-499';
    if (n < 2_000) return '500-1999';
    if (n < 10_000) return '2000-9999';
    if (n < 50_000) return '10000-49999';
    return '>=50000';
}

/** Result payload size in chars → string bucket. */
export function resultSizeBucket(n: number): string {
    if (n < 0 || !Number.isFinite(n)) return 'unknown';
    if (n === 0) return '0';
    if (n <= 100) return '1-100';
    if (n <= 1_000) return '101-1k';
    if (n <= 10_000) return '1k-10k';
    if (n <= 50_000) return '10k-50k';
    return '>50k';
}

/** Uptime in seconds → string bucket. */
export function uptimeBucket(seconds: number): string {
    if (seconds < 0 || !Number.isFinite(seconds)) return 'unknown';
    if (seconds < 10) return '<10s';
    if (seconds < 60) return '10-60s';
    if (seconds < 5 * 60) return '1-5min';
    if (seconds < 30 * 60) return '5-30min';
    if (seconds < 2 * 60 * 60) return '30min-2h';
    if (seconds < 8 * 60 * 60) return '2-8h';
    return '>8h';
}

/** Bucket a line number (used inside `tool.argShape`). */
export function lineBucket(line: number): string {
    if (line < 0 || !Number.isFinite(line)) return 'unknown';
    if (line < 10) return '<10';
    if (line < 100) return '10-99';
    return '100+';
}

/**
 * Bucket workspace folder count. We cap at 5 because anything higher is
 * either a power user or noise; the exact count beyond 5 doesn't drive
 * any product decision worth tracking.
 */
export function workspaceFolderCount(n: number): number {
    if (n < 0 || !Number.isFinite(n)) return 0;
    return Math.min(n, 5);
}

/**
 * Bucket a numeric setting value (e.g. `maxFileSizeKB`) to a coarse band
 * for `engagement.settingsSnapshot`. We don't try to preserve the actual
 * value — we want to learn "is the default OK?", not "what's the median?".
 */
export function settingNumberBucket(n: number): string {
    if (!Number.isFinite(n) || n < 0) return 'unknown';
    if (n === 0) return '0';
    if (n <= 10) return '1-10';
    if (n <= 100) return '11-100';
    if (n <= 1_000) return '101-1k';
    if (n <= 10_000) return '1k-10k';
    if (n <= 100_000) return '10k-100k';
    return '>100k';
}
