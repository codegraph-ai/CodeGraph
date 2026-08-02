// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.server

import com.intellij.testFramework.fixtures.BasePlatformTestCase
import com.sun.net.httpserver.HttpServer
import java.net.InetSocketAddress
import java.nio.file.Files
import java.nio.file.Path
import java.security.MessageDigest

/**
 * Serves a fake release over loopback and downloads from it.
 *
 * The interesting cases are the destructive ones: a corrupted transfer must not
 * leave anything installed, and Windows must not end up with an engine and no
 * `onnxruntime.dll` - a download that succeeds and then fails at startup is
 * worse than one that visibly fails.
 */
class EngineDownloaderTest : BasePlatformTestCase() {

    private lateinit var server: HttpServer
    private lateinit var home: Path
    private val assets = mutableMapOf<String, ByteArray>()

    override fun setUp() {
        super.setUp()
        home = Files.createTempDirectory("codegraph-download-test")
        server = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0)
        server.createContext("/") { exchange ->
            val body = assets[exchange.requestURI.path]
            if (body == null) {
                exchange.sendResponseHeaders(404, -1)
            } else {
                exchange.sendResponseHeaders(200, body.size.toLong())
                exchange.responseBody.use { it.write(body) }
            }
            exchange.close()
        }
        server.start()
    }

    override fun tearDown() {
        try {
            server.stop(0)
            home.toFile().deleteRecursively()
        } finally {
            super.tearDown()
        }
    }

    private fun baseUrl() = "http://127.0.0.1:${server.address.port}/releases/download"

    /** Publish an asset and its checksum, exactly as the release script lays them out. */
    private fun publish(version: String, name: String, content: ByteArray, checksum: String? = null) {
        assets["/releases/download/v$version/$name"] = content
        val digest = checksum ?: MessageDigest.getInstance("SHA-256").digest(content)
            .joinToString("") { "%02x".format(it) }
        assets["/releases/download/v$version/$name.sha256"] = "$digest  $name\n".toByteArray()
    }

    private fun env(os: String, arch: String = "aarch64") =
        ResolverEnvironment(homeDir = home, pathEntries = emptyList(), osName = os, osArch = arch)

    /**
     * Windows and Linux are published for x64 only, so an arm64 environment
     * there is an unsupported platform rather than a machine that downloads the
     * x64 build.
     */
    private fun downloader(os: String, arch: String = "aarch64") =
        EngineDownloader(env(os, arch), baseUrl())

    fun `test downloads and installs the engine for this platform`() {
        val content = "engine".toByteArray()
        publish("0.19.1", "codegraph-server-darwin-arm64", content)

        val path = downloader("Mac OS X").download("0.19.1")

        assertEquals(String(content), Files.readString(path))
        assertTrue("the engine must be executable", path.toFile().canExecute())
    }

    fun `test windows also installs the runtime library the engine loads`() {
        publish("0.19.1", "codegraph-server-win32-x64.exe", "engine".toByteArray())
        publish("0.19.1", EngineDownloader.WINDOWS_SIDECAR, "onnx".toByteArray())

        val path = downloader("Windows 11", "amd64").download("0.19.1")

        assertTrue(Files.exists(path))
        assertTrue(
            "without the sidecar the engine downloads fine and then fails to start",
            Files.exists(path.parent.resolve(EngineDownloader.WINDOWS_SIDECAR)),
        )
    }

    fun `test the installed release is recorded next to the engine`() {
        // Resolution finds the engine by file name, which says nothing about
        // which build it is. Without this marker an engine installed by an
        // older plugin is reused for good.
        publish("0.19.1", "codegraph-server-darwin-arm64", "engine".toByteArray())

        val path = downloader("Mac OS X").download("0.19.1")

        assertEquals(
            "0.19.1",
            Files.readString(path.parent.resolve(CodeGraphServerResolver.VERSION_MARKER)).trim(),
        )
        assertEquals("0.19.1", CodeGraphServerResolver.managedEngineVersion(env("Mac OS X")))
    }

    fun `test a failed download records no version`() {
        publish("0.19.1", "codegraph-server-darwin-arm64", "engine".toByteArray(), checksum = "0".repeat(64))

        runCatching { downloader("Mac OS X").download("0.19.1") }

        assertNull(
            "a marker written before the assets verify would claim an install that never happened",
            CodeGraphServerResolver.managedEngineVersion(env("Mac OS X")),
        )
    }

    fun `test a corrupted download installs nothing`() {
        publish("0.19.1", "codegraph-server-darwin-arm64", "engine".toByteArray(), checksum = "0".repeat(64))

        val failure = runCatching { downloader("Mac OS X").download("0.19.1") }.exceptionOrNull()

        assertTrue(
            "expected a checksum failure, got $failure",
            failure is EngineDownloader.ChecksumMismatchException,
        )
        assertFalse(
            "a mismatched engine must not be left on disk",
            Files.exists(CodeGraphServerResolver.managedInstallDir(env("Mac OS X")).resolve("codegraph-server-darwin-arm64")),
        )
    }

    fun `test a failed download leaves no partial files behind`() {
        publish("0.19.1", "codegraph-server-darwin-arm64", "engine".toByteArray(), checksum = "0".repeat(64))

        runCatching { downloader("Mac OS X").download("0.19.1") }

        val leftovers = Files.list(CodeGraphServerResolver.managedInstallDir(env("Mac OS X"))).use { it.toList() }
        assertTrue("staging files must be cleaned up, found $leftovers", leftovers.isEmpty())
    }

    fun `test a missing release surfaces rather than installing something wrong`() {
        // Nothing published for this version at all.
        val failure = runCatching { downloader("Mac OS X").download("9.9.9") }.exceptionOrNull()

        assertNotNull("a missing release must fail loudly", failure)
    }

    fun `test windows failing on the sidecar does not leave a half install`() {
        // Engine publishes fine, sidecar is corrupt: the install must not be
        // reported as usable.
        publish("0.19.1", "codegraph-server-win32-x64.exe", "engine".toByteArray())
        publish("0.19.1", EngineDownloader.WINDOWS_SIDECAR, "onnx".toByteArray(), checksum = "0".repeat(64))

        val failure = runCatching { downloader("Windows 11", "amd64").download("0.19.1") }.exceptionOrNull()

        assertTrue(failure is EngineDownloader.ChecksumMismatchException)
        assertFalse(
            Files.exists(
                CodeGraphServerResolver.managedInstallDir(env("Windows 11"))
                    .resolve(EngineDownloader.WINDOWS_SIDECAR),
            ),
        )
    }
}
