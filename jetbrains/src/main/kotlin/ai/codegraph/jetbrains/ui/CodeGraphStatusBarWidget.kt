// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.ui

import ai.codegraph.jetbrains.lsp.CodeGraphClient
import ai.codegraph.jetbrains.server.EngineLifecycle
import ai.codegraph.jetbrains.server.ServerEdition
import ai.codegraph.jetbrains.settings.CodeGraphSettings
import com.intellij.openapi.project.DumbAware
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.Disposer
import com.intellij.openapi.wm.StatusBar
import com.intellij.openapi.wm.StatusBarWidget
import com.intellij.openapi.wm.StatusBarWidgetFactory
import com.redhat.devtools.lsp4ij.ServerStatus
import java.awt.event.MouseEvent

/**
 * Status bar entry showing whether the engine is up and which edition is
 * running.
 *
 * The engine is an out-of-process dependency the user never sees, so when it is
 * not running every CodeGraph surface is simply empty with no explanation.
 * This is the one always-visible place that distinguishes "no results" from
 * "nothing is running".
 */
class CodeGraphStatusBarWidgetFactory : StatusBarWidgetFactory {

    override fun getId(): String = WIDGET_ID

    override fun getDisplayName(): String = "CodeGraph"

    override fun isAvailable(project: Project): Boolean =
        CodeGraphSettings.getInstance(project).state.enabled

    override fun createWidget(project: Project): StatusBarWidget = CodeGraphStatusBarWidget(project)

    override fun disposeWidget(widget: StatusBarWidget) = Disposer.dispose(widget)

    override fun canBeEnabledOn(statusBar: StatusBar): Boolean = true

    private companion object {
        const val WIDGET_ID = "CodeGraphStatusBar"
    }
}

/**
 * Every accessor here runs on the EDT during repaint, so all of them read
 * cached state only. Resolving the engine walks PATH and stats several files;
 * doing that per repaint would be filesystem I/O on the UI thread.
 */
private class CodeGraphStatusBarWidget(private val project: Project) :
    StatusBarWidget,
    StatusBarWidget.TextPresentation,
    DumbAware {

    override fun ID(): String = "CodeGraphStatusBar"

    override fun getPresentation(): StatusBarWidget.WidgetPresentation = this

    override fun install(statusBar: StatusBar) = Unit

    override fun dispose() = Unit

    override fun getAlignment(): Float = 0f

    override fun getText(): String {
        val lifecycle = EngineLifecycle.getInstance(project)
        val edition = lifecycle.resolvedServer
            ?.takeIf { it.edition == ServerEdition.PRO }
            ?.let { " Pro" }
            .orEmpty()
        val state = if (lifecycle.isRestartBlocked) {
            "stopped after repeated crashes"
        } else {
            describe(CodeGraphClient.getInstance(project).status())
        }
        return "CodeGraph$edition: $state"
    }

    override fun getTooltipText(): String {
        val lifecycle = EngineLifecycle.getInstance(project)
        if (lifecycle.isRestartBlocked) {
            return "The CodeGraph engine crashed repeatedly and will not restart automatically. " +
                "Use Tools | CodeGraph | Check Engine Connection to try again."
        }
        val resolved = lifecycle.resolvedServer
            ?: return "The CodeGraph engine has not started yet."

        return buildString {
            append("Engine: ${resolved.path}")
            append("\nEdition: ${resolved.edition.name.lowercase()}")
            append("\nFound via: ${resolved.origin.name.lowercase().replace('_', ' ')}")
            if (lifecycle.restartCount > 0) append("\nRestarts this session: ${lifecycle.restartCount}")
        }
    }

    override fun getClickConsumer(): com.intellij.util.Consumer<MouseEvent>? = null

    /**
     * LSP4IJ's status names are transport-level. Users care about whether the
     * graph can answer questions, so they are collapsed to that.
     */
    private fun describe(status: ServerStatus): String = when (status) {
        ServerStatus.started -> "ready"
        ServerStatus.starting -> "starting"
        ServerStatus.stopping -> "stopping"
        ServerStatus.stopped, ServerStatus.none -> "not running"
        ServerStatus.installing, ServerStatus.checking_installed -> "installing"
        ServerStatus.installed -> "ready to start"
        ServerStatus.not_installed -> "not installed"
    }
}
