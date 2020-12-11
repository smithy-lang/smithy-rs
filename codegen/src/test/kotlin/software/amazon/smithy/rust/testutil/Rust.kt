/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.testutil

import com.moandjiezana.toml.TomlWriter
import org.intellij.lang.annotations.Language
import software.amazon.smithy.build.FileManifest
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.codegen.core.writer.CodegenWriterDelegator
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.lang.CargoDependency
import software.amazon.smithy.rust.codegen.lang.RustDependency
import software.amazon.smithy.rust.codegen.lang.RustWriter
import software.amazon.smithy.rust.codegen.lang.raw
import software.amazon.smithy.rust.codegen.lang.rustBlock
import software.amazon.smithy.rust.codegen.smithy.BuildSettings
import software.amazon.smithy.rust.codegen.smithy.CodegenConfig
import software.amazon.smithy.rust.codegen.smithy.RustSettings
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.SymbolVisitorConfig
import software.amazon.smithy.rust.codegen.smithy.finalize
import software.amazon.smithy.rust.codegen.util.CommandFailed
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.runCommand
import java.io.File
import java.nio.file.Path

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
        val workspaceToml = TomlWriter().write(
            mapOf(
                "workspace" to mapOf(
                    "members" to subprojects
                )
            )
        )
        cargoToml.writeText(workspaceToml)
    }

    fun subproject(): File {
        synchronized(subprojects) {
            val newProject = createTempDir(directory = baseDir)
            subprojects.add(newProject.name)
            generate()
            return newProject
        }
    }

    fun testProject(symbolProvider: RustSymbolProvider?): TestWriterDelegator {
        val subprojectDir = subproject()
        val symbolProvider = symbolProvider ?: object : RustSymbolProvider {
            override fun config(): SymbolVisitorConfig {
                TODO("Not yet implemented")
            }

            override fun toSymbol(shape: Shape?): Symbol {
                TODO("Not yet implemented")
            }
        }
        return TestWriterDelegator(
            FileManifest.create(subprojectDir.toPath()),
            symbolProvider
        )
    }
}

fun RustWriter.unitTest(
    @Language("Rust", prefix = "fn test() {", suffix = "}") test: String,
    name: String? = null
) {
    val testName = name ?: safeName("test_")
    raw("#[test]")
    rustBlock("fn $testName()") {
        writeWithNoFormatting(test)
    }
}

class TestWriterDelegator(fileManifest: FileManifest, symbolProvider: RustSymbolProvider) :
    CodegenWriterDelegator<RustWriter>(fileManifest, symbolProvider, RustWriter.Factory) {
    val baseDir: Path = fileManifest.baseDir
}

fun TestWriterDelegator.compileAndTest() {
    this.finalize(
        RustSettings(
            ShapeId.from("fake#Fake"),
            "test_${baseDir.toFile().nameWithoutExtension}",
            "0.0.1",
            runtimeConfig = TestRuntimeConfig,
            codegenConfig = CodegenConfig(),
            build = BuildSettings.Default()
        )
    )
    "cargo test".runCommand(baseDir)
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
    val deps = this.dependencies.map { RustDependency.fromSymbolDependency(it) }.filterIsInstance<CargoDependency>()
    try {
        val module = if (this.namespace.contains("::")) {
            this.namespace.split("::")[1]
        } else {
            "lib"
        }
        val output = this.toString()
            .compileAndTest(deps.toSet(), module = module, main = main, strict = clippy)
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
    deps: Set<CargoDependency>,
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
