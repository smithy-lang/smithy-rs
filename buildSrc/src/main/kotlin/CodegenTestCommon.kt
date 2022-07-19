/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import org.gradle.api.Project
import org.gradle.api.tasks.Exec
import org.gradle.kotlin.dsl.extra
import org.gradle.kotlin.dsl.register
import java.io.File

/**
 * This file contains common functionality shared across the buildscripts for the `codegen-test` and `codegen-server-test`
 * modules.
 */

data class CodegenTest(val service: String, val module: String, val extraConfig: String? = null)

private fun generateSmithyBuild(projectDir: String, pluginName: String, tests: List<CodegenTest>): String {
    val projections = tests.joinToString(",\n") {
        """
        "${it.module}": {
            "plugins": {
                "$pluginName": {
                    "runtimeConfig": {
                        "relativePath": "$projectDir/rust-runtime"
                    },
                    "service": "${it.service}",
                    "module": "${it.module}",
                    "moduleVersion": "0.0.1",
                    "moduleDescription": "test",
                    "moduleAuthors": ["protocoltest@example.com"]
                    ${it.extraConfig ?: ""}
             }
           }
        }
        """.trimIndent()
    }
    return """
        {
            "version": "1.0",
            "projections": {
                $projections
            }
        }
    """.trimIndent()
}

enum class Cargo(val toString: String) {
    CHECK("cargoCheck"),
    TEST("cargoTest"),
    DOCS("cargoDocs"),
    CLIPPY("cargoClippy");
}

private fun generateCargoWorkspace(pluginName: String, tests: List<CodegenTest>) =
    """
    [workspace]
    members = [
        ${tests.joinToString(",") { "\"${it.module}/$pluginName\"" }}
    ]
    """.trimIndent()

/**
 * Filter the service integration tests for which to generate Rust crates in [allTests] using the given [properties].
 */
private fun codegenTests(properties: PropertyRetriever, allTests: List<CodegenTest>): List<CodegenTest> {
    val modulesOverride = properties.get("modules")?.split(",")?.map { it.trim() }

    val ret = if (modulesOverride != null) {
        println("modulesOverride: $modulesOverride")
        allTests.filter { modulesOverride.contains(it.module) }
    } else {
        allTests
    }
    require(ret.isNotEmpty()) {
        "None of the provided module overrides (`$modulesOverride`) are valid test services (`${allTests.map { it.module }}`)"
    }
    return ret
}

val AllCargoCommands = listOf(Cargo.CHECK, Cargo.TEST, Cargo.CLIPPY, Cargo.DOCS)

/**
 * Filter the Cargo commands to be run on the generated Rust crates using the given [properties].
 * The list of Cargo commands that is run by default is defined in [AllCargoCommands].
 */
fun cargoCommands(properties: PropertyRetriever): List<Cargo> {
    val cargoCommandsOverride = properties.get("cargoCommands")?.split(",")?.map { it.trim() }?.map {
        when (it) {
            "check" -> Cargo.CHECK
            "test" -> Cargo.TEST
            "docs" -> Cargo.DOCS
            "clippy" -> Cargo.CLIPPY
            else -> throw IllegalArgumentException("Unexpected Cargo command `$it` (valid commands are `check`, `test`, `docs`, `clippy`)")
        }
    }

    val ret = if (cargoCommandsOverride != null) {
        println("cargoCommandsOverride: $cargoCommandsOverride")
        AllCargoCommands.filter { cargoCommandsOverride.contains(it) }
    } else {
        AllCargoCommands
    }
    require(ret.isNotEmpty()) {
        "None of the provided cargo commands (`$cargoCommandsOverride`) are valid cargo commands (`${AllCargoCommands.map { it.toString }}`)"
    }
    return ret
}

fun Project.registerGenerateSmithyBuildTask(
    rootProject: Project,
    pluginName: String,
    allCodegenTests: List<CodegenTest>
) {
    val properties = PropertyRetriever(rootProject, this)
    this.tasks.register("generateSmithyBuild") {
        description = "generate smithy-build.json"
        outputs.file(project.projectDir.resolve("smithy-build.json"))

        doFirst {
            project.projectDir.resolve("smithy-build.json")
                .writeText(
                    generateSmithyBuild(
                        rootProject.projectDir.absolutePath,
                        pluginName,
                        codegenTests(properties, allCodegenTests)
                    )
                )

            // If this is a rebuild, cache all the hashes of the generated Rust files. These are later used by the
            // `modifyMtime` task.
            project.extra[previousBuildHashesKey] = project.buildDir.walk()
                .filter { it.isFile }
                .map {
                    getChecksumForFile(it) to it.lastModified()
                }
                .toMap()
        }
    }
}

fun Project.registerGenerateCargoWorkspaceTask(
    rootProject: Project,
    pluginName: String,
    allCodegenTests: List<CodegenTest>,
    workingDirUnderBuildDir: String
) {
    val properties = PropertyRetriever(rootProject, this)
    project.tasks.register("generateCargoWorkspace") {
        description = "generate Cargo.toml workspace file"
        doFirst {
            project.buildDir.resolve("$workingDirUnderBuildDir/Cargo.toml")
                .writeText(generateCargoWorkspace(pluginName, codegenTests(properties, allCodegenTests)))
        }
    }
}

fun Project.registerGenerateCargoConfigTomlTask(
    outputDir: File
) {
    this.tasks.register("generateCargoConfigToml") {
        description = "generate `.cargo/config.toml`"
        doFirst {
            outputDir.resolve(".cargo").mkdir()
            outputDir.resolve(".cargo/config.toml")
                .writeText(
                    """
                    [build]
                    rustflags = ["--deny", "warnings"]
                    """.trimIndent()
                )
        }
    }
}

const val previousBuildHashesKey = "previousBuildHashes"

fun Project.registerModifyMtimeTask() {
    // Cargo uses `mtime` (among other factors) to determine whether a compilation unit needs a rebuild. While developing,
    // it is likely that only a small number of the generated crate files are modified across rebuilds. This task compares
    // the hashes of the newly generated files with the (previously cached) old ones, and restores their `mtime`s if the
    // hashes coincide.
    // Debugging tip: it is useful to run with `CARGO_LOG=cargo::core::compiler::fingerprint=trace` to learn why Cargo
    // determines a compilation unit needs a rebuild.
    // For more information see https://github.com/awslabs/smithy-rs/issues/1412.
    this.tasks.register("modifyMtime") {
        description = "modify Rust files' `mtime` if the contents did not change"
        dependsOn("generateSmithyBuild")

        doFirst {
            if (!project.extra.has(previousBuildHashesKey)) {
                println("No hashes from a previous build exist because `generateSmithyBuild` is up to date, skipping `mtime` fixups")
            } else {
                @Suppress("UNCHECKED_CAST") val previousBuildHashes: Map<String, Long> = project.extra[previousBuildHashesKey] as Map<String, Long>

                project.buildDir.walk()
                    .filter { it.isFile }
                    .map {
                        getChecksumForFile(it) to it
                    }
                    .forEach { (currentHash, currentFile) ->
                        previousBuildHashes[currentHash]?.also { oldMtime ->
                            println("Setting `mtime` of $currentFile back to `$oldMtime` because its hash `$currentHash` remained unchanged after a rebuild.")
                            currentFile.setLastModified(oldMtime)
                        }
                    }
            }
        }
    }
}

fun Project.registerCargoCommandsTasks(
    outputDir: File,
    defaultRustDocFlags: String
) {
    this.tasks.register<Exec>(Cargo.CHECK.toString) {
        dependsOn("assemble", "modifyMtime", "generateCargoConfigToml")
        workingDir(outputDir)
        commandLine("cargo", "check", "--lib", "--tests", "--benches")
    }

    this.tasks.register<Exec>(Cargo.TEST.toString) {
        dependsOn("assemble", "modifyMtime", "generateCargoConfigToml")
        workingDir(outputDir)
        commandLine("cargo", "test")
    }

    this.tasks.register<Exec>(Cargo.DOCS.toString) {
        dependsOn("assemble", "modifyMtime", "generateCargoConfigToml")
        workingDir(outputDir)
        environment("RUSTDOCFLAGS", defaultRustDocFlags)
        commandLine("cargo", "doc", "--no-deps", "--document-private-items")
    }

    this.tasks.register<Exec>(Cargo.CLIPPY.toString) {
        dependsOn("assemble", "modifyMtime", "generateCargoConfigToml")
        workingDir(outputDir)
        commandLine("cargo", "clippy")
    }
}
