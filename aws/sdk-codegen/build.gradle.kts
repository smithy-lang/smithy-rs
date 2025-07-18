/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

plugins {
    id("smithy-rs.kotlin-conventions")
    id("smithy-rs.publishing-conventions")
}

description = "AWS Specific Customizations for Smithy code generation"
extra["displayName"] = "Smithy :: Rust :: AWS Codegen"
extra["moduleName"] = "software.amazon.smithy.rustsdk"

dependencies {
    implementation(project(":codegen-core"))
    implementation(project(":codegen-client"))
    implementation(libs.jsoup)
    implementation(libs.smithy.aws.traits)
    implementation(libs.smithy.protocol.test.traits)
    implementation(libs.smithy.rules.engine)
    implementation(libs.smithy.aws.endpoints)
    implementation(libs.smithy.smoke.test.traits)
    implementation(libs.smithy.aws.smoke.test.model)


    implementation(project(":aws:rust-runtime"))
    testImplementation(libs.junit.jupiter)
    testImplementation(libs.kotest.assertions.core.jvm)
}
