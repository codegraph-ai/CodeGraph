// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.server

import ai.codegraph.jetbrains.lsp.CodeGraphClient
import ai.codegraph.jetbrains.notify.CodeGraphNotifications
import com.intellij.openapi.diagnostic.logger
import com.intellij.openapi.progress.ProgressIndicator
import com.intellij.openapi.progress.ProgressManager
import com.intellij.openapi.progress.Task
import com.intellij.openapi.project.Project

/** Runs the engine download behind a progress bar and reports the outcome. */
object EngineInstaller {

    fun downloadInBackground(project: Project) {
        val version = CodeGraphServerResolver.ENGINE_VERSION
        ProgressManager.getInstance().run(
            object : Task.Backgroundable(project, "Downloading the CodeGraph engine", true) {
                override fun run(indicator: ProgressIndicator) {
                    val client = CodeGraphClient.getInstance(project)
                    // The engine holds its own binary open, so an update issued
                    // while it runs cannot replace it - on Windows the move
                    // fails outright, and everywhere else the old process keeps
                    // going and the version marker records an engine nobody is
                    // running. It is stopped once the download is verified, and
                    // started again whichever way the install ends, so a failed
                    // update never leaves the user without an engine.
                    var stopped = false
                    runCatching {
                        EngineDownloader().download(version, indicator) {
                            if (client.isRunning()) {
                                indicator.text = "Stopping the CodeGraph engine to replace it"
                                stopped = true
                                if (!client.stopAndAwait()) {
                                    LOG.warn("CodeGraph engine did not stop before the update; replacing anyway")
                                }
                            }
                        }
                    }.fold(
                        onSuccess = { path ->
                            LOG.info("CodeGraph engine installed at $path")
                            CodeGraphNotifications.info(
                                project,
                                "CodeGraph engine $version installed. Starting it now.",
                            )
                            client.start()
                        },
                        onFailure = { error ->
                            report(project, version, error)
                            if (stopped) client.start()
                        },
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

            error is EngineDownloader.EngineInUseException ->
                "The CodeGraph engine could not be replaced because it is still running. " +
                    "Close other projects using it, or restart the IDE, and try the update again."

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

    private val LOG = logger<EngineInstaller>()
}
