// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

import { describe, it, expect, beforeEach, vi } from 'vitest';
import * as os from 'os';
import * as fs from 'fs';

// Mock the os module
vi.mock('os', () => ({
  platform: vi.fn(),
  arch: vi.fn(),
}));

// Mock the fs module
vi.mock('fs', () => ({
  existsSync: vi.fn(),
}));

// Mock vscode
vi.mock('vscode', async () => {
  const shim = await import('@vsforge/shim');
  return shim.default;
});

describe('getServerPath', () => {
  const mockContext = {
    asAbsolutePath: vi.fn((relativePath: string) => `/test/extension/${relativePath}`),
  };

  beforeEach(() => {
    vi.clearAllMocks();
    (fs.existsSync as any).mockReturnValue(false);
  });

  describe('platform-specific binary names', () => {
    it('should use correct binary name for Linux', async () => {
      (os.platform as any).mockReturnValue('linux');
      (os.arch as any).mockReturnValue('x64');
      (fs.existsSync as any).mockImplementation((path: string) => {
        return path === '/test/extension/bin/codegraph-lsp-linux-x64';
      });

      const { getServerPath } = await import('./server');
      const result = getServerPath(mockContext as any);

      expect(result).toBe('/test/extension/bin/codegraph-lsp-linux-x64');
    });

    it('should use correct binary name for macOS x64', async () => {
      (os.platform as any).mockReturnValue('darwin');
      (os.arch as any).mockReturnValue('x64');
      (fs.existsSync as any).mockImplementation((path: string) => {
        return path === '/test/extension/bin/codegraph-lsp-darwin-x64';
      });

      const { getServerPath } = await import('./server');
      const result = getServerPath(mockContext as any);

      expect(result).toBe('/test/extension/bin/codegraph-lsp-darwin-x64');
    });

    it('should use correct binary name for macOS ARM64 (Apple Silicon)', async () => {
      (os.platform as any).mockReturnValue('darwin');
      (os.arch as any).mockReturnValue('arm64');
      (fs.existsSync as any).mockImplementation((path: string) => {
        return path === '/test/extension/bin/codegraph-lsp-darwin-arm64';
      });

      const { getServerPath } = await import('./server');
      const result = getServerPath(mockContext as any);

      expect(result).toBe('/test/extension/bin/codegraph-lsp-darwin-arm64');
    });

    it('should use correct binary name for Windows', async () => {
      (os.platform as any).mockReturnValue('win32');
      (os.arch as any).mockReturnValue('x64');
      (fs.existsSync as any).mockImplementation((path: string) => {
        return path === '/test/extension/bin/codegraph-lsp-win32-x64.exe';
      });

      const { getServerPath } = await import('./server');
      const result = getServerPath(mockContext as any);

      expect(result).toBe('/test/extension/bin/codegraph-lsp-win32-x64.exe');
    });

    it('should throw error for unsupported platform', async () => {
      (os.platform as any).mockReturnValue('freebsd');
      (os.arch as any).mockReturnValue('x64');

      const { getServerPath } = await import('./server');

      expect(() => getServerPath(mockContext as any)).toThrow('Unsupported platform: freebsd');
    });
  });

  describe('fallback paths', () => {
    beforeEach(() => {
      (os.platform as any).mockReturnValue('darwin');
      (os.arch as any).mockReturnValue('arm64');
    });

    it('should first try the packaged binary path', async () => {
      (fs.existsSync as any).mockImplementation((path: string) => {
        return path === '/test/extension/bin/codegraph-lsp-darwin-arm64';
      });

      const { getServerPath } = await import('./server');
      const result = getServerPath(mockContext as any);

      expect(result).toBe('/test/extension/bin/codegraph-lsp-darwin-arm64');
    });

    it('should fall back to release target path for development', async () => {
      (fs.existsSync as any).mockImplementation((path: string) => {
        return path === '/test/extension/target/release/codegraph-lsp';
      });

      const { getServerPath } = await import('./server');
      const result = getServerPath(mockContext as any);

      expect(result).toBe('/test/extension/target/release/codegraph-lsp');
    });

    it('should fall back to Windows release target path', async () => {
      (os.platform as any).mockReturnValue('win32');
      (fs.existsSync as any).mockImplementation((path: string) => {
        return path === '/test/extension/target/release/codegraph-lsp.exe';
      });

      const { getServerPath } = await import('./server');
      const result = getServerPath(mockContext as any);

      expect(result).toBe('/test/extension/target/release/codegraph-lsp.exe');
    });

    it('should fall back to debug target path', async () => {
      (fs.existsSync as any).mockImplementation((path: string) => {
        return path === '/test/extension/target/debug/codegraph-lsp';
      });

      const { getServerPath } = await import('./server');
      const result = getServerPath(mockContext as any);

      expect(result).toBe('/test/extension/target/debug/codegraph-lsp');
    });

    it('should fall back to Windows debug target path', async () => {
      (os.platform as any).mockReturnValue('win32');
      (fs.existsSync as any).mockImplementation((path: string) => {
        return path === '/test/extension/target/debug/codegraph-lsp.exe';
      });

      const { getServerPath } = await import('./server');
      const result = getServerPath(mockContext as any);

      expect(result).toBe('/test/extension/target/debug/codegraph-lsp.exe');
    });

    it('should throw error when no binary is found', async () => {
      (fs.existsSync as any).mockReturnValue(false);

      const { getServerPath } = await import('./server');

      expect(() => getServerPath(mockContext as any)).toThrow(
        /CodeGraph LSP server binary not found/
      );
      expect(() => getServerPath(mockContext as any)).toThrow(
        /cargo build --release/
      );
    });
  });

  describe('path construction', () => {
    it('should use context.asAbsolutePath for all paths', async () => {
      (os.platform as any).mockReturnValue('linux');
      (os.arch as any).mockReturnValue('x64');
      (fs.existsSync as any).mockReturnValue(false);

      const { getServerPath } = await import('./server');

      try {
        getServerPath(mockContext as any);
      } catch {
        // Expected to throw
      }

      // Verify asAbsolutePath was called for various paths
      expect(mockContext.asAbsolutePath).toHaveBeenCalledWith(
        expect.stringContaining('bin')
      );
    });
  });
});
