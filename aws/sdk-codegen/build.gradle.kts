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
extra["displayName"] = "Smithy :: Rust :: AWS Codegen"
extra["moduleName"] = "software.amazon.smithy.rustsdk"

group = "software.amazon.software.amazon.smithy.rust.codegen.smithy"
version = "0.1.0"

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
}

java {
    sourceCompatibility = JavaVersion.VERSION_11
    targetCompatibility = JavaVersion.VERSION_11
}

tasks.compileKotlin {
    kotlinOptions.jvmTarget = "11"
}

// Reusable license copySpec
val licenseSpec = copySpec {
    from("${project.rootDir}/LICENSE")
    from("${project.rootDir}/NOTICE")
}

// Configure jars to include license related info
tasks.jar {
    metaInf.with(licenseSpec)
    inputs.property("moduleName", project.name)
    manifest {
        attributes["Automatic-Module-Name"] = project.name
    }
}

val sourcesJar by tasks.creating(Jar::class) {
    group = "publishing"
    description = "Assembles Kotlin sources jar"
    archiveClassifier.set("sources")
    from(sourceSets.getByName("main").allSource)
}

val isTestingEnabled: String by project
if (isTestingEnabled.toBoolean()) {
    dependencies {
        runtimeOnly(project(":aws:rust-runtime"))
        testImplementation(libs.junit.jupiter)
        testImplementation(libs.kotest.assertions.core.jvm)
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

publishing {
    publications {
        create<MavenPublication>("default") {
            from(components["java"])
            artifact(sourcesJar)
        }
    }
    repositories { maven { url = uri(layout.buildDirectory.dir("repository")) } }
}
