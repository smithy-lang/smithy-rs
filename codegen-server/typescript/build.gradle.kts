/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

plugins {
    id("smithy-rs.kotlin-conventions")
    id("smithy-rs.publishing-conventions")
}

description = "Generates Rust/Node server-side code from Smithy models"
extra["displayName"] = "Smithy :: Rust :: Codegen :: Server :: Typescript"
extra["moduleName"] = "software.amazon.smithy.rust.codegen.server.typescript"

dependencies {
    implementation(project(":codegen-core"))
    implementation(project(":codegen-server"))
    implementation(libs.smithy.aws.traits)
    implementation(libs.smithy.protocol.test.traits)

    testImplementation(libs.junit.jupiter)
    testImplementation(libs.kotest.assertions.core.jvm)
}
