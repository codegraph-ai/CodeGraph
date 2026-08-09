// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.server

/**
 * Stops the engine being restarted forever on a machine where it cannot run.
 *
 * Without this, a host with antivirus interference, a missing runtime library
 * or too little memory produces an endless crash-restart loop. In the VS Code
 * client this showed up as single machines generating 50+ crash events a week,
 * which is both useless to the user and noise in the data.
 *
 * After [maxCrashes] crashes inside [windowMillis] the breaker opens and stays
 * open until [reset] is called, which is what the "Retry" button does.
 */
class RestartCircuitBreaker(
    private val maxCrashes: Int = DEFAULT_MAX_CRASHES,
    private val windowMillis: Long = DEFAULT_WINDOW_MILLIS,
) {
    private val crashTimestamps = ArrayDeque<Long>()

    var isOpen: Boolean = false
        private set

    /**
     * Record a crash at [now].
     *
     * @return true if this crash opened the breaker, meaning the caller should
     *   stop the engine and tell the user rather than restarting again. Returns
     *   false on subsequent crashes while already open, so the user is warned
     *   once rather than repeatedly.
     */
    @Synchronized
    fun recordCrash(now: Long): Boolean {
        if (isOpen) return false

        crashTimestamps.addLast(now)
        while (crashTimestamps.isNotEmpty() && now - crashTimestamps.first() >= windowMillis) {
            crashTimestamps.removeFirst()
        }

        if (crashTimestamps.size >= maxCrashes) {
            isOpen = true
            return true
        }
        return false
    }

    /** Close the breaker and forget the crash history. */
    @Synchronized
    fun reset() {
        crashTimestamps.clear()
        isOpen = false
    }

    /** Human-readable summary of the trip condition, for the error message. */
    fun describeTripCondition(): String =
        "$maxCrashes times in ${windowMillis / 1000}s"

    companion object {
        const val DEFAULT_MAX_CRASHES = 3
        const val DEFAULT_WINDOW_MILLIS = 60_000L
    }
}
