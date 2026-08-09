// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

pluginManagement {
    repositories {
        gradlePluginPortal()
        mavenCentral()
    }
}

// The IntelliJ Platform Gradle Plugin resolves IDE distributions and marketplace
// plugins (LSP4IJ) through custom repositories that must be visible to the
// dependency-resolution layer as well as the plugin layer.
dependencyResolutionManagement {
    repositories {
        mavenCentral()
    }
}

rootProject.name = "codegraph-jetbrains"
