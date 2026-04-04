# CodeGraph Visual Studio Extension — Development Guide

## Prerequisites

- **Visual Studio 2022 Community/Pro** (17.6+) with workloads:
  - .NET desktop development
  - Visual Studio extension development
- **Git** (already installed)
- **Rust toolchain** (already installed — for building codegraph-server)

Workloads already installed on this machine via:
```
"C:\Program Files (x86)\Microsoft Visual Studio\Installer\setup.exe" modify ^
  --installPath "C:\Program Files\Microsoft Visual Studio\2022\Community" ^
  --add Microsoft.VisualStudio.Workload.ManagedDesktop ^
  --add Microsoft.VisualStudio.Workload.VisualStudioExtension ^
  --passive --wait
```

## Project Location

```
C:\Users\Administrator\projects\codegraph-pro\vs-extension\src\CodegraphExtension\
```

## Known Issue: Project Format

The current `.csproj` uses `Microsoft.NET.Sdk` (new SDK-style format). The VSSDK build targets for VSIX packaging don't work correctly with this format. 

**Fix needed**: Convert to the traditional `.csproj` format. Easiest way:

1. Open Visual Studio 2022
2. File → New → Project → "VSIX Project" (under Extensibility)
3. Name it `CodegraphExtension`, create in a temp location
4. Copy the generated `.csproj` structure (it uses the old format with explicit References)
5. Move our source files (`CodegraphLanguageClient.cs`, `CodegraphPackage.cs`, `source.extension.vsixmanifest`) into the new project
6. Add NuGet packages:
   - `Microsoft.VisualStudio.LanguageServer.Client`
   - `Newtonsoft.Json`
7. Build → should produce `.vsix`

Or alternatively, start from scratch in VS:
1. File → New → Project → "VSIX Project"
2. Add NuGet: `Microsoft.VisualStudio.LanguageServer.Client`
3. Copy in our two `.cs` files and the manifest
4. F5 to debug (launches experimental VS instance)

## Architecture

```
Visual Studio 2022
  │
  ├── ILanguageClient (CodegraphLanguageClient.cs)
  │     Launches codegraph-server.exe as subprocess
  │     Communicates via LSP over stdio
  │
  └── AsyncPackage (CodegraphPackage.cs)
        Extension entry point, registers commands
```

The extension launches the Rust `codegraph-server.exe` binary and communicates via Language Server Protocol. VS 2022 handles all LSP features (diagnostics, code actions, etc.) automatically.

## Server Binary

The extension looks for the server in this order:
1. `server\codegraph-server.exe` (bundled with extension)
2. `codegraph-mcp.cmd` in PATH (npm global install)
3. `codegraph-pro.exe` in PATH

For development, either:
- Copy `C:\Users\Administrator\projects\codegraph\target\release\codegraph-server.exe` to the project's `server\` directory
- Or install globally: `npm install -g @astudioplus/codegraph-mcp`

## Development Workflow

### Debug (F5)

1. Open the `.csproj` in Visual Studio
2. Set as startup project
3. F5 → launches an experimental Visual Studio instance
4. Open any C#/C++ project in the experimental instance
5. The CodeGraph server should start (check Output → CodeGraph)

### Build VSIX

```
MSBuild CodegraphExtension.csproj /p:Configuration=Release
```

Output: `bin\Release\CodegraphExtension.vsix`

### Install

```
VSIXInstaller.exe CodegraphExtension.vsix
```

Or double-click the `.vsix` file.

## Source Files

| File | Purpose |
|------|---------|
| `CodegraphLanguageClient.cs` | LSP client — launches server, manages connection |
| `CodegraphPackage.cs` | VS Package — extension entry point |
| `source.extension.vsixmanifest` | Extension metadata for VS Marketplace |
| `CodegraphExtension.csproj` | Build config (needs conversion to old format) |

## Future: Copilot Integration

VS 2022 17.12+ has `ICopilotChatParticipant` for chat commands (`@codegraph`).
VS 2022 17.13+ has `ICopilotTool` for autonomous tool calling.

These APIs require the `Microsoft.VisualStudio.Copilot` NuGet package (preview).
Add after the basic LSP extension works.

## Future: Tool Windows (Pro features)

| Window | Purpose |
|--------|---------|
| Security Findings | Tree view of scan_security results |
| Coupling Dashboard | Module instability heatmap |
| Unused Code | Dimmed overlay on dead functions |
| Similarity Explorer | Duplicate/similar function browser |

These are WPF UserControls registered as `ToolWindowPane` in the package.

## Repo

| Repo | Location | Remote |
|------|----------|--------|
| codegraph (public) | `C:\Users\Administrator\projects\codegraph` | https://github.com/codegraph-ai/CodeGraph.git |
