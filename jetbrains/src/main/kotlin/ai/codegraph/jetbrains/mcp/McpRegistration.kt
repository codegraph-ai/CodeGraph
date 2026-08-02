// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.mcp

import ai.codegraph.jetbrains.server.CodeGraphServerResolver
import ai.codegraph.jetbrains.settings.CodeGraphSettings
import com.google.gson.GsonBuilder
import com.google.gson.JsonObject
import com.google.gson.JsonParser
import com.intellij.openapi.project.Project
import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.Paths
import java.nio.file.StandardCopyOption

/**
 * Registers the CodeGraph engine as an MCP server for the IDE's AI tooling.
 *
 * The VS Code client exposes 28 `languageModelTools`, which are Copilot-specific
 * and have no JetBrains equivalent. Rather than reimplement that surface, this
 * points the AI tooling at the engine's own MCP mode - so the tool list stays
 * correct as the engine gains tools, instead of drifting in a second hand-written
 * declaration.
 *
 * The target is `<project>/.mcp.json`, the `mcpServers` shape that Junie, Claude
 * Code, Cursor and the AI Assistant MCP settings all read.
 */
object McpRegistration {

    const val SERVER_NAME = "codegraph"
    const val CONFIG_FILE = ".mcp.json"

    private val gson = GsonBuilder().setPrettyPrinting().create()

    /** Suffix for the copy taken before a file we could not parse is replaced. */
    const val BACKUP_SUFFIX = ".codegraph-backup"

    sealed interface Result {
        data class Written(val path: Path, val merged: Boolean, val backup: Path? = null) : Result
        data class NoEngine(val reason: String) : Result
        data class Failed(val reason: String) : Result
    }

    /** The config that would be written, for previewing or copying. */
    fun configSnippet(project: Project): String? =
        serverEntry(project)?.let { entry ->
            gson.toJson(JsonObject().apply { add("mcpServers", JsonObject().apply { add(SERVER_NAME, entry) }) })
        }

    /**
     * Write or update the `codegraph` entry in the project's `.mcp.json`.
     *
     * Existing entries are preserved: a project may already point at other MCP
     * servers, and clobbering someone's config to add ourselves would be a
     * hostile way to install a feature.
     */
    fun register(project: Project): Result {
        val entry = serverEntry(project)
            ?: return Result.NoEngine(
                "No CodeGraph engine found. Install it, or set its path in Settings | Tools | CodeGraph.",
            )
        val basePath = project.basePath
            ?: return Result.Failed("This project has no directory on disk.")

        val configPath = Paths.get(basePath, CONFIG_FILE)
        return try {
            val parsed = readConfig(configPath)
            // A file we could not parse still holds the user's other MCP
            // servers. Writing over it loses every one of them, so the
            // unreadable original is kept before it is replaced.
            val backup = if (parsed == null) backUp(configPath) else null
            val existing = parsed ?: JsonObject()

            val servers = existing.getAsJsonObject("mcpServers")
                ?: JsonObject().also { existing.add("mcpServers", it) }
            val merged = existing.has("mcpServers") && servers.size() > 0 && !servers.has(SERVER_NAME)

            servers.add(SERVER_NAME, entry)
            Files.writeString(configPath, gson.toJson(existing) + "\n")
            Result.Written(configPath, merged, backup)
        } catch (error: Exception) {
            // The message alone is often just the path, which reads as though
            // nothing went wrong; the exception type carries the actual reason.
            Result.Failed("${error::class.java.simpleName}: ${error.message.orEmpty()}".trim(':', ' '))
        }
    }

    /** True when the project already points at this engine. */
    fun isRegistered(project: Project): Boolean {
        val basePath = project.basePath ?: return false
        return runCatching {
            readConfig(Paths.get(basePath, CONFIG_FILE))
                ?.getAsJsonObject("mcpServers")
                ?.has(SERVER_NAME) == true
        }.getOrDefault(false)
    }

    /**
     * The existing config, an empty object when there is no file yet, or null
     * when there is a file we cannot parse.
     *
     * The three are deliberately distinct. Refusing to write because the
     * existing JSON is broken would leave the user stuck with no way forward
     * from inside the IDE, but treating "broken" as "absent" silently discards
     * every other MCP server they had configured - a trailing comma is enough.
     * Telling them apart lets the caller keep a copy before it replaces one.
     */
    private fun readConfig(path: Path): JsonObject? {
        if (!Files.exists(path)) return JsonObject()
        return runCatching {
            JsonParser.parseString(Files.readString(path)).asJsonObject
        }.getOrNull()
    }

    /**
     * Copy the unparseable config aside, returning where it went.
     *
     * A failure here is not fatal to the registration, but it does mean there
     * is no copy: null says so rather than implying one exists.
     */
    private fun backUp(path: Path): Path? =
        runCatching {
            val backup = path.resolveSibling(path.fileName.toString() + BACKUP_SUFFIX)
            Files.copy(path, backup, StandardCopyOption.REPLACE_EXISTING)
            backup
        }.getOrNull()

    private fun serverEntry(project: Project): JsonObject? {
        val settings = CodeGraphSettings.getInstance(project).state
        val server = CodeGraphServerResolver.resolve(project.basePath, settings.serverPath) ?: return null

        return JsonObject().apply {
            addProperty("command", server.path.toString())
            add(
                "args",
                gson.toJsonTree(
                    buildList {
                        add("--mcp")
                        project.basePath?.let {
                            add("--workspace")
                            add(it)
                        }
                        // Pass the model through so an agent session embeds the
                        // same way the editor does; otherwise the two disagree
                        // about what "similar" means.
                        add("--embedding-model")
                        add(settings.embeddingModel)
                        if (settings.fullBodyEmbedding) add("--full-body-embedding")
                    },
                ),
            )
        }
    }
}
