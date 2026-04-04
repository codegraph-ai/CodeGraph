// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

/**
 * Command Execution Tests
 *
 * These tests verify that commands registered via vscode.commands.registerCommand
 * can be executed via vscode.commands.executeCommand and return expected results.
 *
 * This tests the VSForge shim's ability to:
 * 1. Track registered command handlers
 * 2. Execute handlers when executeCommand is called
 * 3. Return proper results from command execution
 */

import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import {
  vscode,
  setMock,
  reset,
  getCalls,
  clearAllMocks,
} from '@vsforge/shim';

// Mock vscode module
vi.mock('vscode', async () => {
  const shim = await import('@vsforge/shim');
  return shim.vscode;
});

describe('Command Registration and Execution', () => {
  beforeEach(() => {
    reset();
    clearAllMocks();
  });

  afterEach(() => {
    vi.resetModules();
  });

  describe('Basic Command Execution', () => {
    it('should execute a registered command and return result', async () => {
      // Register a command
      const disposable = vscode.commands.registerCommand('test.myCommand', () => {
        return 'command-result';
      });

      // Execute the command
      const result = await vscode.commands.executeCommand('test.myCommand');

      expect(result).toBe('command-result');
      disposable.dispose();
    });

    it('should execute command with arguments', async () => {
      const disposable = vscode.commands.registerCommand('test.greet', (name: string) => {
        return `Hello, ${name}!`;
      });

      const result = await vscode.commands.executeCommand('test.greet', 'VSForge');

      expect(result).toBe('Hello, VSForge!');
      disposable.dispose();
    });

    it('should execute command with multiple arguments', async () => {
      const disposable = vscode.commands.registerCommand('test.add', (a: number, b: number) => {
        return a + b;
      });

      const result = await vscode.commands.executeCommand<number>('test.add', 5, 3);

      expect(result).toBe(8);
      disposable.dispose();
    });

    it('should handle async command handlers', async () => {
      const disposable = vscode.commands.registerCommand('test.asyncCommand', async () => {
        await new Promise(resolve => setTimeout(resolve, 10));
        return 'async-result';
      });

      const result = await vscode.commands.executeCommand('test.asyncCommand');

      expect(result).toBe('async-result');
      disposable.dispose();
    });

    it('should return undefined for unregistered commands', async () => {
      const result = await vscode.commands.executeCommand('nonexistent.command');

      expect(result).toBeUndefined();
    });

    it('should list registered commands via getCommands', async () => {
      const disposable1 = vscode.commands.registerCommand('test.cmd1', () => {});
      const disposable2 = vscode.commands.registerCommand('test.cmd2', () => {});

      const commands = await vscode.commands.getCommands();

      expect(commands).toContain('test.cmd1');
      expect(commands).toContain('test.cmd2');

      disposable1.dispose();
      disposable2.dispose();
    });
  });

  describe('Command Disposal', () => {
    it('should remove command when disposed', async () => {
      const disposable = vscode.commands.registerCommand('test.disposable', () => 'result');

      // Command should work before disposal
      let result = await vscode.commands.executeCommand('test.disposable');
      expect(result).toBe('result');

      // Dispose the command
      disposable.dispose();

      // Command should not work after disposal
      result = await vscode.commands.executeCommand('test.disposable');
      expect(result).toBeUndefined();
    });

    it('should not affect other commands when one is disposed', async () => {
      const disposable1 = vscode.commands.registerCommand('test.cmd1', () => 'result1');
      const disposable2 = vscode.commands.registerCommand('test.cmd2', () => 'result2');

      // Dispose first command
      disposable1.dispose();

      // Second command should still work
      const result = await vscode.commands.executeCommand('test.cmd2');
      expect(result).toBe('result2');

      disposable2.dispose();
    });
  });

  describe('Command Error Handling', () => {
    it('should propagate errors from command handlers', async () => {
      const disposable = vscode.commands.registerCommand('test.errorCommand', () => {
        throw new Error('Command failed');
      });

      await expect(
        vscode.commands.executeCommand('test.errorCommand')
      ).rejects.toThrow('Command failed');

      disposable.dispose();
    });

    it('should propagate async errors from command handlers', async () => {
      const disposable = vscode.commands.registerCommand('test.asyncError', async () => {
        await new Promise(resolve => setTimeout(resolve, 10));
        throw new Error('Async command failed');
      });

      await expect(
        vscode.commands.executeCommand('test.asyncError')
      ).rejects.toThrow('Async command failed');

      disposable.dispose();
    });
  });

  describe('Reset Behavior', () => {
    it('should clear all registered commands on reset', async () => {
      vscode.commands.registerCommand('test.reset1', () => 'r1');
      vscode.commands.registerCommand('test.reset2', () => 'r2');

      // Commands work before reset
      expect(await vscode.commands.executeCommand('test.reset1')).toBe('r1');
      expect(await vscode.commands.executeCommand('test.reset2')).toBe('r2');

      // Reset
      reset();

      // Commands should not work after reset
      expect(await vscode.commands.executeCommand('test.reset1')).toBeUndefined();
      expect(await vscode.commands.executeCommand('test.reset2')).toBeUndefined();
    });
  });
});

describe('Extension Command Integration', () => {
  // Simulates how a real extension would register and use commands

  beforeEach(() => {
    reset();
    clearAllMocks();

    // Set up common mocks
    setMock('window.showInformationMessage', { resolvedValue: undefined });
    setMock('window.showWarningMessage', { resolvedValue: undefined });
    setMock('window.showErrorMessage', { resolvedValue: undefined });
  });

  afterEach(() => {
    vi.resetModules();
  });

  it('should simulate extension activation with command registration', async () => {
    // Simulate extension context
    const context = {
      subscriptions: [] as { dispose: () => void }[],
    };

    // Simulate extension activation
    function activate(ctx: typeof context) {
      ctx.subscriptions.push(
        vscode.commands.registerCommand('myext.sayHello', () => {
          vscode.window.showInformationMessage('Hello from extension!');
          return 'hello';
        })
      );

      ctx.subscriptions.push(
        vscode.commands.registerCommand('myext.calculate', (a: number, b: number, op: string) => {
          switch (op) {
            case 'add': return a + b;
            case 'subtract': return a - b;
            case 'multiply': return a * b;
            default: throw new Error(`Unknown operation: ${op}`);
          }
        })
      );
    }

    // Activate
    activate(context);

    // Test commands
    const helloResult = await vscode.commands.executeCommand('myext.sayHello');
    expect(helloResult).toBe('hello');

    const calcResult = await vscode.commands.executeCommand<number>('myext.calculate', 10, 5, 'multiply');
    expect(calcResult).toBe(50);

    // Verify side effects
    const infoCalls = getCalls({ namespace: 'window', method: 'showInformationMessage' });
    expect(infoCalls.some(c => c.args[0] === 'Hello from extension!')).toBe(true);

    // Cleanup
    for (const sub of context.subscriptions) {
      sub.dispose();
    }
  });

  it('should handle commands that interact with workspace', async () => {
    // Mock workspace
    setMock('workspace.getConfiguration', {
      implementation: (section: string) => ({
        get: (key: string, defaultValue?: any) => {
          if (section === 'myext' && key === 'maxItems') return 100;
          return defaultValue;
        },
        update: vi.fn(),
      }),
    });

    // Register command that reads config
    const disposable = vscode.commands.registerCommand('myext.getMaxItems', () => {
      const config = vscode.workspace.getConfiguration('myext');
      return config.get('maxItems', 10);
    });

    const result = await vscode.commands.executeCommand<number>('myext.getMaxItems');
    expect(result).toBe(100);

    disposable.dispose();
  });

  it('should handle commands that show quick pick', async () => {
    // Mock quick pick
    setMock('window.showQuickPick', {
      implementation: async (items: any[]) => {
        // Simulate user selecting first item
        return items[0];
      },
    });

    const disposable = vscode.commands.registerCommand('myext.selectItem', async () => {
      const selected = await vscode.window.showQuickPick([
        { label: 'Option A', value: 'a' },
        { label: 'Option B', value: 'b' },
      ]);
      return selected?.value;
    });

    const result = await vscode.commands.executeCommand<string>('myext.selectItem');
    expect(result).toBe('a');

    disposable.dispose();
  });

  it('should handle commands that create output channels', async () => {
    const outputLines: string[] = [];

    setMock('window.createOutputChannel', {
      implementation: (name: string) => ({
        name,
        appendLine: (line: string) => outputLines.push(line),
        append: vi.fn(),
        clear: () => outputLines.length = 0,
        show: vi.fn(),
        hide: vi.fn(),
        dispose: vi.fn(),
      }),
    });

    const disposable = vscode.commands.registerCommand('myext.log', (message: string) => {
      const channel = vscode.window.createOutputChannel('MyExt');
      channel.appendLine(message);
      channel.show();
      return true;
    });

    const result = await vscode.commands.executeCommand<boolean>('myext.log', 'Test message');
    expect(result).toBe(true);
    expect(outputLines).toContain('Test message');

    disposable.dispose();
  });
});

describe('Command Call Recording', () => {
  beforeEach(() => {
    reset();
    clearAllMocks();
  });

  it('should record registerCommand calls', () => {
    vscode.commands.registerCommand('test.recorded', () => 'value');

    const calls = getCalls({ namespace: 'commands', method: 'registerCommand' });
    expect(calls.length).toBe(1);
    expect(calls[0].args[0]).toBe('test.recorded');
  });

  it('should record executeCommand calls', async () => {
    vscode.commands.registerCommand('test.exec', () => 'result');
    await vscode.commands.executeCommand('test.exec', 'arg1', 'arg2');

    const calls = getCalls({ namespace: 'commands', method: 'executeCommand' });
    expect(calls.length).toBe(1);
    expect(calls[0].args[0]).toBe('test.exec');
    expect(calls[0].args[1]).toBe('arg1');
    expect(calls[0].args[2]).toBe('arg2');
  });

  it('should record getCommands calls', async () => {
    await vscode.commands.getCommands();

    const calls = getCalls({ namespace: 'commands', method: 'getCommands' });
    expect(calls.length).toBe(1);
  });
});

describe('Complex Command Scenarios', () => {
  beforeEach(() => {
    reset();
    clearAllMocks();
  });

  it('should handle command that registers another command', async () => {
    // Command that dynamically registers a new command
    const disposable = vscode.commands.registerCommand('test.createCommand', (name: string) => {
      const innerDisposable = vscode.commands.registerCommand(`test.${name}`, () => `I am ${name}`);
      return innerDisposable;
    });

    // Create a new command dynamically
    const innerDisposable = await vscode.commands.executeCommand<{ dispose: () => void }>(
      'test.createCommand',
      'dynamic'
    );

    // The dynamically created command should work
    const result = await vscode.commands.executeCommand<string>('test.dynamic');
    expect(result).toBe('I am dynamic');

    innerDisposable?.dispose();
    disposable.dispose();
  });

  it('should handle command that calls other commands', async () => {
    // First command
    vscode.commands.registerCommand('test.first', () => 'first-value');

    // Second command that calls first
    vscode.commands.registerCommand('test.second', async () => {
      const firstResult = await vscode.commands.executeCommand<string>('test.first');
      return `Combined: ${firstResult}`;
    });

    const result = await vscode.commands.executeCommand<string>('test.second');
    expect(result).toBe('Combined: first-value');
  });

  it('should handle concurrent command execution', async () => {
    let counter = 0;

    vscode.commands.registerCommand('test.increment', async () => {
      const current = counter;
      await new Promise(resolve => setTimeout(resolve, 10));
      counter = current + 1;
      return counter;
    });

    // Execute multiple times concurrently
    const results = await Promise.all([
      vscode.commands.executeCommand<number>('test.increment'),
      vscode.commands.executeCommand<number>('test.increment'),
      vscode.commands.executeCommand<number>('test.increment'),
    ]);

    // Due to race condition, all should return 1 (they all read 0, then write 1)
    expect(results).toEqual([1, 1, 1]);
  });

  it('should handle command returning complex objects', async () => {
    const disposable = vscode.commands.registerCommand('test.complexReturn', () => ({
      status: 'success',
      data: {
        items: [1, 2, 3],
        metadata: { count: 3, source: 'test' },
      },
      timestamp: new Date('2024-01-01'),
    }));

    const result = await vscode.commands.executeCommand<{
      status: string;
      data: { items: number[]; metadata: { count: number; source: string } };
      timestamp: Date;
    }>('test.complexReturn');

    expect(result?.status).toBe('success');
    expect(result?.data.items).toEqual([1, 2, 3]);
    expect(result?.data.metadata.count).toBe(3);

    disposable.dispose();
  });

  it('should handle command with object argument', async () => {
    const disposable = vscode.commands.registerCommand('test.withOptions', (options: {
      name: string;
      count: number;
      enabled: boolean;
    }) => {
      return `${options.name}: ${options.count} (${options.enabled ? 'enabled' : 'disabled'})`;
    });

    const result = await vscode.commands.executeCommand<string>('test.withOptions', {
      name: 'Feature',
      count: 42,
      enabled: true,
    });

    expect(result).toBe('Feature: 42 (enabled)');

    disposable.dispose();
  });
});

describe('TextEditorCommand Registration', () => {
  beforeEach(() => {
    reset();
    clearAllMocks();
  });

  it('should register text editor commands', async () => {
    // Text editor commands receive (editor, edit, ...args)
    // When called via executeCommand, first two args are injected
    const disposable = vscode.commands.registerTextEditorCommand(
      'test.editorCommand',
      (editor, edit, ...args) => {
        // In real scenario, this would modify the editor
        // The shim injects undefined for editor and edit when called via executeCommand
        return `Executed text editor command`;
      }
    );

    // Text editor commands are tracked the same way
    const result = await vscode.commands.executeCommand<string>('test.editorCommand');
    expect(result).toBe('Executed text editor command');

    disposable.dispose();
  });

  it('should track text editor command registration', () => {
    const disposable = vscode.commands.registerTextEditorCommand(
      'test.tracked',
      () => {}
    );

    const calls = getCalls({ namespace: 'commands', method: 'registerTextEditorCommand' });
    expect(calls.length).toBe(1);
    expect(calls[0].args[0]).toBe('test.tracked');

    disposable.dispose();
  });
});
