// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.server

import com.google.gson.JsonParser
import com.intellij.openapi.diagnostic.logger
import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.Paths

/**
 * Why the engine died, as far as we can tell.
 *
 * [cause] is an enum-like token, never free text from the crash: the engine's
 * panic hook deliberately writes a classification rather than a message so that
 * nothing user-specific leaves the machine.
 */
data class CrashDiagnosis(
    val cause: String,
    val phase: String? = null,
) {
    /** A sentence fit for a notification, not a stack trace. */
    fun describe(): String = when (cause) {
        HARD_CRASH -> "the engine died without running its panic handler, which usually means a segfault, " +
            "an out-of-memory kill, or antivirus terminating it"
        SIGNAL -> "the engine was killed by a signal"
        "oom" -> "the engine ran out of memory"
        "rocksdb_lock" -> "the engine's database was locked by another CodeGraph process"
        "mutex_poison" -> "the engine hit an internal lock poisoning error"
        "utf8_parse" -> "the engine hit a text-encoding error while parsing"
        else -> "the engine stopped unexpectedly ($cause)"
    } + (phase?.let { ", during $it" } ?: "")

    companion object {
        const val HARD_CRASH = "hard_crash"
        const val SIGNAL = "signal"
    }
}

/**
 * Reads the crash breadcrumbs the engine drops in `~/.codegraph`.
 *
 * The engine's panic hook writes `last-crash.<pid>.json` with a classification,
 * and marks the phase it was in via `last-phase.<pid>.json`. Absence of a fresh
 * crash file is itself information: it means the process died in a way that
 * could not run the hook at all.
 *
 * Best effort throughout. A diagnosis is a nicety; failing to read one must
 * never turn into a second error on top of the crash.
 */
class CrashBreadcrumbs(
    private val directory: Path = Paths.get(System.getProperty("user.home").orEmpty(), ".codegraph"),
    private val clock: () -> Long = System::currentTimeMillis,
    private val isProcessAlive: (Long) -> Boolean = { pid -> ProcessHandle.of(pid).isPresent },
) {

    /**
     * Classify the most recent crash and delete the breadcrumbs it left, so a
     * stale file can never be read as a diagnosis of some later crash.
     *
     * `~/.codegraph` is shared by every engine on the machine - a second IDE
     * project, an open VS Code window - and only the ones whose process is gone
     * describe a crash. Reading a live engine's marker would attribute its
     * phase to a death that never happened, and deleting it would destroy the
     * only evidence available if it later dies hard, which is the attribution
     * the engine's own sweeper takes care to preserve.
     */
    fun readAndClear(): CrashDiagnosis {
        val all = runCatching { Files.list(directory).use { it.toList() } }.getOrNull()
            ?: return CrashDiagnosis(CrashDiagnosis.HARD_CRASH)

        val files = all.filter { isBreadcrumb(it) && !belongsToLiveProcess(it) }

        val cause = pickFresh(files, CRASH_PATTERN)?.let { crumb ->
            when {
                crumb["kind"] == "signal" -> CrashDiagnosis.SIGNAL
                crumb["kind"] == "panic" -> crumb["class"]
                else -> null
            }
        } ?: CrashDiagnosis.HARD_CRASH

        val phase = pickFresh(files, PHASE_PATTERN)?.get("phase")

        files.forEach { runCatching { Files.deleteIfExists(it) } }

        return CrashDiagnosis(cause, phase)
    }

    private fun isBreadcrumb(path: Path): Boolean {
        val name = path.fileName.toString()
        return CRASH_PATTERN.matches(name) || PHASE_PATTERN.matches(name)
    }

    /**
     * A breadcrumb is named `last-<kind>.<pid>.json`. An unparseable pid is
     * treated as dead: it cannot belong to a process we could be harming, and
     * leaving it forever would let it outlive every crash it might describe.
     */
    private fun belongsToLiveProcess(path: Path): Boolean {
        val pid = path.fileName.toString().split('.').getOrNull(1)?.toLongOrNull() ?: return false
        return runCatching { isProcessAlive(pid) }.getOrDefault(false)
    }

    /**
     * Newest file matching [pattern], parsed to a flat string map - but only if
     * it was written recently enough to belong to the crash we are diagnosing.
     * Without the freshness window a breadcrumb from a previous session would
     * mislabel today's crash.
     */
    private fun pickFresh(files: List<Path>, pattern: Regex): Map<String, String>? {
        val newest = files
            .filter { pattern.matches(it.fileName.toString()) }
            .mapNotNull { path -> runCatching { path to Files.getLastModifiedTime(path).toMillis() }.getOrNull() }
            .maxByOrNull { it.second }
            ?: return null

        if (clock() - newest.second > FRESHNESS_WINDOW_MS) return null

        return runCatching {
            JsonParser.parseString(Files.readString(newest.first)).asJsonObject
                .entrySet()
                .mapNotNull { (key, value) ->
                    val primitive = value.takeIf { it.isJsonPrimitive } ?: return@mapNotNull null
                    key to primitive.asString
                }
                .toMap()
        }.onFailure { LOG.debug("Unreadable CodeGraph crash breadcrumb", it) }.getOrNull()
    }

    private companion object {
        val LOG = logger<CrashBreadcrumbs>()
        val CRASH_PATTERN = Regex("""^last-crash\..*\.json$""")
        val PHASE_PATTERN = Regex("""^last-phase\..*\.json$""")

        /** How recent a breadcrumb must be to describe the crash at hand. */
        const val FRESHNESS_WINDOW_MS = 15_000L
    }
}
