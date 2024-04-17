/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import org.gradle.api.tasks.testing.logging.TestExceptionFormat

plugins {
    kotlin("jvm")
    `maven-publish`
}

description = "Generates Rust client code from Smithy models"
extra["displayName"] = "Smithy :: Rust :: Codegen :: Client"
extra["moduleName"] = "software.amazon.smithy.rust.codegen.client"

val smithyVersion: String by project

dependencies {
    implementation(project(":smithy-rust-codegen-core"))
    implementation(kotlin("stdlib-jdk8"))
    api("software.amazon.smithy:smithy-codegen-core:$smithyVersion")
    implementation("software.amazon.smithy:smithy-aws-traits:$smithyVersion")
    implementation("software.amazon.smithy:smithy-protocol-test-traits:$smithyVersion")
    implementation("software.amazon.smithy:smithy-waiters:$smithyVersion")
    implementation("software.amazon.smithy:smithy-rules-engine:$smithyVersion")

    // `smithy.framework#ValidationException` is defined here, which is used in event stream
// marshalling/unmarshalling tests.
    testImplementation("software.amazon.smithy:smithy-validation-model:$smithyVersion")
}

tasks.compileKotlin {
    kotlinOptions.jvmTarget = "11"
}

val isTestingEnabled: String by project
if (isTestingEnabled.toBoolean()) {
    val kotestVersion: String by project

    dependencies {
        runtimeOnly(project(":rust-runtime"))
        testImplementation("org.junit.jupiter:junit-jupiter:5.6.1")
        testImplementation("io.kotest:kotest-assertions-core-jvm:$kotestVersion")
    }

    tasks.compileTestKotlin {
        kotlinOptions.jvmTarget = "11"
    }

    tasks.test {
        useJUnitPlatform()
        testLogging {
            events("failed")
            exceptionFormat = TestExceptionFormat.FULL
            showCauses = true
            showExceptions = true
            showStackTraces = true
        }
    }
}
