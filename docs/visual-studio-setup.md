# CodeGraph for Visual Studio

## Visual Studio 2026+ (recommended)

VS 2026 supports MCP natively. No extension needed.

### Setup

1. Install CodeGraph:
   ```
   npm install -g @astudioplus/codegraph-mcp
   ```

2. Add `.mcp.json` to your solution root:
   ```json
   {
     "mcpServers": {
       "codegraph": {
         "command": "codegraph-mcp",
         "args": []
       }
     }
   }
   ```

3. Open your solution in VS 2026. Copilot agent mode will automatically discover CodeGraph's 28 tools.

### Configuration

Add flags for multi-project workspaces or exclusions:

```json
{
  "mcpServers": {
    "codegraph": {
      "command": "codegraph-mcp",
      "args": ["--workspace", "src/MyProject", "--exclude", "generated"]
    }
  }
}
```

### Custom Agent (optional)

Create `.github/agents/codegraph.agent.md` in your repo to teach Copilot when and how to use CodeGraph tools:

```markdown
---
name: CodeGraph
description: Code intelligence assistant with structural understanding
tools:
  - codegraph
---

You have access to CodeGraph tools that provide structural code intelligence.

## When to use CodeGraph

- **Before modifying code**: Use `codegraph_get_edit_context` to understand callers, tests, and dependencies
- **Navigating unfamiliar code**: Use `codegraph_get_ai_context` with intent=explain
- **Finding implementations**: Use `codegraph_symbol_search` or `codegraph_find_entry_points`
- **Assessing impact**: Use `codegraph_analyze_impact` before renaming or deleting
- **Understanding call flow**: Use `codegraph_get_callers` and `codegraph_get_callees`

## Preferred workflow

1. Start with `codegraph_symbol_search` to find the target symbol
2. Use `codegraph_get_ai_context` for full understanding
3. Check `codegraph_analyze_impact` before making changes
4. Verify with `codegraph_find_related_tests` after changes
```

### MCP config locations

| Location | Scope |
|----------|-------|
| `%USERPROFILE%\.mcp.json` | Global (all solutions) |
| `<solution>\.mcp.json` | Per-solution (check into repo) |
| `<solution>\.vs\mcp.json` | Per-user per-solution |
| `<solution>\.vscode\mcp.json` | Shared with VS Code |

---

## Visual Studio 2022

VS 2022 does not support MCP natively. Install the CodeGraph extension from the Visual Studio Marketplace.

The extension launches the CodeGraph server as an LSP subprocess and exposes tools to GitHub Copilot via the Copilot extensibility API.

### Install

1. Download `CodegraphExtension.vsix` from [releases](https://github.com/codegraph-ai/CodeGraph/releases)
2. Double-click to install, or: `VSIXInstaller.exe CodegraphExtension.vsix`
3. Restart Visual Studio

The extension automatically indexes your solution when you open a supported file (C#, C++, Python, Rust, Go, Java, TypeScript, and 10 more languages).
