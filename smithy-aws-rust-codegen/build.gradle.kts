/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import org.gradle.api.tasks.testing.logging.TestExceptionFormat

plugins {
    kotlin("jvm")
    `maven-publish`
}

description = "AWS Specific Customizations for Smithy code generation"
extra["displayName"] = "Smithy :: AWS :: Rust :: Codegen"
extra["moduleName"] = "software.amazon.smithy.aws.rust.codegen"

val smithyVersion: String by project

buildscript {
    val smithyVersion: String by project

    repositories {
        mavenLocal()
        mavenCentral()
    }
}

dependencies {
    implementation(project(":smithy-rust-codegen-core"))
    implementation(project(":smithy-rust-codegen-client"))
    implementation("org.jsoup:jsoup:1.16.2")
    implementation("software.amazon.smithy:smithy-aws-traits:$smithyVersion")
    implementation("software.amazon.smithy:smithy-protocol-test-traits:$smithyVersion")
    implementation("software.amazon.smithy:smithy-rules-engine:$smithyVersion")
    implementation("software.amazon.smithy:smithy-aws-endpoints:$smithyVersion")
}

tasks.compileKotlin {
    kotlinOptions.jvmTarget = "11"
}

val isTestingEnabled: String by project
if (isTestingEnabled.toBoolean()) {
    val kotestVersion: String by project

    dependencies {
        runtimeOnly(project(":aws:rust-runtime"))
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
