/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

description = "Rust Runtime"
plugins {
    kotlin("jvm")
    `maven-publish`
}

group = "software.amazon.rustruntime"

version = "0.0.3"

tasks.jar {
    from("./") {
        include("inlineable/src/")
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

tasks.register("fixRuntimeCrateVersions") {
    dependsOn("copyRuntimeCrates")
    doLast {
        CrateSet.ENTIRE_SMITHY_RUNTIME.forEach { moduleName ->
            patchFile(runtimeOutputDir.resolve("$moduleName/Cargo.toml")) { line ->
                rewriteRuntimeCrateVersion(properties, line)
            }
        }
    }
}

tasks.register<ExecRustBuildTool>("fixManifests") {
    description = "Run the publisher tool's `fix-manifests` sub-command on the runtime crates"
    toolPath = rootProject.projectDir.resolve("tools/ci-build/publisher")
    binaryName = "publisher"
    arguments = listOf("fix-manifests", "--location", runtimeOutputDir.absolutePath)
    dependsOn("fixRuntimeCrateVersions")
}

publishing {
    publications {
        create<MavenPublication>("default") {
            from(components["java"])
        }
    }
    repositories { maven { url = uri("$buildDir/repository") } }
}
