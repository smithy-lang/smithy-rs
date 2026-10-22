/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

plugins {
    id("smithy-rs.kotlin-conventions")
    id("smithy-rs.publishing-conventions")
}

description = "Generates Rust server-side code from Smithy models"
extra["displayName"] = "Smithy :: Rust :: Codegen :: Server"
extra["moduleName"] = "software.amazon.smithy.rust.codegen.server"

dependencies {
    implementation(project(":codegen-core"))
    implementation(project(":codegen-traits"))
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

// Embed the artifact version so generated schema crates can record it separately from the source commit.
val generatedResourcesDir = layout.buildDirectory.dir("generated/resources")
val generateServerCodegenVersion by tasks.registering {
    val codegenVersion = project.version.toString()
    inputs.property("codegenVersion", codegenVersion)
    outputs.dir(generatedResourcesDir)
    doLast {
        val versionFile =
            generatedResourcesDir.get()
                .file("software/amazon/smithy/rust/codegen/server/smithy/codegen-release.txt").asFile
        versionFile.parentFile.mkdirs()
        versionFile.writeText(codegenVersion)
    }
}

sourceSets.main {
    resources.srcDir(generatedResourcesDir)
}

tasks.processResources {
    dependsOn(generateServerCodegenVersion)
}

tasks.named("sourcesJar").configure {
    dependsOn(generateServerCodegenVersion)
}
