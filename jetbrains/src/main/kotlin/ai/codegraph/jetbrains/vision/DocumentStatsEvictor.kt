// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.vision

import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.fileEditor.FileEditorManagerListener
import com.intellij.openapi.vfs.VirtualFile

/**
 * Drops a file's cached code-vision stats when its editor closes.
 *
 * [DocumentStatsCache] is keyed by URI and otherwise only cleared wholesale on
 * reindex, so a long session that browses a large tree accumulates one entry -
 * with its full symbol list - per file it ever opened. A closed editor asks for
 * nothing, so its entry is pure residue.
 */
class DocumentStatsEvictor : FileEditorManagerListener {

    override fun fileClosed(source: FileEditorManager, file: VirtualFile) {
        runCatching { DocumentStatsCache.getInstance(source.project).evict(file) }
    }
}
