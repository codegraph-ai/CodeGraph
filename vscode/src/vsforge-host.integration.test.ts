// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

/**
 * Integration tests for vsforge-host
 *
 * These tests verify the real IPC communication between Rust vsforge-host
 * and the Node.js host-runner from @vsforge/shim.
 *
 * NOTE: These tests spawn a real Node.js process and communicate via stdin/stdout.
 * They test the actual IPC protocol, not mocked behavior.
 */

import { describe, it, expect, beforeAll, afterAll, beforeEach } from 'vitest';
import { spawn, ChildProcess } from 'child_process';
import * as path from 'path';
import * as readline from 'readline';
import * as fs from 'fs';
import * as os from 'os';

// ============================================================================
// IPC Types (matching the Rust implementation)
// ============================================================================

interface IpcRequest {
  id: number;
  request: {
    type: string;
    [key: string]: unknown;
  };
}

interface IpcResponse {
  request_id: number;
  success: boolean;
  data?: unknown;
  error?: string;
}

// ============================================================================
// Test Helpers
// ============================================================================

/**
 * Helper class to communicate with the host-runner process
 */
class HostRunnerClient {
  private process: ChildProcess;
  private rl: readline.Interface;
  private requestId = 1;
  private pendingRequests = new Map<number, {
    resolve: (response: IpcResponse) => void;
    reject: (error: Error) => void;
  }>();
  private readyPromise: Promise<IpcResponse>;
  private readyResolve!: (response: IpcResponse) => void;

  constructor(hostRunnerPath: string) {
    this.readyPromise = new Promise((resolve) => {
      this.readyResolve = resolve;
    });

    // Spawn the host-runner process
    this.process = spawn('node', [hostRunnerPath], {
      stdio: ['pipe', 'pipe', 'inherit'],
    });

    if (!this.process.stdout || !this.process.stdin) {
      throw new Error('Failed to create process pipes');
    }

    // Set up readline to read responses
    this.rl = readline.createInterface({
      input: this.process.stdout,
      terminal: false,
    });

    this.rl.on('line', (line) => {
      if (!line.trim()) return;

      try {
        const response: IpcResponse = JSON.parse(line);

        // Handle ready signal (request_id 0)
        if (response.request_id === 0) {
          this.readyResolve(response);
          return;
        }

        const pending = this.pendingRequests.get(response.request_id);
        if (pending) {
          this.pendingRequests.delete(response.request_id);
          pending.resolve(response);
        }
      } catch (e) {
        console.error('Failed to parse response:', line, e);
      }
    });

    this.process.on('error', (err) => {
      console.error('Process error:', err);
      // Reject all pending requests
      for (const [, pending] of this.pendingRequests) {
        pending.reject(err);
      }
    });
  }

  async waitForReady(): Promise<IpcResponse> {
    return this.readyPromise;
  }

  async sendRequest(request: IpcRequest['request']): Promise<IpcResponse> {
    const id = this.requestId++;
    const message: IpcRequest = { id, request };

    return new Promise((resolve, reject) => {
      this.pendingRequests.set(id, { resolve, reject });

      const line = JSON.stringify(message) + '\n';
      this.process.stdin!.write(line, (err) => {
        if (err) {
          this.pendingRequests.delete(id);
          reject(err);
        }
      });

      // Timeout after 30 seconds
      setTimeout(() => {
        if (this.pendingRequests.has(id)) {
          this.pendingRequests.delete(id);
          reject(new Error(`Request timed out: ${request.type}`));
        }
      }, 30000);
    });
  }

  async shutdown(): Promise<void> {
    this.rl.close();
    this.process.stdin?.end();
    this.process.kill('SIGTERM');

    // Wait for process to exit
    await new Promise<void>((resolve) => {
      this.process.on('exit', () => resolve());
      setTimeout(resolve, 2000); // Timeout
    });
  }
}

/**
 * Create a temporary test extension
 */
function createTestExtension(): { extensionPath: string; cleanup: () => void } {
  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'vsforge-test-'));

  // Create package.json
  const packageJson = {
    name: 'test-extension',
    version: '0.0.1',
    main: './extension.js',
    engines: { vscode: '^1.85.0' },
    activationEvents: ['onCommand:test.*'],
    contributes: {
      commands: [
        { command: 'test.hello', title: 'Hello' },
        { command: 'test.greet', title: 'Greet' },
        { command: 'test.fail', title: 'Fail' },
      ],
    },
  };

  fs.writeFileSync(
    path.join(tempDir, 'package.json'),
    JSON.stringify(packageJson, null, 2)
  );

  // Create extension.js
  const extensionCode = `
const vscode = require('vscode');

let activateCount = 0;

async function activate(context) {
  activateCount++;
  console.error('Test extension activated, count:', activateCount);

  // Register commands
  context.subscriptions.push(
    vscode.commands.registerCommand('test.hello', () => {
      vscode.window.showInformationMessage('Hello from test extension!');
      return 'hello-result';
    })
  );

  context.subscriptions.push(
    vscode.commands.registerCommand('test.greet', (name) => {
      const message = 'Hello, ' + (name || 'World') + '!';
      vscode.window.showInformationMessage(message);
      return message;
    })
  );

  context.subscriptions.push(
    vscode.commands.registerCommand('test.fail', () => {
      throw new Error('Intentional failure');
    })
  );

  // Set context
  vscode.commands.executeCommand('setContext', 'testExtension.activated', true);

  return {
    getActivateCount: () => activateCount,
  };
}

async function deactivate() {
  console.error('Test extension deactivated');
}

module.exports = { activate, deactivate };
`;

  fs.writeFileSync(path.join(tempDir, 'extension.js'), extensionCode);

  return {
    extensionPath: tempDir,
    cleanup: () => {
      try {
        fs.rmSync(tempDir, { recursive: true, force: true });
      } catch {
        // Ignore cleanup errors
      }
    },
  };
}

// ============================================================================
// Tests
// ============================================================================

describe('VSForge Host Runner Integration', () => {
  // Path to the host-runner in the linked vsforge package
  const hostRunnerPath = path.resolve(
    __dirname,
    '../node_modules/@vsforge/shim/dist/host-runner.js'
  );

  let client: HostRunnerClient;
  let testExtension: { extensionPath: string; cleanup: () => void };

  beforeAll(async () => {
    // Verify host-runner exists
    if (!fs.existsSync(hostRunnerPath)) {
      throw new Error(
        `Host runner not found at ${hostRunnerPath}. Run 'npm run build' in @vsforge/shim first.`
      );
    }

    // Create test extension
    testExtension = createTestExtension();
  });

  afterAll(async () => {
    testExtension?.cleanup();
  });

  beforeEach(async () => {
    // Create a fresh client for each test
    client = new HostRunnerClient(hostRunnerPath);

    // Wait for ready signal
    const ready = await client.waitForReady();
    expect(ready.success).toBe(true);
    expect(ready.data).toEqual({ ready: true, version: '0.1.0' });
  });

  afterAll(async () => {
    await client?.shutdown();
  });

  describe('Initialization', () => {
    it('should send ready signal on startup', async () => {
      // Ready signal already verified in beforeEach
      expect(true).toBe(true);
    });

    it('should initialize with valid extension path', async () => {
      const response = await client.sendRequest({
        type: 'initialize',
        extensionPath: testExtension.extensionPath,
      });

      expect(response.success).toBe(true);
      expect(response.error).toBeUndefined();
    });

    it('should fail with non-existent extension path', async () => {
      const response = await client.sendRequest({
        type: 'initialize',
        extensionPath: '/non/existent/path',
      });

      expect(response.success).toBe(false);
      expect(response.error).toContain('does not exist');
    });
  });

  describe('Extension Lifecycle', () => {
    beforeEach(async () => {
      await client.sendRequest({
        type: 'initialize',
        extensionPath: testExtension.extensionPath,
      });
    });

    it('should activate extension successfully', async () => {
      const response = await client.sendRequest({
        type: 'activateExtension',
      });

      expect(response.success).toBe(true);
      // Extension returns an API object
      expect(response.data).toBeDefined();
    });

    it('should deactivate extension successfully', async () => {
      // Activate first
      await client.sendRequest({ type: 'activateExtension' });

      // Then deactivate
      const response = await client.sendRequest({
        type: 'deactivateExtension',
      });

      expect(response.success).toBe(true);
    });

    it('should handle multiple activate calls gracefully', async () => {
      const response1 = await client.sendRequest({ type: 'activateExtension' });
      const response2 = await client.sendRequest({ type: 'activateExtension' });

      expect(response1.success).toBe(true);
      expect(response2.success).toBe(true);
    });
  });

  describe('Command Execution', () => {
    beforeEach(async () => {
      await client.sendRequest({
        type: 'initialize',
        extensionPath: testExtension.extensionPath,
      });
      await client.sendRequest({ type: 'activateExtension' });
    });

    it('should execute registered command', async () => {
      const response = await client.sendRequest({
        type: 'executeCommand',
        command: 'test.hello',
        args: null,
      });

      expect(response.success).toBe(true);
      expect(response.data).toBe('hello-result');
    });

    it('should execute command with arguments', async () => {
      const response = await client.sendRequest({
        type: 'executeCommand',
        command: 'test.greet',
        args: 'VSForge',
      });

      expect(response.success).toBe(true);
      expect(response.data).toBe('Hello, VSForge!');
    });

    it('should handle command that throws error', async () => {
      const response = await client.sendRequest({
        type: 'executeCommand',
        command: 'test.fail',
        args: null,
      });

      // The command throws, which should be caught and reported
      expect(response.success).toBe(false);
      expect(response.error).toContain('Intentional failure');
    });
  });

  describe('API Call Recording', () => {
    beforeEach(async () => {
      await client.sendRequest({
        type: 'initialize',
        extensionPath: testExtension.extensionPath,
      });
      await client.sendRequest({ type: 'activateExtension' });
    });

    it('should record API calls made by extension', async () => {
      // Execute a command that calls vscode.window.showInformationMessage
      await client.sendRequest({
        type: 'executeCommand',
        command: 'test.hello',
        args: null,
      });

      // Get recorded API calls
      const response = await client.sendRequest({
        type: 'getApiCalls',
      });

      expect(response.success).toBe(true);
      expect(Array.isArray(response.data)).toBe(true);

      const calls = response.data as Array<{
        namespace: string;
        method: string;
        args: unknown[];
      }>;

      // Should have recorded the showInformationMessage call
      const infoMessages = calls.filter(
        (c) => c.namespace === 'window' && c.method === 'showInformationMessage'
      );
      expect(infoMessages.length).toBeGreaterThan(0);
      expect(infoMessages[0].args[0]).toBe('Hello from test extension!');
    });

    it('should reset state between tests', async () => {
      // Execute a command
      await client.sendRequest({
        type: 'executeCommand',
        command: 'test.hello',
        args: null,
      });

      // Verify calls were recorded
      let response = await client.sendRequest({ type: 'getApiCalls' });
      expect((response.data as unknown[]).length).toBeGreaterThan(0);

      // Reset state
      await client.sendRequest({ type: 'resetState' });

      // Verify calls were cleared
      response = await client.sendRequest({ type: 'getApiCalls' });
      expect((response.data as unknown[]).length).toBe(0);
    });
  });

  describe('Mock Configuration', () => {
    beforeEach(async () => {
      await client.sendRequest({
        type: 'initialize',
        extensionPath: testExtension.extensionPath,
      });
    });

    it('should set mock for API method', async () => {
      const response = await client.sendRequest({
        type: 'setMock',
        path: 'window.showInformationMessage',
        config: { resolvedValue: 'Mocked!' },
      });

      expect(response.success).toBe(true);
    });

    it('should configure shim behavior', async () => {
      const response = await client.sendRequest({
        type: 'configure',
        config: { strictMode: true },
      });

      expect(response.success).toBe(true);
    });
  });

  describe('Document Handling', () => {
    beforeEach(async () => {
      await client.sendRequest({
        type: 'initialize',
        extensionPath: testExtension.extensionPath,
      });
      await client.sendRequest({ type: 'activateExtension' });
    });

    it('should open a virtual document', async () => {
      const response = await client.sendRequest({
        type: 'openDocument',
        uri: 'file:///test/document.ts',
        content: 'const x = 1;\nconst y = 2;',
        languageId: 'typescript',
      });

      expect(response.success).toBe(true);
    });

    it('should close a virtual document', async () => {
      // Open first
      await client.sendRequest({
        type: 'openDocument',
        uri: 'file:///test/document.ts',
        content: 'const x = 1;',
        languageId: 'typescript',
      });

      // Then close
      const response = await client.sendRequest({
        type: 'closeDocument',
        uri: 'file:///test/document.ts',
      });

      expect(response.success).toBe(true);
    });
  });

  describe('Error Handling', () => {
    it('should handle unknown request type', async () => {
      const response = await client.sendRequest({
        type: 'unknownRequestType',
      });

      expect(response.success).toBe(false);
      expect(response.error).toContain('Unknown request type');
    });

    it('should handle activation before initialization', async () => {
      const response = await client.sendRequest({
        type: 'activateExtension',
      });

      expect(response.success).toBe(false);
      expect(response.error).toContain('not initialized');
    });
  });
});

describe('IPC Protocol Compliance', () => {
  const hostRunnerPath = path.resolve(
    __dirname,
    '../node_modules/@vsforge/shim/dist/host-runner.js'
  );

  it('should handle rapid sequential requests', async () => {
    const client = new HostRunnerClient(hostRunnerPath);
    await client.waitForReady();

    const testExtension = createTestExtension();

    try {
      await client.sendRequest({
        type: 'initialize',
        extensionPath: testExtension.extensionPath,
      });

      await client.sendRequest({ type: 'activateExtension' });

      // Send multiple rapid requests
      const promises = [];
      for (let i = 0; i < 10; i++) {
        promises.push(
          client.sendRequest({
            type: 'executeCommand',
            command: 'test.greet',
            args: `User${i}`,
          })
        );
      }

      const responses = await Promise.all(promises);

      // All should succeed
      for (const response of responses) {
        expect(response.success).toBe(true);
      }
    } finally {
      await client.shutdown();
      testExtension.cleanup();
    }
  });

  it('should maintain request ID correlation', async () => {
    const client = new HostRunnerClient(hostRunnerPath);
    await client.waitForReady();

    const testExtension = createTestExtension();

    try {
      await client.sendRequest({
        type: 'initialize',
        extensionPath: testExtension.extensionPath,
      });

      await client.sendRequest({ type: 'activateExtension' });

      // Send concurrent requests with different delays
      const results = await Promise.all([
        client.sendRequest({ type: 'executeCommand', command: 'test.greet', args: 'A' }),
        client.sendRequest({ type: 'getApiCalls' }),
        client.sendRequest({ type: 'executeCommand', command: 'test.greet', args: 'B' }),
      ]);

      // Each response should match its request
      expect(results[0].success).toBe(true);
      expect(results[0].data).toBe('Hello, A!');

      expect(results[1].success).toBe(true);
      expect(Array.isArray(results[1].data)).toBe(true);

      expect(results[2].success).toBe(true);
      expect(results[2].data).toBe('Hello, B!');
    } finally {
      await client.shutdown();
      testExtension.cleanup();
    }
  });
});
