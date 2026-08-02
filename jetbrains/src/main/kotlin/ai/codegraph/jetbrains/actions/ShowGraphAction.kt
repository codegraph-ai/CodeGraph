// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.actions

import ai.codegraph.jetbrains.graph.GraphKind
import ai.codegraph.jetbrains.graph.GraphPanel
import ai.codegraph.jetbrains.lsp.CodeGraphClient
import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.Disposer
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.openapi.wm.ToolWindowManager
import com.intellij.ui.content.ContentFactory

/**
 * Opens a graph for the current file in a tab of the CodeGraph tool window.
 *
 * A tool window tab rather than an editor tab: the graph is a companion to the
 * code you are reading, and putting it in the editor area means it competes
 * with the file it describes.
 */
sealed class ShowGraphAction(private val kind: GraphKind) : AnAction() {

    override fun getActionUpdateThread(): ActionUpdateThread = ActionUpdateThread.BGT

    override fun update(e: AnActionEvent) {
        e.presentation.isEnabled = e.project != null && currentFile(e) != null
    }

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val file = currentFile(e) ?: return

        CodeGraphClient.getInstance(project).start()

        val toolWindow = ToolWindowManager.getInstance(project).getToolWindow(TOOL_WINDOW_ID) ?: return
        val contentManager = toolWindow.contentManager
        val label = "${kind.title}: ${file.name}"

        // This is reachable from a lens above every declaration, not just from
        // the Tools menu, so adding a tab per invocation means a handful of
        // clicks in one file leaves a row of identical tabs, each holding its
        // own JCEF browser. The same graph of the same file is one tab.
        //
        // Matched on the file's URL rather than the tab label, which is only
        // the file name: two `index.ts` in different directories are different
        // graphs and must not quietly replace one another.
        val existing = contentManager.contents.firstOrNull { content ->
            val panel = content.component as? GraphPanel
            panel != null && panel.kind == kind && panel.fileUri == file.url
        }
        if (existing != null) {
            contentManager.setSelectedContent(existing)
            toolWindow.show()
            (existing.component as GraphPanel).load(file.url)
            return
        }

        val panel = GraphPanel(project, kind)
        val content = ContentFactory.getInstance().createContent(panel, label, true).apply {
            isCloseable = true
            setDisposer(panel)
        }
        Disposer.register(toolWindow.disposable, panel)

        contentManager.addContent(content)
        contentManager.setSelectedContent(content)
        toolWindow.show()

        panel.load(file.url)
    }

    private fun currentFile(e: AnActionEvent): VirtualFile? =
        e.getData(CommonDataKeys.VIRTUAL_FILE)?.takeIf { !it.isDirectory }

    private companion object {
        const val TOOL_WINDOW_ID = "CodeGraph"
    }
}

class ShowDependencyGraphAction : ShowGraphAction(GraphKind.DEPENDENCIES)

class ShowCallGraphAction : ShowGraphAction(GraphKind.CALLS)
