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

/** Runs the engine download behind a progress bar and reports the outcome. */
object EngineInstaller {

    fun downloadInBackground(project: Project) {
        ProgressManager.getInstance().run(
            object : Task.Backgroundable(project, "Downloading the CodeGraph engine", true) {
                override fun run(indicator: ProgressIndicator) {
                    val version = engineVersion()
                    runCatching { EngineDownloader().download(version, indicator) }.fold(
                        onSuccess = { path ->
                            LOG.info("CodeGraph engine installed at $path")
                            CodeGraphNotifications.info(
                                project,
                                "CodeGraph engine $version installed. Starting it now.",
                            )
                            CodeGraphClient.getInstance(project).start()
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
     * plugin's own version names the release to fetch.
     */
    private fun engineVersion(): String =
        PluginManagerCore.getPlugin(PluginId.getId(PLUGIN_ID))?.version
            ?: error("CodeGraph plugin descriptor is unavailable")

    private val LOG = logger<EngineInstaller>()
    private const val PLUGIN_ID = "ai.codegraph.jetbrains"
}
