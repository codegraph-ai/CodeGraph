// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// All rights reserved. Proprietary and confidential.

using Microsoft.VisualStudio.LanguageServer.Client;
using Microsoft.VisualStudio.Shell;
using Microsoft.VisualStudio.Threading;
using Microsoft.VisualStudio.Utilities;
using System;
using System.Collections.Generic;
using System.ComponentModel.Composition;
using System.Diagnostics;
using System.IO;
using System.Reflection;
using System.Threading;
using System.Threading.Tasks;

namespace CodegraphExtension
{
    [ContentType("CSharp")]
    [ContentType("C/C++")]
    [Export(typeof(ILanguageClient))]
    public class CodegraphLanguageClient : ILanguageClient
    {
        public string Name => "CodeGraph";

        public IEnumerable<string> ConfigurationSections => null;

        public object InitializationOptions => new
        {
            embeddingModel = "bge-small"
        };

        public IEnumerable<string> FilesToWatch => null;

        public bool ShowNotificationOnInitializeFailed => true;

        public event AsyncEventHandler<EventArgs> StartAsync;
#pragma warning disable CS0067
        public event AsyncEventHandler<EventArgs> StopAsync;
#pragma warning restore CS0067

        private Process _serverProcess;

        public async Task<Connection> ActivateAsync(CancellationToken token)
        {
            var serverPath = FindServerBinary();

            var startInfo = new ProcessStartInfo
            {
                FileName = serverPath,
                Arguments = "--stdio",
                RedirectStandardInput = true,
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                UseShellExecute = false,
                CreateNoWindow = true,
            };

            _serverProcess = Process.Start(startInfo);

            _serverProcess.ErrorDataReceived += (sender, e) =>
            {
                if (!string.IsNullOrEmpty(e.Data))
                {
                    Debug.WriteLine($"[CodeGraph] {e.Data}");
                }
            };
            _serverProcess.BeginErrorReadLine();

            return new Connection(
                _serverProcess.StandardOutput.BaseStream,
                _serverProcess.StandardInput.BaseStream);
        }

        public async Task OnLoadedAsync()
        {
            if (StartAsync != null)
            {
                await StartAsync.InvokeAsync(this, EventArgs.Empty);
            }
        }

        public Task<InitializationFailureContext> OnServerInitializeFailedAsync(ILanguageClientInitializationInfo initializationState)
        {
            var message = initializationState.InitializationException?.Message ?? "Unknown error";
            ActivityLog.LogError("CodeGraph", $"Server initialization failed: {message}");
            return Task.FromResult<InitializationFailureContext>(null);
        }

        public Task OnServerInitializedAsync()
        {
            ActivityLog.LogInformation("CodeGraph", "Server initialized successfully");
            return Task.CompletedTask;
        }

        private string FindServerBinary()
        {
            // 1. Bundled with extension
            var extensionDir = Path.GetDirectoryName(Assembly.GetExecutingAssembly().Location);
            var bundledPath = Path.Combine(extensionDir, "server", "codegraph-server.exe");
            if (File.Exists(bundledPath))
                return bundledPath;

            // 2. npm global install
            var npmPath = FindInPath("codegraph-mcp.cmd");
            if (npmPath != null)
                return npmPath;

            // 3. Pro binary
            var proPath = FindInPath("codegraph-pro.exe");
            if (proPath != null)
                return proPath;

            throw new FileNotFoundException(
                "CodeGraph server binary not found.\n" +
                "Install via: npm install -g @astudioplus/codegraph-mcp\n" +
                "Or download from: https://github.com/codegraph-ai/CodeGraph/releases");
        }

        private static string FindInPath(string executable)
        {
            var pathDirs = Environment.GetEnvironmentVariable("PATH")?.Split(';') ?? Array.Empty<string>();
            foreach (var dir in pathDirs)
            {
                var fullPath = Path.Combine(dir.Trim(), executable);
                if (File.Exists(fullPath))
                    return fullPath;
            }
            return null;
        }
    }
}
