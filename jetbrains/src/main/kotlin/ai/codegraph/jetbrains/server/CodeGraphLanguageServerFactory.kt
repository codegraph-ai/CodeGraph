// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.server

import com.intellij.openapi.project.Project
import com.redhat.devtools.lsp4ij.LanguageServerFactory
import com.redhat.devtools.lsp4ij.client.LanguageClientImpl
import com.redhat.devtools.lsp4ij.server.StreamConnectionProvider

/** Server id shared by `plugin.xml` and every call site that talks to the engine. */
const val CODEGRAPH_SERVER_ID: String = "codegraph"

/** Wires the CodeGraph engine into LSP4IJ. */
class CodeGraphLanguageServerFactory : LanguageServerFactory {

    override fun createConnectionProvider(project: Project): StreamConnectionProvider =
        CodeGraphConnectionProvider(project)

    override fun createLanguageClient(project: Project): LanguageClientImpl =
        CodeGraphLanguageClient(project)
}

/**
 * Client-side LSP endpoint.
 *
 * Kept deliberately thin for now. Phase 2 overrides [refreshCodeLenses] here so
 * the engine can invalidate Code Vision after a reindex, mirroring
 * `codeLensRefresh.ts` in the VS Code client.
 */
class CodeGraphLanguageClient(project: Project) : LanguageClientImpl(project)
