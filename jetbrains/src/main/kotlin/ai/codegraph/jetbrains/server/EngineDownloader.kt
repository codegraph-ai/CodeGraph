// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.server

import com.intellij.openapi.diagnostic.logger
import com.intellij.openapi.progress.ProgressIndicator
import com.intellij.util.io.HttpRequests
import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.StandardCopyOption
import java.security.MessageDigest
import java.util.Locale

/**
 * Fetches the engine for this platform into the managed install directory.
 *
 * The plugin does not bundle engines: the JetBrains Marketplace serves one
 * artifact to every platform, so bundling all four would mean a ~120 MB
 * download for every user to obtain the ~30 MB they can run. The alternative
 * for users without Node is worse - install a 498 MB npm package for one
 * binary - so the engine is fetched directly from the release that
 * `scripts/publish-release-assets.sh` produces.
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
     * Download and install the engine for [version], returning its path.
     *
     * Each file is staged next to its destination and moved into place only
     * after its checksum matches, so an interrupted or corrupted download can
     * never leave something behind that later looks like a valid install.
     */
    fun download(version: String, indicator: ProgressIndicator? = null): Path {
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

        assets.forEachIndexed { index, asset ->
            indicator?.text = "Downloading the CodeGraph engine ($version): $asset"
            indicator?.fraction = index.toDouble() / assets.size
            fetchVerified(version, asset, targetDir, indicator)
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

    private fun fetchVerified(version: String, asset: String, targetDir: Path, indicator: ProgressIndicator?) {
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
            Files.move(staged, targetDir.resolve(asset), StandardCopyOption.REPLACE_EXISTING)
        } finally {
            runCatching { Files.deleteIfExists(staged) }
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
