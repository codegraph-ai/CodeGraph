// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.indexing

import ai.codegraph.jetbrains.lsp.CodeGraphClient
import ai.codegraph.jetbrains.lsp.CodeGraphCommand
import ai.codegraph.jetbrains.notify.CodeGraphNotifications
import ai.codegraph.jetbrains.telemetry.TelemetryReporter
import com.google.gson.JsonElement
import com.intellij.openapi.components.Service
import com.intellij.openapi.components.service
import com.intellij.openapi.diagnostic.logger
import com.intellij.openapi.progress.ProgressIndicator
import com.intellij.openapi.progress.ProgressManager
import com.intellij.openapi.progress.Task
import com.intellij.openapi.project.Project
import java.util.concurrent.TimeUnit

/**
 * Indexing state and the reindex operation.
 *
 * Everything here is a thin wrapper over engine commands; the value it adds is
 * knowing what an empty result actually means, which is the difference between
 * "nothing indexed yet" and "indexed, nothing matched".
 */
@Service(Service.Level.PROJECT)
class IndexingService(private val project: Project) {

    /**
     * Whether the engine already holds a graph for this workspace.
     *
     * Asks for a single symbol rather than a count because that is the cheapest
     * question the command surface can answer. Note the caller must not run this
     * the instant the engine starts: the engine loads its persisted graph and
     * rebuilds search indexes after the LSP handshake, so an immediate query can
     * report an empty index while tens of thousands of nodes are still loading,
     * and the user gets told to index a workspace that is already indexed.
     */
    fun isIndexed(timeoutSeconds: Long = QUERY_TIMEOUT_SECONDS): Boolean =
        runCatching {
            val response = CodeGraphClient.getInstance(project)
                .execute(CodeGraphCommand.SYMBOL_SEARCH, mapOf("query" to "*", "limit" to 1))
                .get(timeoutSeconds, TimeUnit.SECONDS)
            resultCount(response) > 0
        }.getOrElse { error ->
            LOG.info("Could not determine CodeGraph index state: ${error.message}")
            false
        }

    /**
     * Reindex the workspace behind a cancellable progress bar, reporting the
     * outcome once it finishes.
     */
    fun reindexInBackground() {
        ProgressManager.getInstance().run(
            object : Task.Backgroundable(project, "Indexing workspace with CodeGraph", true) {
                override fun run(indicator: ProgressIndicator) {
                    indicator.isIndeterminate = true
                    val startedAt = System.currentTimeMillis()
                    val outcome = runCatching {
                        CodeGraphClient.getInstance(project)
                            .execute(CodeGraphCommand.REINDEX_WORKSPACE, emptyMap<String, Any>())
                            .get(REINDEX_TIMEOUT_MINUTES, TimeUnit.MINUTES)
                    }
                    val elapsed = System.currentTimeMillis() - startedAt
                    outcome.fold(
                        onSuccess = { response ->
                            val count = filesIndexed(response)
                            runCatching {
                                TelemetryReporter.getInstance(project)
                                    .indexCompleted("ok", elapsed, count)
                            }
                            reportSuccess(count)
                        },
                        onFailure = { error ->
                            LOG.warn("CodeGraph reindex failed", error)
                            runCatching {
                                TelemetryReporter.getInstance(project)
                                    .indexCompleted("error", elapsed, 0)
                            }
                            CodeGraphNotifications.error(
                                project,
                                "Indexing failed: ${error.message ?: error::class.java.simpleName}",
                            )
                        },
                    )
                }
            },
        )
    }

    /**
     * A successful reindex that found nothing is a failure from the user's point
     * of view, and the usual cause is an exclude pattern or an index-paths entry
     * that matches everything. Saying so beats reporting "Indexed 0 files".
     */
    private fun reportSuccess(fileCount: Int) {
        if (fileCount > 0) {
            CodeGraphNotifications.info(project, "Indexed $fileCount ${"file".pluralize(fileCount)}")
        } else {
            CodeGraphNotifications.warn(
                project,
                "Indexing finished without reading any files. Check the exclude patterns and " +
                    "index paths in Settings | Tools | CodeGraph.",
            )
        }
    }

    /** Number of results in a symbol-search response. */
    private fun resultCount(response: JsonElement?): Int =
        response?.takeIf { it.isJsonObject }
            ?.asJsonObject?.get("results")
            ?.takeIf { it.isJsonArray }
            ?.asJsonArray?.size()
            ?: 0

    private fun String.pluralize(count: Int): String = if (count == 1) this else this + "s"

    companion object {
        private val LOG = logger<IndexingService>()

        private const val QUERY_TIMEOUT_SECONDS = 30L
        private const val REINDEX_TIMEOUT_MINUTES = 60L

        fun getInstance(project: Project): IndexingService = project.service()

        /**
         * Files read during an index run.
         *
         * The engine answers `codegraph.reindexWorkspace` with snake_case keys
         * (`files_indexed`, `files_parsed`, `by_language`, ...), unlike its
         * camelCase query responses. Getting this wrong does not fail loudly -
         * it reports zero files and sends the user to the "indexing found
         * nothing" path with a healthy index.
         */
        fun filesIndexed(response: JsonElement?): Int =
            response?.takeIf { it.isJsonObject }
                ?.asJsonObject?.get("files_indexed")
                ?.takeIf { it.isJsonPrimitive && it.asJsonPrimitive.isNumber }
                ?.asInt
                ?: 0
    }
}
