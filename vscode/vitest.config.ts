import { defineConfig, configDefaults } from 'vitest/config';

// The VS Code extension test suite depends on a private `vsforge` test-harness
// monorepo (`@vsforge/shim` = a mock `vscode` module, `@vsforge/test` =
// config mocks), linked via `file:../vsforge/packages/@vsforge/*`. That repo
// was never committed to GitHub and is permanently lost, so every suite below
// fails to import (`Cannot find package '@vsforge/shim'`).
//
// These suites are QUARANTINED until the harness is rebuilt or the tests are
// migrated to a standard local vscode mock. `passWithNoTests` keeps
// `vitest run` green in the meantime; any NEW (non-vsforge) test still runs.
// Extension/telemetry TS is meanwhile validated by `tsc --noEmit`.
const QUARANTINED_VSFORGE_SUITES = [
    'src/extension.test.ts',
    'src/server.test.ts',
    'src/vsforge-host.integration.test.ts',
    'src/ai/contextProvider.test.ts',
    'src/ai/toolManager.test.ts',
    'src/commands/index.test.ts',
    'src/commands/execution.test.ts',
    'src/views/graphPanel.test.ts',
    'src/views/treeProviders.test.ts',
];

export default defineConfig({
    test: {
        exclude: [...configDefaults.exclude, ...QUARANTINED_VSFORGE_SUITES],
        passWithNoTests: true,
    },
});
