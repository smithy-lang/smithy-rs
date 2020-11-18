/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.testutil

import org.intellij.lang.annotations.Language
import software.amazon.smithy.rust.codegen.lang.RustDependency
import software.amazon.smithy.rust.codegen.lang.RustWriter
import software.amazon.smithy.rust.codegen.util.CommandFailed
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.runCommand
import java.io.File

/**
 * Creates a Cargo workspace shared among all tests
 *
 * This workspace significantly improves test performance by sharing dependencies between different tests.
 */
object TestWorkspace {
    private val baseDir = System.getenv("SMITHY_TEST_WORKSPACE")?.let { File(it) } ?: createTempDir()
    private val subprojects = mutableListOf<String>()

    init {
        baseDir.mkdirs()
    }

    private fun generate() {
        val cargoToml = baseDir.resolve("Cargo.toml")
        cargoToml.writeText(
            """
            [workspace]
            members = [
                ${subprojects.joinToString { it.dq() }}
            ]
            """.trimIndent()
        )
    }

    fun subproject(): File {
        synchronized(subprojects) {
            val newProject = createTempDir(directory = baseDir)
            subprojects.add(newProject.name)
            generate()
            return newProject
        }
    }
}

// TODO: unify these test helpers a bit
fun String.shouldParseAsRust() {
    // quick hack via rustfmt
    val tempFile = createTempFile(suffix = ".rs")
    tempFile.writeText(this)
    "rustfmt ${tempFile.absolutePath}".runCommand()
}

/**
 * Compiles the contents of the given writer (including dependencies) and runs the tests
 */
fun RustWriter.compileAndTest(
    @Language("Rust", prefix = "fn test() {", suffix = "}")
    main: String = "",
    clippy: Boolean = false,
    expectFailure: Boolean = false
): String {
    // TODO: if there are no dependencies, we can be a bit quicker
    val deps = this.dependencies.map { RustDependency.fromSymbolDependency(it) }
    try {
        val output = this.toString()
            .compileAndTest(deps.toSet(), module = this.namespace.split("::")[1], main = main, strict = clippy)
        if (expectFailure) {
            println(this.toString())
        }
        return output
    } catch (e: CommandFailed) {
        // When the test fails, print the code for convenience
        if (!expectFailure) {
            println(this.toString())
        }
        throw e
    }
}

fun String.compileAndTest(
    deps: Set<RustDependency>,
    module: String? = null,
    main: String = "",
    strict: Boolean = false
): String {
    this.shouldParseAsRust()
    val tempDir = TestWorkspace.subproject()
    // TODO: unify this with CargoTomlGenerator
    val cargoToml = """
    [package]
    name = ${tempDir.nameWithoutExtension.dq()}
    version = "0.0.1"
    authors = ["rcoh@amazon.com"]
    edition = "2018"

    [dependencies]
    ${deps.joinToString("\n") { it.toString() }}
    """.trimIndent()
    tempDir.resolve("Cargo.toml").writeText(cargoToml)
    tempDir.resolve("src").mkdirs()
    val mainRs = tempDir.resolve("src/main.rs")
    val testModule = tempDir.resolve("src/$module.rs")
    testModule.writeText(this)
    if (main.isNotBlank()) {
        testModule.appendText(
            """
            #[test]
            fn test() {
                $main
            }
            """.trimIndent()
        )
    }
    mainRs.appendText(
        """
        pub mod $module;
        pub use crate::$module::*;
        pub fn main() {}
        """.trimIndent()
    )
    val testOutput = if ((mainRs.readText() + testModule.readText()).contains("#[test]")) {
        "cargo test".runCommand(tempDir.toPath())
    } else {
        "cargo check".runCommand(tempDir.toPath())
    }
    if (strict) {
        "cargo clippy -- -D warnings".runCommand(tempDir.toPath())
    }
    return testOutput
}

fun String.shouldCompile(): File {
    this.shouldParseAsRust()
    val tempFile = createTempFile(suffix = ".rs")
    val tempDir = createTempDir()
    tempFile.writeText(this)
    if (!this.contains("fn main")) {
        tempFile.appendText("\nfn main() {}\n")
    }
    "rustc ${tempFile.absolutePath} -o ${tempDir.absolutePath}/output".runCommand()
    return tempDir.resolve("output")
}

/**
 * Inserts the provided strings as a main function and executes the result. This is intended to be used to validate
 * that generated code compiles and has some basic properties.
 *
 * Example usage:
 * ```
 * "struct A { a: u32 }".quickTest("let a = A { a: 5 }; assert_eq!(a.a, 5);")
 * ```
 */
fun String.compileAndRun(vararg strings: String) {
    val contents = this + "\nfn main() { \n ${strings.joinToString("\n")} }"
    val binary = contents.shouldCompile()
    binary.absolutePath.runCommand()
}
