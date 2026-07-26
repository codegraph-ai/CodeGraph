<!--
Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
SPDX-License-Identifier: Apache-2.0
-->

# CodeGraph for JetBrains IDEs

A thin client for the CodeGraph engine, the same `codegraph-server` binary the VS Code extension drives.

## Architecture

All analysis lives in the Rust engine.
The plugin spawns it, speaks LSP over stdio, and renders the results.

```
IntelliJ IDEA / PyCharm / GoLand / Android Studio ...
  │
  ├── LSP4IJ ......... JSON-RPC transport + document synchronisation
  │     └── codegraph-server (Rust)   LSP over stdio
  │
  ├── CodeGraphClient  every capability, as workspace/executeCommand
  └── UI surfaces      tool windows, Code Vision, graph panel
```

The engine exposes no editor-specific behaviour: every feature is a
`workspace/executeCommand` call listed in [`CodeGraphCommand`](src/main/kotlin/ai/codegraph/jetbrains/lsp/CodeGraphCommand.kt).
That is why a second editor client is mostly UI work.

### Why LSP4IJ rather than the platform LSP API

The IntelliJ Platform's own `com.intellij.platform.lsp` API is available only in
the paid IDEs.
Depending on it would exclude IntelliJ IDEA Community, PyCharm Community and
Android Studio, which is the larger share of the audience.
LSP4IJ is Apache-2.0, works on every JetBrains IDE from 2024.2, and exposes the
underlying LSP4J `LanguageServer`, so dropping to raw LSP4J stays available if
the dependency ever becomes a problem.

## Engine resolution

The plugin does **not** bundle engine binaries.
The VSIX can, because VS Code ships per-platform artifacts; the JetBrains
Marketplace has no equivalent, so bundling all four platforms would mean a
~120 MB download for every user regardless of platform.

Resolution order, implemented in
[`CodeGraphServerResolver`](src/main/kotlin/ai/codegraph/jetbrains/server/CodeGraphServerResolver.kt):

1. Explicit path from settings
2. CodeGraph Pro on `PATH`, then its known install directories
3. `codegraph-server` on `PATH` (npm or homebrew installs)
4. An engine under `~/.codegraph/bin`
5. Cargo build output, when the open project is the CodeGraph repo itself

### Installing the engine

Until per-platform binaries are published as release assets, there is no
one-click install.
The engine ships bundled inside the npm package and the VSIX, so users install
it with:

```sh
npm i -g @astudioplus/codegraph-mcp
```

That puts `codegraph-server` where step 3 finds it.
A checksum-verifying downloader for step 4 was written and then removed: the
release assets it would fetch do not exist yet, and shipping code that cannot
run is worse than not shipping it.
Publishing those assets is tracked separately; the `MANAGED_INSTALL` slot in the
resolver is reserved for it.

## Surfaces

| Surface | Backed by | Notes |
|---|---|---|
| Code Vision | `codegraph.getDocumentCodeLens` | Callers, tests and complexity above declarations |
| Symbols tool window | `codegraph.getWorkspaceSymbols` | Tree with search; double-click navigates |
| Graph panel | `codegraph.getDependencyGraph`, `codegraph.getCallGraph` | JCEF, with a text fallback |
| Status bar | engine state | Distinguishes "no results" from "not running" |

Code Vision never blocks the daemon: a cache miss returns nothing, schedules one
fetch and restarts the daemon when the answer lands.

The graph panel renders a self-contained page - a small force simulation
emitting SVG, no external scripts. A CDN dependency would be less code and would
fail on exactly the machines that most need it to work: offline, air-gapped, or
behind a blocking proxy. JCEF is absent from some JBR builds and from Remote Dev
clients, so an unavailable browser degrades to a text listing.

One caveat worth knowing when calling the engine directly:
`getWorkspaceSymbols` treats a **missing** `query` as "functions, classes and
modules" but an **empty string** as "modules only". Sending `""` for the
unfiltered view yields an empty tree on a perfectly healthy index.

## AI tooling

The VS Code client declares 28 `languageModelTools`. Those are Copilot-specific
and have no JetBrains equivalent, and reimplementing them would mean a second
hand-written tool list to keep in step with the engine.

Instead, **Tools | CodeGraph | Register with AI Assistant** writes the engine's
own MCP mode into `<project>/.mcp.json`, the `mcpServers` shape that Junie,
Claude Code, Cursor and the AI Assistant MCP settings all read:

```json
{
  "mcpServers": {
    "codegraph": {
      "command": "/path/to/codegraph-server",
      "args": ["--mcp", "--workspace", "/path/to/project",
               "--embedding-model", "bge-small", "--full-body-embedding"]
    }
  }
}
```

Verified end to end: that exact command answers an MCP `initialize` and lists
**42 tools** - more than the VS Code client declares by hand, which is the
argument for this approach rather than a port.

Registration merges rather than overwrites; a project that already points at
other MCP servers keeps them. The config is also offered on the clipboard,
because every AI client keeps its MCP configuration somewhere different and
pasting is the one path that always works.

## Engine lifecycle

The engine is a native process that things outside the plugin can kill:
antivirus, the OOM killer, a missing system library.

[`EngineLifecycle`](src/main/kotlin/ai/codegraph/jetbrains/server/EngineLifecycle.kt)
turns an unexpected death into one explained message, using the crash
breadcrumbs the engine leaves in `~/.codegraph`, and
[`RestartCircuitBreaker`](src/main/kotlin/ai/codegraph/jetbrains/server/RestartCircuitBreaker.kt)
stops the restart loop after three crashes in a minute.
Without the breaker, a host where the engine simply cannot run produces an
endless crash-restart cycle - in the VS Code client that showed up as single
machines generating 50+ crash events a week.

## Building

Requires JDK 21.

```sh
export JAVA_HOME=/opt/homebrew/opt/openjdk@21   # or any JDK 21
./gradlew buildPlugin                            # -> build/distributions/*.zip
```

Run the tests:

```sh
./gradlew test
```

Run a sandbox IDE with the plugin installed:

```sh
./gradlew runIde -PsandboxProject=/path/to/some/project
```

## Checking the IDE side

A sandbox IDE normally needs a human to click a menu item before anything is
exercised, which leaves the integration that matters most - LSP4IJ carrying a
CodeGraph `executeCommand` to a live engine - as the only part never checked
automatically.
Arming the self-check runs it on project open and writes the verdict to the IDE
log:

```sh
./gradlew runIde -PsandboxProject=/path/to/some/project \
    -PrunIdeSystemProperty=codegraph.selfcheck=true

grep codegraph-selfcheck \
    .intellijPlatform/sandbox/codegraph-jetbrains/*/log/idea.log
```

The activity is inert without that system property, so it costs users nothing.

**Trust the sandbox project first.** IntelliJ holds back every project activity
until a project is trusted, and in a sandbox the trust dialog is easy to miss -
the symptom is a plugin that loads cleanly and then does nothing at all, with no
error anywhere. Pre-trust the path before launching:

```sh
cat > .intellijPlatform/sandbox/codegraph-jetbrains/*/config/options/trusted-paths.xml <<'XML'
<application>
  <component name="Trusted.Paths.Settings">
    <option name="TRUSTED_PATHS">
      <list><option value="/path/to/your/sandbox/projects" /></list>
    </option>
  </component>
</application>
XML
```

Only one sandbox IDE can run at a time: a second instance fails to start with
`MVStoreException: This store is read-only` because the first still holds the
config store.

## Checking the engine contract

`scripts/engine_probe.py` replays, over raw stdio, exactly what the plugin
sends: the `initialize` options built by `CodeGraphConnectionProvider` followed
by the `executeCommand` calls the plugin makes.
It needs no IDE, so it answers in seconds the question a sandbox IDE answers in
minutes, and it diffs `CodeGraphCommand.kt` against the command list the engine
advertises so that hand-transcribed enum cannot drift unnoticed.

```sh
python3 scripts/engine_probe.py ../target/release/codegraph-server ..
```

Two known engine deviations are recorded in the probe rather than hidden by it:
`codegraph.getDocumentCodeLens` is dispatched but not advertised, and the engine
ignores the LSP `exit` notification, terminating only when stdin closes.
Both are masked by the clients today and are tracked as engine fixes.
