/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import org.gradle.api.tasks.testing.logging.TestExceptionFormat
import java.io.ByteArrayOutputStream

plugins {
    kotlin("jvm")
    jacoco
    `maven-publish`
}

description = "Common code generation logic for generating Rust code from Smithy models"
extra["displayName"] = "Smithy :: Rust :: CodegenCore"
extra["moduleName"] = "software.amazon.smithy.rust.codegen.core"

group = "software.amazon.smithy.rust.codegen"
version = "0.1.0"

val smithyVersion: String by project

dependencies {
    implementation(kotlin("stdlib-jdk8"))
    implementation("org.jsoup:jsoup:1.14.3")
    api("software.amazon.smithy:smithy-codegen-core:$smithyVersion")
    api("com.moandjiezana.toml:toml4j:0.7.2")
    implementation("software.amazon.smithy:smithy-aws-traits:$smithyVersion")
    implementation("software.amazon.smithy:smithy-protocol-test-traits:$smithyVersion")
    implementation("software.amazon.smithy:smithy-waiters:$smithyVersion")
}

fun gitCommitHash(): String {
    // Use commit hash from env if provided, it is helpful to override commit hash in some contexts.
    // For example: while generating diff for generated SDKs we don't want to see version diff,
    // so we are overriding commit hash to something fixed
    val commitHashFromEnv = System.getenv("SMITHY_RS_VERSION_COMMIT_HASH_OVERRIDE")
    if (commitHashFromEnv != null) {
        return commitHashFromEnv
    }

    return try {
        val output = ByteArrayOutputStream()
        exec {
            commandLine = listOf("git", "rev-parse", "HEAD")
            standardOutput = output
        }
        output.toString().trim()
    } catch (ex: Exception) {
        "unknown"
    }
}

val generateSmithyRuntimeCrateVersion by tasks.registering {
    // generate the version of the runtime to use as a resource.
    // this keeps us from having to manually change version numbers in multiple places
    val resourcesDir = "$buildDir/resources/main/software/amazon/smithy/rust/codegen/core"
    val versionFile = file("$resourcesDir/runtime-crate-version.txt")
    outputs.file(versionFile)
    val crateVersion = project.properties["smithy.rs.runtime.crate.version"].toString()
    inputs.property("crateVersion", crateVersion)
    // version format must be in sync with `software.amazon.smithy.rust.codegen.core.Version`
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
    archiveClassifier.set("sources")
    from(sourceSets.getByName("main").allSource)
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
        kotlinOptions.jvmTarget = "1.8"
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

    // Configure jacoco (code coverage) to generate an HTML report
    tasks.jacocoTestReport {
        reports {
            xml.required.set(false)
            csv.required.set(false)
            html.outputLocation.set(file("$buildDir/reports/jacoco"))
        }
    }

    // Always run the jacoco test report after testing.
    tasks["test"].finalizedBy(tasks["jacocoTestReport"])
}

publishing {
    publications {
        create<MavenPublication>("default") {
            from(components["java"])
            artifact(sourcesJar)
        }
    }
    repositories { maven { url = uri("$buildDir/repository") } }
}
