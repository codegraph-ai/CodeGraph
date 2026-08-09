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
 * Resolution order mirrors `vscode/src/server.ts`. No client bundles platform
 * binaries any more (each engine is 100-126 MB), and the case against it is
 * strongest here: VS Code could at least ship one artifact per platform, while
 * the JetBrains Marketplace serves a single artifact to everyone, so a bundled
 * plugin would carry every published engine to every user. Instead the binary
 * is resolved from an existing install and, failing that, downloaded once into
 * the managed install directory (Phase 1).
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

    /**
     * Binary name for this platform, or null when no engine is published for it.
     *
     * macOS and Linux are both built for x64 and arm64, and are answered by
     * exact platform-arch match and nothing else: falling back to x64 on an
     * arm64 machine installs ~120 MB that cannot execute, which surfaces as an
     * exec-format error at first use instead of as the unsupported platform it
     * is. Windows on ARM is the one exception - it runs the x64 build under the
     * OS's own emulation layer, so refusing it would leave those users with no
     * engine at all.
     *
     * Mirrors `platformBinaryName()` in `mcp-package/bin/fetch-engine.js`, which
     * is the same rule for the JavaScript channels. The plugin cannot import
     * that list, so this mapping has to be edited in lockstep with it whenever a
     * platform is added or dropped.
     */
    fun platformBinaryNameOrNull(env: ResolverEnvironment = ResolverEnvironment.fromSystem()): String? {
        val os = env.osName.lowercase()
        val arch = env.osArch.lowercase()
        val isArm64 = arch in ARM64_ARCHES
        val isX64 = arch in X64_ARCHES
        return when {
            os.contains("mac") || os.contains("darwin") -> when {
                isArm64 -> "codegraph-server-darwin-arm64"
                isX64 -> "codegraph-server-darwin-x64"
                else -> null
            }
            os.contains("win") -> if (isX64 || isArm64) "codegraph-server-win32-x64.exe" else null
            os.contains("linux") -> when {
                isArm64 -> "codegraph-server-linux-arm64"
                isX64 -> "codegraph-server-linux-x64"
                else -> null
            }
            else -> null
        }
    }

    /** Binary name for this platform, for callers that treat "no build" as an error. */
    fun platformBinaryName(env: ResolverEnvironment = ResolverEnvironment.fromSystem()): String =
        platformBinaryNameOrNull(env) ?: throw UnsupportedPlatformException(env.osName, env.osArch)

    /** Where downloaded engines live. Shared with the CLI so installs are reused. */
    fun managedInstallDir(env: ResolverEnvironment = ResolverEnvironment.fromSystem()): Path =
        env.homeDir.resolve(".codegraph").resolve("bin")

    /** True when a managed install already exists, used to skip the download prompt. */
    fun hasManagedInstall(env: ResolverEnvironment = ResolverEnvironment.fromSystem()): Boolean =
        platformBinaryNameOrNull(env)
            ?.let { Files.isRegularFile(managedInstallDir(env).resolve(it)) }
            ?: false

    /**
     * Which release the managed install came from, or null when unknown.
     *
     * The engine is resolved by filename, which says nothing about which build
     * it is. Without this marker an engine installed by an older plugin is
     * indistinguishable from the one this plugin was built against, and gets
     * reused for good. Written by [EngineDownloader] and by the shared
     * JavaScript installer, which use the same file name.
     */
    fun managedEngineVersion(env: ResolverEnvironment = ResolverEnvironment.fromSystem()): String? =
        runCatching { Files.readString(managedInstallDir(env).resolve(VERSION_MARKER)).trim() }
            .getOrNull()
            ?.takeIf { it.isNotEmpty() }

    /**
     * True when the managed engine predates [expected] and is worth replacing.
     * An unmarked or unreadable version counts as stale: it predates the
     * marker, so which build it is cannot be established.
     *
     * Deliberately not `installed != expected`. The managed directory is shared
     * with the VS Code extension and the CLI, which ship through their own
     * channels on their own schedules, so finding a *newer* engine there is
     * normal. Treating that as a mismatch has each client reinstall its own
     * version over the other's on every launch, with a notification each time
     * and no way for the user to end it.
     */
    fun isManagedEngineStale(installed: String?, expected: String): Boolean {
        val order = compareVersions(installed ?: return true, expected) ?: return true
        return order < 0
    }

    /**
     * Release order of two versions, or null when either is not a plain numeric
     * version - a caller cannot tell older from newer then, and guessing is
     * what produces the loop [isManagedEngineStale] exists to avoid.
     */
    fun compareVersions(a: String, b: String): Int? {
        val left = versionParts(a) ?: return null
        val right = versionParts(b) ?: return null
        for (i in 0 until maxOf(left.size, right.size)) {
            val difference = (left.getOrNull(i) ?: 0).compareTo(right.getOrNull(i) ?: 0)
            if (difference != 0) return difference
        }
        return 0
    }

    /** Numeric release components, ignoring any prerelease suffix. */
    private fun versionParts(version: String): List<Int>? =
        version.trim()
            .substringBefore('-')
            .takeIf { it.isNotEmpty() }
            ?.split('.')
            ?.map { it.toIntOrNull() ?: return null }

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

        platformBinaryNameOrNull(env)
            ?.let { managedInstallDir(env).resolve(it) }
            ?.takeIf { it.isExecutableFile(env) }
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

    /** File name shared with the JavaScript installer, so all channels agree. */
    const val VERSION_MARKER = ".engine-version"

    /**
     * The engine release this plugin fetches, and the version a managed install
     * is expected to be.
     *
     * Deliberately not the plugin's own version. Release assets are tagged with
     * the *engine's* version (`scripts/publish-release-assets.sh` reads
     * Cargo.toml), so a plugin-only patch would ask for `v<plugin version>/…`
     * and get a 404 - and the plugin no longer bundles an engine to fall back
     * on. It is also what makes the shared `~/.codegraph/bin` marker meaningful:
     * all three clients compare it against the same number rather than against
     * three separately drifting client versions.
     *
     * Mirrors `ENGINE_VERSION` in `mcp-package/bin/fetch-engine.js`; both are
     * held equal to Cargo.toml by `scripts/publish-release-assets.sh`, which
     * refuses to publish while they disagree.
     */
    const val ENGINE_VERSION = "0.20.1"

    private val ARM64_ARCHES = setOf("aarch64", "arm64")
    private val X64_ARCHES = setOf("x86_64", "amd64", "x64")
}
