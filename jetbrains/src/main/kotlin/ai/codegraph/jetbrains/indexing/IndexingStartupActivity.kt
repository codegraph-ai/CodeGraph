// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.indexing

import ai.codegraph.jetbrains.lsp.CodeGraphClient
import ai.codegraph.jetbrains.notify.CodeGraphNotifications
import ai.codegraph.jetbrains.server.CodeGraphServerResolver
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

        if (CodeGraphServerResolver.resolve(project.basePath, settings.serverPath) == null) {
            // No one-click install yet: the engine is only distributed bundled
            // inside the npm package and the VSIX, and the JetBrains
            // Marketplace ships a single artifact for every platform so the
            // plugin cannot carry a ~120 MB binary set of its own.
            CodeGraphNotifications.warn(
                project,
                "The CodeGraph engine is not installed. Install it with " +
                    "<code>npm i -g @astudioplus/codegraph-mcp</code>, then reopen this project, " +
                    "or point CodeGraph at an existing engine in Settings | Tools | CodeGraph.",
            )
            return
        }

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

    private companion object {
        val LOG = logger<IndexingStartupActivity>()

        /** Matches the VS Code client's post-handshake wait before probing the index. */
        const val GRAPH_LOAD_GRACE_MILLIS = 2_000L
    }
}
