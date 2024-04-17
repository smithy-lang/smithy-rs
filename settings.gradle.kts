/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

rootProject.name = "smithy-rs"

include(":smithy-rust-codegen-core")
include(":smithy-rust-codegen-client")
include(":smithy-rust-codegen-client-test")
include(":smithy-rust-codegen-server")
include(":smithy-rust-codegen-server-test")
include(":smithy-rust-codegen-server-python")
include(":smithy-rust-codegen-server-python-test")
include(":smithy-rust-codegen-server-typescript")
include(":smithy-rust-codegen-server-typescript-test")
include(":smithy-aws-rust-codegen")
include(":rust-runtime")
include(":aws:rust-runtime")
include(":aws:sdk")
include(":aws:sdk-adhoc-test")

pluginManagement {
    val smithyGradlePluginVersion: String by settings
    plugins {
        id("software.amazon.smithy.gradle.smithy-base") version smithyGradlePluginVersion
        id("software.amazon.smithy.gradle.smithy-jar") version smithyGradlePluginVersion
    }
}
