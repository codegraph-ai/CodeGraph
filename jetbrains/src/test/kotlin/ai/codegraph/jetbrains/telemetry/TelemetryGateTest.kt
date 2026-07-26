// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.telemetry

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The gate decides whether data leaves someone's machine.
 *
 * A bug here is not a broken feature - it is measuring a user who declined,
 * which nothing downstream can detect or undo. Every refusal path is asserted
 * individually rather than trusting one happy-path test.
 */
class TelemetryGateTest {

    private fun allows(
        hasKey: Boolean = true,
        pluginEnabled: Boolean = true,
        errorReportsOnly: Boolean = false,
        isErrorEvent: Boolean = false,
    ) = TelemetryGate.allows(hasKey, pluginEnabled, errorReportsOnly, isErrorEvent)

    @Test
    fun `sends when every gate is open`() {
        assertTrue(allows())
    }

    @Test
    fun `a build with no compiled-in key never sends`() {
        // Builds from source and forks must be silent without anyone having to
        // remember a setting.
        assertFalse(allows(hasKey = false))
        assertFalse(allows(hasKey = false, isErrorEvent = true))
        assertFalse(allows(hasKey = false, pluginEnabled = true))
    }

    @Test
    fun `the plugin switch alone is enough to stop everything`() {
        assertFalse(allows(pluginEnabled = false))
        assertFalse(allows(pluginEnabled = false, isErrorEvent = true))
    }

    @Test
    fun `error-reports-only drops ordinary events but keeps failures`() {
        assertFalse(allows(errorReportsOnly = true, isErrorEvent = false))
        assertTrue(allows(errorReportsOnly = true, isErrorEvent = true))
    }

    @Test
    fun `error events still respect every other refusal`() {
        // An error is not a licence to ignore consent.
        assertFalse(allows(isErrorEvent = true, hasKey = false))
        assertFalse(allows(isErrorEvent = true, pluginEnabled = false))
    }

    @Test
    fun `unknown values are dropped rather than sent as placeholder strings`() {
        val cleaned = TelemetryGate.clean(
            mapOf(
                "ide" to "jetbrains",
                "serverEdition" to null,
                "pluginVersion" to "",
                "fileCount" to 0,
                "ok" to false,
            ),
        )

        // A literal "unknown" or an empty string looks like a real value in a
        // dashboard and silently inflates whatever bucket it lands in.
        assertEquals(setOf("ide", "fileCount", "ok"), cleaned.keys)
        assertEquals(0, cleaned["fileCount"])
        assertEquals(false, cleaned["ok"])
    }

    @Test
    fun `cleaning an empty map is empty rather than null`() {
        assertTrue(TelemetryGate.clean(emptyMap()).isEmpty())
    }
}
