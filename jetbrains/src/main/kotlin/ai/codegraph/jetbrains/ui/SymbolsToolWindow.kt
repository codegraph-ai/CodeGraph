// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.ui

import ai.codegraph.jetbrains.lsp.CodeGraphClient
import ai.codegraph.jetbrains.lsp.CodeGraphCommand
import com.google.gson.Gson
import com.intellij.icons.AllIcons
import com.intellij.openapi.actionSystem.ActionManager
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.DefaultActionGroup
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.fileEditor.OpenFileDescriptor
import com.intellij.openapi.project.DumbAware
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.Disposer
import com.intellij.openapi.vfs.VirtualFileManager
import com.intellij.openapi.wm.ToolWindow
import com.intellij.openapi.wm.ToolWindowFactory
import com.intellij.ui.SearchTextField
import com.intellij.ui.SimpleTextAttributes
import com.intellij.ui.components.JBLabel
import com.intellij.ui.content.ContentFactory
import com.intellij.ui.treeStructure.SimpleTree
import com.intellij.util.ui.JBUI
import java.awt.BorderLayout
import java.awt.event.MouseAdapter
import java.awt.event.MouseEvent
import javax.swing.JPanel
import javax.swing.JComponent
import javax.swing.ScrollPaneConstants
import javax.swing.JScrollPane
import javax.swing.tree.DefaultMutableTreeNode
import javax.swing.tree.DefaultTreeModel
import javax.swing.tree.TreeSelectionModel

/** One symbol as the engine reports it for the tree view. */
data class SymbolInfo(
    val id: String = "",
    val name: String = "",
    val kind: String = "",
    val language: String = "",
    val uri: String = "",
    val range: SymbolRange? = null,
    val children: List<SymbolInfo>? = null,
)

data class SymbolRange(val start: SymbolPosition? = null, val end: SymbolPosition? = null)

data class SymbolPosition(val line: Int = 0, val character: Int = 0)

private data class WorkspaceSymbolsResponse(val symbols: List<SymbolInfo> = emptyList())

/**
 * The Symbols tool window: the workspace graph as a navigable tree.
 *
 * Along with the inline lenses this is where non-agent usage concentrates, so
 * it is worth more than the agent-facing command surface despite being simpler.
 */
class SymbolsToolWindowFactory : ToolWindowFactory, DumbAware {

    override fun createToolWindowContent(project: Project, toolWindow: ToolWindow) {
        val factory = ContentFactory.getInstance()

        val symbols = SymbolsPanel(project)
        Disposer.register(toolWindow.disposable, symbols)
        toolWindow.contentManager.addContent(factory.createContent(symbols, "Symbols", false))

        // Memories share the tool window rather than claiming their own slot in
        // the sidebar: they are the same graph seen from a different angle, and
        // two CodeGraph icons would be two things to learn.
        val memories = MemoriesPanel(project)
        Disposer.register(toolWindow.disposable, memories)
        toolWindow.contentManager.addContent(factory.createContent(memories, "Memories", false))

        symbols.refresh()
        memories.reload()
    }
}

private class SymbolsPanel(private val project: Project) : JPanel(BorderLayout()), com.intellij.openapi.Disposable {

    private val gson = Gson()
    private val root = DefaultMutableTreeNode("Workspace")
    private val model = DefaultTreeModel(root)
    private val tree = SimpleTree(model)
    private val status = JBLabel().apply { border = JBUI.Borders.empty(4, 8) }

    /**
     * Searches on Enter rather than on every keystroke: each query is a round
     * trip to the engine over a graph that can hold hundreds of thousands of
     * symbols.
     */
    private val search = SearchTextField().apply {
        textEditor.emptyText.text = "Search symbols"
        textEditor.addActionListener { refresh(text.trim()) }
    }

    init {
        tree.isRootVisible = false
        tree.selectionModel.selectionMode = TreeSelectionModel.SINGLE_TREE_SELECTION
        tree.cellRenderer = SymbolCellRenderer()
        tree.addMouseListener(object : MouseAdapter() {
            override fun mouseClicked(event: MouseEvent) {
                if (event.clickCount == 2) navigateToSelection()
            }
        })

        val header = JPanel(BorderLayout()).apply {
            add(toolbar(), BorderLayout.WEST)
            add(search, BorderLayout.CENTER)
        }
        add(header, BorderLayout.NORTH)
        add(JScrollPane(tree).apply {
            horizontalScrollBarPolicy = ScrollPaneConstants.HORIZONTAL_SCROLLBAR_AS_NEEDED
        }, BorderLayout.CENTER)
        add(status, BorderLayout.SOUTH)
    }

    private fun toolbar(): JComponent {
        val group = DefaultActionGroup(
            object : AnAction("Refresh", "Reload symbols from the graph", AllIcons.Actions.Refresh), DumbAware {
                override fun actionPerformed(e: AnActionEvent) = refresh(search.text.trim())
            },
        )
        val toolbar = ActionManager.getInstance().createActionToolbar("CodeGraphSymbols", group, true)
        toolbar.targetComponent = this
        return toolbar.component
    }

    fun refresh(filter: String = "") {
        setStatus("Loading symbols...")
        // The query key must be *absent* for the unfiltered view. The engine
        // treats a missing query as "functions, classes and modules" but an
        // empty string as "modules only", so sending "" yields an empty tree on
        // a perfectly healthy index.
        val arguments = if (filter.isBlank()) emptyMap() else mapOf("query" to filter)
        CodeGraphClient.getInstance(project)
            .execute(CodeGraphCommand.GET_WORKSPACE_SYMBOLS, arguments)
            .whenComplete { json, error ->
                val symbols = if (error != null) {
                    emptyList()
                } else {
                    runCatching { gson.fromJson(json, WorkspaceSymbolsResponse::class.java)?.symbols }
                        .getOrNull().orEmpty()
                }
                val message = when {
                    error != null -> "CodeGraph engine unavailable"
                    symbols.isEmpty() -> "No symbols yet - index this workspace to populate the graph"
                    else -> "${symbols.size} top-level ${"symbol".plural(symbols.size)}"
                }
                // Swing model mutation belongs on the EDT; this callback runs on
                // whichever thread completed the LSP future.
                ApplicationManager.getApplication().invokeLater {
                    root.removeAllChildren()
                    symbols.forEach { root.add(nodeFor(it)) }
                    model.reload()
                    setStatus(message)
                }
            }
    }

    private fun nodeFor(symbol: SymbolInfo): DefaultMutableTreeNode {
        val node = DefaultMutableTreeNode(symbol)
        symbol.children?.forEach { node.add(nodeFor(it)) }
        return node
    }

    private fun navigateToSelection() {
        val symbol = (tree.lastSelectedPathComponent as? DefaultMutableTreeNode)?.userObject as? SymbolInfo ?: return
        // The fallback parses the URI itself, and both steps throw on anything
        // malformed or non-`file:`. This runs on the EDT from a double-click,
        // so an escape surfaces as an IDE error dialog instead of the status
        // message the fallback exists to produce.
        val file = VirtualFileManager.getInstance().findFileByUrl(symbol.uri)
            ?: runCatching {
                VirtualFileManager.getInstance().findFileByNioPath(
                    java.nio.file.Paths.get(java.net.URI.create(symbol.uri)),
                )
            }.getOrNull()
            ?: run {
                setStatus("Cannot open ${symbol.uri}")
                return
            }
        val line = symbol.range?.start?.line ?: 0
        val column = symbol.range?.start?.character ?: 0
        OpenFileDescriptor(project, file, line, column).navigate(true)
    }

    private fun setStatus(text: String) {
        ApplicationManager.getApplication().invokeLater { status.text = text }
    }

    private fun String.plural(count: Int): String = if (count == 1) this else this + "s"

    override fun dispose() = Unit
}

private class SymbolCellRenderer : com.intellij.ui.ColoredTreeCellRenderer() {
    override fun customizeCellRenderer(
        tree: javax.swing.JTree,
        value: Any?,
        selected: Boolean,
        expanded: Boolean,
        leaf: Boolean,
        row: Int,
        hasFocus: Boolean,
    ) {
        val symbol = (value as? DefaultMutableTreeNode)?.userObject as? SymbolInfo
        if (symbol == null) {
            append(value?.toString().orEmpty())
            return
        }
        icon = iconFor(symbol.kind)
        append(symbol.name)
        append("  ${symbol.language} ${symbol.kind.lowercase()}", SimpleTextAttributes.GRAYED_ATTRIBUTES)
    }

    private fun iconFor(kind: String) = when (kind.lowercase()) {
        "function", "method" -> AllIcons.Nodes.Method
        "class", "struct" -> AllIcons.Nodes.Class
        "interface", "trait" -> AllIcons.Nodes.Interface
        "module", "file" -> AllIcons.Nodes.Module
        "variable", "field", "constant" -> AllIcons.Nodes.Field
        "enum" -> AllIcons.Nodes.Enum
        else -> AllIcons.Nodes.Unknown
    }
}
