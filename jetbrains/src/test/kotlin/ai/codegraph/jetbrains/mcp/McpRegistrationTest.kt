// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.mcp

import ai.codegraph.jetbrains.settings.CodeGraphSettings
import com.google.gson.JsonParser
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import java.nio.file.Files
import java.nio.file.Path

/**
 * Registration writes into a file the user may already own.
 *
 * The failure that matters is not "the entry is missing" - that is visible
 * immediately - but "the other entries are gone", which is silent, destroys
 * configuration the plugin did not create, and is only noticed later when some
 * unrelated AI tool stops working.
 */
class McpRegistrationTest : BasePlatformTestCase() {

    private lateinit var projectDir: Path
    private lateinit var engine: Path

    override fun setUp() {
        super.setUp()
        projectDir = Files.createTempDirectory("codegraph-mcp-test")
        // The test fixture's basePath is a temp path that is never materialised,
        // so create it before anything tries to write a file there.
        Files.createDirectories(Path.of(project.basePath!!))
        engine = projectDir.resolve("target/release/codegraph-server")
        Files.createDirectories(engine.parent)
        Files.createFile(engine)
        engine.toFile().setExecutable(true)

        // Point resolution at the fake engine explicitly; the resolver would
        // otherwise find whatever this machine happens to have installed.
        CodeGraphSettings.getInstance(project).state.serverPath = engine.toString()
    }

    override fun tearDown() {
        try {
            CodeGraphSettings.getInstance(project).state.serverPath = ""
            projectDir.toFile().deleteRecursively()
        } finally {
            super.tearDown()
        }
    }

    private fun configFile(): Path = Path.of(project.basePath!!, McpRegistration.CONFIG_FILE)

    private fun writeConfig(json: String) {
        Files.writeString(configFile(), json)
    }

    private fun readServers() =
        JsonParser.parseString(Files.readString(configFile())).asJsonObject.getAsJsonObject("mcpServers")

    fun `test writes a codegraph entry into a fresh project`() {
        val result = McpRegistration.register(project)

        assertTrue("expected a written result, got $result", result is McpRegistration.Result.Written)
        val servers = readServers()
        assertTrue(servers.has(McpRegistration.SERVER_NAME))
        val args = servers.getAsJsonObject(McpRegistration.SERVER_NAME).getAsJsonArray("args").map { it.asString }
        assertTrue("--mcp must be passed or the engine starts in LSP mode", args.contains("--mcp"))
    }

    fun `test preserves MCP servers the project already had`() {
        writeConfig(
            """
            {"mcpServers":{"stellarion":{"command":"/usr/local/bin/stellarion-server","args":["--mcp"]}}}
            """.trimIndent(),
        )

        McpRegistration.register(project)

        val servers = readServers()
        assertTrue("the pre-existing server must survive", servers.has("stellarion"))
        assertTrue(servers.has(McpRegistration.SERVER_NAME))
        assertEquals(
            "/usr/local/bin/stellarion-server",
            servers.getAsJsonObject("stellarion").get("command").asString,
        )
    }

    fun `test keeps unrelated top-level keys`() {
        writeConfig("""{"someOtherTool":{"enabled":true},"mcpServers":{}}""")

        McpRegistration.register(project)

        val root = JsonParser.parseString(Files.readString(configFile())).asJsonObject
        assertTrue("unrelated configuration must not be dropped", root.has("someOtherTool"))
    }

    fun `test re-registering updates in place rather than duplicating`() {
        McpRegistration.register(project)
        McpRegistration.register(project)

        val servers = readServers()
        assertEquals(1, servers.keySet().size)
        assertTrue(McpRegistration.isRegistered(project))
    }

    fun `test malformed existing config does not block registration`() {
        // Refusing to write because the file is broken would leave the user
        // stuck with no way forward from inside the IDE.
        writeConfig("{ this is not json")

        val result = McpRegistration.register(project)

        assertTrue(result is McpRegistration.Result.Written)
        assertTrue(readServers().has(McpRegistration.SERVER_NAME))
    }

    fun `test isRegistered is false before registering`() {
        assertFalse(McpRegistration.isRegistered(project))
    }

    fun `test reports a missing engine instead of writing a broken config`() {
        CodeGraphSettings.getInstance(project).state.serverPath = projectDir.resolve("nope").toString()
        Files.deleteIfExists(engine)

        val result = McpRegistration.register(project)

        // Resolution may still find a real engine on a developer machine; the
        // point is that it never writes an entry with no command.
        if (result is McpRegistration.Result.Written) {
            val command = readServers().getAsJsonObject(McpRegistration.SERVER_NAME).get("command").asString
            assertTrue("a written entry must name a real engine", command.isNotBlank())
        } else {
            assertTrue(result is McpRegistration.Result.NoEngine)
        }
    }
}
