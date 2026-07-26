// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.server

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The breaker is what stops a machine that cannot run the engine from
 * restarting it forever, so its edge cases are the ones that matter: crashes
 * spread over time must not trip it, and a tripped breaker must report the trip
 * exactly once.
 */
class RestartCircuitBreakerTest {

    private fun breaker() = RestartCircuitBreaker(maxCrashes = 3, windowMillis = 60_000)

    @Test
    fun `stays closed below the crash threshold`() {
        val breaker = breaker()

        assertFalse(breaker.recordCrash(0))
        assertFalse(breaker.recordCrash(1_000))

        assertFalse(breaker.isOpen)
    }

    @Test
    fun `opens on the third crash inside the window`() {
        val breaker = breaker()

        breaker.recordCrash(0)
        breaker.recordCrash(1_000)

        assertTrue("third rapid crash should trip the breaker", breaker.recordCrash(2_000))
        assertTrue(breaker.isOpen)
    }

    @Test
    fun `crashes spread beyond the window never accumulate`() {
        val breaker = breaker()

        // One crash every ten minutes is a flaky engine, not a crash loop, and
        // must not stop a user's session.
        repeat(20) { index ->
            assertFalse(
                "crash ${index + 1} should not trip the breaker",
                breaker.recordCrash(index * 600_000L),
            )
        }
        assertFalse(breaker.isOpen)
    }

    @Test
    fun `a crash at exactly the window edge does not count toward the trip`() {
        val breaker = breaker()

        breaker.recordCrash(0)
        breaker.recordCrash(30_000)
        // The first crash is now exactly 60s old, so it has aged out and only
        // two crashes remain inside the window.
        assertFalse(breaker.recordCrash(60_000))
        assertFalse(breaker.isOpen)
    }

    @Test
    fun `reports the trip only once so the user is warned once`() {
        val breaker = breaker()

        breaker.recordCrash(0)
        breaker.recordCrash(1)
        assertTrue(breaker.recordCrash(2))

        assertFalse("already-open breaker should not re-report", breaker.recordCrash(3))
        assertFalse(breaker.recordCrash(4))
    }

    @Test
    fun `reset closes the breaker and forgets history`() {
        val breaker = breaker()
        breaker.recordCrash(0)
        breaker.recordCrash(1)
        breaker.recordCrash(2)
        assertTrue(breaker.isOpen)

        breaker.reset()

        assertFalse(breaker.isOpen)
        // History is gone, so it takes a fresh run of three to trip again.
        assertFalse(breaker.recordCrash(3))
        assertFalse(breaker.recordCrash(4))
        assertTrue(breaker.recordCrash(5))
    }

    @Test
    fun `trip condition reads as a sentence for the notification`() {
        assertEquals("3 times in 60s", breaker().describeTripCondition())
    }
}
