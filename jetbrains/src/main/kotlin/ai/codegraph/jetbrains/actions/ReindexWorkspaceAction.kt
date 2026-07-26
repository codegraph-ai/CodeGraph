// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.actions

import ai.codegraph.jetbrains.indexing.IndexingService
import ai.codegraph.jetbrains.lsp.CodeGraphClient
import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent

/** Rebuild the workspace graph from scratch. */
class ReindexWorkspaceAction : AnAction() {

    override fun getActionUpdateThread(): ActionUpdateThread = ActionUpdateThread.BGT

    override fun update(e: AnActionEvent) {
        e.presentation.isEnabled = e.project != null
    }

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        // Reindexing is the usual reason a user reaches for this after the
        // engine died, so make sure it is running rather than failing the
        // command on a stopped engine.
        CodeGraphClient.getInstance(project).start()
        IndexingService.getInstance(project).reindexInBackground()
    }
}
