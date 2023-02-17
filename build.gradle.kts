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

plugins { }

allprojects {
    repositories {
        mavenLocal()
        mavenCentral()
        google()
    }
}

allprojects.forEach {
    it.apply(plugin = "jacoco")

    it.the<JacocoPluginExtension>().apply {
        toolVersion = "0.8.8"
        reportsDirectory.set(file("$buildDir/jacoco-reports"))
    }
}

val ktlint by configurations.creating {
    // https://github.com/pinterest/ktlint/issues/1114#issuecomment-805793163
    attributes {
        attribute(Bundling.BUNDLING_ATTRIBUTE, objects.named(Bundling.EXTERNAL))
    }
}
val ktlintVersion: String by project

dependencies {
    ktlint("com.pinterest:ktlint:$ktlintVersion")
    ktlint("com.pinterest.ktlint:ktlint-ruleset-standard:$ktlintVersion")
}

val lintPaths = listOf(
    "**/*.kt",
    // Exclude build output directories
    "!**/build/**",
    "!**/node_modules/**",
    "!**/target/**",
)

tasks.register<JavaExec>("ktlint") {
    description = "Check Kotlin code style."
    group = "Verification"
    classpath = configurations.getByName("ktlint")
    mainClass.set("com.pinterest.ktlint.Main")
    args = listOf("--log-level=info", "--relative", "--") + lintPaths
    // https://github.com/pinterest/ktlint/issues/1195#issuecomment-1009027802
    jvmArgs("--add-opens", "java.base/java.lang=ALL-UNNAMED")
}

tasks.register<JavaExec>("ktlintFormat") {
    description = "Auto fix Kotlin code style violations"
    group = "formatting"
    classpath = configurations.getByName("ktlint")
    mainClass.set("com.pinterest.ktlint.Main")
    args = listOf("--log-level=info", "--relative", "--format", "--") + lintPaths
    // https://github.com/pinterest/ktlint/issues/1195#issuecomment-1009027802
    jvmArgs("--add-opens", "java.base/java.lang=ALL-UNNAMED")
}
