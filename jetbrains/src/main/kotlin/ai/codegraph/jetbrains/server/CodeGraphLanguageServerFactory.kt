// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.server

import ai.codegraph.jetbrains.settings.CodeGraphSettings
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiFile
import com.redhat.devtools.lsp4ij.LanguageServerFactory
import com.redhat.devtools.lsp4ij.client.LanguageClientImpl
import com.redhat.devtools.lsp4ij.client.features.LSPClientFeatures
import com.redhat.devtools.lsp4ij.client.features.LSPHoverFeature
import com.redhat.devtools.lsp4ij.server.StreamConnectionProvider

/** Server id shared by `plugin.xml` and every call site that talks to the engine. */
const val CODEGRAPH_SERVER_ID: String = "codegraph"

/** Wires the CodeGraph engine into LSP4IJ. */
class CodeGraphLanguageServerFactory : LanguageServerFactory {

    override fun createConnectionProvider(project: Project): StreamConnectionProvider =
        CodeGraphConnectionProvider(project)

    override fun createLanguageClient(project: Project): LanguageClientImpl =
        CodeGraphLanguageClient(project)

    /**
     * The engine advertises `hoverProvider`, so LSP4IJ shows graph information
     * on hover by default. Binding that to the setting is what makes the
     * "Show graph information on hover" checkbox mean anything - without it the
     * hover is on regardless of what the user chose.
     */
    override fun createClientFeatures(): LSPClientFeatures =
        LSPClientFeatures().setHoverFeature(CodeGraphHoverFeature())
}

private class CodeGraphHoverFeature : LSPHoverFeature() {
    override fun isEnabled(file: PsiFile): Boolean =
        CodeGraphSettings.getInstance(file.project).state.hoverEnabled && super.isEnabled(file)
}

/**
 * Client-side LSP endpoint.
 *
 * Kept deliberately thin for now. Phase 2 overrides [refreshCodeLenses] here so
 * the engine can invalidate Code Vision after a reindex, mirroring
 * `codeLensRefresh.ts` in the VS Code client.
 */
class CodeGraphLanguageClient(project: Project) : LanguageClientImpl(project)
