/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import org.gradle.api.tasks.testing.logging.TestExceptionFormat

plugins {
    kotlin("jvm")
    `maven-publish`
}

description = "Plugin to generate a fuzz harness"
extra["displayName"] = "Smithy :: Rust :: Fuzzer Generation"
extra["moduleName"] = "software.amazon.smithy.rust.codegen.client"

group = "software.amazon.smithy.rust.codegen.serde"
version = "0.1.0"

dependencies {
    implementation(project(":codegen-core"))
    implementation(project(":codegen-client"))
    implementation(project(":codegen-server"))
    implementation(libs.smithy.protocol.test.traits)
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
        runtimeOnly(project(":rust-runtime"))
        testImplementation(libs.junit.jupiter)
        testImplementation(libs.smithy.validation.model)
        testImplementation(libs.smithy.aws.protocol.tests)
        testImplementation(libs.kotest.assertions.core.jvm)
    }

    tasks.compileTestKotlin {
        kotlinOptions.jvmTarget = "11"
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
