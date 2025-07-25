/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

plugins {
    id("smithy-rs.kotlin-conventions")
    id("smithy-rs.publishing-conventions")
}

description = "Rust Runtime"
extra["displayName"] = "Smithy :: Rust :: Rust Runtime"
extra["moduleName"] = "software.amazon.smithy.rust.runtime"

tasks.jar {
    from("./") {
        include("inlineable/src/")
        include("inlineable/Cargo.toml")
    }
}

val properties = PropertyRetriever(rootProject, project)
val outputDir = layout.buildDirectory.dir("smithy-rs")
val runtimeOutputDir = outputDir.get().dir("rust-runtime")

tasks.assemble.configure {
    dependsOn("copyRuntimeCrates")
    dependsOn("fixRuntimeCrateVersions")
    dependsOn("fixManifests")
}

tasks.register<Copy>("copyRuntimeCrates") {
    from("$rootDir/rust-runtime") {
        CrateSet.ENTIRE_SMITHY_RUNTIME.forEach { include("${it.name}/**") }
    }
    exclude("**/target")
    exclude("**/Cargo.lock")
    exclude("**/node_modules")
    into(runtimeOutputDir)
}

tasks.register("fixRuntimeCrateVersions") {
    dependsOn("copyRuntimeCrates")
    doLast {
        CrateSet.ENTIRE_SMITHY_RUNTIME.forEach { module ->
            patchFile(runtimeOutputDir.file("${module.name}/Cargo.toml").asFile) { line ->
                rewriteRuntimeCrateVersion(properties.get(module.versionPropertyName)!!, line)
            }
        }
    }
}

tasks.register<ExecRustBuildTool>("fixManifests") {
    description = "Run the publisher tool's `fix-manifests` sub-command on the runtime crates"
    toolPath = rootProject.projectDir.resolve("tools/ci-build/publisher")
    binaryName = "publisher"
    arguments = listOf("fix-manifests", "--location", runtimeOutputDir.asFile.absolutePath)
    dependsOn("fixRuntimeCrateVersions")
}
