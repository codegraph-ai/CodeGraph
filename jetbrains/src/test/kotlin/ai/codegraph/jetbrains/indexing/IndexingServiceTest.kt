// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.indexing

import com.google.gson.JsonParser
import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * The engine answers `reindexWorkspace` with snake_case keys while its query
 * responses are camelCase. Reading the wrong one does not fail loudly: it
 * reports zero files and tells the user indexing found nothing, on a workspace
 * that indexed perfectly.
 */
class IndexingServiceTest {

    private fun filesIndexed(json: String) =
        IndexingService.filesIndexed(JsonParser.parseString(json))

    @Test
    fun `reads the engine's snake_case file count`() {
        val response = """
            {
              "status": "success",
              "message": "Workspace reindexed: 1432 files",
              "files_indexed": 1432,
              "files_parsed": 1400,
              "files_skipped": 32,
              "duration_ms": 8123,
              "by_language": {"rust": 900, "python": 532}
            }
        """.trimIndent()

        assertEquals(1432, filesIndexed(response))
    }

    @Test
    fun `a genuinely empty index reports zero`() {
        assertEquals(0, filesIndexed("""{"status":"success","files_indexed":0}"""))
    }

    @Test
    fun `a camelCase spelling is not silently accepted`() {
        // If the engine ever renames the key, this must read as zero so the
        // mismatch surfaces, rather than being papered over by guessing at
        // alternative spellings.
        assertEquals(0, filesIndexed("""{"filesIndexed":1432}"""))
    }

    @Test
    fun `a non-numeric value does not throw`() {
        assertEquals(0, filesIndexed("""{"files_indexed":"lots"}"""))
    }

    @Test
    fun `a null or non-object response reports zero`() {
        assertEquals(0, IndexingService.filesIndexed(null))
        assertEquals(0, filesIndexed("[]"))
        assertEquals(0, filesIndexed("\"done\""))
    }
}
