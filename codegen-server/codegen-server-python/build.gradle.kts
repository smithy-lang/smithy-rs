/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

plugins {
    id("smithy-rs.kotlin-conventions")
}

description = "Generates Rust/Python server-side code from Smithy models"
extra["displayName"] = "Smithy :: Rust :: Codegen :: Server :: Python"
extra["moduleName"] = "software.amazon.smithy.rust.codegen.server.python"

dependencies {
    implementation(project(":codegen-core"))
    implementation(project(":codegen-server"))
    implementation(libs.smithy.aws.traits)
    implementation(libs.smithy.protocol.test.traits)

    // `smithy.framework#ValidationException` is defined here, which is used in `PythonServerTypesTest`.
    testImplementation(libs.smithy.validation.model)

    testImplementation(libs.junit.jupiter)
    testImplementation(libs.kotest.assertions.core.jvm)
}
