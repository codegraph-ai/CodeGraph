// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.telemetry

import java.util.Properties

/**
 * The analytics endpoint, injected at build time.
 *
 * The key comes from `CODEGRAPH_POSTHOG_KEY` in the release build environment
 * and is absent everywhere else, which is the point: a developer build, a fork,
 * or anyone building from source reports nothing at all, with no setting to
 * remember to turn off.
 */
object TelemetryConfig {

    private val properties: Properties = Properties().apply {
        TelemetryConfig::class.java.getResourceAsStream(RESOURCE)?.use { load(it) }
    }

    val key: String = properties.getProperty("posthogKey").orEmpty()

    val host: String = properties.getProperty("posthogHost")
        ?.takeIf { it.isNotBlank() }
        ?: "https://us.posthog.com"

    /** No key means the whole reporter is inert. */
    val hasKey: Boolean get() = key.isNotBlank()

    private const val RESOURCE = "/codegraph-telemetry.properties"
}
