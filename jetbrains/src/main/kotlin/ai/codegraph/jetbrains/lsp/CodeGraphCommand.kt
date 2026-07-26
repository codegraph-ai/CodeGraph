// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.lsp

/**
 * The engine's `workspace/executeCommand` surface.
 *
 * Authority is `CodeGraphBackend::execute_command` in
 * `crates/codegraph-server/src/backend.rs`; this enum is a transcription of the
 * dispatch arms there. Transcription means drift, so it is on the roadmap to
 * generate both this file and the VS Code client's equivalent from a
 * `--dump-capabilities` output of the engine itself.
 *
 * The engine accepts an alternative command prefix and remaps it internally, so
 * ids are always spelled `codegraph.*` here.
 */
enum class CodeGraphCommand(val id: String) {
    // Graph structure
    GET_DEPENDENCY_GRAPH("codegraph.getDependencyGraph"),
    GET_CALL_GRAPH("codegraph.getCallGraph"),
    TRAVERSE_GRAPH("codegraph.traverseGraph"),
    GET_CALLERS("codegraph.getCallers"),
    GET_CALLEES("codegraph.getCallees"),
    ANALYZE_IMPACT("codegraph.analyzeImpact"),
    FIND_IMPLEMENTORS("codegraph.findImplementors"),
    FIND_ENTRY_POINTS("codegraph.findEntryPoints"),

    // Symbols and search
    SYMBOL_SEARCH("codegraph.symbolSearch"),
    GET_WORKSPACE_SYMBOLS("codegraph.getWorkspaceSymbols"),
    GET_DETAILED_SYMBOL_INFO("codegraph.getDetailedSymbolInfo"),
    GET_NODE_LOCATION("codegraph.getNodeLocation"),
    FIND_BY_IMPORTS("codegraph.findByImports"),
    FIND_BY_SIGNATURE("codegraph.findBySignature"),
    FIND_RELATED_TESTS("codegraph.findRelatedTests"),
    ANALYZE_COMPLEXITY("codegraph.analyzeComplexity"),

    // Editor surfaces
    GET_DOCUMENT_CODE_LENS("codegraph.getDocumentCodeLens"),

    // AI context
    GET_AI_CONTEXT("codegraph.getAIContext"),
    GET_EDIT_CONTEXT("codegraph.getEditContext"),
    GET_CURATED_CONTEXT("codegraph.getCuratedContext"),

    // Memory
    MEMORY_STORE("codegraph.memoryStore"),
    MEMORY_SEARCH("codegraph.memorySearch"),
    MEMORY_GET("codegraph.memoryGet"),
    MEMORY_UPDATE("codegraph.memoryUpdate"),
    MEMORY_INVALIDATE("codegraph.memoryInvalidate"),
    MEMORY_LIST("codegraph.memoryList"),
    MEMORY_CONTEXT("codegraph.memoryContext"),
    MEMORY_STATS("codegraph.memoryStats"),

    // Git mining
    MINE_GIT_HISTORY("codegraph.mineGitHistory"),
    MINE_GIT_HISTORY_FOR_FILE("codegraph.mineGitHistoryForFile"),
    SEARCH_GIT_HISTORY("codegraph.searchGitHistory"),

    // Indexing and lifecycle
    REINDEX_WORKSPACE("codegraph.reindexWorkspace"),
    INDEX_FILES("codegraph.indexFiles"),
    INDEX_DIRECTORY("codegraph.indexDirectory"),
    UPDATE_CONFIGURATION("codegraph.updateConfiguration"),
    GET_PARSER_METRICS("codegraph.getParserMetrics"),
    ;

    override fun toString(): String = id
}
