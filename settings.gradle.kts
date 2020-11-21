/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

pluginManagement {
    repositories {
        maven("https://dl.bintray.com/kotlin/kotlin-eap")
        mavenCentral()
        maven("https://plugins.gradle.org/m2/")
        google()
        gradlePluginPortal()
    }
}


rootProject.name = "software.amazon.smithy.rust.codegen.smithy-rs"
enableFeaturePreview("GRADLE_METADATA")

include(":codegen")
include(":codegen-test")
include(":rust-runtime")
