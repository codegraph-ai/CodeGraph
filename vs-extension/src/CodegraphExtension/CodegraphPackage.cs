// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// All rights reserved. Proprietary and confidential.

using Microsoft.VisualStudio.Shell;
using System;
using System.Runtime.InteropServices;
using System.Threading;
using System.Threading.Tasks;

namespace CodegraphExtension
{
    /// <summary>
    /// VS Package entry point for CodeGraph extension.
    /// Registers commands, tool windows, and initializes the extension.
    /// </summary>
    [PackageRegistration(UseManagedResourcesOnly = true, AllowsBackgroundLoading = true)]
    [Guid("a1b2c3d4-e5f6-7890-abcd-ef1234567890")]
    [ProvideAutoLoad(Microsoft.VisualStudio.Shell.Interop.UIContextGuids80.SolutionExists,
        PackageAutoLoadFlags.BackgroundLoad)]
    public sealed class CodegraphPackage : AsyncPackage
    {
        protected override async Task InitializeAsync(
            CancellationToken cancellationToken,
            IProgress<ServiceProgressData> progress)
        {
            await JoinableTaskFactory.SwitchToMainThreadAsync(cancellationToken);

            ActivityLog.LogInformation("CodeGraph",
                "CodeGraph extension initialized. Server will start when a supported file is opened.");
        }
    }
}
