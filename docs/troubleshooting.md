# Troubleshooting

## No files indexed

CodeGraph reported "indexed 0 files".
The index is empty, so symbol search, the call graph, CodeLens and every agent tool have nothing to answer with.
The cause is almost always one of the four below, and the extension's notification already names which one it diagnosed.

### No folder is open

CodeGraph indexes workspace folders.
With no folder open there is nothing to walk.
Open the project folder (**File → Open Folder**) and run **CodeGraph: Reindex Workspace** from the command palette.

### `codegraph.indexPaths` points at locations with no source files

When `codegraph.indexPaths` is non-empty it *replaces* the whole-workspace scan: only those directories are indexed.
A path that was renamed, that lives outside the workspace folder, or that holds no supported source file yields an empty index even though the rest of the project is full of code.

Fix it one of two ways:

- Clear `codegraph.indexPaths` (set it to `[]`) to index every workspace folder.
- Correct the entries so they are workspace-relative paths that actually contain source, for example `["src", "crates"]`.

Paths are resolved relative to the first workspace folder.

### `codegraph.excludePatterns` matches everything

`codegraph.excludePatterns` defaults to build and dependency directories (`**/node_modules/**`, `**/target/**`, `**/dist/**`, `**/build/**`, `**/vendor/**`, and similar).
A broad addition such as `**/src/**` or a lone `**` removes every candidate file and the index comes back empty.

Remove or narrow the offending pattern.
Patterns are globs matched against the full path, so anchor them at the directory you mean: `**/generated/**`, not `**`.

### No files in a supported language

CodeGraph parses 38 languages (see the language table in the [README](../README.md#languages)).
A workspace made only of, say, Markdown, JSON and images has nothing for the parsers to do, and that is expected.

Two non-obvious variants of this:

- **Community build limits.** COBOL, Fortran, Perl, Dart, Zig and R are only compiled into builds made with `--features extra-languages`. A project written entirely in one of those indexes as zero files on the default community engine.
- **File size cap.** Files larger than `codegraph.maxFileSizeKB` (default 1024 KB) are skipped. Generated single-file sources can exceed it; raise the setting if that is your case.

### Files are present and it still indexes zero

If supported files exist, are inside the index scope, and survive the excludes, but the count is still zero, this is a bug rather than a configuration problem.

Collect the details before reporting:

1. Open the **CodeGraph** output channel (**View → Output**, then pick *CodeGraph*) and look for parse or engine-startup errors.
2. Check the engine responds: `codegraph-server --info`.
3. Open an issue at <https://github.com/codegraph-ai/CodeGraph/issues> with the output channel contents, your `codegraph.*` settings, and the languages in the workspace.

## Related

- [README — Configuration](../README.md#configuration) for the full settings and MCP flag reference.
- [README — Quick Start](../README.md#quick-start) to verify the engine and client are installed as expected.
