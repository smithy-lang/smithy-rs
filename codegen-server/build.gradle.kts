/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import java.io.ByteArrayOutputStream

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

// Server Cargo metadata records the artifact version separately from its source commit.
val generateServerCodegenVersion by tasks.registering {
    val resourcesDir = layout.buildDirectory.dir("generated/server-version")
    val versionFile = resourcesDir.get().file("software/amazon/smithy/rust/codegen/server/server-codegen-version.json")
    val codegenVersion = project.version.toString()
    val gitHash =
        System.getenv("SMITHY_RS_VERSION_COMMIT_HASH_OVERRIDE") ?: try {
            val output = ByteArrayOutputStream()
            exec {
                commandLine = listOf("git", "rev-parse", "HEAD")
                standardOutput = output
            }
            output.toString().trim()
        } catch (ex: Exception) {
            "unknown"
        }
    inputs.property("codegenVersion", codegenVersion)
    inputs.property("gitHash", gitHash)
    outputs.dir(resourcesDir)
    doLast {
        versionFile.asFile.parentFile.mkdirs()
        versionFile.asFile.writeText(groovy.json.JsonOutput.toJson(mapOf("codegenVersion" to codegenVersion, "gitHash" to gitHash)))
    }
}

sourceSets.main {
    resources.srcDir(generateServerCodegenVersion)
}
