// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.notify

import com.intellij.notification.Notification
import com.intellij.notification.NotificationAction
import com.intellij.notification.NotificationGroupManager
import com.intellij.notification.NotificationType
import com.intellij.openapi.project.Project

/**
 * User-facing notifications.
 *
 * Every helper here is fire-and-forget by construction: nothing returns a
 * future and nothing waits on a button. Agent-driven code paths hit the same
 * functions as interactive ones, and a notification that blocks on user input
 * turns a tool call into a hang.
 */
object CodeGraphNotifications {
    private const val GROUP_ID = "CodeGraph"

    fun info(project: Project, message: String) = notify(project, message, NotificationType.INFORMATION)

    fun warn(project: Project, message: String) = notify(project, message, NotificationType.WARNING)

    fun error(project: Project, message: String) = notify(project, message, NotificationType.ERROR)

    fun infoWithActions(
        project: Project,
        message: String,
        vararg actions: Pair<String, (Notification) -> Unit>,
    ) = withActions(project, message, NotificationType.INFORMATION, *actions)

    fun errorWithActions(
        project: Project,
        message: String,
        vararg actions: Pair<String, (Notification) -> Unit>,
    ) = withActions(project, message, NotificationType.ERROR, *actions)

    /**
     * A notification carrying buttons.
     *
     * Still fire-and-forget: this returns as soon as the balloon is posted, and
     * each action runs later on its own. Callers must not treat an action as a
     * reply they can wait for.
     */
    private fun withActions(
        project: Project,
        message: String,
        type: NotificationType,
        vararg actions: Pair<String, (Notification) -> Unit>,
    ) {
        val notification = NotificationGroupManager.getInstance()
            .getNotificationGroup(GROUP_ID)
            .createNotification("CodeGraph", message, type)
        actions.forEach { (label, handler) ->
            notification.addAction(
                NotificationAction.create(label) { _, shown -> handler(shown) },
            )
        }
        notification.notify(project)
    }

    private fun notify(project: Project, message: String, type: NotificationType) {
        NotificationGroupManager.getInstance()
            .getNotificationGroup(GROUP_ID)
            .createNotification("CodeGraph", message, type)
            .notify(project)
    }
}
