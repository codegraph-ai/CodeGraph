// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.server

import ai.codegraph.jetbrains.notify.CodeGraphNotifications
import ai.codegraph.jetbrains.telemetry.TelemetryReporter
import com.intellij.openapi.components.Service
import com.intellij.openapi.components.service
import com.intellij.openapi.diagnostic.logger
import com.intellij.openapi.project.Project
import com.redhat.devtools.lsp4ij.LanguageServerManager
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicLong

/**
 * Owns what happens when the engine starts, stops, or dies.
 *
 * The engine is a native process doing heavy work, so it can be killed by
 * things the plugin cannot control: antivirus, the OOM killer, a missing system
 * library. This service turns those events into one clear message and, when the
 * engine cannot stay up at all, into a decision to stop trying.
 */
@Service(Service.Level.PROJECT)
class EngineLifecycle(private val project: Project) {

    private val breaker = RestartCircuitBreaker()
    private val breadcrumbs = CrashBreadcrumbs()

    private val startedAt = AtomicLong(0)
    private val restarts = AtomicInteger(0)

    /** True while the engine is being deliberately stopped, so it is not counted as a crash. */
    @Volatile
    private var shutdownExpected = false

    /**
     * The engine we most recently resolved, published by
     * [CodeGraphConnectionProvider] as it starts.
     *
     * Callers that only want to *display* which engine is in play read this
     * instead of resolving again. Resolution walks PATH and stats several
     * files, and the status bar repaints often enough that doing it there would
     * put filesystem I/O on the EDT.
     */
    @Volatile
    var resolvedServer: ResolvedServer? = null
        private set

    fun publishResolvedServer(server: ResolvedServer) {
        resolvedServer = server
        runCatching { TelemetryReporter.getInstance(project).serverEdition = server.edition }
    }

    /** How many times the engine has come back up since the project opened. */
    val restartCount: Int get() = restarts.get()

    /** Milliseconds the engine has been up, or 0 when it is not running. */
    val uptimeMillis: Long
        get() = startedAt.get().takeIf { it > 0 }?.let { System.currentTimeMillis() - it } ?: 0

    /**
     * True when the engine has crashed too often to keep restarting.
     * [ai.codegraph.jetbrains.lsp.CodeGraphClient] refuses to start while this
     * holds, which is what actually breaks the loop.
     */
    val isRestartBlocked: Boolean get() = breaker.isOpen

    fun onEngineStarted() {
        // A deliberate stop that never produced a stop event would otherwise
        // leave the flag armed for the lifetime of the project, and the first
        // real crash after it would be swallowed silently.
        shutdownExpected = false
        if (startedAt.getAndSet(System.currentTimeMillis()) > 0) {
            restarts.incrementAndGet()
        }
    }

    /** Mark the next stop as deliberate. Consumed by the following stop event. */
    fun expectShutdown() {
        shutdownExpected = true
    }

    /**
     * Called when the engine process disappears without being asked to.
     *
     * Reads the crash breadcrumb for a cause, counts the crash, and once the
     * breaker opens, stops the engine and explains why rather than letting
     * LSP4IJ start it again on the next request.
     */
    fun onUnexpectedStop() {
        if (shutdownExpected) {
            shutdownExpected = false
            return
        }
        val uptime = uptimeMillis
        startedAt.set(0)

        val diagnosis = breadcrumbs.readAndClear()
        LOG.warn("CodeGraph engine stopped after ${uptime}ms: ${diagnosis.cause} (phase=${diagnosis.phase})")

        runCatching {
            TelemetryReporter.getInstance(project).engineCrashed(
                cause = diagnosis.cause,
                phase = diagnosis.phase,
                uptimeSeconds = uptime / 1000,
                restartCount = restarts.get(),
            )
        }

        if (!breaker.recordCrash(System.currentTimeMillis())) return

        expectShutdown()
        runCatching { LanguageServerManager.getInstance(project).stop(CODEGRAPH_SERVER_ID) }
            .onFailure { LOG.warn("Could not stop the CodeGraph engine after tripping the restart breaker", it) }

        CodeGraphNotifications.errorWithActions(
            project,
            "The CodeGraph engine crashed ${breaker.describeTripCondition()}, so it will not be restarted " +
                "automatically. Diagnosis: ${diagnosis.describe()}. This is most often caused by antivirus " +
                "software, a missing system library, or too little memory.",
            "Retry" to { notification ->
                notification.expire()
                retry()
            },
        )
    }

    /** Close the breaker and start the engine again. */
    fun retry() {
        breaker.reset()
        restarts.set(0)
        runCatching { LanguageServerManager.getInstance(project).start(CODEGRAPH_SERVER_ID) }
            .onFailure { LOG.warn("Retrying the CodeGraph engine failed", it) }
    }

    companion object {
        private val LOG = logger<EngineLifecycle>()

        fun getInstance(project: Project): EngineLifecycle = project.service()
    }
}
