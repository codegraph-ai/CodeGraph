// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.server

import com.intellij.testFramework.fixtures.BasePlatformTestCase
import java.nio.file.Files
import java.nio.file.Path

/**
 * Resolution-order tests.
 *
 * The resolver decides which engine a user actually runs, and its failure mode
 * is silent: picking a stale cargo build over an installed release, or
 * reporting "not found" while a perfectly good binary sits on PATH.
 *
 * Every case runs against a synthetic [ResolverEnvironment] rooted in a temp
 * directory. Reading the real home directory and PATH would make these tests
 * agree with whatever the developer happens to have installed.
 */
class CodeGraphServerResolverTest : BasePlatformTestCase() {

    private lateinit var tempDir: Path
    private lateinit var fakeHome: Path

    override fun setUp() {
        super.setUp()
        tempDir = Files.createTempDirectory("codegraph-resolver-test")
        fakeHome = Files.createDirectories(tempDir.resolve("home"))
    }

    override fun tearDown() {
        try {
            tempDir.toFile().deleteRecursively()
        } finally {
            super.tearDown()
        }
    }

    /** A macOS/arm64 machine with an empty PATH and an empty home directory. */
    private fun env(pathEntries: List<Path> = emptyList()) = ResolverEnvironment(
        homeDir = fakeHome,
        pathEntries = pathEntries,
        osName = "Mac OS X",
        osArch = "aarch64",
    )

    private fun executableAt(relative: String): Path {
        val path = tempDir.resolve(relative)
        Files.createDirectories(path.parent)
        Files.createFile(path)
        check(path.toFile().setExecutable(true)) { "could not mark $path executable" }
        return path
    }

    private fun projectRoot() = tempDir.resolve("project").toString()

    fun `test explicit override wins over every other candidate`() {
        val override = executableAt("custom/codegraph-server")
        executableAt("project/target/release/codegraph-server")

        val resolved = CodeGraphServerResolver.resolve(projectRoot(), override.toString(), env())

        assertNotNull(resolved)
        assertEquals(override, resolved!!.path)
        assertEquals(ResolvedServer.Origin.USER_OVERRIDE, resolved.origin)
    }

    fun `test unusable override falls through instead of failing`() {
        val cargoBuild = executableAt("project/target/release/codegraph-server")

        val resolved = CodeGraphServerResolver.resolve(
            projectRoot(),
            tempDir.resolve("does-not-exist").toString(),
            env(),
        )

        assertNotNull(resolved)
        assertEquals(cargoBuild, resolved!!.path)
        assertEquals(ResolvedServer.Origin.CARGO_BUILD, resolved.origin)
    }

    fun `test release build is preferred over debug build`() {
        executableAt("project/target/debug/codegraph-server")
        val release = executableAt("project/target/release/codegraph-server")

        val resolved = CodeGraphServerResolver.resolve(projectRoot(), null, env())

        assertNotNull(resolved)
        assertEquals(release, resolved!!.path)
    }

    fun `test PATH install is preferred over a managed download`() {
        val binDir = tempDir.resolve("usr-bin")
        Files.createDirectories(binDir)
        val onPath = executableAt("usr-bin/codegraph-server")
        executableAt("home/.codegraph/bin/codegraph-server-darwin-arm64")

        val resolved = CodeGraphServerResolver.resolve(projectRoot(), null, env(listOf(binDir)))

        assertNotNull(resolved)
        assertEquals(onPath, resolved!!.path)
        assertEquals(ResolvedServer.Origin.SYSTEM_PATH, resolved.origin)
    }

    fun `test managed download is preferred over a cargo build`() {
        val managed = executableAt("home/.codegraph/bin/codegraph-server-darwin-arm64")
        executableAt("project/target/release/codegraph-server")

        val resolved = CodeGraphServerResolver.resolve(projectRoot(), null, env())

        assertNotNull(resolved)
        assertEquals(managed, resolved!!.path)
        assertEquals(ResolvedServer.Origin.MANAGED_INSTALL, resolved.origin)
    }

    fun `test pro binary outranks a community install on PATH`() {
        val binDir = tempDir.resolve("usr-bin")
        Files.createDirectories(binDir)
        executableAt("usr-bin/codegraph-server")
        val pro = executableAt("home/.codegraph-pro/bin/codegraph-pro")

        val resolved = CodeGraphServerResolver.resolve(projectRoot(), null, env(listOf(binDir)))

        assertNotNull(resolved)
        assertEquals(pro, resolved!!.path)
        assertEquals(ServerEdition.PRO, resolved.edition)
        assertEquals(ResolvedServer.Origin.PRO_INSTALL_DIR, resolved.origin)
    }

    fun `test nothing installed resolves to null rather than throwing`() {
        assertNull(CodeGraphServerResolver.resolve(projectRoot(), null, env()))
    }

    fun `test no project open still resolves an installed engine`() {
        val managed = executableAt("home/.codegraph/bin/codegraph-server-darwin-arm64")

        val resolved = CodeGraphServerResolver.resolve(null, null, env())

        assertNotNull(resolved)
        assertEquals(managed, resolved!!.path)
    }

    fun `test platform binary name follows os and architecture`() {
        fun nameFor(os: String, arch: String) = CodeGraphServerResolver.platformBinaryName(
            ResolverEnvironment(fakeHome, emptyList(), os, arch),
        )

        assertEquals("codegraph-server-darwin-arm64", nameFor("Mac OS X", "aarch64"))
        assertEquals("codegraph-server-darwin-x64", nameFor("Mac OS X", "x86_64"))
        assertEquals("codegraph-server-linux-x64", nameFor("Linux", "amd64"))
        assertEquals("codegraph-server-win32-x64.exe", nameFor("Windows 11", "amd64"))
    }

    fun `test unsupported platform is reported rather than guessed`() {
        assertThrows(CodeGraphServerResolver.UnsupportedPlatformException::class.java) {
            CodeGraphServerResolver.platformBinaryName(
                ResolverEnvironment(fakeHome, emptyList(), "AIX", "ppc64"),
            )
        }
    }

    fun `test arm64 linux has no published engine but arm64 windows emulates x64`() {
        // Handing the x64 asset to an arm64 Linux machine installs something
        // that cannot execute, which shows up as an exec-format error rather
        // than as the missing build it is. Windows on ARM is the exception: it
        // runs x64 binaries under the OS's own emulation, so refusing there
        // would leave those users with no engine for no reason.
        fun nameFor(os: String, arch: String) = CodeGraphServerResolver.platformBinaryNameOrNull(
            ResolverEnvironment(fakeHome, emptyList(), os, arch),
        )

        assertNull(nameFor("Linux", "aarch64"))
        assertNull(nameFor("Linux", "arm64"))
        assertEquals("codegraph-server-win32-x64.exe", nameFor("Windows 11", "aarch64"))
        assertEquals("codegraph-server-win32-x64.exe", nameFor("Windows 11", "arm64"))
        assertEquals("codegraph-server-linux-x64", nameFor("Linux", "x86_64"))
        assertEquals("codegraph-server-darwin-arm64", nameFor("Mac OS X", "aarch64"))
    }

    fun `test only an older managed engine counts as stale`() {
        // The managed directory is shared with the VS Code extension, which
        // ships on its own schedule. Treating "different" as "stale" makes the
        // two clients reinstall over each other on every launch.
        assertTrue(CodeGraphServerResolver.isManagedEngineStale("0.19.1", "0.20.0"))
        assertFalse(CodeGraphServerResolver.isManagedEngineStale("0.20.0", "0.20.0"))
        assertFalse(CodeGraphServerResolver.isManagedEngineStale("0.21.0", "0.20.0"))
        assertFalse(CodeGraphServerResolver.isManagedEngineStale("0.20", "0.20.0"))

        // Nothing to compare against is the one case worth replacing: an
        // unmarked install predates the marker, so its build is unknown.
        assertTrue(CodeGraphServerResolver.isManagedEngineStale(null, "0.20.0"))
        assertTrue(CodeGraphServerResolver.isManagedEngineStale("nightly", "0.20.0"))
    }

    fun `test resolution on an unpublished platform reports nothing rather than throwing`() {
        // A null resolve sends the caller to the "offer a download" path; an
        // exception here would escape project startup instead.
        val armLinux = ResolverEnvironment(fakeHome, emptyList(), "Linux", "aarch64")

        assertNull(CodeGraphServerResolver.resolve(projectRoot(), null, armLinux))
        assertFalse(CodeGraphServerResolver.hasManagedInstall(armLinux))
    }

    fun `test the managed install records which release it came from`() {
        assertNull("no marker means no known version", CodeGraphServerResolver.managedEngineVersion(env()))

        val marker = fakeHome.resolve(".codegraph/bin/${CodeGraphServerResolver.VERSION_MARKER}")
        Files.createDirectories(marker.parent)
        Files.writeString(marker, "0.20.0\n")

        assertEquals("0.20.0", CodeGraphServerResolver.managedEngineVersion(env()))
    }

    private fun assertThrows(expected: Class<out Throwable>, block: () -> Unit) {
        try {
            block()
        } catch (error: Throwable) {
            assertTrue("expected ${expected.name} but got $error", expected.isInstance(error))
            return
        }
        fail("expected ${expected.name} but nothing was thrown")
    }
}
