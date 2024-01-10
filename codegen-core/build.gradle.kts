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
    implementation("org.jsoup:jsoup:1.16.2")
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
    // Generate the version of the runtime to use as a resource.
    // This keeps us from having to manually change version numbers in multiple places.
    val resourcesDir = layout.buildDirectory.dir("resources/main/software/amazon/smithy/rust/codegen/core")
    val versionsFile = resourcesDir.get().file("runtime-crate-versions.json")
    outputs.file(versionsFile)

    val stableCrateVersion = project.properties["smithy.rs.runtime.crate.stable.version"].toString()
    val unstableCrateVersion = project.properties["smithy.rs.runtime.crate.unstable.version"].toString()
    inputs.property("stableCrateVersion", stableCrateVersion)
    inputs.property("unstableCrateVersion", stableCrateVersion)

    sourceSets.main.get().output.dir(resourcesDir)
    doLast {
        // Version format must be kept in sync with `software.amazon.smithy.rust.codegen.core.Version`
        versionsFile.asFile.writeText(StringBuilder().append("{\n").also { json ->
            fun StringBuilder.keyVal(key: String, value: String) = append("\"$key\": \"$value\"")

            json.append("  ").keyVal("gitHash", gitCommitHash()).append(",\n")
            json.append("  \"runtimeCrates\": {\n")
            var first = true
            for (runtimePath in arrayOf("../rust-runtime", "../aws/rust-runtime")) {
                for (path in project.projectDir.resolve(runtimePath).listFiles()!!) {
                    if (!path.isDirectory || !path.resolve("Cargo.toml").exists()) {
                        continue
                    }

                    val manifestLines = path.resolve("Cargo.toml").readLines()
                    val publish = manifestLines.none { line -> line == "publish = false" }
                    if (!publish) {
                        continue
                    }

                    val stable = manifestLines.any { line -> line == "stable = true" }
                    val versionLine = manifestLines.first { line -> line.startsWith("version = \"") }
                    val maybeVersion = versionLine.slice(("version = \"".length)..(versionLine.length - 2))
                    val version = if (maybeVersion == "0.0.0-smithy-rs-head") {
                        when (stable) {
                            true -> stableCrateVersion
                            else -> unstableCrateVersion
                        }
                    } else {
                        maybeVersion
                    }
                    if (!first) {
                        json.append(",\n")
                    }
                    json.append("    ").keyVal(path.name, version)
                    first = false
                }
            }
            json.append("  }\n")
        }.append("}").toString())
    }
}

java {
    sourceCompatibility = JavaVersion.VERSION_11
    targetCompatibility = JavaVersion.VERSION_11
}

tasks.compileKotlin {
    kotlinOptions.jvmTarget = "11"
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

    // Configure jacoco (code coverage) to generate an HTML report
    tasks.jacocoTestReport {
        reports {
            xml.required.set(false)
            csv.required.set(false)
            html.outputLocation.set(layout.buildDirectory.dir("reports/jacoco"))
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
    repositories { maven { url = uri(layout.buildDirectory.dir("repository")) } }
}
