// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.lsp

import ai.codegraph.jetbrains.server.CODEGRAPH_SERVER_ID
import ai.codegraph.jetbrains.server.EngineLifecycle
import com.google.gson.Gson
import com.google.gson.JsonElement
import com.intellij.openapi.components.Service
import com.intellij.openapi.components.service
import com.intellij.openapi.diagnostic.logger
import com.intellij.openapi.project.Project
import com.redhat.devtools.lsp4ij.LanguageServerManager
import com.redhat.devtools.lsp4ij.ServerStatus
import org.eclipse.lsp4j.ExecuteCommandParams
import java.util.concurrent.CompletableFuture

/**
 * The single door to the CodeGraph engine.
 *
 * Every capability the engine exposes to an editor arrives as a
 * `workspace/executeCommand` call; see [CodeGraphCommand] for the catalogue.
 * Keeping that in one place means the UI layers never touch LSP4IJ directly and
 * the command surface stays greppable.
 */
@Service(Service.Level.PROJECT)
class CodeGraphClient(private val project: Project) {

    private val gson = Gson()

    /**
     * Current engine status, for the status bar and for guard checks.
     * A server that has never been referenced reports no status at all, which
     * is the same situation as [ServerStatus.none].
     */
    fun status(): ServerStatus =
        LanguageServerManager.getInstance(project).getServerStatus(CODEGRAPH_SERVER_ID) ?: ServerStatus.none

    /**
     * Start the engine if it is not already running.
     *
     * The engine is not tied to any one file type - its value is workspace-wide
     * - so it is started explicitly rather than waiting for LSP4IJ's file
     * mappings to trigger a lazy start.
     *
     * Does nothing once the restart breaker has opened: that state means the
     * engine has already proved it cannot stay up on this machine, and the user
     * has been told. Restarting anyway is what produces crash loops.
     */
    fun start() {
        if (EngineLifecycle.getInstance(project).isRestartBlocked) {
            LOG.info("Not starting the CodeGraph engine: restarts are blocked after repeated crashes")
            return
        }
        LanguageServerManager.getInstance(project).start(CODEGRAPH_SERVER_ID)
    }

    /** True while the engine is up, or on its way up. */
    fun isRunning(): Boolean = status() in RUNNING_STATUSES

    /**
     * Stop the engine and wait until its process is gone, up to [timeoutMillis].
     * Returns whether it went away in time.
     *
     * Replacing the engine binary needs the process gone first: on Windows a
     * running `.exe` cannot be overwritten at all, and on every platform a
     * binary swapped under a live process is simply not the engine that is
     * running. Stopping is asynchronous, and `stopping` is not `stopped`, so
     * asking is not enough - the caller has to know when it finished.
     *
     * Blocks, so callers must be on a background thread. [start] afterwards
     * brings the engine back: LSP4IJ's default stop disables the server and its
     * default start re-enables it.
     */
    fun stopAndAwait(timeoutMillis: Long = STOP_TIMEOUT_MILLIS): Boolean {
        LanguageServerManager.getInstance(project).stop(CODEGRAPH_SERVER_ID)
        val deadline = System.currentTimeMillis() + timeoutMillis
        while (!isStopped() && System.currentTimeMillis() < deadline) {
            try {
                Thread.sleep(STOP_POLL_MILLIS)
            } catch (_: InterruptedException) {
                Thread.currentThread().interrupt()
                return isStopped()
            }
        }
        return isStopped()
    }

    /** True once the engine process is definitely gone, rather than on its way out. */
    private fun isStopped(): Boolean = status() in STOPPED_STATUSES

    /**
     * A future that completes once the engine has finished `initialize`.
     *
     * [start] only asks LSP4IJ to bring the engine up; the process is not
     * spawned synchronously. Anything that needs to time itself against a live
     * engine - rather than against the moment we asked for one - must wait on
     * this instead.
     */
    fun awaitReady(): CompletableFuture<Unit> =
        LanguageServerManager.getInstance(project)
            .getLanguageServer(CODEGRAPH_SERVER_ID)
            .thenCompose { server ->
                server?.initializedServer?.thenApply { } ?: CompletableFuture.completedFuture(Unit)
            }

    /**
     * Send a `workspace/executeCommand` and return the raw JSON result.
     *
     * The future completes exceptionally if the engine cannot be started; the
     * caller decides whether that is worth surfacing to the user.
     */
    fun execute(command: CodeGraphCommand, arguments: Any? = null): CompletableFuture<JsonElement?> {
        val params = ExecuteCommandParams(
            command.id,
            if (arguments == null) emptyList() else listOf(arguments),
        )
        return withServer(command) { server -> server.workspaceService.executeCommand(params) }
            .thenApply { raw -> raw?.let { toJson(it) } }
            .whenComplete { _, error ->
                if (error != null) LOG.warn("CodeGraph command ${command.id} failed", error)
            }
    }

    /**
     * Resolve the engine and run [action] against it.
     *
     * `start()` is asynchronous, so a command issued straight after it can
     * arrive before LSP4IJ has a server to hand and get back null. That is
     * "not up yet", not "cannot run" - failing fast on it made the first
     * command of a session fail spuriously, which is exactly the command a
     * user is most likely to notice (the reindex they just asked for). So a
     * null resolves once more after asking for a start.
     */
    private fun <T> withServer(
        command: CodeGraphCommand,
        action: (com.redhat.devtools.lsp4ij.LanguageServerItem) -> CompletableFuture<T>,
    ): CompletableFuture<T> {
        val manager = LanguageServerManager.getInstance(project)
        return manager.getLanguageServer(CODEGRAPH_SERVER_ID)
            .thenCompose { server ->
                if (server != null) {
                    action(server)
                } else {
                    start()
                    manager.getLanguageServer(CODEGRAPH_SERVER_ID).thenCompose { retried ->
                        if (retried != null) {
                            action(retried)
                        } else {
                            CompletableFuture.failedFuture(EngineUnavailableException(command))
                        }
                    }
                }
            }
    }

    /** Convenience wrapper that deserialises the result into [T]. */
    fun <T> execute(command: CodeGraphCommand, arguments: Any?, type: Class<T>): CompletableFuture<T?> =
        execute(command, arguments).thenApply { json -> json?.let { gson.fromJson(it, type) } }

    /**
     * LSP4J hands back whatever Gson produced for an untyped result, which is
     * already a [JsonElement] in practice. Re-serialising anything else keeps
     * callers from having to care.
     */
    private fun toJson(raw: Any): JsonElement =
        raw as? JsonElement ?: gson.toJsonTree(raw)

    class EngineUnavailableException(command: CodeGraphCommand) :
        RuntimeException("CodeGraph engine is not running; cannot execute ${command.id}")

    companion object {
        private val LOG = logger<CodeGraphClient>()

        private val RUNNING_STATUSES = setOf(ServerStatus.started, ServerStatus.starting)

        /** `stopping` is deliberately absent: the process is not gone yet. */
        private val STOPPED_STATUSES = setOf(ServerStatus.none, ServerStatus.stopped)

        /** Long enough for an orderly shutdown, short enough not to look hung. */
        private const val STOP_TIMEOUT_MILLIS = 10_000L
        private const val STOP_POLL_MILLIS = 100L

        fun getInstance(project: Project): CodeGraphClient = project.service()
    }
}
