// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.telemetry

import ai.codegraph.jetbrains.server.ServerEdition
import ai.codegraph.jetbrains.settings.CodeGraphSettings
import com.google.gson.Gson
import com.intellij.internal.statistic.utils.StatisticsUploadAssistant
import com.intellij.openapi.application.ApplicationInfo
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.components.Service
import com.intellij.openapi.components.service
import com.intellij.openapi.diagnostic.logger
import com.intellij.openapi.project.Project
import com.intellij.openapi.application.PermanentInstallationID
import java.io.OutputStream
import java.net.HttpURLConnection
import java.net.URI
import java.util.UUID
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

/**
 * Sends the same events as the VS Code client, so both editors land in one
 * funnel rather than two that have to be reconciled.
 *
 * Event names and property names are deliberately identical to
 * `vscode/src/telemetry/reporter.ts`; only `ide`, `ideProduct` and `ideBuild`
 * are added, so a dashboard can split by editor without a second schema.
 *
 * Nothing is sent unless every gate in [TelemetryGate] passes, and no build
 * without a compiled-in key can send at all.
 */
@Service(Service.Level.PROJECT)
class TelemetryReporter(private val project: Project) {

    private val gson = Gson()
    private val sessionId = UUID.randomUUID().toString()

    /**
     * A single daemon thread. Telemetry must never delay anything the user is
     * waiting for, and it must never keep the IDE alive on shutdown.
     */
    private val sender = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "CodeGraph telemetry").apply { isDaemon = true }
    }

    /** Set once the engine is resolved, so events can be split by edition. */
    @Volatile
    var serverEdition: ServerEdition? = null

    fun activationStarted(workspaceFolders: Int) =
        send("activation_start", mapOf("workspaceFolders" to workspaceFolders), isError = false)

    fun engineStartResult(outcome: String, durationMs: Long, errorHint: String? = null) =
        send(
            "activation_server_start_result",
            mapOf("outcome" to outcome, "durationMs" to durationMs, "errorHint" to errorHint),
            isError = outcome != "ok",
        )

    fun engineCrashed(cause: String, phase: String?, uptimeSeconds: Long, restartCount: Int) =
        send(
            "server_crash",
            mapOf(
                "crashCause" to cause,
                "crashPhase" to phase,
                "uptimeSeconds" to uptimeSeconds,
                "restartCount" to restartCount,
            ),
            isError = true,
        )

    fun indexCompleted(outcome: String, durationMs: Long, fileCount: Int) =
        send(
            "index_completed",
            mapOf("outcome" to outcome, "durationMs" to durationMs, "fileCount" to fileCount),
            isError = outcome != "ok",
        )

    /**
     * Properties every event carries.
     *
     * `machineId` is the IDE's own installation id rather than anything derived
     * from the user or the workspace: it is already the identifier JetBrains
     * uses for this purpose, and it is one the user can reset.
     */
    private fun commonProperties(): Map<String, Any?> {
        val info = ApplicationInfo.getInstance()
        return mapOf(
            "ide" to "jetbrains",
            "ideProduct" to info.build.productCode,
            "ideBuild" to info.build.asStringWithoutProductCode(),
            "pluginVersion" to pluginVersion(),
            "os" to System.getProperty("os.name"),
            "serverEdition" to serverEdition?.name?.lowercase(),
            "machineId" to PermanentInstallationID.get(),
            "sessionId" to sessionId,
        )
    }

    private fun send(event: String, properties: Map<String, Any?>, isError: Boolean) {
        // Explicit rather than relying on test builds happening to have no key.
        if (ApplicationManager.getApplication()?.isUnitTestMode == true) return

        val settings = CodeGraphSettings.getInstance(project).state

        val allowed = TelemetryGate.allows(
            hasKey = TelemetryConfig.hasKey,
            ideConsent = ideConsent(),
            pluginEnabled = settings.telemetryEnabled,
            errorReportsOnly = settings.telemetryErrorReportsOnly,
            isErrorEvent = isError,
        )
        if (!allowed) return

        val payload = TelemetryGate.clean(commonProperties() + properties)
        if (settings.debug) LOG.info("telemetry $event $payload")

        sender.execute { post(event, payload) }
    }

    /**
     * The IDE-level statistics consent. A user who turned JetBrains' own usage
     * reporting off has already answered this question, and the plugin has no
     * business asking again with a different default.
     */
    private fun ideConsent(): Boolean =
        runCatching { StatisticsUploadAssistant.isSendAllowed() }.getOrDefault(false)

    private fun pluginVersion(): String =
        runCatching {
            com.intellij.ide.plugins.PluginManagerCore
                .getPlugin(com.intellij.openapi.extensions.PluginId.getId(PLUGIN_ID))
                ?.version
        }.getOrNull().orEmpty()

    private fun post(event: String, properties: Map<String, Any>) {
        runCatching {
            val body = gson.toJson(
                mapOf(
                    "api_key" to TelemetryConfig.key,
                    "event" to event,
                    "properties" to properties + mapOf("distinct_id" to properties["machineId"]),
                ),
            )
            val connection = URI("${TelemetryConfig.host}/capture/").toURL().openConnection() as HttpURLConnection
            connection.apply {
                requestMethod = "POST"
                doOutput = true
                connectTimeout = TIMEOUT_MS
                readTimeout = TIMEOUT_MS
                setRequestProperty("Content-Type", "application/json")
            }
            connection.outputStream.use { stream: OutputStream -> stream.write(body.toByteArray()) }
            connection.responseCode
            connection.disconnect()
        }.onFailure {
            // Never surface or retry: a machine that cannot reach the endpoint
            // is not a machine whose user should hear about it.
            LOG.debug("Telemetry send failed", it)
        }
    }

    fun shutdown() {
        sender.shutdown()
        runCatching { sender.awaitTermination(SHUTDOWN_WAIT_SECONDS, TimeUnit.SECONDS) }
    }

    companion object {
        private val LOG = logger<TelemetryReporter>()
        private const val PLUGIN_ID = "ai.codegraph.jetbrains"
        private const val TIMEOUT_MS = 5_000
        private const val SHUTDOWN_WAIT_SECONDS = 2L

        fun getInstance(project: Project): TelemetryReporter = project.service()
    }
}
