/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import org.gradle.api.tasks.testing.logging.TestExceptionFormat
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    `kotlin-dsl`
}

repositories {
    mavenCentral()
    google()
}

java {
    sourceCompatibility = JavaVersion.VERSION_11
    targetCompatibility = JavaVersion.VERSION_11
}

tasks.compileKotlin {
    compilerOptions.jvmTarget.set(JvmTarget.JVM_11)
}

tasks.compileTestKotlin {
    compilerOptions.jvmTarget.set(JvmTarget.JVM_11)
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
    // required for convention plugins to use
    implementation(libs.kotlin.gradle.plugin)
    testImplementation(libs.junit.jupiter)

    // https://github.com/gradle/gradle/issues/15383
    implementation(files(libs.javaClass.superclass.protectionDomain.codeSource.location))

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
