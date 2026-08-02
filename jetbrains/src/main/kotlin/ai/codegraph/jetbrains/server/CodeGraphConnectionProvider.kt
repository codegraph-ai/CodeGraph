// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.server

import ai.codegraph.jetbrains.settings.CodeGraphSettings
import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.openapi.diagnostic.logger
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.redhat.devtools.lsp4ij.server.CannotStartProcessException
import com.redhat.devtools.lsp4ij.server.OSProcessStreamConnectionProvider
import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.Paths

/**
 * Spawns and configures the `codegraph-server` engine process.
 *
 * The engine speaks LSP over stdio; LSP4IJ owns the JSON-RPC framing and
 * document synchronisation, so all this type does is build the command line and
 * hand over the `initialize` options.
 */
class CodeGraphConnectionProvider(private val project: Project) : OSProcessStreamConnectionProvider() {

    /** Set on a successful resolve so the status bar and telemetry can read it. */
    @Volatile
    var resolved: ResolvedServer? = null
        private set

    override fun start() {
        val settings = CodeGraphSettings.getInstance(project).state
        val server = CodeGraphServerResolver.resolve(project.basePath, settings.serverPath)
            ?: throw CannotStartProcessException(
                "CodeGraph engine not found. Install it with `npm i -g @astudioplus/codegraph-mcp`, " +
                    "or set the engine path in Settings | Tools | CodeGraph.",
            )
        resolved = server

        val commandLine = GeneralCommandLine(server.path.toString()).apply {
            // No shell, so a path containing spaces is passed as a single argv
            // entry. This is the class of bug that broke the VS Code client on
            // Windows (issue #2); do not reintroduce a shell here.
            withWorkDirectory(project.basePath)
            withCharset(Charsets.UTF_8)
            if (settings.embeddingModel == "static") {
                withEnvironment("CODEGRAPH_STATIC_MODEL", staticModelDir(settings.staticModelPath).toString())
            }
        }
        setCommandLine(commandLine)

        val lifecycle = EngineLifecycle.getInstance(project)
        lifecycle.publishResolvedServer(server)
        // Registered before the process exists so a death during startup - the
        // most common failure on a machine with antivirus or a missing runtime
        // library - is still counted rather than silently retried.
        addUnexpectedServerStopHandler { lifecycle.onUnexpectedStop() }

        LOG.info("Starting CodeGraph engine: ${server.path} (${server.edition}, via ${server.origin})")
        super.start()
        lifecycle.onEngineStarted()
    }

    /**
     * `initialize` options, matching the shape the engine parses in
     * `backend.rs::initialize`.
     *
     * `extensionPath` is a VS Code-era name for "the directory the client owns
     * for its resources". The engine currently only uses it as the gate that
     * enables `embeddingModel` and `fullBodyEmbedding`, so it must be present or
     * full-body embeddings silently turn off. We pass a stable per-client
     * directory; see the follow-up to un-gate those settings server-side.
     */
    override fun getInitializationOptions(rootUri: VirtualFile?): Any {
        val settings = CodeGraphSettings.getInstance(project).state
        return mapOf(
            "extensionPath" to clientResourceDir().toString(),
            "indexOnStartup" to settings.indexOnStartup,
            "excludePatterns" to settings.excludePatterns.toList(),
            "indexPaths" to settings.indexPaths.toList(),
            "maxFileSizeKB" to settings.maxFileSizeKB,
            "embeddingModel" to settings.embeddingModel,
            "staticModelPath" to settings.staticModelPath.ifBlank { null },
            "fullBodyEmbedding" to settings.fullBodyEmbedding,
            "embedOnOpen" to settings.embedOnOpen,
        )
    }

    override fun getTrace(rootUri: VirtualFile?): String =
        if (CodeGraphSettings.getInstance(project).state.debug) "verbose" else "off"

    private fun clientResourceDir(): Path {
        val dir = Paths.get(System.getProperty("user.home"), ".codegraph", "jetbrains")
        runCatching { Files.createDirectories(dir) }
            .onFailure { LOG.warn("Could not create client resource dir $dir", it) }
        return dir
    }

    private fun staticModelDir(override: String): Path =
        if (override.isNotBlank()) {
            Paths.get(override)
        } else {
            Paths.get(System.getProperty("user.home"), ".codegraph", "static_models", "jina-code-static-256")
        }

    private companion object {
        val LOG = logger<CodeGraphConnectionProvider>()
    }
}
