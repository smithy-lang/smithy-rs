import java.lang.IllegalArgumentException

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * This file contains common functionality shared across the buildscripts for the `codegen-test` and `codegen-server-test`
 * modules.
 */

data class CodegenTest(val service: String, val module: String, val extraConfig: String? = null)

fun generateSmithyBuild(projectDir: String, pluginName: String, tests: List<CodegenTest>): String {
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

fun generateCargoWorkspace(pluginName: String, tests: List<CodegenTest>) =
    """
    [workspace]
    members = [
        ${tests.joinToString(",") { "\"${it.module}/$pluginName\"" }}
    ]
    """.trimIndent()

/**
 * Filter the service integration tests for which to generate Rust crates in [allTests] using the given [properties].
 */
fun codegenTests(properties: PropertyRetriever, allTests: List<CodegenTest>): List<CodegenTest> {
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
