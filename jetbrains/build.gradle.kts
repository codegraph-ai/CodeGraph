// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

import org.jetbrains.intellij.platform.gradle.IntelliJPlatformType
import org.jetbrains.intellij.platform.gradle.TestFrameworkType
import org.jetbrains.kotlin.gradle.dsl.JvmTarget
import org.jetbrains.kotlin.gradle.dsl.KotlinVersion

plugins {
    id("java")
    // 2.2.x is the oldest line with Gradle 9 support, which the IntelliJ
    // Platform Gradle Plugin 2.18 now requires.
    id("org.jetbrains.kotlin.jvm") version "2.2.21"
    id("org.jetbrains.intellij.platform") version "2.18.1"
}

group = "ai.codegraph"
version = providers.gradleProperty("pluginVersion").get()

repositories {
    mavenCentral()
    intellijPlatform {
        defaultRepositories()
    }
}

dependencies {
    intellijPlatform {
        create(
            providers.gradleProperty("platformType"),
            providers.gradleProperty("platformVersion"),
        )
        // LSP4IJ carries the JSON-RPC transport and document synchronisation.
        // It is a required runtime dependency, not a bundled library: the
        // marketplace installs it alongside this plugin.
        plugin(
            providers.gradleProperty("lsp4ijVersion").map { "com.redhat.devtools.lsp4ij:$it" },
        )
        testFramework(TestFrameworkType.Platform)
    }

    testImplementation("junit:junit:4.13.2")
}

kotlin {
    jvmToolchain(21)
    compilerOptions {
        jvmTarget = JvmTarget.JVM_21
        // Compile against the Kotlin API the *oldest supported* IDE actually
        // guarantees - 2.0 for since-build 243, not the 2.1 the local compiler
        // offers. Getting this wrong links against stdlib symbols that IDE does
        // not ship, and the failure is a NoSuchMethodError at runtime on the
        // user's machine rather than anything the build would show.
        // Raise this only together with pluginSinceBuild.
        apiVersion = KotlinVersion.KOTLIN_2_0
        languageVersion = KotlinVersion.KOTLIN_2_0
        freeCompilerArgs.add("-Xjvm-default=all")
    }
}

intellijPlatform {
    pluginConfiguration {
        id = "ai.codegraph.jetbrains"
        name = "CodeGraph"
        version = providers.gradleProperty("pluginVersion")
        vendor {
            name = "Andrey Vasilevsky"
            email = "anvanster@gmail.com"
        }
        ideaVersion {
            sinceBuild = providers.gradleProperty("pluginSinceBuild")
            // Unbounded: the plugin uses stable platform APIs only, and an
            // untilBuild pin would strand users on every IDE upgrade.
            untilBuild = provider { null }
        }
    }

    pluginVerification {
        ides {
            // Only the development platform by default. `recommended()` pulls a
            // full IDE distribution per recommended release - roughly 3 GB each
            // - which is a surprising amount of disk to consume on someone
            // else's machine. Widen this deliberately before a release, on a
            // machine with room for it.
            select {
                types = listOf(IntelliJPlatformType.IntellijIdeaCommunity)
                sinceBuild = providers.gradleProperty("pluginSinceBuild")
                untilBuild = providers.gradleProperty("pluginSinceBuild")
            }
        }
    }
}

/**
 * Bakes the analytics key into the artifact from the release environment.
 * Absent by default, so builds from source report nothing - matching how the
 * VS Code client injects `__POSTHOG_KEY__` at bundle time.
 */
val generateTelemetryConfig by tasks.registering {
    val output = layout.buildDirectory.file("generated/telemetry/codegraph-telemetry.properties")
    val key = providers.environmentVariable("CODEGRAPH_POSTHOG_KEY").orElse("")
    val host = providers.environmentVariable("CODEGRAPH_POSTHOG_HOST").orElse("")
    outputs.file(output)
    inputs.property("key", key)
    inputs.property("host", host)
    doLast {
        val file = output.get().asFile
        file.parentFile.mkdirs()
        file.writeText("posthogKey=${key.get()}\nposthogHost=${host.get()}\n")
    }
}

sourceSets {
    main {
        resources.srcDir(generateTelemetryConfig.map { it.outputs.files.singleFile.parentFile })
    }
}

tasks {
    // Generating searchable options boots a headless IDE purely to index the
    // settings page. It roughly doubles build time for a marginal gain, and the
    // settings this plugin exposes are reachable under an obvious name.
    buildSearchableOptions {
        enabled = false
    }

    runIde {
        // Open a project on launch so project-level services actually
        // initialise; the welcome screen alone exercises almost nothing.
        // Override with -PsandboxProject=/path/to/project.
        val sandboxProject = providers.gradleProperty("sandboxProject").orNull
        if (sandboxProject != null) {
            args = listOf(sandboxProject)
        }
        // -PrunIdeSystemProperty=key=value, repeatable with commas. Used to arm
        // the self-check activity without a bespoke Gradle task per flag.
        providers.gradleProperty("runIdeSystemProperty").orNull
            ?.split(",")
            ?.mapNotNull { entry -> entry.split("=", limit = 2).takeIf { it.size == 2 } }
            ?.forEach { (key, value) -> systemProperty(key, value) }
    }
}
