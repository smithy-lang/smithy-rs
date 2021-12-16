/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

description = "Rust Runtime"
plugins {
    kotlin("jvm")
}

group = "software.amazon.rustruntime"

version = "0.0.3"

tasks.jar {
    from("./") {
        include("inlineable/src/*.rs")
        include("inlineable/Cargo.toml")
    }
}

val properties = PropertyRetriever(rootProject, project)
val outputDir = buildDir.resolve("smithy-rs")
val runtimeOutputDir = outputDir.resolve("rust-runtime")

tasks["assemble"].apply {
    dependsOn("copyRuntimeCrates")
    dependsOn("fixRuntimeCrateVersions")
    dependsOn("fixManifests")
}

tasks.register<Copy>("copyRuntimeCrates") {
    from("$rootDir/rust-runtime") {
        CrateSet.ENTIRE_SMITHY_RUNTIME.forEach { include("$it/**") }
    }
    exclude("**/target")
    exclude("**/Cargo.lock")
    exclude("**/node_modules")
    into(runtimeOutputDir)
}

task("fixRuntimeCrateVersions") {
    dependsOn("copyRuntimeCrates")
    doLast {
        CrateSet.ENTIRE_SMITHY_RUNTIME.forEach { moduleName ->
            patchFile(runtimeOutputDir.resolve("$moduleName/Cargo.toml")) { line ->
                rewriteSmithyRsCrateVersion(properties, line)
            }
        }
    }
}

tasks.register<Exec>("fixManifests") {
    description = "Run the publisher tool's `fix-manifests` sub-command on the runtime crates"
    workingDir(rootProject.projectDir.resolve("tools/publisher"))
    commandLine("cargo", "run", "--", "fix-manifests", "--location", runtimeOutputDir.absolutePath)
    dependsOn("fixRuntimeCrateVersions")
}
