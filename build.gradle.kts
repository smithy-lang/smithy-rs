/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
import org.jreleaser.model.Active

buildscript {
    repositories {
        mavenCentral()
        google()
    }

    val kotlinVersion: String by project
    dependencies {
        classpath("org.jetbrains.kotlin:kotlin-gradle-plugin:$kotlinVersion")
    }
}

allprojects {
    val allowLocalDeps: String by project
    repositories {
        if (allowLocalDeps.toBoolean()) {
         mavenLocal()
        }
        mavenCentral()
        google()
    }

    // Sane default for submodules
    group = "software.amazon.smithy.rust"
    version = "0.1.0"
}

val ktlint by configurations.creating
val ktlintVersion: String by project

dependencies {
    ktlint("com.pinterest.ktlint:ktlint-cli:$ktlintVersion") {
        attributes {
            attribute(Bundling.BUNDLING_ATTRIBUTE, objects.named(Bundling.EXTERNAL))
        }
    }
}

val lintPaths =
    listOf(
        "**/*.kt",
        // Exclude build output directories
        "!**/build/**",
        "!**/node_modules/**",
        "!**/target/**",
    )

tasks.register<JavaExec>("ktlint") {
    description = "Check Kotlin code style."
    group = LifecycleBasePlugin.VERIFICATION_GROUP
    classpath = ktlint
    mainClass.set("com.pinterest.ktlint.Main")
    args = listOf("--log-level=info", "--relative", "--") + lintPaths
    // https://github.com/pinterest/ktlint/issues/1195#issuecomment-1009027802
    jvmArgs("--add-opens", "java.base/java.lang=ALL-UNNAMED")
}

tasks.register<JavaExec>("ktlintFormat") {
    description = "Auto fix Kotlin code style violations"
    group = LifecycleBasePlugin.VERIFICATION_GROUP
    classpath = ktlint
    mainClass.set("com.pinterest.ktlint.Main")
    args = listOf("--log-level=info", "--relative", "--format", "--") + lintPaths
    // https://github.com/pinterest/ktlint/issues/1195#issuecomment-1009027802
    jvmArgs("--add-opens", "java.base/java.lang=ALL-UNNAMED")
}

tasks.register<JavaExec>("ktlintPreCommit") {
    description = "Check Kotlin code style (for the pre-commit hooks)."
    group = LifecycleBasePlugin.VERIFICATION_GROUP
    classpath = ktlint
    mainClass.set("com.pinterest.ktlint.Main")
    args = listOf("--log-level=warn", "--color", "--relative", "--format", "--") +
        System.getProperty("ktlintPreCommitArgs").let { args ->
            check(args.isNotBlank()) { "need to pass in -DktlintPreCommitArgs=<some file paths to check>" }
            args.split(" ")
        }
    // https://github.com/pinterest/ktlint/issues/1195#issuecomment-1009027802
    jvmArgs("--add-opens", "java.base/java.lang=ALL-UNNAMED")
}

plugins {
    `java-library`
    `maven-publish`
    id("org.jreleaser") version "1.11.0"
}

// The root project doesn't produce a JAR.
tasks["jar"].enabled = false

val projectsToSkipPublication = listOf(
    "aws",
    "sdk",
    "rust-runtime",
    "sdk-adhoc-test",
    "smithy-rust-codegen-client-test",
    "smithy-rust-codegen-server-test",
    "smithy-rust-codegen-server-python", // Remove when decided to publish
    "smithy-rust-codegen-server-python-test",
    "smithy-rust-codegen-server-typescript", // Remove when decided to publish
    "smithy-rust-codegen-server-typescript-test",
)
subprojects {
    val subproject = this

    /*
     * Java
     * ====================================================
     */
    if (!projectsToSkipPublication.contains(subproject.name)) {
        apply(plugin = "java-library")

        java {
            toolchain {
                languageVersion.set(JavaLanguageVersion.of(11))
            }
        }

        tasks.withType<JavaCompile> {
            options.encoding = "UTF-8"
        }

        // Reusable license copySpec
        val licenseSpec = copySpec {
            from("${project.rootDir}/LICENSE")
            from("${project.rootDir}/NOTICE")
        }

        // Set up tasks that build source and javadoc jars.
        tasks.register<Jar>("sourcesJar") {
            metaInf.with(licenseSpec)
            from(sourceSets.main.get().allJava)
            archiveClassifier.set("sources")
        }

        tasks.register<Jar>("javadocJar") {
            metaInf.with(licenseSpec)
            from(tasks.javadoc)
            archiveClassifier.set("javadoc")
        }

        // Configure jars to include license related info
        tasks.jar {
            metaInf.with(licenseSpec)
            inputs.property("moduleName", subproject.extra["moduleName"])
            manifest {
                attributes["Automatic-Module-Name"] = subproject.extra["moduleName"]
            }
        }

        // Always run javadoc after build.
        tasks["build"].finalizedBy(tasks["javadoc"])

        /*
         * Maven
         * ====================================================
         */
        apply(plugin = "maven-publish")

        repositories {
            mavenLocal()
            mavenCentral()
        }

        publishing {
            repositories {
                maven {
                    name = "staging"
                    url = uri(rootProject.layout.buildDirectory.dir("staging"))
                }
            }

            publications {
                create<MavenPublication>("mavenJava") {
                    from(components["java"])

                    // Ship the source and javadoc jars.
                    artifact(tasks["sourcesJar"])
                    artifact(tasks["javadocJar"])

                    // Include extra information in the POMs.
                    afterEvaluate {
                        pom {
                            name.set(subproject.extra["displayName"].toString())
                            description.set(subproject.description)
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
        }
    }
}

// Configure JReleaser for publishing to maven-central
// JReleaser publishes artifacts from a local staging repository, rather than maven local.
// https://jreleaser.org/guide/latest/examples/maven/staging-artifacts.html#_gradle
jreleaser {
    dryrun = true

    project {
        website = "https://smithy-lang.github.io/smithy-rs/design/"
        authors = listOf("Smithy")
        vendor = "Smithy"
        license = "Apache-2.0"
        description = "Smithy code generators for Rust that generate clients, servers, and the entire AWS SDK"
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
        verify = true
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
                    stagingRepository(rootProject.layout.buildDirectory.dir("staging").get().toString())
                }
            }
        }
    }
}

