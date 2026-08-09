// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.server

import com.intellij.openapi.diagnostic.logger
import com.intellij.openapi.progress.ProgressIndicator
import com.intellij.util.io.HttpRequests
import java.nio.file.AccessDeniedException
import java.nio.file.FileSystemException
import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.StandardCopyOption
import java.security.MessageDigest
import java.util.Locale

/**
 * Fetches the engine for this platform into the managed install directory.
 *
 * The plugin does not bundle engines: the JetBrains Marketplace serves one
 * artifact to every platform, so bundling every published engine would mean a
 * download several times the size of the one a given user can run. Sending
 * users to the npm package instead is not an answer either: it needs Node, and
 * it fetches the same engine from the same release. So the engine is fetched
 * directly from the release that `scripts/publish-release-assets.sh` produces.
 *
 * Downloads are verified against the checksum published beside each asset. An
 * engine is a native binary that runs with the user's permissions; TLS says
 * nothing about a mirror, a proxy, or a truncated transfer.
 */
class EngineDownloader(
    private val env: ResolverEnvironment = ResolverEnvironment.fromSystem(),
    private val releaseBaseUrl: String = DEFAULT_RELEASE_BASE_URL,
) {

    class ChecksumMismatchException(asset: String, expected: String, actual: String) : RuntimeException(
        "$asset failed checksum verification (expected $expected, got $actual). The download was discarded.",
    )

    /**
     * Raised when the engine on disk cannot be replaced because something still
     * holds it open - on Windows, a running engine holds its own `.exe`.
     * Distinguished from an ordinary I/O failure because the answer is
     * different: stop the engine, then try again.
     */
    class EngineInUseException(asset: String, cause: Throwable) : RuntimeException(
        "$asset could not be replaced because it is in use. Stop the CodeGraph engine and try again.",
        cause,
    )

    /**
     * Download and install the engine for [version], returning its path.
     *
     * Every file is staged next to its destination and verified before anything
     * is moved into place, so an interrupted or corrupted download can never
     * leave something behind that later looks like a valid install - and a
     * Windows install cannot end up with a new `.exe` beside the old sidecar.
     *
     * [beforeInstall] runs after the last download is verified and before the
     * first file is moved. The engine holds its own binary open while it runs,
     * so the caller uses this to stop it - at the last possible moment, since
     * stopping it for the length of a transfer that may fail costs the user a
     * working engine for nothing.
     */
    fun download(
        version: String,
        indicator: ProgressIndicator? = null,
        beforeInstall: () -> Unit = {},
    ): Path {
        val binaryName = CodeGraphServerResolver.platformBinaryName(env)
        val targetDir = CodeGraphServerResolver.managedInstallDir(env)
        Files.createDirectories(targetDir)

        // Windows loads onnxruntime.dll at runtime. Fetching only the exe
        // produces a download that succeeds and then fails at startup - the
        // npm packaging script warns about exactly this - so the sidecar is
        // part of the install, not an afterthought.
        val assets = buildList {
            add(binaryName)
            if (env.isWindows) add(WINDOWS_SIDECAR)
        }

        val staged = LinkedHashMap<String, Path>()
        try {
            assets.forEachIndexed { index, asset ->
                indicator?.text = "Downloading the CodeGraph engine ($version): $asset"
                indicator?.fraction = index.toDouble() / assets.size
                staged[asset] = fetchVerified(version, asset, targetDir, indicator)
            }
            beforeInstall()
            staged.forEach { (asset, file) -> install(file, targetDir.resolve(asset), asset) }
        } finally {
            staged.values.forEach { runCatching { Files.deleteIfExists(it) } }
        }

        val engine = targetDir.resolve(binaryName)
        engine.toFile().setExecutable(true, /* ownerOnly = */ true)
        // Written only once every asset has been verified and moved into place:
        // a marker recorded earlier would claim an install a later failure
        // never completed. Without it the engine is identified by filename
        // alone, and one left by an older plugin is reused for good.
        Files.writeString(targetDir.resolve(CodeGraphServerResolver.VERSION_MARKER), "$version\n")
        LOG.info("Installed CodeGraph engine $version at $engine")
        return engine
    }

    /** Downloads and verifies one asset, returning the staged file. */
    private fun fetchVerified(
        version: String,
        asset: String,
        targetDir: Path,
        indicator: ProgressIndicator?,
    ): Path {
        val assetUrl = "$releaseBaseUrl/v$version/$asset"
        val expected = fetchChecksum("$assetUrl.sha256")

        val staged = Files.createTempFile(targetDir, "$asset.", ".partial")
        try {
            HttpRequests.request(assetUrl)
                .productNameAsUserAgent()
                .saveToFile(staged, indicator)

            val actual = sha256(staged)
            if (!actual.equals(expected, ignoreCase = true)) {
                throw ChecksumMismatchException(asset, expected, actual)
            }
        } catch (error: Throwable) {
            // A file the caller was never handed back is this function's to
            // clean up; leaving it behind is how a failed download turns into
            // stray `.partial` files in the user's install directory.
            runCatching { Files.deleteIfExists(staged) }
            throw error
        }
        return staged
    }

    private fun install(staged: Path, destination: Path, asset: String) {
        try {
            Files.move(staged, destination, StandardCopyOption.REPLACE_EXISTING)
        } catch (e: FileSystemException) {
            // Windows refuses to replace a file another process holds open, and
            // reports that as AccessDenied or as a sharing violation depending
            // on the call. Reporting either as a failed download sends the user
            // to debug a network they have no problem with.
            val locked = e is AccessDeniedException ||
                e.reason?.contains("another process", ignoreCase = true) == true
            throw if (locked) EngineInUseException(asset, e) else e
        }
    }

    /**
     * The checksum file is `<digest>  <filename>`, the format `shasum -a 256`
     * and `sha256sum` both write. Only the digest matters here.
     */
    private fun fetchChecksum(url: String): String =
        HttpRequests.request(url)
            .productNameAsUserAgent()
            .readString()
            .trim()
            .substringBefore(' ')
            .lowercase(Locale.ROOT)

    private fun sha256(file: Path): String {
        val digest = MessageDigest.getInstance("SHA-256")
        Files.newInputStream(file).use { stream ->
            val buffer = ByteArray(DIGEST_BUFFER_BYTES)
            while (true) {
                val read = stream.read(buffer)
                if (read < 0) break
                digest.update(buffer, 0, read)
            }
        }
        return digest.digest().joinToString("") { byte -> "%02x".format(byte) }
    }

    companion object {
        private val LOG = logger<EngineDownloader>()

        /**
         * Matches the tag scheme in `scripts/publish-release-assets.sh` and the
         * repository name used by the npm package's model fetch - the casing is
         * load-bearing on a case-sensitive redirect.
         */
        const val DEFAULT_RELEASE_BASE_URL =
            "https://github.com/codegraph-ai/CodeGraph/releases/download"

        const val WINDOWS_SIDECAR = "onnxruntime.dll"

        private const val DIGEST_BUFFER_BYTES = 1 shl 16
    }
}
