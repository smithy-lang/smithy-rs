/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import org.gradle.api.tasks.testing.logging.TestExceptionFormat

plugins {
    id("smithy-rs.kotlin-conventions")
    id("smithy-rs.publishing-conventions")
}

description = "Plugin to generate `serde` implementations. NOTE: This is separate from generalized serialization."
extra["displayName"] = "Smithy :: Rust :: Codegen Serde"
extra["moduleName"] = "software.amazon.smithy.rust.codegen.client"

dependencies {
    implementation(project(":codegen-core"))
    implementation(project(":codegen-client"))
    implementation(project(":codegen-server"))

    testRuntimeOnly(project(":rust-runtime"))
    testImplementation(libs.junit.jupiter)
    testImplementation(libs.smithy.validation.model)
    testImplementation(libs.smithy.aws.protocol.tests)
    testImplementation(libs.kotest.assertions.core.jvm)
}
