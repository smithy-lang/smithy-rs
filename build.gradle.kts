/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

buildscript {
    repositories {
        mavenCentral()
        google()
    }

    dependencies {
        classpath(libs.kotlin.gradle.plugin)
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

    group = "software.amazon.smithy.rust"
    val codegenVersion: String by project
    version = codegenVersion
}


val ktlint by configurations.creating

dependencies {
    ktlint(libs.ktlint.cli) {
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

if (System.getProperties().containsKey("ktlintPreCommitArgs")) {
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
}
