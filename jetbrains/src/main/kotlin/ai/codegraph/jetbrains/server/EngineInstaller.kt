// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.server

import ai.codegraph.jetbrains.lsp.CodeGraphClient
import ai.codegraph.jetbrains.notify.CodeGraphNotifications
import com.intellij.ide.plugins.PluginManagerCore
import com.intellij.openapi.diagnostic.logger
import com.intellij.openapi.extensions.PluginId
import com.intellij.openapi.progress.ProgressIndicator
import com.intellij.openapi.progress.ProgressManager
import com.intellij.openapi.progress.Task
import com.intellij.openapi.project.Project
import com.redhat.devtools.lsp4ij.ServerStatus

/** Runs the engine download behind a progress bar and reports the outcome. */
object EngineInstaller {

    fun downloadInBackground(project: Project) {
        val version = pluginVersion()
        if (version == null) {
            CodeGraphNotifications.error(
                project,
                "The CodeGraph plugin descriptor is unavailable, so there is no version to download.",
            )
            return
        }
        ProgressManager.getInstance().run(
            object : Task.Backgroundable(project, "Downloading the CodeGraph engine", true) {
                override fun run(indicator: ProgressIndicator) {
                    val client = CodeGraphClient.getInstance(project)
                    runCatching { EngineDownloader().download(version, indicator) }.fold(
                        onSuccess = { path ->
                            LOG.info("CodeGraph engine installed at $path")
                            // Replacing the binary under a live process does not
                            // change the process. Saying so beats implying the
                            // new engine is already in use.
                            val running = client.status() in RUNNING_STATUSES
                            CodeGraphNotifications.info(
                                project,
                                if (running) {
                                    "CodeGraph engine $version installed. It takes effect the next " +
                                        "time the engine starts."
                                } else {
                                    "CodeGraph engine $version installed. Starting it now."
                                },
                            )
                            if (!running) client.start()
                        },
                        onFailure = { error -> report(project, version, error) },
                    )
                }
            },
        )
    }

    /**
     * Distinguishes "this platform has no published build" and "the download was
     * tampered with or truncated" from an ordinary network failure, because the
     * three call for completely different responses from the user.
     */
    private fun report(project: Project, version: String, error: Throwable) {
        LOG.warn("CodeGraph engine download failed", error)
        val message = when {
            error is EngineDownloader.ChecksumMismatchException ->
                "The downloaded engine did not match its published checksum and was discarded. " +
                    "This can mean a corrupted transfer or an untrusted proxy; nothing was installed."

            error is CodeGraphServerResolver.UnsupportedPlatformException ->
                "CodeGraph does not publish an engine for this platform. " +
                    "Point it at your own build in Settings | Tools | CodeGraph."

            error.message?.contains("404") == true ->
                "No engine was published for version $version on this platform. " +
                    "Install it with <code>npm i -g @astudioplus/codegraph-mcp</code> instead."

            else ->
                "Could not download the CodeGraph engine: ${error.message ?: error::class.java.simpleName}"
        }
        CodeGraphNotifications.error(project, message)
    }

    /**
     * The plugin ships in lockstep with the engine it was built against, so the
     * plugin's own version names the release to fetch - and the version a
     * managed install is expected to be.
     *
     * Null outside a real IDE (tests, headless tooling), where there is no
     * plugin descriptor to read.
     */
    fun pluginVersion(): String? =
        runCatching { PluginManagerCore.getPlugin(PluginId.getId(PLUGIN_ID))?.version }.getOrNull()

    private val RUNNING_STATUSES = setOf(ServerStatus.started, ServerStatus.starting)

    private val LOG = logger<EngineInstaller>()
    private const val PLUGIN_ID = "ai.codegraph.jetbrains"
}
