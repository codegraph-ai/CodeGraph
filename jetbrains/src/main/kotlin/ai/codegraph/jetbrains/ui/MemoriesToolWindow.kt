// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.ui

import ai.codegraph.jetbrains.lsp.CodeGraphClient
import ai.codegraph.jetbrains.lsp.CodeGraphCommand
import ai.codegraph.jetbrains.notify.CodeGraphNotifications
import com.google.gson.Gson
import com.intellij.icons.AllIcons
import com.intellij.openapi.actionSystem.ActionManager
import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.DefaultActionGroup
import com.intellij.openapi.actionSystem.ToggleAction
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.project.DumbAware
import com.intellij.openapi.project.Project
import com.intellij.ui.ColoredListCellRenderer
import com.intellij.ui.SearchTextField
import com.intellij.ui.SimpleTextAttributes
import com.intellij.ui.components.JBLabel
import com.intellij.ui.components.JBList
import com.intellij.ui.components.JBScrollPane
import com.intellij.util.ui.JBUI
import java.awt.BorderLayout
import javax.swing.DefaultListModel
import javax.swing.JComponent
import javax.swing.JPanel
import javax.swing.JTextArea
import javax.swing.JSplitPane
import javax.swing.ListSelectionModel

/** A memory as the engine reports it. */
data class MemoryEntry(
    val id: String = "",
    val kind: String = "",
    val title: String = "",
    val content: String = "",
    val tags: List<String> = emptyList(),
    val score: Double = 0.0,
    val isCurrent: Boolean = true,
    val agentSource: String? = null,
)

private data class MemoryListResponse(
    val memories: List<MemoryEntry> = emptyList(),
    val total: Int = 0,
    val hasMore: Boolean = false,
)

private data class MemorySearchResponse(
    val results: List<MemoryEntry> = emptyList(),
    val total: Int = 0,
)

/**
 * Memories: the durable notes the graph accumulates about this codebase, from
 * agents and from git-history mining.
 *
 * They are invisible without a view like this, which makes them easy to
 * mistrust - you cannot check what you cannot see.
 */
class MemoriesPanel(private val project: Project) : JPanel(BorderLayout()), com.intellij.openapi.Disposable {

    private val gson = Gson()
    private val model = DefaultListModel<MemoryEntry>()
    private val list = JBList(model)
    private val detail = JTextArea().apply {
        isEditable = false
        lineWrap = true
        wrapStyleWord = true
        border = JBUI.Borders.empty(8)
    }
    private val status = JBLabel().apply { border = JBUI.Borders.empty(4, 8) }

    /** Invalidated memories are hidden by default; they are history, not advice. */
    private var showInvalidated = false

    private val search = SearchTextField().apply {
        textEditor.emptyText.text = "Search memories"
        textEditor.addActionListener { reload() }
    }

    init {
        list.selectionMode = ListSelectionModel.SINGLE_SELECTION
        list.cellRenderer = MemoryCellRenderer()
        list.addListSelectionListener {
            if (!it.valueIsAdjusting) showDetail(list.selectedValue)
        }

        val header = JPanel(BorderLayout()).apply {
            add(toolbar(), BorderLayout.WEST)
            add(search, BorderLayout.CENTER)
        }

        val split = JSplitPane(
            JSplitPane.VERTICAL_SPLIT,
            JBScrollPane(list),
            JBScrollPane(detail),
        ).apply { resizeWeight = LIST_WEIGHT }

        add(header, BorderLayout.NORTH)
        add(split, BorderLayout.CENTER)
        add(status, BorderLayout.SOUTH)
    }

    private fun toolbar(): JComponent {
        val group = DefaultActionGroup(
            object : AnAction("Refresh", "Reload memories", AllIcons.Actions.Refresh), DumbAware {
                override fun actionPerformed(e: AnActionEvent) = reload()
            },
            // ToggleAction, not ToggleActionButton: the latter is deprecated
            // and scheduled for removal, and until-build here is unbounded.
            object : ToggleAction("Show Invalidated", "Include memories that have been invalidated", AllIcons.Actions.Show),
                DumbAware {
                override fun getActionUpdateThread(): ActionUpdateThread = ActionUpdateThread.EDT
                override fun isSelected(e: AnActionEvent) = showInvalidated
                override fun setSelected(e: AnActionEvent, state: Boolean) {
                    showInvalidated = state
                    reload()
                }
            },
            object : AnAction("Statistics", "Show memory statistics", AllIcons.Actions.Preview), DumbAware {
                override fun actionPerformed(e: AnActionEvent) = showStats()
            },
        )
        val toolbar = ActionManager.getInstance().createActionToolbar("CodeGraphMemories", group, true)
        toolbar.targetComponent = this
        return toolbar.component
    }

    /**
     * Search and list are different commands with different response shapes, so
     * the query decides which one to call.
     */
    fun reload() {
        val query = search.text.trim()
        setStatus(if (query.isEmpty()) "Loading memories..." else "Searching for \"$query\"...")

        val client = CodeGraphClient.getInstance(project)
        val request = if (query.isEmpty()) {
            client.execute(
                CodeGraphCommand.MEMORY_LIST,
                mapOf("currentOnly" to !showInvalidated, "limit" to PAGE_SIZE),
            ).thenApply { json ->
                runCatching { gson.fromJson(json, MemoryListResponse::class.java) }.getOrNull()
                    ?.let { it.memories to it.total }
            }
        } else {
            client.execute(
                CodeGraphCommand.MEMORY_SEARCH,
                mapOf("query" to query, "limit" to PAGE_SIZE, "currentOnly" to !showInvalidated),
            ).thenApply { json ->
                runCatching { gson.fromJson(json, MemorySearchResponse::class.java) }.getOrNull()
                    ?.let { it.results to it.total }
            }
        }

        request.whenComplete { result, error ->
            // Both commands take currentOnly, so the engine has already applied
            // the filter; re-filtering here would only hide a disagreement.
            val entries = result?.first.orEmpty()
            val total = result?.second ?: 0
            val message = when {
                error != null -> "CodeGraph engine unavailable"
                entries.isEmpty() && query.isNotEmpty() -> "No memories match \"$query\""
                entries.isEmpty() -> "No memories yet. Agents add them as they work, " +
                    "or mine them from git history."
                else -> "${entries.size} of $total"
            }
            ApplicationManager.getApplication().invokeLater {
                model.clear()
                entries.forEach { model.addElement(it) }
                detail.text = ""
                status.text = message
            }
        }
    }

    private fun showDetail(entry: MemoryEntry?) {
        detail.text = entry?.let {
            buildString {
                appendLine(it.title)
                appendLine("=".repeat(it.title.length.coerceAtMost(TITLE_RULE_MAX)))
                appendLine()
                appendLine(it.content)
                appendLine()
                appendLine("Kind: ${it.kind}")
                if (it.tags.isNotEmpty()) appendLine("Tags: ${it.tags.joinToString(", ")}")
                it.agentSource?.let { source -> appendLine("Recorded by: $source") }
                if (!it.isCurrent) appendLine("This memory has been invalidated.")
            }
        }.orEmpty()
        detail.caretPosition = 0
    }

    private fun showStats() {
        CodeGraphClient.getInstance(project)
            .execute(CodeGraphCommand.MEMORY_STATS, emptyMap<String, Any>())
            .whenComplete { json, error ->
                if (error != null) {
                    CodeGraphNotifications.warn(project, "Could not read memory statistics: ${error.message}")
                } else {
                    CodeGraphNotifications.info(project, json?.toString().orEmpty().take(STATS_PREVIEW))
                }
            }
    }

    private fun setStatus(text: String) {
        ApplicationManager.getApplication().invokeLater { status.text = text }
    }

    override fun dispose() = Unit

    private companion object {
        const val PAGE_SIZE = 100
        const val LIST_WEIGHT = 0.6
        const val TITLE_RULE_MAX = 60
        const val STATS_PREVIEW = 500
    }
}

private class MemoryCellRenderer : ColoredListCellRenderer<MemoryEntry>() {
    override fun customizeCellRenderer(
        list: javax.swing.JList<out MemoryEntry>,
        value: MemoryEntry?,
        index: Int,
        selected: Boolean,
        hasFocus: Boolean,
    ) {
        val entry = value ?: return
        icon = if (entry.isCurrent) AllIcons.Nodes.Bookmark else AllIcons.General.Warning
        // Struck through rather than hidden: an invalidated memory that still
        // shows is a signal, and silently rendering it as current would be worse.
        val titleStyle = if (entry.isCurrent) {
            SimpleTextAttributes.REGULAR_ATTRIBUTES
        } else {
            SimpleTextAttributes.GRAYED_ATTRIBUTES
        }
        append(entry.title.ifBlank { "(untitled)" }, titleStyle)
        append("  ${entry.kind}", SimpleTextAttributes.GRAYED_SMALL_ATTRIBUTES)
        if (entry.tags.isNotEmpty()) {
            append("  ${entry.tags.joinToString(" ") { "#$it" }}", SimpleTextAttributes.GRAYED_SMALL_ATTRIBUTES)
        }
    }
}
