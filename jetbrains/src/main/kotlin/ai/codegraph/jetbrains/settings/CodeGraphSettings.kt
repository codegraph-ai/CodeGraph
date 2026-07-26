// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.settings

import com.intellij.openapi.components.PersistentStateComponent
import com.intellij.openapi.components.Service
import com.intellij.openapi.components.State
import com.intellij.openapi.components.Storage
import com.intellij.openapi.components.service
import com.intellij.openapi.project.Project
import com.intellij.util.xmlb.XmlSerializerUtil

/**
 * Project-scoped CodeGraph settings.
 *
 * These mirror the `codegraph.*` keys in `vscode/package.json`. Indexing scope
 * is inherently per-project, so the whole set is stored at project level rather
 * than split across application/project scopes.
 *
 * Only the keys the engine actually consumes at `initialize` time live here so
 * far; the remaining VS Code keys land with the settings UI in Phase 1.
 */
@Service(Service.Level.PROJECT)
@State(name = "CodeGraphSettings", storages = [Storage("codegraph.xml")])
class CodeGraphSettings : PersistentStateComponent<CodeGraphSettings.State> {

    /**
     * Mutable state bag. Kept as plain JVM types with public fields because
     * [XmlSerializerUtil] serialises fields, not Kotlin properties with custom
     * accessors.
     */
    class State {
        @JvmField var enabled: Boolean = true

        /** Explicit engine binary path; empty means "resolve automatically". */
        @JvmField var serverPath: String = ""

        /**
         * Off by default, matching the VS Code client and the engine itself.
         * Turning it on makes the engine index during `initialize`, which races
         * the "not indexed yet" prompt and can index the workspace twice.
         */
        @JvmField var indexOnStartup: Boolean = false
        @JvmField var excludePatterns: MutableList<String> = mutableListOf(
            "**/node_modules/**",
            "**/target/**",
            "**/.git/**",
            "**/dist/**",
            "**/build/**",
            "**/__pycache__/**",
            "**/venv/**",
            "**/.venv/**",
        )
        @JvmField var indexPaths: MutableList<String> = mutableListOf()
        @JvmField var maxFileSizeKB: Int = 1024

        /** One of `bge-small`, `granite-97m`, `static`. */
        @JvmField var embeddingModel: String = "bge-small"

        /** Overrides the bundled model directory when [embeddingModel] is `static`. */
        @JvmField var staticModelPath: String = ""

        /**
         * Embed whole symbol bodies rather than signatures. Must default to
         * true: duplicate detection, clustering and similarity search all
         * degrade badly without it.
         */
        @JvmField var fullBodyEmbedding: Boolean = true

        @JvmField var embedOnOpen: Boolean = true

        @JvmField var codeLensEnabled: Boolean = true
        @JvmField var hoverEnabled: Boolean = true

        @JvmField var telemetryEnabled: Boolean = true
        @JvmField var telemetryErrorReportsOnly: Boolean = false

        @JvmField var debug: Boolean = false
    }

    private var state = State()

    override fun getState(): State = state

    override fun loadState(loaded: State) {
        XmlSerializerUtil.copyBean(loaded, state)
    }

    companion object {
        fun getInstance(project: Project): CodeGraphSettings = project.service()
    }
}
