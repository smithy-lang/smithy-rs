/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import org.gradle.api.tasks.testing.logging.TestExceptionFormat

plugins {
    kotlin("jvm")
}

description = "Generates Rust/Python server-side code from Smithy models"
extra["displayName"] = "Smithy :: Rust :: Codegen :: Server :: Python"
extra["moduleName"] = "software.amazon.smithy.rust.codegen.server.python"

val smithyVersion: String by project

dependencies {
    implementation(project(":smithy-rust-codegen-core"))
    implementation(project(":smithy-rust-codegen-server"))
    implementation("software.amazon.smithy:smithy-aws-traits:$smithyVersion")
    implementation("software.amazon.smithy:smithy-protocol-test-traits:$smithyVersion")

    // `smithy.framework#ValidationException` is defined here, which is used in `PythonServerTypesTest`.
    testImplementation("software.amazon.smithy:smithy-validation-model:$smithyVersion")
}


tasks.compileKotlin { kotlinOptions.jvmTarget = "11" }

val isTestingEnabled: String by project
if (isTestingEnabled.toBoolean()) {
    val kotestVersion: String by project

    dependencies {
        testImplementation("org.junit.jupiter:junit-jupiter:5.6.1")
        testImplementation("io.kotest:kotest-assertions-core-jvm:$kotestVersion")
    }

    tasks.compileTestKotlin { kotlinOptions.jvmTarget = "11" }

    tasks.test {
        useJUnitPlatform()
        testLogging {
            events("passed", "skipped", "failed")
            exceptionFormat = TestExceptionFormat.FULL
            showCauses = true
            showExceptions = true
            showStackTraces = true
            showStandardStreams = true
        }
    }
}
