/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import org.gradle.api.tasks.testing.logging.TestExceptionFormat
import java.io.ByteArrayOutputStream

plugins {
    kotlin("jvm")
    id("org.jetbrains.dokka")
    jacoco
    `maven-publish`
}

description = "Generates Rust code from Smithy models"
extra["displayName"] = "Smithy :: Rust :: Codegen"
extra["moduleName"] = "software.amazon.software.amazon.smithy.rust.codegen.smithy.rust.codegen"

group = "software.amazon.software.amazon.smithy.rust.codegen.smithy"
version = "0.1.0"

val smithyVersion: String by project
val kotestVersion: String by project

dependencies {
    implementation(kotlin("stdlib-jdk8"))
    implementation("org.jsoup:jsoup:1.14.3")
    api("software.amazon.smithy:smithy-codegen-core:$smithyVersion")
    api("com.moandjiezana.toml:toml4j:0.7.2")
    implementation("software.amazon.smithy:smithy-aws-traits:$smithyVersion")
    implementation("software.amazon.smithy:smithy-protocol-test-traits:$smithyVersion")
    implementation("software.amazon.smithy:smithy-waiters:$smithyVersion")
    runtimeOnly(project(":rust-runtime"))
    testImplementation("org.junit.jupiter:junit-jupiter:5.6.1")
    testImplementation("io.kotest:kotest-assertions-core-jvm:$kotestVersion")
}

tasks.compileTestKotlin {
    kotlinOptions.jvmTarget = "1.8"
}

fun gitCommitHash() =
    try {
        val output = ByteArrayOutputStream()
        exec {
            commandLine = listOf("git", "rev-parse", "HEAD")
            standardOutput = output
        }
        output.toString().trim()
    } catch (ex: Exception) {
        "unknown"
    }

val generateSmithyRuntimeCrateVersion by tasks.registering {
    // generate the version of the runtime to use as a resource.
    // this keeps us from having to manually change version numbers in multiple places
    val resourcesDir = "$buildDir/resources/main/software/amazon/smithy/rust/codegen/smithy"
    val versionFile = file("$resourcesDir/runtime-crate-version.txt")
    outputs.file(versionFile)
    val crateVersion = project.properties["smithy.rs.runtime.crate.version"].toString()
    inputs.property("crateVersion", crateVersion)
    // version format must be in sync with `software.amazon.smithy.rust.codegen.smithy.Version`
    val version = "$crateVersion\n${gitCommitHash()}"
    sourceSets.main.get().output.dir(resourcesDir)
    doLast {
        versionFile.writeText(version)
    }
}

tasks.compileKotlin {
    kotlinOptions.jvmTarget = "1.8"
    dependsOn(generateSmithyRuntimeCrateVersion)
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
    classifier = "sources"
    from(sourceSets.getByName("main").allSource)
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

tasks.dokka {
    outputFormat = "html"
    outputDirectory = "$buildDir/javadoc"
}

// Always build documentation
tasks["build"].finalizedBy(tasks["dokka"])

// Configure jacoco (code coverage) to generate an HTML report
tasks.jacocoTestReport {
    reports {
        xml.isEnabled = false
        csv.isEnabled = false
        html.destination = file("$buildDir/reports/jacoco")
    }
}

// Always run the jacoco test report after testing.
tasks["test"].finalizedBy(tasks["jacocoTestReport"])

publishing {
    publications {
        create<MavenPublication>("default") {
            from(components["java"])
            artifact(sourcesJar)
        }
    }
    repositories { maven { url = uri("$buildDir/repository") } }
}
