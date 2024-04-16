/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import org.gradle.api.tasks.testing.logging.TestExceptionFormat
import org.jreleaser.model.Active

plugins {
    kotlin("jvm")
    `maven-publish`
    id("org.jreleaser") version "1.11.0"
}

description = "Generates Rust server-side code from Smithy models"

extra["displayName"] = "Smithy :: Rust :: Codegen :: Server"

extra["moduleName"] = "software.amazon.smithy.rust.codegen.server"

group = "software.amazon.smithy.rust.codegen.server.smithy"

version = "0.1.0"

val smithyVersion: String by project

dependencies {
    implementation(project(":codegen-core"))
    implementation("software.amazon.smithy:smithy-aws-traits:$smithyVersion")
    implementation("software.amazon.smithy:smithy-protocol-test-traits:$smithyVersion")

    // `smithy.framework#ValidationException` is defined here, which is used in `constraints.smithy`, which is used
    // in `CustomValidationExceptionWithReasonDecoratorTest`.
    testImplementation("software.amazon.smithy:smithy-validation-model:$smithyVersion")
}

java {
    sourceCompatibility = JavaVersion.VERSION_11
    targetCompatibility = JavaVersion.VERSION_11
}

tasks.compileKotlin { kotlinOptions.jvmTarget = "11" }

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

val javadocJar by tasks.creating(Jar::class) {
    group = "publishing"
    description = "Assembles Kotlin javadoc jar"
    archiveClassifier.set("javadoc")
    from(tasks.javadoc)
}

val isTestingEnabled: String by project
if (isTestingEnabled.toBoolean()) {
    val kotestVersion: String by project

    dependencies {
        testImplementation("org.junit.jupiter:junit-jupiter:5.6.1")
        testImplementation("io.kotest:kotest-assertions-core-jvm:$kotestVersion")
    }

    tasks.compileTestKotlin { kotlinOptions.jvmTarget = "11" }

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


// JReleaser publishes artifacts from a local staging repository, rather than maven local.
// https://jreleaser.org/guide/latest/examples/maven/staging-artifacts.html#_gradle
val stagingDirectory = layout.buildDirectory.dir("staging")

publishing {
    publications {
        create<MavenPublication>("default") {
            from(components["java"])
            artifact(sourcesJar)
            artifact(javadocJar)
        }
    }
    repositories {
        maven {
            name = "staging"
            url = uri(stagingDirectory)
        }
    }

    // Add license spec to all maven publications
    publications.withType<MavenPublication>() {
        project.afterEvaluate {
            pom {
                name.set(project.name)
                description.set(project.description)
                url.set("https://github.com/smithy-lang/smithy-rs")
                licenses {
                    license {
                        name.set("Apache License 2.0")
                        url.set("http://www.apache.org/licenses/LICENSE-2.0.txt")
                        distribution.set("repo")
                    }
                }
                developers {
                    developer {
                        id.set("smithy")
                        name.set("Smithy")
                        organization.set("Amazon Web Services")
                        organizationUrl.set("https://aws.amazon.com")
                        roles.add("developer")
                    }
                }
                scm {
                    url.set("https://github.com/smithy-lang/smithy-rs.git")
                }
            }
        }
    }
}

jreleaser {
    dryrun = false
    gitRootSearch = true

    project {
        website = "https://smithy-lang.github.io/smithy-rs/"
        authors = listOf("Smithy")
        vendor = "Smithy"
        license = "Apache-2.0"
        description = "Smithy code generator for Rust that generates server code"
        copyright = "2020"
    }

    // Creates a generic release, which won't publish anything (we are only interested in publishing the jar)
    // https://jreleaser.org/guide/latest/reference/release/index.html
    release {
        generic {
            enabled = true
            skipRelease = true
        }
    }

    // Used to announce a release to configured announcers.
    // https://jreleaser.org/guide/latest/reference/announce/index.html
    announce {
        active = Active.ALWAYS
    }

    // Signing configuration.
    // https://jreleaser.org/guide/latest/reference/signing.html
    signing {
        active = Active.ALWAYS
        armored = true
    }

    // Configuration for deploying to Maven Central.
    // https://jreleaser.org/guide/latest/examples/maven/maven-central.html#_gradle
    deploy {
        maven {
            nexus2 {
                create("maven-central") {
                    active = Active.ALWAYS
                    url = "https://aws.oss.sonatype.org/service/local"
                    snapshotUrl = "https://aws.oss.sonatype.org/content/repositories/snapshots"
                    closeRepository = true
                    releaseRepository = true
                    stagingRepository(stagingDirectory.get().toString())
                }
            }
        }
    }
}
