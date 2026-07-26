// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.vision

import ai.codegraph.jetbrains.lsp.CodeGraphClient
import ai.codegraph.jetbrains.lsp.CodeGraphCommand
import com.google.gson.Gson
import com.google.gson.annotations.SerializedName
import com.intellij.codeInsight.daemon.DaemonCodeAnalyzer
import com.intellij.openapi.components.Service
import com.intellij.openapi.components.service
import com.intellij.openapi.diagnostic.logger
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiFile
import java.util.concurrent.ConcurrentHashMap

/** Graph-derived stats for one declaration, as the engine reports them. */
data class CodeLensSymbol(
    val name: String = "",
    /** 0-based start line, matching the LSP convention the engine uses. */
    val line: Int = 0,
    @SerializedName("callerCount") val callerCount: Int = 0,
    @SerializedName("testCount") val testCount: Int = 0,
    val complexity: Int = 0,
)

private data class DocumentCodeLensResponse(val symbols: List<CodeLensSymbol> = emptyList())

/**
 * Per-document stats, cached by document modification stamp.
 *
 * The Code Vision daemon asks for entries synchronously and often - on every
 * scroll and re-render. The engine answers over LSP, so fetching inline would
 * either block the daemon or hammer the engine. Instead a miss returns nothing,
 * schedules one fetch, and restarts the daemon when the answer arrives.
 *
 * Entries are stored even when the engine returns no symbols. Without that, a
 * file the engine knows nothing about would miss forever and re-request on
 * every single pass.
 */
@Service(Service.Level.PROJECT)
class DocumentStatsCache(private val project: Project) {

    private data class Entry(val stamp: Long, val symbols: List<CodeLensSymbol>)

    private val entries = ConcurrentHashMap<String, Entry>()

    /** URIs with a fetch in flight, so concurrent daemon passes issue one request. */
    private val inFlight = ConcurrentHashMap.newKeySet<String>()

    private val gson = Gson()

    /**
     * Cached stats for [file] at [stamp], or null when a fetch is needed.
     * A null return also schedules that fetch.
     */
    fun get(file: PsiFile, stamp: Long): List<CodeLensSymbol>? {
        val uri = uriOf(file) ?: return emptyList()
        entries[uri]?.takeIf { it.stamp == stamp }?.let { return it.symbols }
        requestRefresh(file, uri, stamp)
        return null
    }

    /** Drop everything, for when the graph itself changed under us. */
    fun invalidateAll() {
        entries.clear()
        DaemonCodeAnalyzer.getInstance(project).restart()
    }

    private fun requestRefresh(file: PsiFile, uri: String, stamp: Long) {
        if (!inFlight.add(uri)) return

        CodeGraphClient.getInstance(project)
            .execute(CodeGraphCommand.GET_DOCUMENT_CODE_LENS, mapOf("uri" to uri))
            .whenComplete { json, error ->
                try {
                    if (error != null) {
                        // Usually just "the engine is not running yet". Caching
                        // an empty result here would hide the stats until the
                        // next edit, so leave the miss in place instead.
                        LOG.debug("Code vision fetch failed for $uri", error)
                        return@whenComplete
                    }
                    val symbols = runCatching {
                        gson.fromJson(json, DocumentCodeLensResponse::class.java)?.symbols
                    }.getOrNull().orEmpty()

                    entries[uri] = Entry(stamp, symbols)
                    if (file.isValid) {
                        DaemonCodeAnalyzer.getInstance(project).restart(file)
                    }
                } finally {
                    inFlight.remove(uri)
                }
            }
    }

    private fun uriOf(file: PsiFile): String? =
        file.virtualFile?.takeIf { it.isInLocalFileSystem }?.let { java.io.File(it.path).toURI().toString() }

    companion object {
        private val LOG = logger<DocumentStatsCache>()

        fun getInstance(project: Project): DocumentStatsCache = project.service()
    }
}
