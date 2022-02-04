/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.testutil

import com.moandjiezana.toml.TomlWriter
import org.intellij.lang.annotations.Language
import software.amazon.smithy.build.FileManifest
import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.EnumDefinition
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustDependency
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.raw
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.smithy.CodegenConfig
import software.amazon.smithy.rust.codegen.smithy.DefaultPublicModules
import software.amazon.smithy.rust.codegen.smithy.MaybeRenamed
import software.amazon.smithy.rust.codegen.smithy.RustCrate
import software.amazon.smithy.rust.codegen.smithy.RustSettings
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.SymbolVisitorConfig
import software.amazon.smithy.rust.codegen.util.CommandFailed
import software.amazon.smithy.rust.codegen.util.PANIC
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.runCommand
import java.io.File
import java.nio.file.Files.createTempDirectory
import java.nio.file.Path

/**
 * Waiting for Kotlin to stabilize their temp directory stuff
 */
private fun tempDir(directory: File? = null): File {
    return if (directory != null) {
        createTempDirectory(directory.toPath(), "smithy-test").toFile()
    } else {
        createTempDirectory("smithy-test").toFile()
    }
}

/**
 * Creates a Cargo workspace shared among all tests
 *
 * This workspace significantly improves test performance by sharing dependencies between different tests.
 */
object TestWorkspace {
    private val baseDir =
        System.getenv("SMITHY_TEST_WORKSPACE")?.let { File(it) } ?: tempDir()
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
            val newProject = tempDir(directory = baseDir)
            newProject.resolve("Cargo.toml").writeText(
                """
                [package]
                name = "stub-${newProject.name}"
                version = "0.0.1"
                """.trimIndent()
            )
            subprojects.add(newProject.name)
            generate()
            return newProject
        }
    }

    @Suppress("NAME_SHADOWING")
    fun testProject(symbolProvider: RustSymbolProvider? = null): TestWriterDelegator {
        val subprojectDir = subproject()
        val symbolProvider = symbolProvider ?: object : RustSymbolProvider {
            override fun config(): SymbolVisitorConfig {
                PANIC("")
            }

            override fun toEnumVariantName(definition: EnumDefinition): MaybeRenamed? {
                PANIC("")
            }

            override fun toSymbol(shape: Shape?): Symbol {
                PANIC("")
            }

            override fun isRequiredTraitHandled(member: MemberShape, useNullableIndex: Boolean): Boolean {
                PANIC("")
            }
        }
        return TestWriterDelegator(
            FileManifest.create(subprojectDir.toPath()),
            symbolProvider
        )
    }
}

/**
 * Generates a test plugin context for [model] and returns the plugin context and the path it is rooted it.
 *
 * Example:
 * ```kotlin
 * val (pluginContext, path) = generatePluginContext(model)
 * CodegenVisitor(pluginContext).execute()
 * "cargo test".runCommand(path)
 * ```
 */
fun generatePluginContext(model: Model): Pair<PluginContext, Path> {
    val testDir = TestWorkspace.subproject()
    val moduleName = "test_${testDir.nameWithoutExtension}"
    val testPath = testDir.toPath()
    val manifest = FileManifest.create(testPath)
    val settings = Node.objectNodeBuilder()
        .withMember("module", Node.from(moduleName))
        .withMember("moduleVersion", Node.from("1.0.0"))
        .withMember("moduleDescription", Node.from("test"))
        .withMember("moduleAuthors", Node.fromStrings("testgenerator@smithy.com"))
        .withMember(
            "runtimeConfig",
            Node.objectNodeBuilder().withMember(
                "relativePath",
                Node.from((TestRuntimeConfig.runtimeCrateLocation).path)
            ).build()
        )
        .build()
    val pluginContext = PluginContext.builder().model(model).fileManifest(manifest).settings(settings).build()
    return pluginContext to testPath
}

fun RustWriter.unitTest(
    name: String? = null,
    @Language("Rust", prefix = "fn test() {", suffix = "}") test: String
) {
    val testName = name ?: safeName("test")
    raw("#[test]")
    rustBlock("fn $testName()") {
        writeWithNoFormatting(test)
    }
}

class TestWriterDelegator(fileManifest: FileManifest, symbolProvider: RustSymbolProvider) :
    RustCrate(fileManifest, symbolProvider, DefaultPublicModules) {
    val baseDir: Path = fileManifest.baseDir
}

/**
 * Setting `runClippy` to true can be helpful when debugging clippy failures, but
 * should generally be set to false to avoid invalidating the Cargo cache between
 * every unit test run.
 */
fun TestWriterDelegator.compileAndTest(runClippy: Boolean = false) {
    val stubModel = """
        namespace fake
        service Fake {
            version: "123"
        }
    """.asSmithyModel()
    this.finalize(
        rustSettings(stubModel),
        stubModel,
        manifestCustomizations = emptyMap(),
        libRsCustomizations = listOf(),
    )
    try {
        "cargo fmt".runCommand(baseDir)
    } catch (e: Exception) {
        // cargo fmt errors are useless, ignore
    }
    val env = mapOf("RUSTFLAGS" to "-A dead_code")
    "cargo test".runCommand(baseDir, env)
    if (runClippy) {
        "cargo clippy".runCommand(baseDir, env)
    }
}

fun TestWriterDelegator.rustSettings(stubModel: Model = "namespace test".asSmithyModel()) =
    RustSettings(
        ShapeId.from("fake#Fake"),
        "test_${baseDir.toFile().nameWithoutExtension}",
        "0.0.1",
        moduleAuthors = listOf("test@module.com"),
        moduleDescription = "test",
        moduleRepository = null,
        runtimeConfig = TestRuntimeConfig,
        codegenConfig = CodegenConfig(),
        license = null,
        model = stubModel
    )

fun String.shouldParseAsRust() {
    // quick hack via rustfmt
    val tempFile = File.createTempFile("rust_test", ".rs")
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
    val deps = this.dependencies.map { RustDependency.fromSymbolDependency(it) }.filterIsInstance<CargoDependency>()
    val module = if (this.namespace.contains("::")) {
        this.namespace.split("::")[1]
    } else {
        "lib"
    }
    val tempDir = this.toString()
        .intoCrate(deps.toSet(), module = module, main = main, strict = clippy)
    val mainRs = tempDir.resolve("src/main.rs")
    val testModule = tempDir.resolve("src/$module.rs")
    try {
        val testOutput = if ((mainRs.readText() + testModule.readText()).contains("#[test]")) {
            "cargo test".runCommand(tempDir.toPath())
        } else {
            "cargo check".runCommand(tempDir.toPath())
        }
        if (expectFailure) {
            println("Test sources for debugging: file://${testModule.absolutePath}")
        }
        return testOutput
    } catch (e: CommandFailed) {
        if (!expectFailure) {
            println("Test sources for debugging: file://${testModule.absolutePath}")
        }
        throw e
    }
}

@JvmOverloads
private fun String.intoCrate(
    deps: Set<CargoDependency>,
    module: String? = null,
    main: String = "",
    strict: Boolean = false
): File {
    this.shouldParseAsRust()
    val tempDir = TestWorkspace.subproject()
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
    return tempDir
}

fun String.shouldCompile(): File {
    this.shouldParseAsRust()
    val tempFile = File.createTempFile("rust_test", ".rs")
    val tempDir = tempDir()
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
