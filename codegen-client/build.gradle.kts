/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

plugins {
    id("smithy-rs.kotlin-conventions")
    id("smithy-rs.publishing-conventions")
}

description = "Generates Rust client code from Smithy models"
extra["displayName"] = "Smithy :: Rust :: CodegenClient"
extra["moduleName"] = "software.amazon.smithy.rust.codegen.client"

dependencies {
    implementation(project(":codegen-core"))
    implementation(kotlin("stdlib-jdk8"))
    api(libs.smithy.codegen.core)
    implementation(libs.jackson.dataformat.cbor)
    implementation(libs.smithy.aws.traits)
    implementation(libs.smithy.protocol.test.traits)
    implementation(libs.smithy.waiters)
    implementation(libs.smithy.rules.engine)
    implementation(libs.smithy.protocol.traits)

    // `smithy.framework#ValidationException` is defined here, which is used in event stream
    // marshalling/unmarshalling tests.
    testImplementation(libs.smithy.validation.model)

    testRuntimeOnly(project(":rust-runtime"))
    testImplementation(libs.junit.jupiter)
    testImplementation(libs.kotest.assertions.core.jvm)
}
