/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import org.gradle.api.tasks.testing.logging.TestExceptionFormat

plugins {
    kotlin("jvm")
    `maven-publish`
}

description = "Generates Rust/Python server-side code from Smithy models"

extra["displayName"] = "Smithy :: Rust :: Codegen :: Server :: Python"

extra["moduleName"] = "software.amazon.smithy.rust.codegen.server.python"

group = "software.amazon.smithy.rust.codegen.server.python.smithy"

version = "0.1.0"

val smithyVersion: String by project

dependencies {
    implementation(project(":codegen-core"))
    implementation(project(":codegen-server"))
    implementation("software.amazon.smithy:smithy-aws-traits:$smithyVersion")
    implementation("software.amazon.smithy:smithy-protocol-test-traits:$smithyVersion")

    // `smithy.framework#ValidationException` is defined here, which is used in `PythonServerTypesTest`.
    testImplementation("software.amazon.smithy:smithy-validation-model:$smithyVersion")
}

tasks.compileKotlin { kotlinOptions.jvmTarget = "1.8" }

// Reusable license copySpec
val licenseSpec = copySpec {
    from("${project.rootDir}/LICENSE")
    from("${project.rootDir}/NOTICE")
}

// Configure jars to include license related info
tasks.jar {
    metaInf.with(licenseSpec)
    inputs.property("moduleName", project.name)
    manifest { attributes["Automatic-Module-Name"] = project.name }
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
        testImplementation("org.junit.jupiter:junit-jupiter:5.6.1")
        testImplementation("io.kotest:kotest-assertions-core-jvm:$kotestVersion")
    }

    tasks.compileTestKotlin { kotlinOptions.jvmTarget = "1.8" }

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

publishing {
    publications {
        create<MavenPublication>("default") {
            from(components["java"])
            artifact(sourcesJar)
        }
    }
    repositories { maven { url = uri("$buildDir/repository") } }
}
