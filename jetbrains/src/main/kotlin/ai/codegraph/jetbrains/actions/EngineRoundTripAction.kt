// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.actions

import ai.codegraph.jetbrains.lsp.CodeGraphClient
import ai.codegraph.jetbrains.lsp.CodeGraphCommand
import ai.codegraph.jetbrains.notify.CodeGraphNotifications
import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.diagnostic.logger

/**
 * Diagnostic action: starts the engine and completes one `executeCommand` round
 * trip, reporting what came back.
 *
 * This is the Phase 0 proof that the LSP4IJ transport carries CodeGraph's
 * command surface unchanged. It stays in the plugin afterwards as the first
 * thing to run when a user reports "CodeGraph does nothing" - it separates
 * "engine never started" from "engine started but returned nothing".
 */
class EngineRoundTripAction : AnAction() {

    override fun getActionUpdateThread(): ActionUpdateThread = ActionUpdateThread.BGT

    override fun update(e: AnActionEvent) {
        e.presentation.isEnabled = e.project != null
    }

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val client = CodeGraphClient.getInstance(project)

        client.start()
        CodeGraphNotifications.info(project, "Starting engine, status: ${client.status()}")

        client.execute(CodeGraphCommand.GET_PARSER_METRICS)
            .thenCompose { metrics ->
                val parsers = metrics?.takeIf { it.isJsonObject }?.asJsonObject?.size() ?: 0
                CodeGraphNotifications.info(project, "Engine replied: $parsers parser metric groups")
                client.execute(
                    CodeGraphCommand.SYMBOL_SEARCH,
                    mapOf("query" to "main", "limit" to 5),
                )
            }
            .whenComplete { symbols, error ->
                if (error != null) {
                    LOG.warn("Engine round trip failed", error)
                    CodeGraphNotifications.error(
                        project,
                        "Engine round trip failed: ${error.message ?: error::class.java.simpleName}",
                    )
                } else {
                    CodeGraphNotifications.info(project, "symbolSearch returned: ${summarize(symbols?.toString())}")
                }
            }
    }

    private fun summarize(raw: String?): String = when {
        raw == null -> "null"
        raw.length <= MAX_PREVIEW -> raw
        else -> raw.take(MAX_PREVIEW) + "..."
    }

    private companion object {
        val LOG = logger<EngineRoundTripAction>()
        const val MAX_PREVIEW = 400
    }
}
