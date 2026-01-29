/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import org.gradle.api.Project
import org.gradle.api.tasks.Copy
import org.gradle.api.tasks.Exec
import org.gradle.kotlin.dsl.extra
import org.gradle.kotlin.dsl.register
import java.io.File

/**
 * This file contains common functionality shared across the build scripts for the
 * `codegen-client-test` and `codegen-server-test` modules.
 */

data class CodegenTest(
    val service: String,
    val module: String,
    val extraConfig: String? = null,
    val extraCodegenConfig: String? = null,
    val imports: List<String> = emptyList(),
)

/**
 * Extension function to generate both http@0 (legacy) and http@1 versions of a CodegenTest.
 *
 * @return List containing both http@0 (with -http0x suffix) and http@1 (no suffix) versions
 */
fun CodegenTest.bothHttpVersions(): List<CodegenTest> {
    return listOf(
        // http@0 version (legacy) - no http-1x flag, add -http0x suffix
        this.copy(
            module = "${this.module}-http0x",
        ),
        // http@1 version - with http-1x flag, no suffix
        this.copy(
            extraCodegenConfig =
                if (this.extraCodegenConfig?.isNotEmpty() == true) {
                    """"http-1x": true, ${this.extraCodegenConfig}"""
                } else {
                    """"http-1x": true"""
                },
        ),
    )
}

fun generateImports(imports: List<String>): String =
    if (imports.isEmpty()) {
        ""
    } else {
        "\"imports\": [${imports.joinToString(", ") { "\"$it\"" }}],"
    }

val RustKeywords =
    setOf(
        "as",
        "break",
        "const",
        "continue",
        "crate",
        "else",
        "enum",
        "extern",
        "false",
        "fn",
        "for",
        "if",
        "impl",
        "in",
        "let",
        "loop",
        "match",
        "mod",
        "move",
        "mut",
        "pub",
        "ref",
        "return",
        "self",
        "Self",
        "static",
        "struct",
        "super",
        "trait",
        "true",
        "type",
        "unsafe",
        "use",
        "where",
        "while",
        "async",
        "await",
        "dyn",
        "abstract",
        "become",
        "box",
        "do",
        "final",
        "macro",
        "override",
        "priv",
        "typeof",
        "unsized",
        "virtual",
        "yield",
        "try",
    )

fun toRustCrateName(input: String): String {
    if (input.isBlank()) {
        throw IllegalArgumentException("Rust crate name cannot be empty")
    }
    val lowerCased = input.lowercase()
    // Replace any sequence of characters that are not lowercase letters, numbers, dashes, or underscores with a single underscore.
    val sanitized = lowerCased.replace(Regex("[^a-z0-9_-]+"), "_")
    // Trim leading or trailing underscores.
    val trimmed = sanitized.trim('_')
    // Check if the resulting string is empty, purely numeric, or a reserved name
    val finalName =
        when {
            trimmed.isEmpty() -> throw IllegalArgumentException("Rust crate name after sanitizing cannot be empty.")
            trimmed.matches(Regex("\\d+")) -> "n$trimmed" // Prepend 'n' if the name is purely numeric.
            trimmed in RustKeywords -> "${trimmed}_" // Append an underscore if the name is reserved.
            else -> trimmed
        }
    return finalName
}

private fun generateSmithyBuild(
    projectDir: String,
    pluginName: String,
    tests: List<CodegenTest>,
): String {
    val projections =
        tests.joinToString(",\n") {
            """
            "${it.module}": {
                ${generateImports(it.imports)}
                "plugins": {
                    "$pluginName": {
                        "runtimeConfig": {
                            "relativePath": "$projectDir/rust-runtime"
                        },
                        "codegen": {
                            ${it.extraCodegenConfig ?: ""}
                        },
                        "service": "${it.service}",
                        "module": "${toRustCrateName(it.module)}",
                        "moduleVersion": "0.0.1",
                        "moduleDescription": "test",
                        "moduleAuthors": ["protocoltest@example.com"]
                        ${it.extraConfig ?: ""}
                    }
                }
            }
            """.trimIndent()
        }
    return (
        """
        {
            "version": "1.0",
            "projections": {
                $projections
            }
        }
        """.trimIndent()
    )
}

enum class Cargo(val toString: String) {
    CHECK("cargoCheck"),
    TEST("cargoTest"),
    DOCS("cargoDoc"),
    CLIPPY("cargoClippy"),
}

private fun generateCargoWorkspace(
    pluginName: String,
    tests: List<CodegenTest>,
) = (
    """
    [workspace]
    resolver = "2"
    members = [
        ${tests.joinToString(",") { "\"${it.module}/$pluginName\"" }}
    ]
    """.trimIndent()
)

/**
 * Filter the service integration tests for which to generate Rust crates in [allTests] using the given [properties].
 */
private fun codegenTests(
    properties: PropertyRetriever,
    allTests: List<CodegenTest>,
): List<CodegenTest> {
    val modulesOverride = properties.get("modules")?.split(",")?.map { it.trim() }

    val ret =
        if (modulesOverride != null) {
            println("modulesOverride: $modulesOverride")
            allTests.filter { modulesOverride.contains(it.module) }
        } else {
            allTests
        }
    require(ret.isNotEmpty()) {
        "None of the provided module overrides (`$modulesOverride`) are valid test services (`${
            allTests.map {
                it.module
            }
        }`)"
    }
    return ret
}

val AllCargoCommands = listOf(Cargo.CHECK, Cargo.TEST, Cargo.CLIPPY, Cargo.DOCS)

/**
 * Filter the Cargo commands to be run on the generated Rust crates using the given [properties].
 * The list of Cargo commands that is run by default is defined in [AllCargoCommands].
 */
fun cargoCommands(properties: PropertyRetriever): List<Cargo> {
    val cargoCommandsOverride =
        properties.get("cargoCommands")?.split(",")?.map { it.trim() }?.map {
            when (it) {
                "check" -> Cargo.CHECK
                "test" -> Cargo.TEST
                "doc" -> Cargo.DOCS
                "clippy" -> Cargo.CLIPPY
                else -> throw IllegalArgumentException("Unexpected Cargo command `$it` (valid commands are `check`, `test`, `doc`, `clippy`)")
            }
        }

    val ret =
        if (cargoCommandsOverride != null) {
            println("cargoCommandsOverride: $cargoCommandsOverride")
            AllCargoCommands.filter { cargoCommandsOverride.contains(it) }
        } else {
            AllCargoCommands
        }
    require(ret.isNotEmpty()) {
        "None of the provided cargo commands (`$cargoCommandsOverride`) are valid cargo commands (`${
            AllCargoCommands.map {
                it.toString
            }
        }`)"
    }
    return ret
}

fun Project.registerGenerateSmithyBuildTask(
    rootProject: Project,
    pluginName: String,
    allCodegenTests: List<CodegenTest>,
) {
    val properties = PropertyRetriever(rootProject, this)
    this.tasks.register("generateSmithyBuild") {
        description = "generate smithy-build.json"
        outputs.file(project.projectDir.resolve("smithy-build.json"))
        // Declare Smithy model files as inputs so task reruns when they change
        allCodegenTests.flatMap { it.imports }.forEach {
            val resolvedPath = project.projectDir.resolve(it)
            if (resolvedPath.exists()) {
                inputs.file(resolvedPath)
            }
        }

        doFirst {
            project.projectDir.resolve("smithy-build.json")
                .writeText(
                    generateSmithyBuild(
                        rootProject.projectDir.absolutePath,
                        pluginName,
                        codegenTests(properties, allCodegenTests),
                    ),
                )

            // If this is a rebuild, cache all the hashes of the generated Rust files. These are later used by the
            // `modifyMtime` task.
            project.extra[PREVIOUS_BUILD_HASHES_KEY] =
                project.layout.buildDirectory.get().asFile.walk()
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
    workingDirUnderBuildDir: String,
) {
    val properties = PropertyRetriever(rootProject, this)
    project.tasks.register("generateCargoWorkspace") {
        description = "generate Cargo.toml workspace file"
        val path = project.layout.buildDirectory.file("$workingDirUnderBuildDir/Cargo.toml")
        outputs.file(path)
        doFirst {
            path.get().asFile.writeText(generateCargoWorkspace(pluginName, codegenTests(properties, allCodegenTests)))
        }
    }
}

fun Project.registerGenerateCargoConfigTomlTask(outputDir: File) {
    this.tasks.register("generateCargoConfigToml") {
        description = "generate `.cargo/config.toml`"
        // TODO(https://github.com/smithy-lang/smithy-rs/issues/1068): Once doc normalization
        // is completed, warnings can be prohibited in rustdoc by setting `rustdocflags` to `-D warnings`.
        doFirst {
            outputDir.resolve(".cargo").mkdirs()
            outputDir.resolve(".cargo/config.toml")
                .writeText(
                    """
                    [build]
                    rustflags = ["--deny", "warnings", "--cfg", "aws_sdk_unstable"]
                    """.trimIndent(),
                )
        }
    }
}

fun Project.registerCopyCheckedInCargoLockfileTask(
    lockfile: File,
    destinationDir: File,
) {
    this.tasks.register<Copy>("copyCheckedInCargoLockfile") {
        description = "copy checked-in `Cargo.lock` to the destination directory"
        from(lockfile)
        into(destinationDir)
    }
}

const val PREVIOUS_BUILD_HASHES_KEY = "previousBuildHashes"

fun Project.registerModifyMtimeTask() {
    // Cargo uses `mtime` (among other factors) to determine whether a compilation unit needs a rebuild. While developing,
    // it is likely that only a small number of the generated crate files are modified across rebuilds. This task compares
    // the hashes of the newly generated files with the (previously cached) old ones, and restores their `mtime`s if the
    // hashes coincide.
    // Debugging tip: it is useful to run with `CARGO_LOG=cargo::core::compiler::fingerprint=trace` to learn why Cargo
    // determines a compilation unit needs a rebuild.
    // For more information see https://github.com/smithy-lang/smithy-rs/issues/1412.
    this.tasks.register("modifyMtime") {
        description = "modify Rust files' `mtime` if the contents did not change"
        dependsOn("generateSmithyBuild")

        doFirst {
            if (!project.extra.has(PREVIOUS_BUILD_HASHES_KEY)) {
                println("No hashes from a previous build exist because `generateSmithyBuild` is up to date, skipping `mtime` fixups")
            } else {
                @Suppress("UNCHECKED_CAST")
                val previousBuildHashes: Map<String, Long> =
                    project.extra[PREVIOUS_BUILD_HASHES_KEY] as Map<String, Long>

                project.layout.buildDirectory.get().asFile.walk()
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

fun Project.registerCargoCommandsTasks(outputDir: File) {
    val dependentTasks =
        listOfNotNull(
            "assemble",
            "generateCargoConfigToml",
            this.tasks.findByName("modifyMtime")?.let { "modifyMtime" },
        )

    this.tasks.register<Exec>(Cargo.CHECK.toString) {
        dependsOn(dependentTasks)
        workingDir(outputDir)
        commandLine("cargo", "check", "--lib", "--tests", "--benches", "--all-features")
    }

    this.tasks.register<Exec>(Cargo.TEST.toString) {
        dependsOn(dependentTasks)
        workingDir(outputDir)
        commandLine("cargo", "test", "--all-features", "--no-fail-fast")
    }

    this.tasks.register<Exec>(Cargo.DOCS.toString) {
        dependsOn(dependentTasks)
        workingDir(outputDir)
        val args =
            mutableListOf(
                "--no-deps",
                "--document-private-items",
            )
        commandLine("cargo", "doc", *args.toTypedArray())
    }

    this.tasks.register<Exec>(Cargo.CLIPPY.toString) {
        dependsOn(dependentTasks)
        workingDir(outputDir)
        commandLine("cargo", "clippy")
    }
}
