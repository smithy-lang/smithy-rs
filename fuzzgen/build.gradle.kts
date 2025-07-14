/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import org.gradle.api.tasks.testing.logging.TestExceptionFormat

plugins {
    id("smithy-rs.kotlin-conventions")
    id("smithy-rs.publishing-conventions")
}

description = "Plugin to generate a fuzz harness"
extra["displayName"] = "Smithy :: Rust :: Fuzzer Generation"
extra["moduleName"] = "software.amazon.smithy.rust.codegen.client"

dependencies {
    implementation(project(":codegen-core"))
    implementation(project(":codegen-client"))
    implementation(project(":codegen-server"))
    implementation(libs.smithy.protocol.test.traits)


    testRuntimeOnly(project(":rust-runtime"))
    testImplementation(libs.junit.jupiter)
    testImplementation(libs.smithy.validation.model)
    testImplementation(libs.smithy.aws.protocol.tests)
    testImplementation(libs.kotest.assertions.core.jvm)
}


tasks.register("generateClasspath") {
    doLast {
        // Get the runtime classpath
        val runtimeClasspath = sourceSets["main"].runtimeClasspath

        // Add the 'libs' directory to the classpath
        val libsDir = file(layout.buildDirectory.dir("libs"))
        val fullClasspath = runtimeClasspath + files(libsDir.listFiles())

        // Convert to classpath string
        val classpath = fullClasspath.asPath
    }
}
