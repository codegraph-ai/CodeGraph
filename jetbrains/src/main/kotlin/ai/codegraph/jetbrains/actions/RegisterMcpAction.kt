// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.actions

import ai.codegraph.jetbrains.mcp.McpRegistration
import ai.codegraph.jetbrains.notify.CodeGraphNotifications
import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.ide.CopyPasteManager
import com.intellij.openapi.vfs.LocalFileSystem
import java.awt.datatransfer.StringSelection

/**
 * Points the IDE's AI tooling at the CodeGraph engine over MCP.
 *
 * Writing `.mcp.json` covers the clients that read it from the project root.
 * The config is also offered on the clipboard, because MCP configuration lives
 * in a different place in every AI client and pasting it is the one path that
 * always works.
 */
class RegisterMcpAction : AnAction() {

    override fun getActionUpdateThread(): ActionUpdateThread = ActionUpdateThread.BGT

    override fun update(e: AnActionEvent) {
        val project = e.project
        e.presentation.isEnabled = project != null
        e.presentation.text = if (project != null && McpRegistration.isRegistered(project)) {
            "Update AI Assistant Registration"
        } else {
            "Register with AI Assistant"
        }
    }

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return

        when (val result = McpRegistration.register(project)) {
            is McpRegistration.Result.Written -> {
                LocalFileSystem.getInstance().refreshAndFindFileByNioFile(result.path)
                val note = if (result.merged) " alongside the servers already configured there" else ""
                // Silently replacing a config we could not parse would lose
                // whatever else was in it, so say what happened to it.
                val rescued = result.backup?.let {
                    " The previous file could not be parsed and was kept as <code>${it.fileName}</code>."
                }.orEmpty()
                CodeGraphNotifications.infoWithActions(
                    project,
                    "CodeGraph is registered as an MCP server in <code>${result.path.fileName}</code>$note. " +
                        "Restart your AI client to pick it up.$rescued",
                    "Copy Config" to { notification ->
                        notification.expire()
                        copyConfig(e)
                    },
                )
            }

            is McpRegistration.Result.NoEngine ->
                CodeGraphNotifications.warn(project, result.reason)

            is McpRegistration.Result.Failed ->
                CodeGraphNotifications.error(project, "Could not write the MCP config: ${result.reason}")
        }
    }

    private fun copyConfig(e: AnActionEvent) {
        val project = e.project ?: return
        val snippet = McpRegistration.configSnippet(project) ?: return
        CopyPasteManager.getInstance().setContents(StringSelection(snippet))
        CodeGraphNotifications.info(project, "MCP configuration copied to the clipboard.")
    }
}
