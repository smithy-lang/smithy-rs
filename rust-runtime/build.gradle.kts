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
    dependsOn("fixRuntimeManifests")
}

tasks.register<Copy>("copyRuntimeCrates") {
    from("$rootDir/rust-runtime") {
        Crates.SMITHY_RUNTIME.forEach { include("$it/**") }
    }
    exclude("**/target")
    exclude("**/Cargo.lock")
    exclude("**/node_modules")
    into(runtimeOutputDir)
}

task("fixRuntimeManifests") {
    dependsOn("copyRuntimeCrates")
    doLast {
        Crates.SMITHY_RUNTIME.forEach { moduleName ->
            patchFile(runtimeOutputDir.resolve("$moduleName/Cargo.toml")) { line ->
                rewriteSmithyRsCrateVersion(properties, line)
            }
        }
    }
}
