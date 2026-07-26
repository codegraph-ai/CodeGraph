// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.settings

import ai.codegraph.jetbrains.lsp.CodeGraphClient
import ai.codegraph.jetbrains.lsp.CodeGraphCommand
import com.intellij.openapi.diagnostic.logger
import com.intellij.openapi.options.BoundConfigurable
import com.intellij.openapi.project.Project
import com.intellij.openapi.ui.DialogPanel
import com.intellij.ui.dsl.builder.bindIntText
import com.intellij.ui.dsl.builder.bindItem
import com.intellij.ui.dsl.builder.bindSelected
import com.intellij.ui.dsl.builder.bindText
import com.intellij.ui.dsl.builder.columns
import com.intellij.ui.dsl.builder.panel

/**
 * Settings | Tools | CodeGraph.
 *
 * Mirrors the `codegraph.*` keys the VS Code client exposes so a user moving
 * between editors finds the same knobs under the same names.
 */
class CodeGraphConfigurable(private val project: Project) : BoundConfigurable(DISPLAY_NAME) {

    private val state get() = CodeGraphSettings.getInstance(project).state

    override fun createPanel(): DialogPanel = panel {
        group("Engine") {
            row {
                checkBox("Enable CodeGraph")
                    .bindSelected(state::enabled)
            }
            row("Engine path:") {
                textFieldWithBrowseButton()
                    .columns(COLUMNS_WIDE)
                    .bindText(state::serverPath)
                    .comment(
                        "Leave empty to resolve automatically: CodeGraph Pro, then PATH, " +
                            "then a downloaded engine under ~/.codegraph/bin.",
                    )
            }
        }

        group("Indexing") {
            row {
                checkBox("Index the workspace on startup")
                    .bindSelected(state::indexOnStartup)
            }
            row("Exclude patterns:") {
                expandableTextField({ text -> splitList(text) }, { values -> joinList(values) })
                    .columns(COLUMNS_WIDE)
                    .bindText(
                        getter = { joinList(state.excludePatterns) },
                        setter = { text -> state.excludePatterns = splitList(text) },
                    )
                    .comment("Comma-separated globs.")
            }
            row("Index only these paths:") {
                expandableTextField({ text -> splitList(text) }, { values -> joinList(values) })
                    .columns(COLUMNS_WIDE)
                    .bindText(
                        getter = { joinList(state.indexPaths) },
                        setter = { text -> state.indexPaths = splitList(text) },
                    )
                    .comment("Comma-separated. Empty means the whole workspace.")
            }
            row("Maximum file size (KB):") {
                intTextField(range = MIN_FILE_SIZE_KB..MAX_FILE_SIZE_KB)
                    .bindIntText(state::maxFileSizeKB)
            }
        }

        group("Embeddings") {
            row("Model:") {
                comboBox(EMBEDDING_MODELS)
                    .bindItem(
                        getter = { state.embeddingModel },
                        setter = { value -> state.embeddingModel = value ?: DEFAULT_EMBEDDING_MODEL },
                    )
            }
            row("Static model directory:") {
                textFieldWithBrowseButton()
                    .columns(COLUMNS_WIDE)
                    .bindText(state::staticModelPath)
                    .comment("Only used when the model is set to <code>static</code>.")
            }
            row {
                checkBox("Embed whole symbol bodies")
                    .bindSelected(state::fullBodyEmbedding)
                    .comment(
                        "Turning this off degrades duplicate detection, clustering and " +
                            "similarity search. Leave it on unless indexing time is a problem.",
                    )
            }
            row {
                checkBox("Embed files as they are opened")
                    .bindSelected(state::embedOnOpen)
            }
        }

        group("Editor") {
            row {
                checkBox("Show graph information above declarations")
                    .bindSelected(state::codeLensEnabled)
            }
            row {
                checkBox("Show graph information on hover")
                    .bindSelected(state::hoverEnabled)
            }
        }

        group("Diagnostics") {
            row {
                checkBox("Send anonymous usage data")
                    .bindSelected(state::telemetryEnabled)
            }
            row {
                checkBox("Send error reports only")
                    .bindSelected(state::telemetryErrorReportsOnly)
            }
            row {
                checkBox("Verbose logging")
                    .bindSelected(state::debug)
            }
        }
    }

    /**
     * Push the new configuration to a running engine.
     *
     * Some settings, such as the embedding model, are only read at
     * `initialize`, so this covers the ones the engine can adopt live and the
     * rest take effect on the next start.
     */
    override fun apply() {
        super.apply()

        val client = CodeGraphClient.getInstance(project)
        val updated = mapOf(
            "indexOnStartup" to state.indexOnStartup,
            "excludePatterns" to state.excludePatterns.toList(),
            "indexPaths" to state.indexPaths.toList(),
            "maxFileSizeKB" to state.maxFileSizeKB,
            "embedOnOpen" to state.embedOnOpen,
        )
        client.execute(CodeGraphCommand.UPDATE_CONFIGURATION, updated)
            .whenComplete { _, error ->
                // The engine simply may not be running, which is not worth
                // interrupting someone who just clicked OK in a settings dialog.
                if (error != null) LOG.info("Could not push CodeGraph settings to the engine: ${error.message}")
            }
    }

    private companion object {
        val LOG = logger<CodeGraphConfigurable>()

        const val DISPLAY_NAME = "CodeGraph"
        const val COLUMNS_WIDE = 40
        const val MIN_FILE_SIZE_KB = 1
        const val MAX_FILE_SIZE_KB = 1024 * 64
        const val DEFAULT_EMBEDDING_MODEL = "bge-small"

        val EMBEDDING_MODELS = listOf("bge-small", "granite-97m", "static")

        fun joinList(values: List<String>): String = values.joinToString(", ")

        /** Returns a MutableList because the platform's SAM type demands one. */
        fun splitList(text: String): MutableList<String> =
            text.split(',').map { it.trim() }.filter { it.isNotEmpty() }.toMutableList()
    }
}
