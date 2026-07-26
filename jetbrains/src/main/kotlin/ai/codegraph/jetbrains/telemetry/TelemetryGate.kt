// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.telemetry

/**
 * Whether one event may be sent.
 *
 * Kept as a pure function, separate from any transport or IDE service, because
 * this is the code where a mistake means sending data from someone who asked
 * not to be measured. That deserves to be readable and directly testable rather
 * than tangled in a class that needs a running IDE to exercise.
 *
 * Every gate must pass; they are deliberately expressed as reasons to refuse.
 */
object TelemetryGate {

    /**
     * @param hasKey false when no PostHog key was compiled in - the default for
     *   any build that is not an official release, so a local or forked build
     *   reports nothing at all.
     * @param ideConsent the IDE-level "send usage statistics" consent. The
     *   plugin's own switch can only ever narrow this, never widen it.
     * @param pluginEnabled the plugin's `telemetry.enabled` setting.
     * @param errorReportsOnly the plugin's `telemetry.errorReportsOnly` setting.
     * @param isErrorEvent whether the event being considered reports a failure.
     */
    fun allows(
        hasKey: Boolean,
        ideConsent: Boolean,
        pluginEnabled: Boolean,
        errorReportsOnly: Boolean,
        isErrorEvent: Boolean,
    ): Boolean = when {
        !hasKey -> false
        !ideConsent -> false
        !pluginEnabled -> false
        errorReportsOnly && !isErrorEvent -> false
        else -> true
    }

    /**
     * Drop null and blank values.
     *
     * A property whose value is unknown must be absent, never the string
     * "unknown" or "undefined": those look like real values in a dashboard and
     * silently inflate whatever bucket they land in.
     */
    fun clean(properties: Map<String, Any?>): Map<String, Any> =
        properties.mapNotNull { (key, value) ->
            when {
                value == null -> null
                value is String && value.isBlank() -> null
                else -> key to value
            }
        }.toMap()
}
