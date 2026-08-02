// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.server

import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import java.nio.file.Files
import java.nio.file.Path

/**
 * The breadcrumb reader turns a crashed native process into a sentence a user
 * can act on. Its two failure modes are both silent: reading a stale file and
 * blaming the wrong thing, or leaving files behind so the next crash inherits
 * this one's diagnosis.
 */
class CrashBreadcrumbsTest {

    private lateinit var dir: Path
    private var now: Long = 1_000_000L

    /**
     * Which pids the reader should see as still running. Stubbed rather than
     * asked of the OS: real low pids (1 is init on every platform these tests
     * run on) would make the outcome depend on the machine.
     */
    private val livePids = mutableSetOf<Long>()

    @Before
    fun setUp() {
        dir = Files.createTempDirectory("codegraph-breadcrumbs-test")
    }

    @After
    fun tearDown() {
        dir.toFile().deleteRecursively()
    }

    private fun breadcrumbs() =
        CrashBreadcrumbs(directory = dir, clock = { now }, isProcessAlive = { it in livePids })

    private fun write(name: String, json: String, ageMillis: Long = 0) {
        val file = dir.resolve(name)
        Files.writeString(file, json)
        Files.setLastModifiedTime(file, java.nio.file.attribute.FileTime.fromMillis(now - ageMillis))
    }

    @Test
    fun `a panic breadcrumb yields its recorded class`() {
        write("last-crash.4242.json", """{"kind":"panic","class":"oom"}""")

        val diagnosis = breadcrumbs().readAndClear()

        assertEquals("oom", diagnosis.cause)
        assertTrue(diagnosis.describe().contains("out of memory"))
    }

    @Test
    fun `a signal breadcrumb is distinguished from a panic`() {
        write("last-crash.4242.json", """{"kind":"signal"}""")

        assertEquals(CrashDiagnosis.SIGNAL, breadcrumbs().readAndClear().cause)
    }

    @Test
    fun `no breadcrumb at all means the process died too hard to write one`() {
        val diagnosis = breadcrumbs().readAndClear()

        assertEquals(CrashDiagnosis.HARD_CRASH, diagnosis.cause)
        assertNull(diagnosis.phase)
    }

    @Test
    fun `a stale breadcrumb is ignored rather than blamed for this crash`() {
        // Written during a previous session; reporting "oom" here would send the
        // user chasing a memory problem that already happened days ago.
        write("last-crash.4242.json", """{"kind":"panic","class":"oom"}""", ageMillis = 60_000)

        assertEquals(CrashDiagnosis.HARD_CRASH, breadcrumbs().readAndClear().cause)
    }

    @Test
    fun `the newest breadcrumb wins when several processes crashed`() {
        write("last-crash.1.json", """{"kind":"panic","class":"rocksdb_lock"}""", ageMillis = 5_000)
        write("last-crash.2.json", """{"kind":"panic","class":"utf8_parse"}""", ageMillis = 100)

        assertEquals("utf8_parse", breadcrumbs().readAndClear().cause)
    }

    @Test
    fun `the phase marker says where the engine was when it died`() {
        write("last-crash.4242.json", """{"kind":"signal"}""")
        write("last-phase.4242.json", """{"phase":"onnx_load"}""")

        val diagnosis = breadcrumbs().readAndClear()

        assertEquals("onnx_load", diagnosis.phase)
        assertTrue(diagnosis.describe().contains("during onnx_load"))
    }

    @Test
    fun `every breadcrumb is deleted so the next crash starts clean`() {
        write("last-crash.1.json", """{"kind":"panic","class":"oom"}""")
        write("last-phase.1.json", """{"phase":"startup"}""")
        write("unrelated.json", "{}")

        breadcrumbs().readAndClear()

        assertTrue(Files.notExists(dir.resolve("last-crash.1.json")))
        assertTrue(Files.notExists(dir.resolve("last-phase.1.json")))
        assertTrue("unrelated files must be left alone", Files.exists(dir.resolve("unrelated.json")))
    }

    @Test
    fun `malformed json degrades to hard crash instead of throwing`() {
        write("last-crash.4242.json", "{ this is not json")

        assertEquals(CrashDiagnosis.HARD_CRASH, breadcrumbs().readAndClear().cause)
    }

    @Test
    fun `a missing codegraph directory is a normal first run`() {
        val missing = dir.resolve("does-not-exist")

        val diagnosis = CrashBreadcrumbs(directory = missing, clock = { now }).readAndClear()

        assertEquals(CrashDiagnosis.HARD_CRASH, diagnosis.cause)
    }

    @Test
    fun `a running engine's breadcrumbs are neither read nor deleted`() {
        // `~/.codegraph` is shared with every other engine on the machine. Its
        // phase marker is live state, not a post-mortem, and deleting it throws
        // away the attribution for a crash that has not happened yet.
        livePids += 77L
        write("last-crash.77.json", """{"kind":"panic","class":"oom"}""")
        write("last-phase.77.json", """{"phase":"indexing"}""")

        val diagnosis = breadcrumbs().readAndClear()

        assertEquals(CrashDiagnosis.HARD_CRASH, diagnosis.cause)
        assertNull(diagnosis.phase)
        assertTrue(Files.exists(dir.resolve("last-crash.77.json")))
        assertTrue(Files.exists(dir.resolve("last-phase.77.json")))
    }

    @Test
    fun `the dead engine is diagnosed even when a newer marker belongs to a live one`() {
        livePids += 9L
        write("last-crash.2.json", """{"kind":"panic","class":"oom"}""", ageMillis = 200)
        write("last-phase.2.json", """{"phase":"onnx_load"}""", ageMillis = 200)
        write("last-phase.9.json", """{"phase":"indexing"}""", ageMillis = 0)

        val diagnosis = breadcrumbs().readAndClear()

        assertEquals("oom", diagnosis.cause)
        assertEquals("onnx_load", diagnosis.phase)
        assertTrue(Files.notExists(dir.resolve("last-crash.2.json")))
        assertTrue(Files.exists(dir.resolve("last-phase.9.json")))
    }

    @Test
    fun `a breadcrumb with no readable pid is still cleaned up`() {
        write("last-crash.abc.json", """{"kind":"signal"}""")
        write("last-phase.abc.json", """{"phase":"startup"}""")

        breadcrumbs().readAndClear()

        assertTrue(Files.notExists(dir.resolve("last-crash.abc.json")))
        assertTrue(Files.notExists(dir.resolve("last-phase.abc.json")))
    }
}
