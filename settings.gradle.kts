/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

rootProject.name = "software.amazon.smithy.rust.codegen.smithy-rs"

include(":codegen-core")
include(":codegen-client")
include(":codegen-client-test")
include(":codegen-server")
include(":codegen-server:python")
include(":codegen-server-test")
include(":codegen-server-test:python")
include(":rust-runtime")
include(":aws:sdk-codegen")
include(":aws:sdk-adhoc-test")
include(":aws:sdk")
include(":aws:rust-runtime")

pluginManagement {
    val smithyGradlePluginVersion: String by settings
    plugins {
        id("software.amazon.smithy") version smithyGradlePluginVersion
    }
}

buildscript {
    repositories {
        mavenLocal()
        mavenCentral()
        google()
    }
}
