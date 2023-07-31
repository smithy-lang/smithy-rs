/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import org.gradle.api.tasks.testing.logging.TestExceptionFormat

plugins {
    kotlin("jvm")
    jacoco
    `maven-publish`
}

description = "AWS Specific Customizations for Smithy code generation"
extra["displayName"] = "Smithy :: Rust :: AWS Codegen"
extra["moduleName"] = "software.amazon.smithy.rustsdk"

group = "software.amazon.software.amazon.smithy.rust.codegen.smithy"
version = "0.1.0"

val smithyVersion: String by project

dependencies {
    implementation(project(":codegen-core"))
    implementation(project(":codegen-client"))
    implementation("org.jsoup:jsoup:1.14.3")
    implementation("software.amazon.smithy:smithy-aws-traits:$smithyVersion")
    implementation("software.amazon.smithy:smithy-protocol-test-traits:$smithyVersion")
    implementation("software.amazon.smithy:smithy-rules-engine:$smithyVersion")
}

val generateAwsRuntimeCrateVersion by tasks.registering {
    // generate the version of the runtime to use as a resource.
    // this keeps us from having to manually change version numbers in multiple places
    val resourcesDir = "$buildDir/resources/main/software/amazon/smithy/rustsdk"
    val versionFile = file("$resourcesDir/sdk-crate-version.txt")
    outputs.file(versionFile)
    val crateVersion = project.properties["smithy.rs.runtime.crate.version"]?.toString()!!
    inputs.property("crateVersion", crateVersion)
    sourceSets.main.get().output.dir(resourcesDir)
    doLast {
        versionFile.writeText(crateVersion)
    }
}

tasks.compileKotlin {
    kotlinOptions.jvmTarget = "1.8"
    dependsOn(generateAwsRuntimeCrateVersion)
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
        runtimeOnly(project(":aws:rust-runtime"))
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
