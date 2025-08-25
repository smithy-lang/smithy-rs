/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import org.gradle.api.tasks.testing.logging.TestExceptionFormat

plugins {
    id("smithy-rs.kotlin-conventions")
    id("smithy-rs.publishing-conventions")
}

description = "Generates Rust server-side code from Smithy models"
extra["displayName"] = "Smithy :: Rust :: Codegen :: Server"
extra["moduleName"] = "software.amazon.smithy.rust.codegen.server"

dependencies {
    implementation(project(":codegen-core"))
    implementation(libs.smithy.aws.traits)
    implementation(libs.smithy.protocol.test.traits)
    implementation(libs.smithy.protocol.traits)

    // `smithy.framework#ValidationException` is defined here, which is used in `constraints.smithy`, which is used
    // in `CustomValidationExceptionWithReasonDecoratorTest`.
    testImplementation(libs.smithy.validation.model)

    // It's handy to re-use protocol test suite models from Smithy in our Kotlin tests.
    testImplementation(libs.smithy.protocol.tests)

    testImplementation(libs.junit.jupiter)
    testImplementation(libs.kotest.assertions.core.jvm)
}
