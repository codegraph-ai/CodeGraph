// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.diagnostics

import ai.codegraph.jetbrains.graph.GraphKind
import ai.codegraph.jetbrains.graph.GraphPanel
import ai.codegraph.jetbrains.lsp.CodeGraphClient
import ai.codegraph.jetbrains.mcp.McpRegistration
import ai.codegraph.jetbrains.lsp.CodeGraphCommand
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.application.EDT
import com.intellij.openapi.util.Disposer
import com.intellij.openapi.wm.ToolWindowManager
import com.intellij.ui.jcef.JBCefApp
import com.intellij.openapi.diagnostic.logger
import com.intellij.openapi.project.Project
import com.intellij.openapi.startup.ProjectActivity
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.util.concurrent.TimeUnit

/**
 * Opt-in smoke test that runs the plugin's own transport end to end and writes
 * the verdict to the IDE log.
 *
 * A sandbox IDE otherwise needs a human to click a menu item before anything is
 * exercised, which makes the one integration that matters - LSP4IJ actually
 * carrying a CodeGraph `executeCommand` to a live engine - the only part never
 * checked automatically. `scripts/engine_probe.py` covers the engine side of
 * that contract; this covers the IDE side.
 *
 * Inert unless `-Dcodegraph.selfcheck=true` is set, so it costs users nothing:
 *
 *     ./gradlew runIde -PsandboxProject=/some/project \
 *         -PrunIdeSystemProperty=codegraph.selfcheck=true
 */
class SelfCheckActivity : ProjectActivity {

    override suspend fun execute(project: Project) {
        if (System.getProperty(PROPERTY) != "true") return
        if (ApplicationManager.getApplication().isUnitTestMode) return

        val client = CodeGraphClient.getInstance(project)
        LOG.warn("$TAG starting, engine status: ${client.status()}")
        client.start()

        runCheck("getParserMetrics") {
            client.execute(CodeGraphCommand.GET_PARSER_METRICS)
                .get(STARTUP_TIMEOUT_SECONDS, TimeUnit.SECONDS)
        }
        runCheck("symbolSearch") {
            client.execute(CodeGraphCommand.SYMBOL_SEARCH, mapOf("query" to "helper", "limit" to 5))
                .get(COMMAND_TIMEOUT_SECONDS, TimeUnit.SECONDS)
        }

        // The queries behind the visible surfaces. Checking them separately
        // distinguishes "the engine has no answer" from "the UI dropped it".
        runCheck("getWorkspaceSymbols") {
            // No query key, exactly as the tool window sends it: an empty string
            // would take the engine's modules-only branch and check nothing.
            client.execute(CodeGraphCommand.GET_WORKSPACE_SYMBOLS, emptyMap<String, Any>())
                .get(COMMAND_TIMEOUT_SECONDS, TimeUnit.SECONDS)
        }
        firstSourceFileUri(project)?.let { uri ->
            runCheck("getDocumentCodeLens") {
                client.execute(CodeGraphCommand.GET_DOCUMENT_CODE_LENS, mapOf("uri" to uri))
                    .get(COMMAND_TIMEOUT_SECONDS, TimeUnit.SECONDS)
            }
        }

        runCheck("memoryList") {
            client.execute(
                CodeGraphCommand.MEMORY_LIST,
                mapOf("currentOnly" to true, "limit" to 5),
            ).get(COMMAND_TIMEOUT_SECONDS, TimeUnit.SECONDS)
        }

        // Writes into the open project, which is only acceptable because this
        // whole activity is opt-in and runs against a sandbox project.
        runCheck("mcpRegistration") { McpRegistration.register(project) }

        // JCEF availability is a property of the running JBR, not of the build,
        // so it can only be answered here.
        runCheck("graphPanel") {
            val uri = firstSourceFileUri(project) ?: error("no source file to graph")
            withContext(Dispatchers.EDT) {
                val panel = GraphPanel(project, GraphKind.DEPENDENCIES)
                try {
                    panel.load(uri)
                    "jcefSupported=${JBCefApp.isSupported()}"
                } finally {
                    Disposer.dispose(panel)
                }
            }
        }

        // Instantiating the tool window is the only way to catch a renderer or
        // layout failure; a tool window that compiles can still throw the first
        // time it is shown.
        runCheck("toolWindow") {
            withContext(Dispatchers.EDT) {
                val toolWindow = ToolWindowManager.getInstance(project).getToolWindow("CodeGraph")
                    ?: error("CodeGraph tool window is not registered")
                toolWindow.show()
                "shown with ${toolWindow.contentManager.contentCount} tab(s)"
            }
        }

        LOG.warn("$TAG finished, engine status: ${client.status()}")
    }

    /** Any indexable-looking source file, used as a concrete code-lens target. */
    private fun firstSourceFileUri(project: Project): String? {
        val base = project.basePath?.let { java.nio.file.Paths.get(it) } ?: return null
        return runCatching {
            java.nio.file.Files.walk(base, SOURCE_SCAN_DEPTH).use { paths ->
                paths.filter { java.nio.file.Files.isRegularFile(it) }
                    .filter { path -> SOURCE_SUFFIXES.any { path.toString().endsWith(it) } }
                    .findFirst()
                    .orElse(null)
                    ?.toUri()
                    ?.toString()
            }
        }.getOrNull()
    }

    /**
     * `runCatching` cannot wrap a suspending lambda, so the try/catch is
     * explicit. Throwable rather than Exception: a check that trips an assertion
     * or a linkage error should be reported, not propagated out of startup.
     */
    private suspend fun runCheck(name: String, block: suspend () -> Any?) {
        try {
            val value = block()
            LOG.warn("$TAG PASS $name -> ${value.toString().take(PREVIEW)}")
        } catch (error: Throwable) {
            LOG.warn("$TAG FAIL $name -> ${error.message ?: error::class.java.name}", error)
        }
    }

    private companion object {
        val LOG = logger<SelfCheckActivity>()
        const val PROPERTY = "codegraph.selfcheck"

        /** Grep handle: one string to search the IDE log for. */
        const val TAG = "[codegraph-selfcheck]"

        /** The first command also waits for process spawn and engine init. */
        const val STARTUP_TIMEOUT_SECONDS = 120L
        const val COMMAND_TIMEOUT_SECONDS = 60L
        const val PREVIEW = 300

        /** Shallow walk: enough to find a source file, cheap on a large repo. */
        const val SOURCE_SCAN_DEPTH = 4
        val SOURCE_SUFFIXES = listOf(".py", ".rs", ".go", ".ts", ".js", ".java", ".kt", ".c", ".cpp")
    }
}
