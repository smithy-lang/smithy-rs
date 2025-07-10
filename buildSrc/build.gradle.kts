/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import org.gradle.api.tasks.testing.logging.TestExceptionFormat

plugins {
    `kotlin-dsl`
}

repositories {
    mavenCentral()
    google()
}

dependencies {
    api(libs.smithy.codegen.core)
    implementation(libs.smithy.utils)
    implementation(libs.smithy.protocol.test.traits)
    implementation(libs.smithy.aws.traits)
    implementation(libs.smithy.aws.iam.traits)
    implementation(libs.smithy.aws.cloudformation.traits)
    implementation(gradleApi())
    implementation(libs.toml4j)
    testImplementation(libs.junit.jupiter)

    constraints {
        implementation("com.google.code.gson:gson:2.8.9") {
            because("transitive dependency of toml4j has vulnerabilities; this upgrades it to the patched version")
        }
    }
}

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
