// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.indexing

import ai.codegraph.jetbrains.lsp.CodeGraphClient
import ai.codegraph.jetbrains.notify.CodeGraphNotifications
import ai.codegraph.jetbrains.server.CodeGraphServerResolver
import ai.codegraph.jetbrains.server.EngineInstaller
import ai.codegraph.jetbrains.server.ResolvedServer
import ai.codegraph.jetbrains.settings.CodeGraphSettings
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.diagnostic.logger
import com.intellij.openapi.project.Project
import com.intellij.openapi.startup.ProjectActivity
import kotlinx.coroutines.delay
import kotlinx.coroutines.future.await

/**
 * Starts the engine when a project opens and, if the workspace has never been
 * indexed, offers to index it.
 *
 * Without an index every CodeGraph surface is empty, and an empty surface reads
 * as a broken plugin rather than as a missing first step.
 */
class IndexingStartupActivity : ProjectActivity {

    override suspend fun execute(project: Project) {
        val application = ApplicationManager.getApplication()
        // Headless runs - the plugin verifier, searchable-options generation,
        // any CI inspection - open a project with no user and no UI. Starting a
        // native engine there costs a process and a full index for nobody, and
        // it is what made searchable-options generation hang.
        if (application.isUnitTestMode || application.isHeadlessEnvironment) return

        val settings = CodeGraphSettings.getInstance(project).state
        if (!settings.enabled) return

        val resolved = CodeGraphServerResolver.resolve(project.basePath, settings.serverPath)
        if (resolved == null) {
            // Offered rather than done automatically: this is a ~30 MB download
            // of a native binary that will run with the user's permissions, and
            // starting that unasked on project open is not a decision the
            // plugin should make for them.
            CodeGraphNotifications.infoWithActions(
                project,
                "The CodeGraph engine is not installed, so there is no graph to answer questions from. " +
                    "It can be downloaded for this platform, or installed separately with " +
                    "<code>npm i -g @astudioplus/codegraph-mcp</code>.",
                "Download Engine" to { notification ->
                    notification.expire()
                    EngineInstaller.downloadInBackground(project)
                },
            )
            return
        }

        offerEngineUpdateIfStale(project, resolved)

        val client = CodeGraphClient.getInstance(project)
        client.start()

        // Wait for the engine to finish `initialize` before starting the clock.
        // `start()` only requests a launch - the process is spawned lazily - so
        // sleeping straight after it times the grace period against the wrong
        // event and provides no grace at all.
        runCatching { client.awaitReady().await() }
            .onFailure { error ->
                LOG.info("CodeGraph engine did not become ready: ${error.message}")
                return
            }

        // Even once initialized, the engine loads its persisted graph and
        // rebuilds search indexes in the background. Asking too early reports an
        // empty index for a workspace that is already indexed, and sends the
        // user to redo work that is already done.
        delay(GRAPH_LOAD_GRACE_MILLIS)

        val indexing = IndexingService.getInstance(project)
        val indexed = indexing.isIndexed()
        // The single most common support question is "why is CodeGraph empty",
        // and the answer is almost always this decision. Record it.
        LOG.info("CodeGraph workspace index present: $indexed")
        if (indexed) return

        CodeGraphNotifications.infoWithActions(
            project,
            "This workspace has not been indexed yet, so CodeGraph has no graph to answer questions from.",
            "Index Now" to { notification ->
                notification.expire()
                indexing.reindexInBackground()
            },
        )
    }

    /**
     * A managed engine is found by file name, which says nothing about which
     * build it is. The plugin ships in lockstep with the engine it was built
     * against, so one installed by an earlier plugin would otherwise be reused
     * for good, and this build would keep talking to it.
     *
     * Offered rather than forced: the engine on disk still runs, and an update
     * that cannot reach the release must not cost the user a working install.
     * Only managed installs are ours to replace - a Pro, PATH or locally built
     * engine is the user's to manage - and only ones that are actually older,
     * since the VS Code extension installs into the same directory and may
     * legitimately be ahead of this plugin.
     */
    private fun offerEngineUpdateIfStale(project: Project, resolved: ResolvedServer) {
        if (resolved.origin != ResolvedServer.Origin.MANAGED_INSTALL) return
        val expected = CodeGraphServerResolver.ENGINE_VERSION
        val installed = CodeGraphServerResolver.managedEngineVersion()
        if (!CodeGraphServerResolver.isManagedEngineStale(installed, expected)) return

        LOG.info("Managed CodeGraph engine reports version ${installed ?: "unknown"}, expected $expected")
        CodeGraphNotifications.infoWithActions(
            project,
            "The installed CodeGraph engine (${installed ?: "unknown version"}) predates the one " +
                "this plugin ships against ($expected). They ship together, so features this " +
                "build expects may be missing.",
            "Update Engine" to { notification ->
                notification.expire()
                EngineInstaller.downloadInBackground(project)
            },
        )
    }

    private companion object {
        val LOG = logger<IndexingStartupActivity>()

        /** Matches the VS Code client's post-handshake wait before probing the index. */
        const val GRAPH_LOAD_GRACE_MILLIS = 2_000L
    }
}
