// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.vision

import ai.codegraph.jetbrains.settings.CodeGraphSettings
import com.intellij.codeInsight.codeVision.CodeVisionAnchorKind
import com.intellij.codeInsight.codeVision.CodeVisionEntry
import com.intellij.codeInsight.codeVision.CodeVisionRelativeOrdering
import com.intellij.codeInsight.codeVision.ui.model.ClickableTextCodeVisionEntry
import com.intellij.codeInsight.hints.codeVision.DaemonBoundCodeVisionProvider
import com.intellij.openapi.actionSystem.ActionManager
import com.intellij.openapi.actionSystem.ActionPlaces
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiFile
import java.awt.event.MouseEvent

/**
 * Inline graph facts above declarations: how many callers a function has, how
 * many tests reach it, and how complex it is.
 *
 * This is the surface people actually use. Telemetry from the VS Code client
 * showed the inline lenses and tree views getting far more engagement than the
 * agent-facing tools, because they put the graph where someone is already
 * reading code rather than requiring them to go and ask a question.
 */
class CodeGraphCodeVisionProvider : DaemonBoundCodeVisionProvider {

    override val id: String get() = ID

    override val name: String get() = "CodeGraph"

    override val groupId: String get() = ID

    override val defaultAnchor: CodeVisionAnchorKind get() = CodeVisionAnchorKind.Top

    override val relativeOrderings: List<CodeVisionRelativeOrdering>
        get() = listOf(CodeVisionRelativeOrdering.CodeVisionRelativeOrderingLast)

    override fun computeForEditor(editor: Editor, file: PsiFile): List<Pair<TextRange, CodeVisionEntry>> {
        val project = file.project
        if (!CodeGraphSettings.getInstance(project).state.codeLensEnabled) return emptyList()

        val document = editor.document
        // A miss schedules a fetch and restarts the daemon when it lands, so
        // returning nothing here means "not yet", not "nothing to show".
        val symbols = DocumentStatsCache.getInstance(project).get(file, document.modificationStamp)
            ?: return emptyList()

        return symbols.mapNotNull { symbol ->
            val range = lineRange(document, symbol.line) ?: return@mapNotNull null
            entryFor(symbol)?.let { range to it }
        }
    }

    /**
     * One entry per declaration rather than one per statistic: three separate
     * lenses above every function is visual noise in a dense file.
     *
     * Clicking opens the call graph for the file, which is what the counts are
     * a summary of - the VS Code CodeLens does the same. A lens that renders as
     * clickable and does nothing is worse than a plain one.
     */
    private fun entryFor(symbol: CodeLensSymbol): CodeVisionEntry? {
        val parts = buildList {
            if (symbol.callerCount > 0) add("${symbol.callerCount} ${"caller".plural(symbol.callerCount)}")
            if (symbol.testCount > 0) add("${symbol.testCount} ${"test".plural(symbol.testCount)}")
            if (symbol.complexity >= COMPLEXITY_FLOOR) add("complexity ${symbol.complexity}")
        }
        if (parts.isEmpty()) return null

        return ClickableTextCodeVisionEntry(
            parts.joinToString(" · "),
            ID,
            { event, clickedIn -> showCallGraph(event, clickedIn) },
            null,
            parts.joinToString(", "),
            tooltipFor(symbol),
            emptyList(),
        )
    }

    /**
     * Runs the same action as Tools | CodeGraph | Show Call Graph, rather than
     * duplicating its tool-window plumbing here.
     */
    private fun showCallGraph(event: MouseEvent?, clickedIn: Editor) {
        val manager = ActionManager.getInstance()
        val action = manager.getAction(SHOW_CALL_GRAPH_ACTION_ID) ?: return
        // The editor component, not the focus owner: the action reads the
        // current file out of the data context, and an inlay click does not
        // necessarily leave focus where that would resolve.
        manager.tryToExecute(action, event, clickedIn.contentComponent, ActionPlaces.EDITOR_INLAY, true)
    }

    private fun tooltipFor(symbol: CodeLensSymbol): String = buildString {
        append(symbol.name)
        append("\nCallers: ${symbol.callerCount}")
        append("\nTests reaching this: ${symbol.testCount}")
        append("\nCyclomatic complexity: ${symbol.complexity}")
    }

    /**
     * The engine reports 0-based lines. A stale graph can point past the end of
     * a document the user has since shortened, so the bound is checked rather
     * than trusted.
     */
    private fun lineRange(document: com.intellij.openapi.editor.Document, line: Int): TextRange? {
        if (line < 0 || line >= document.lineCount) return null
        return TextRange(document.getLineStartOffset(line), document.getLineEndOffset(line))
    }

    private fun String.plural(count: Int): String = if (count == 1) this else this + "s"

    private companion object {
        const val ID = "CodeGraph"

        /** Declared in `plugin.xml`; the lens runs the action rather than copying it. */
        const val SHOW_CALL_GRAPH_ACTION_ID = "CodeGraph.ShowCallGraph"

        /**
         * Complexity is only worth screen space once it is high enough to be a
         * signal; every small function scoring 1 or 2 would just add noise.
         */
        const val COMPLEXITY_FLOOR = 5
    }
}
