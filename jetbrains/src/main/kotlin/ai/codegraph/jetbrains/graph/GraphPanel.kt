// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.graph

import ai.codegraph.jetbrains.lsp.CodeGraphClient
import ai.codegraph.jetbrains.lsp.CodeGraphCommand
import com.google.gson.Gson
import com.google.gson.JsonElement
import com.intellij.openapi.Disposable
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.diagnostic.logger
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.Disposer
import com.intellij.ui.jcef.JBCefApp
import com.intellij.ui.jcef.JBCefBrowser
import com.intellij.ui.components.JBLabel
import com.intellij.ui.components.JBScrollPane
import com.intellij.util.ui.JBUI
import java.awt.BorderLayout
import javax.swing.JPanel
import javax.swing.JTextArea

/** Which graph the panel is showing. */
enum class GraphKind(val command: CodeGraphCommand, val title: String) {
    DEPENDENCIES(CodeGraphCommand.GET_DEPENDENCY_GRAPH, "Dependency Graph"),
    CALLS(CodeGraphCommand.GET_CALL_GRAPH, "Call Graph"),
}

/**
 * Renders a graph for one file.
 *
 * Uses JCEF, because the graph is a force-directed layout that Swing would need
 * a bespoke renderer for and a browser gets for free. JCEF is not always
 * available - some JBR builds ship without it, and Remote Dev clients cannot use
 * it - so an unavailable browser degrades to a readable text listing rather than
 * an empty panel or a crash.
 */
class GraphPanel(private val project: Project, val kind: GraphKind) :
    JPanel(BorderLayout()),
    Disposable {

    /** The file this panel is showing, so a second request for it reuses the tab. */
    var fileUri: String? = null
        private set

    private val gson = Gson()
    private val browser: JBCefBrowser? = if (JBCefApp.isSupported()) JBCefBrowser() else null
    private val fallback = JTextArea().apply {
        isEditable = false
        border = JBUI.Borders.empty(8)
    }
    private val status = JBLabel().apply { border = JBUI.Borders.empty(4, 8) }

    init {
        if (browser != null) {
            Disposer.register(this, browser)
            add(browser.component, BorderLayout.CENTER)
        } else {
            LOG.info("JCEF is unavailable; the CodeGraph graph panel falls back to a text listing")
            add(JBScrollPane(fallback), BorderLayout.CENTER)
        }
        add(status, BorderLayout.SOUTH)
    }

    /** Load the graph for [fileUri]. */
    fun load(fileUri: String, depth: Int = DEFAULT_DEPTH) {
        this.fileUri = fileUri
        setStatus("Loading ${kind.title.lowercase()}...")
        CodeGraphClient.getInstance(project)
            .execute(kind.command, mapOf("uri" to fileUri, "depth" to depth))
            .whenComplete { json, error ->
                if (error != null) {
                    setStatus("Could not load the graph: ${error.message}")
                    return@whenComplete
                }
                val graph = runCatching { GraphData.from(json, gson) }.getOrNull()
                if (graph == null || graph.nodes.isEmpty()) {
                    setStatus("Nothing to show. Index the workspace, or pick a file with known relationships.")
                    render(GraphData(emptyList(), emptyList()))
                    return@whenComplete
                }
                setStatus("${graph.nodes.size} nodes, ${graph.edges.size} edges")
                render(graph)
            }
    }

    private fun render(graph: GraphData) {
        ApplicationManager.getApplication().invokeLater {
            if (browser != null) {
                browser.loadHTML(GraphHtml.render(graph, kind.title))
            } else {
                fallback.text = GraphHtml.renderText(graph, kind.title)
                fallback.caretPosition = 0
            }
        }
    }

    private fun setStatus(text: String) {
        ApplicationManager.getApplication().invokeLater { status.text = text }
    }

    override fun dispose() = Unit

    private companion object {
        val LOG = logger<GraphPanel>()
        const val DEFAULT_DEPTH = 2
    }
}

/** Node and edge lists, normalised across the dependency and call graph shapes. */
data class GraphData(val nodes: List<GraphNode>, val edges: List<GraphEdge>) {

    companion object {
        /**
         * The two graph commands answer with different shapes: the dependency
         * graph labels nodes with `label`/`type`, the call graph with `name`.
         * Both are normalised here so the renderer only knows one shape.
         */
        fun from(json: JsonElement?, gson: Gson): GraphData {
            val obj = json?.takeIf { it.isJsonObject }?.asJsonObject ?: return GraphData(emptyList(), emptyList())

            val nodes = obj.getAsJsonArray("nodes")?.mapNotNull { element ->
                val node = element.takeIf { it.isJsonObject }?.asJsonObject ?: return@mapNotNull null
                val id = node.get("id")?.asString ?: return@mapNotNull null
                GraphNode(
                    id = id,
                    label = node.get("label")?.asString
                        ?: node.get("name")?.asString
                        ?: id,
                    type = node.get("type")?.asString ?: node.get("kind")?.asString ?: "unknown",
                    language = node.get("language")?.asString.orEmpty(),
                    uri = node.get("uri")?.asString.orEmpty(),
                )
            }.orEmpty()

            val edges = obj.getAsJsonArray("edges")?.mapNotNull { element ->
                val edge = element.takeIf { it.isJsonObject }?.asJsonObject ?: return@mapNotNull null
                val from = edge.get("from")?.asString ?: return@mapNotNull null
                val to = edge.get("to")?.asString ?: return@mapNotNull null
                GraphEdge(from, to, edge.get("type")?.asString ?: "calls")
            }.orEmpty()

            return GraphData(nodes, edges)
        }
    }
}

data class GraphNode(
    val id: String,
    val label: String,
    val type: String,
    val language: String,
    val uri: String,
)

data class GraphEdge(val from: String, val to: String, val type: String)
