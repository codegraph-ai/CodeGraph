// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.server

import com.intellij.openapi.diagnostic.logger
import java.io.File
import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.Paths
import kotlin.io.path.isExecutable
import kotlin.io.path.isRegularFile

/**
 * Which build of the engine we resolved. Mirrors `ServerInfo.edition` in the
 * VS Code client so both report the same value to telemetry and the status bar.
 */
enum class ServerEdition { PRO, COMMUNITY }

/** A resolved engine binary plus how we found it. */
data class ResolvedServer(
    val path: Path,
    val edition: ServerEdition,
    /** Where the binary came from, for diagnostics and telemetry. */
    val origin: Origin,
) {
    enum class Origin { PRO_PATH, PRO_INSTALL_DIR, SYSTEM_PATH, MANAGED_INSTALL, CARGO_BUILD, USER_OVERRIDE }
}

/**
 * Everything about the machine that resolution depends on.
 *
 * Resolution reads the home directory, `PATH` and the OS/architecture, so
 * without this seam its tests would pass or fail according to whatever the
 * developer happens to have installed - which is exactly how the first version
 * of these tests broke.
 */
data class ResolverEnvironment(
    val homeDir: Path,
    val pathEntries: List<Path>,
    val osName: String,
    val osArch: String,
) {
    val isWindows: Boolean get() = osName.lowercase().contains("win")

    companion object {
        fun fromSystem(): ResolverEnvironment = ResolverEnvironment(
            homeDir = Paths.get(System.getProperty("user.home").orEmpty()),
            pathEntries = System.getenv("PATH").orEmpty()
                .split(File.pathSeparatorChar)
                .filter { it.isNotBlank() }
                .map { Paths.get(it) },
            osName = System.getProperty("os.name").orEmpty(),
            osArch = System.getProperty("os.arch").orEmpty(),
        )
    }
}

/**
 * Locates the `codegraph-server` engine binary.
 *
 * Resolution order mirrors `vscode/src/server.ts`, with one deliberate
 * difference: the JetBrains plugin does not bundle platform binaries. The VSIX
 * carries four of them (100-126 MB each) because VS Code can ship per-platform
 * artifacts; the JetBrains Marketplace cannot, so a bundled plugin would be a
 * ~120 MB download for every user regardless of platform. Instead the binary is
 * resolved from an existing install and, failing that, downloaded once into the
 * managed install directory (Phase 1).
 *
 * Order:
 *  1. Explicit user override (settings)
 *  2. CodeGraph Pro on PATH, then its known install directories
 *  3. `codegraph-server` on PATH (npm / homebrew installs)
 *  4. Previously downloaded binary under `~/.codegraph/bin`
 *  5. Cargo build outputs, for developing CodeGraph itself
 */
object CodeGraphServerResolver {
    private val LOG = logger<CodeGraphServerResolver>()

    class UnsupportedPlatformException(os: String, arch: String) :
        RuntimeException("CodeGraph does not ship an engine for $os/$arch")

    /** Binary name for this platform, matching the names published in releases. */
    fun platformBinaryName(env: ResolverEnvironment = ResolverEnvironment.fromSystem()): String {
        val os = env.osName.lowercase()
        val arch = env.osArch.lowercase()
        return when {
            os.contains("win") -> "codegraph-server-win32-x64.exe"
            os.contains("mac") || os.contains("darwin") ->
                if (arch == "aarch64" || arch == "arm64") {
                    "codegraph-server-darwin-arm64"
                } else {
                    "codegraph-server-darwin-x64"
                }
            os.contains("linux") -> "codegraph-server-linux-x64"
            else -> throw UnsupportedPlatformException(env.osName, env.osArch)
        }
    }

    /** Where downloaded engines live. Shared with the CLI so installs are reused. */
    fun managedInstallDir(env: ResolverEnvironment = ResolverEnvironment.fromSystem()): Path =
        env.homeDir.resolve(".codegraph").resolve("bin")

    /** True when a managed install already exists, used to skip the download prompt. */
    fun hasManagedInstall(env: ResolverEnvironment = ResolverEnvironment.fromSystem()): Boolean =
        Files.isRegularFile(managedInstallDir(env).resolve(platformBinaryName(env)))

    /**
     * Resolve the engine, or return null when nothing is installed yet. A null
     * result is a normal first-run state, not an error: the caller offers the
     * download instead of failing activation.
     *
     * @param projectBasePath used only to find cargo build outputs when the
     *   open project *is* the CodeGraph repo.
     * @param override an explicit path from settings; when set and valid it wins.
     */
    fun resolve(
        projectBasePath: String?,
        override: String? = null,
        env: ResolverEnvironment = ResolverEnvironment.fromSystem(),
    ): ResolvedServer? {
        override?.takeIf { it.isNotBlank() }?.let { raw ->
            val path = Paths.get(raw)
            if (path.isExecutableFile(env)) {
                return ResolvedServer(path, editionForName(path), ResolvedServer.Origin.USER_OVERRIDE)
            }
            // A stale path in settings must not brick the plugin: warn and keep
            // looking, which is what a user who just moved the binary expects.
            LOG.warn("Configured CodeGraph engine path is not an executable file: $raw")
        }

        findProBinary(env)?.let { return it }

        findOnPath(if (env.isWindows) "codegraph-server.exe" else "codegraph-server", env)?.let {
            return ResolvedServer(it, ServerEdition.COMMUNITY, ResolvedServer.Origin.SYSTEM_PATH)
        }

        managedInstallDir(env).resolve(platformBinaryName(env))
            .takeIf { it.isExecutableFile(env) }
            ?.let { return ResolvedServer(it, ServerEdition.COMMUNITY, ResolvedServer.Origin.MANAGED_INSTALL) }

        findCargoBuild(projectBasePath, env)?.let {
            return ResolvedServer(it, ServerEdition.COMMUNITY, ResolvedServer.Origin.CARGO_BUILD)
        }
        return null
    }

    private fun editionForName(path: Path): ServerEdition =
        if (path.fileName.toString().startsWith("codegraph-pro")) ServerEdition.PRO else ServerEdition.COMMUNITY

    private fun findProBinary(env: ResolverEnvironment): ResolvedServer? {
        val name = if (env.isWindows) "codegraph-pro.exe" else "codegraph-pro"
        findOnPath(name, env)?.let {
            return ResolvedServer(it, ServerEdition.PRO, ResolvedServer.Origin.PRO_PATH)
        }
        val candidates = listOf(
            env.homeDir.resolve(".codegraph-pro").resolve("bin").resolve(name),
            env.homeDir.resolve(".local").resolve("bin").resolve(name),
            Paths.get("/usr/local/bin", name),
        )
        return candidates.firstOrNull { it.isExecutableFile(env) }
            ?.let { ResolvedServer(it, ServerEdition.PRO, ResolvedServer.Origin.PRO_INSTALL_DIR) }
    }

    /**
     * PATH lookup done in-process. The VS Code client shells out to
     * `which`/`where`; doing it here avoids spawning a shell entirely, which
     * also sidesteps the Windows path-with-spaces class of bug (issue #2).
     */
    private fun findOnPath(binaryName: String, env: ResolverEnvironment): Path? {
        val extensions = if (env.isWindows) listOf("", ".exe", ".cmd", ".bat") else listOf("")
        return env.pathEntries
            .asSequence()
            .flatMap { dir ->
                extensions.asSequence().map { ext ->
                    dir.resolve(if (binaryName.endsWith(ext)) binaryName else binaryName + ext)
                }
            }
            .firstOrNull { it.isExecutableFile(env) }
    }

    /**
     * Cargo build outputs, for contributors running the plugin against a
     * locally built engine. Release is preferred over debug: a contributor who
     * has both almost always means the optimised one, and a debug engine
     * indexes slowly enough to look like a hang.
     */
    private fun findCargoBuild(projectBasePath: String?, env: ResolverEnvironment): Path? {
        val base = projectBasePath?.let { Paths.get(it) } ?: return null
        val exe = if (env.isWindows) ".exe" else ""
        val candidates = listOf(
            base.resolve("target/release/codegraph-server$exe"),
            base.resolve("target/debug/codegraph-server$exe"),
            // The plugin may be opened with `jetbrains/` itself as the project root.
            base.resolve("../target/release/codegraph-server$exe"),
            base.resolve("../target/debug/codegraph-server$exe"),
        )
        return candidates.firstOrNull { it.isExecutableFile(env) }?.normalize()
    }

    /**
     * Windows has no executable bit, so file-ness is the only check available
     * there; on POSIX both must hold.
     */
    private fun Path.isExecutableFile(env: ResolverEnvironment): Boolean =
        try {
            isRegularFile() && (env.isWindows || isExecutable())
        } catch (_: SecurityException) {
            false
        }
}
