/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
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

plugins {
    kotlin("jvm") version "1.3.72" apply false
    id("org.jetbrains.dokka") version "0.10.0"
}

allprojects {
    repositories {
        mavenLocal()
        mavenCentral()
        google()
    }
}

apply(from = rootProject.file("gradle/maincodecoverage.gradle"))

val ktlint by configurations.creating {
    // https://github.com/pinterest/ktlint/issues/1114#issuecomment-805793163
    attributes {
        attribute(Bundling.BUNDLING_ATTRIBUTE, objects.named(Bundling.EXTERNAL))
    }
}
val ktlintVersion: String by project

dependencies {
    ktlint("com.pinterest:ktlint:$ktlintVersion")
}

val lintPaths = listOf(
    "codegen/src/**/*.kt"
)

tasks.register<JavaExec>("ktlint") {
    description = "Check Kotlin code style."
    group = "Verification"
    classpath = configurations.getByName("ktlint")
    main = "com.pinterest.ktlint.Main"
    args = lintPaths
}

tasks.register<JavaExec>("ktlintFormat") {
    description = "Auto fix Kotlin code style violations"
    group = "formatting"
    classpath = configurations.getByName("ktlint")
    main = "com.pinterest.ktlint.Main"
    args = listOf("-F") + lintPaths
}

@Suppress("UnstableApiUsage")
tasks.register<JacocoMerge>("jacocoMerge") {
    group = LifecycleBasePlugin.VERIFICATION_GROUP
    description = "Merge the JaCoCo data files from all subprojects into one"
    afterEvaluate {
        // An empty FileCollection
        val execFiles = objects.fileCollection()
        val projectList = subprojects + project
        projectList.forEach { subProject: Project ->
            if (subProject.pluginManager.hasPlugin("jacoco")) {
                val testTasks = subProject.tasks.withType<Test>()
                // ensure that .exec files are actually present
                dependsOn(testTasks)

                testTasks.forEach { task: Test ->
                    // The JacocoTaskExtension is the source of truth for the location of the .exec file.
                    val extension = task.extensions.findByType(JacocoTaskExtension::class.java)
                    extension?.let {
                        execFiles.from(it.destinationFile)
                    }
                }
            }
        }
        executionData = execFiles
    }
    doFirst {
        // .exec files might be missing if a project has no tests. Filter in execution phase.
        executionData = executionData.filter { it.canRead() }
    }
}
