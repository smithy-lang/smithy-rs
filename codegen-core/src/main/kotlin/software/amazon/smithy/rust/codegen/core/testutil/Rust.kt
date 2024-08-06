/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.testutil

import com.moandjiezana.toml.TomlWriter
import org.intellij.lang.annotations.Language
import software.amazon.smithy.build.FileManifest
import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.loader.ModelAssembler
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.generated.BuildEnvironment
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.DependencyScope
import software.amazon.smithy.rust.codegen.core.rustlang.RustDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.raw
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CoreCodegenConfig
import software.amazon.smithy.rust.codegen.core.smithy.ModuleDocProvider
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.generators.CargoTomlGenerator
import software.amazon.smithy.rust.codegen.core.smithy.mergeDependencyFeatures
import software.amazon.smithy.rust.codegen.core.smithy.mergeIdenticalTestDependencies
import software.amazon.smithy.rust.codegen.core.util.CommandError
import software.amazon.smithy.rust.codegen.core.util.letIf
import software.amazon.smithy.rust.codegen.core.util.orNullIfEmpty
import software.amazon.smithy.rust.codegen.core.util.runCommand
import java.io.File
import java.nio.file.Files
import java.nio.file.Files.createTempDirectory
import java.nio.file.Path
import kotlin.io.path.Path
import kotlin.io.path.absolutePathString
import kotlin.io.path.writeText

val TestModuleDocProvider =
    object : ModuleDocProvider {
        override fun docsWriter(module: RustModule.LeafModule): Writable =
            writable {
                docs("Some test documentation\n\nSome more details...")
            }
    }

/**
 * Waiting for Kotlin to stabilize their temp directory functionality
 */
private fun tempDir(directory: File? = null): File {
    return if (directory != null) {
        createTempDirectory(directory.toPath(), "smithy-test").toFile()
    } else {
        createTempDirectory("smithy-test").toFile()
    }
}

/**
 * This function returns the minimum supported Rust version, as specified in the `gradle.properties` file
 * located at the root of the project.
 */
fun msrv(): String = BuildEnvironment.MSRV

/**
 * Generates the `rust-toolchain.toml` file in the specified directory.
 *
 * The compiler version is set in `gradle.properties` under the `rust.msrv` property.
 * The Gradle task `generateRustMsrvFile` generates the Kotlin class
 * `software.amazon.smithy.rust.codegen.core.generated.RustMsrv.kt` and writes the value of `rust.msrv` into it.
 */
private fun File.generateRustToolchainToml() {
    resolve("rust-toolchain.toml").writeText(
        // Help rust select the right version when we run cargo test.
        """
        [toolchain]
        channel = "${msrv()}"
        """.trimIndent(),
    )
}

/**
 * Creates a Cargo workspace shared among all tests
 *
 * This workspace significantly improves test performance by sharing dependencies between different tests.
 */
object TestWorkspace {
    private val baseDir by lazy {
        val appDataDir =
            System.getProperty("APPDATA")
                ?: System.getenv("XDG_DATA_HOME")
                ?: System.getProperty("user.home")
                    ?.let { Path.of(it, ".local", "share").absolutePathString() }
                    ?.also { File(it).mkdirs() }
        if (appDataDir != null) {
            File(Path.of(appDataDir, "smithy-test-workspace").absolutePathString())
        } else {
            System.getenv("SMITHY_TEST_WORKSPACE")?.let { File(it) } ?: tempDir()
        }
    }
    private val subprojects = mutableListOf<String>()

    private val cargoLock: File by lazy {
        File(BuildEnvironment.PROJECT_DIR).resolve("aws/sdk/Cargo.lock")
    }

    init {
        baseDir.mkdirs()
    }

    private fun generate() {
        val cargoToml = baseDir.resolve("Cargo.toml")
        val workspaceToml =
            TomlWriter().write(
                mapOf(
                    "workspace" to
                        mapOf(
                            "resolver" to "2",
                            "members" to subprojects,
                        ),
                ),
            )
        cargoToml.writeText(workspaceToml)
        cargoLock.copyTo(baseDir.resolve("Cargo.lock"), true)
    }

    fun subproject(): File {
        synchronized(subprojects) {
            val newProject = tempDir(directory = baseDir)
            newProject.resolve("Cargo.toml").writeText(
                """
                [package]
                name = "stub-${newProject.name}"
                version = "0.0.1"
                """.trimIndent(),
            )
            newProject.generateRustToolchainToml()
            // ensure there at least an empty lib.rs file to avoid broken crates
            newProject.resolve("src").mkdirs()
            newProject.resolve("src/lib.rs").writeText("")
            subprojects.add(newProject.name)
            generate()
            return newProject
        }
    }

    fun testProject(
        model: Model = ModelAssembler().assemble().unwrap(),
        codegenConfig: CoreCodegenConfig = CoreCodegenConfig(),
    ): TestWriterDelegator = testProject(testSymbolProvider(model), codegenConfig)

    fun testProject(
        symbolProvider: RustSymbolProvider,
        codegenConfig: CoreCodegenConfig = CoreCodegenConfig(),
    ): TestWriterDelegator {
        val subprojectDir = subproject()
        return TestWriterDelegator(
            FileManifest.create(subprojectDir.toPath()),
            symbolProvider,
            codegenConfig,
        ).apply {
            lib {
                // If the test fails before the crate is finalized, we'll end up with a broken crate.
                // Since all tests are generated into the same workspace (to avoid re-compilation) a broken crate
                // breaks the workspace and all subsequent unit tests. By putting this comment in, we prevent
                // that state from occurring.
                rust("// touch lib.rs")
            }
        }
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
fun generatePluginContext(
    model: Model,
    additionalSettings: ObjectNode = ObjectNode.builder().build(),
    addModuleToEventStreamAllowList: Boolean = false,
    moduleVersion: String = "1.0.0",
    service: String? = null,
    runtimeConfig: RuntimeConfig? = null,
    overrideTestDir: File? = null,
): Pair<PluginContext, Path> {
    val testDir =
        overrideTestDir?.apply {
            mkdirs()
            generateRustToolchainToml()
        } ?: TestWorkspace.subproject()
    val moduleName = "test_${testDir.nameWithoutExtension}"
    val testPath = testDir.toPath()
    val manifest = FileManifest.create(testPath)
    var settingsBuilder =
        Node.objectNodeBuilder()
            .withMember("module", Node.from(moduleName))
            .withMember("moduleVersion", Node.from(moduleVersion))
            .withMember("moduleDescription", Node.from("test"))
            .withMember("moduleAuthors", Node.fromStrings("testgenerator@smithy.com"))
            .letIf(service != null) { it.withMember("service", service) }
            .withMember(
                "runtimeConfig",
                Node.objectNodeBuilder().withMember(
                    "relativePath",
                    Node.from(((runtimeConfig ?: TestRuntimeConfig).runtimeCrateLocation).path),
                ).build(),
            )

    if (addModuleToEventStreamAllowList) {
        settingsBuilder =
            settingsBuilder.withMember(
                "codegen",
                Node.objectNodeBuilder().withMember(
                    "eventStreamAllowList",
                    Node.fromStrings(moduleName),
                ).build(),
            )
    }

    val settings = settingsBuilder.merge(additionalSettings).build()
    val pluginContext = PluginContext.builder().model(model).fileManifest(manifest).settings(settings).build()
    return pluginContext to testPath
}

fun RustWriter.unitTest(
    name: String? = null,
    @Language("Rust", prefix = "fn test() {", suffix = "}") test: String,
) {
    val testName = name ?: safeName("test")
    raw("#[test]")
    rustBlock("fn $testName()") {
        writeWithNoFormatting(test)
    }
}

/*
 * Writes a Rust-style unit test
 */
fun RustWriter.unitTest(
    name: String,
    vararg args: Any,
    attribute: Attribute = Attribute.Test,
    additionalAttributes: List<Attribute> = emptyList(),
    async: Boolean = false,
    block: Writable,
): RustWriter {
    attribute.render(this)
    additionalAttributes.forEach { it.render(this) }
    if (async) {
        rust("async")
    }
    return testDependenciesOnly { rustBlock("fn $name()", *args, block = block) }
}

fun RustWriter.cargoDependencies() =
    dependencies.map { RustDependency.fromSymbolDependency(it) }
        .filterIsInstance<CargoDependency>().distinct()

fun RustWriter.assertNoNewDependencies(
    block: Writable,
    dependencyFilter: (CargoDependency) -> String?,
): RustWriter {
    val startingDependencies = cargoDependencies().toSet()
    block(this)
    val endingDependencies = cargoDependencies().toSet()
    val newDeps = (endingDependencies - startingDependencies)
    val invalidDeps =
        newDeps.mapNotNull { dep -> dependencyFilter(dep)?.let { message -> message to dep } }.orNullIfEmpty()
    if (invalidDeps != null) {
        val badDeps = invalidDeps.map { it.second.rustName }
        val writtenOut = this.toString()
        val badLines = writtenOut.lines().filter { line -> badDeps.any { line.contains(it) } }
        throw CodegenException(
            "found invalid dependencies. ${
                invalidDeps.map {
                    it.first
                }
            }\nHint: the following lines may be the problem.\n${
                badLines.joinToString(
                    separator = "\n",
                    prefix = "   ",
                )
            }",
        )
    }
    return this
}

fun RustWriter.testDependenciesOnly(block: Writable) =
    assertNoNewDependencies(block) { dep ->
        if (dep.scope != DependencyScope.Dev) {
            "Cannot add $dep — this writer should only add test dependencies."
        } else {
            null
        }
    }

fun testDependenciesOnly(block: Writable): Writable =
    {
        testDependenciesOnly(block)
    }

fun RustWriter.tokioTest(
    name: String,
    vararg args: Any,
    block: Writable,
) {
    unitTest(name, attribute = Attribute.TokioTest, async = true, block = block, args = args)
}

/**
 * WriterDelegator used for test purposes
 *
 * This exposes both the base directory and a list of [generatedFiles] for test purposes
 */
class TestWriterDelegator(
    private val fileManifest: FileManifest,
    symbolProvider: RustSymbolProvider,
    val codegenConfig: CoreCodegenConfig,
    moduleDocProvider: ModuleDocProvider = TestModuleDocProvider,
) : RustCrate(fileManifest, symbolProvider, codegenConfig, moduleDocProvider) {
    val baseDir: Path = fileManifest.baseDir

    fun printGeneratedFiles() {
        fileManifest.printGeneratedFiles()
    }

    fun generatedFiles() = fileManifest.files.map { baseDir.relativize(it) }
}

/**
 * Generate a new test module
 *
 * This should only be used in test code—the generated module name will be something like `tests_123`
 */
fun RustCrate.testModule(block: Writable) =
    lib {
        withInlineModule(
            RustModule.inlineTests(safeName("tests")),
            TestModuleDocProvider,
            block,
        )
    }

fun FileManifest.printGeneratedFiles() {
    println("Generated files:")
    this.files.forEach { path ->
        println("file:///$path")
    }
}

/**
 * Setting `runClippy` to true can be helpful when debugging clippy failures, but
 * should generally be set to `false` to avoid invalidating the Cargo cache between
 * every unit test run.
 */
fun TestWriterDelegator.compileAndTest(
    runClippy: Boolean = false,
    expectFailure: Boolean = false,
): String {
    val stubModel =
        """
        namespace fake
        service Fake {
            version: "123"
        }
        """.asSmithyModel()
    this.finalize(
        rustSettings(),
        stubModel,
        manifestCustomizations = emptyMap(),
        libRsCustomizations = listOf(),
    )
    println("Generated files:")
    printGeneratedFiles()
    try {
        "cargo fmt".runCommand(baseDir)
    } catch (e: Exception) {
        // cargo fmt errors are useless, ignore
    }

    // Clean `RUSTFLAGS` because in CI we pass in `--deny warnings` and
    // we still generate test code with warnings.
    // TODO(https://github.com/smithy-lang/smithy-rs/issues/3194)
    val env = mapOf("RUSTFLAGS" to "")
    baseDir.writeDotCargoConfigToml(listOf("--allow", "dead_code"))

    val testOutput = "cargo test".runCommand(baseDir, env)
    if (runClippy) {
        "cargo clippy --all-features".runCommand(baseDir, env)
    }
    return testOutput
}

fun Path.writeDotCargoConfigToml(rustFlags: List<String>) {
    val dotCargoDir = this.resolve(".cargo")
    Files.createDirectory(dotCargoDir)

    dotCargoDir.resolve("config.toml")
        .writeText(
            """
            [build]
            rustflags = [${rustFlags.joinToString(",") { "\"$it\"" }}]
            """.trimIndent(),
        )
}

fun TestWriterDelegator.rustSettings() =
    testRustSettings(
        service = ShapeId.from("fake#Fake"),
        moduleName = "test_${baseDir.toFile().nameWithoutExtension}",
        codegenConfig = this.codegenConfig,
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
    expectFailure: Boolean = false,
): String {
    val deps =
        this.dependencies
            .map { RustDependency.fromSymbolDependency(it) }
            .filterIsInstance<CargoDependency>()
            .distinct()
            .mergeDependencyFeatures()
            .mergeIdenticalTestDependencies()
    val module =
        if (this.namespace.contains("::")) {
            this.namespace.split("::")[1]
        } else {
            "lib"
        }
    val tempDir =
        this.toString()
            .intoCrate(deps, module = module, main = main, strict = clippy)
    val mainRs = tempDir.resolve("src/main.rs")
    val testModule = tempDir.resolve("src/$module.rs")
    try {
        val testOutput =
            if ((mainRs.readText() + testModule.readText()).contains("#[test]")) {
                "cargo test".runCommand(tempDir.toPath())
            } else {
                "cargo check".runCommand(tempDir.toPath())
            }
        if (expectFailure) {
            println("Test sources for debugging: file://${testModule.absolutePath}")
        }
        return testOutput
    } catch (e: CommandError) {
        if (!expectFailure) {
            println("Test sources for debugging: file://${testModule.absolutePath}")
        }
        throw e
    }
}

private fun String.intoCrate(
    deps: List<CargoDependency>,
    module: String? = null,
    main: String = "",
    strict: Boolean = false,
): File {
    this.shouldParseAsRust()
    val tempDir = TestWorkspace.subproject()
    val cargoToml =
        RustWriter.toml("Cargo.toml").apply {
            CargoTomlGenerator(
                moduleName = tempDir.nameWithoutExtension,
                moduleVersion = "0.0.1",
                moduleAuthors = listOf("Testy McTesterson"),
                moduleDescription = null,
                moduleLicense = null,
                moduleRepository = null,
                minimumSupportedRustVersion = null,
                writer = this,
                dependencies = deps,
            ).render()
        }.toString()
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
            """.trimIndent(),
        )
    }

    if (strict) {
        mainRs.appendText(
            """
            #![deny(clippy::all)]
            """.trimIndent(),
        )
    }

    mainRs.appendText(
        """
        pub mod $module;
        pub use crate::$module::*;
        pub fn main() {}
        """.trimIndent(),
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

fun RustCrate.integrationTest(
    name: String,
    writable: Writable,
) = this.withFile("tests/$name.rs", writable)

fun TestWriterDelegator.unitTest(
    additionalAttributes: List<Attribute> = emptyList(),
    test: Writable,
): TestWriterDelegator {
    lib {
        val name = safeName("test")
        withInlineModule(RustModule.inlineTests(name), TestModuleDocProvider) {
            unitTest(name, additionalAttributes = additionalAttributes) {
                test(this)
            }
        }
    }
    return this
}

fun String.runWithWarnings(crate: Path) = this.runCommand(crate, mapOf("RUSTFLAGS" to "-D warnings"))
