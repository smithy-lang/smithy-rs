/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy

import software.amazon.smithy.build.FileManifest
import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.codegen.core.WriterDelegator
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.Feature
import software.amazon.smithy.rust.codegen.core.rustlang.InlineDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.smithy.generators.CargoTomlGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.ManifestCustomizations

/**
 * RustCrate abstraction.
 *
 * **Note**: This is the only implementation, `open` only for test purposes.
 *
 * All code-generation at some point goes through this class. `RustCrate` maintains a `CodegenWriterDelegator` internally
 * which tracks a set of file-writer pairs and allows them to be loaded and cached (see: [useShapeWriter])
 *
 * On top of this, it adds Rust specific features:
 * - Generation of a `lib.rs` which adds `mod` statements automatically for every module that was used
 * - Tracking dependencies and crate features used during code generation, enabling generation of `Cargo.toml`
 *
 * Users will generally want to use two main entry points:
 * 1. [useShapeWriter]: Find or create a writer that will contain a given shape. See [locatedIn] for context about how
 *    shape locations are determined.
 * 2. [finalize]: Write the crate out to the file system, generating a lib.rs and Cargo.toml
 */
open class RustCrate(
    fileManifest: FileManifest,
    symbolProvider: SymbolProvider,
    /**
     * For core modules like `input`, `output`, and `error`, we need to specify whether these modules should be public or
     * private as well as any other metadata. [baseModules] enables configuring this. See [DefaultPublicModules].
     */
    baseModules: Map<String, RustModule>,
    coreCodegenConfig: CoreCodegenConfig,
) {
    private val inner = WriterDelegator(fileManifest, symbolProvider, RustWriter.factory(coreCodegenConfig.debugMode))
    private val modules: MutableMap<String, RustModule> = baseModules.toMutableMap()
    private val features: MutableSet<Feature> = mutableSetOf()

    /**
     * Write into the module that this shape is [locatedIn]
     */
    fun useShapeWriter(shape: Shape, f: Writable) {
        inner.useShapeWriter(shape, f)
    }

    /**
     * Write directly into lib.rs
     */
    fun lib(moduleWriter: Writable) {
        inner.useFileWriter("src/lib.rs", "crate", moduleWriter)
    }

    fun mergeFeature(feature: Feature) {
        when (val existing = features.firstOrNull { it.name == feature.name }) {
            null -> features.add(feature)
            else -> {
                features.remove(existing)
                features.add(
                    existing.copy(
                        deps = (existing.deps + feature.deps).toSortedSet().toList(),
                    ),
                )
            }
        }
    }

    /**
     * Finalize Cargo.toml and lib.rs and flush the writers to the file system.
     *
     * This is also where inline dependencies are actually reified and written, potentially recursively.
     */
    fun finalize(
        settings: CoreRustSettings,
        model: Model,
        manifestCustomizations: ManifestCustomizations,
        libRsCustomizations: List<LibRsCustomization>,
        requireDocs: Boolean = true,
    ) {
        injectInlineDependencies()
        val modules = inner.writers.values.mapNotNull { it.module() }.filter { it != "lib" }
            .mapNotNull { modules[it] }
        inner.finalize(
            settings,
            model,
            manifestCustomizations,
            libRsCustomizations,
            modules,
            this.features.toList(),
            requireDocs,
        )
    }

    private fun injectInlineDependencies() {
        val writtenDependencies = mutableSetOf<String>()
        val unloadedDependencies = {
            this
                .inner.dependencies
                .map { dep -> RustDependency.fromSymbolDependency(dep) }
                .filterIsInstance<InlineDependency>().distinctBy { it.key() }
                .filter { !writtenDependencies.contains(it.key()) }
        }
        while (unloadedDependencies().isNotEmpty()) {
            unloadedDependencies().forEach { dep ->
                writtenDependencies.add(dep.key())
                this.withModule(dep.module) {
                    dep.renderer(this)
                }
            }
        }
    }

    /**
     * Create a new module directly. The resulting module will be placed in `src/<modulename>.rs`
     */
    fun withModule(
        module: RustModule,
        moduleWriter: Writable,
    ): RustCrate {
        val moduleName = module.name
        modules[moduleName] = module
        inner.useFileWriter("src/$moduleName.rs", "crate::$moduleName", moduleWriter)
        return this
    }

    /**
     * Create a new non-root module directly. For example, if given the namespace `crate::foo::bar`,
     * this will create `src/foo/bar.rs` with the contents from the given `moduleWriter`.
     * Multiple calls to this with the same namespace are additive, so new code can be added
     * by various customizations.
     *
     * Caution: this does not automatically add the required Rust `mod` statements to make this
     * file an official part of the generated crate. This step needs to be done manually.
     */
    fun withNonRootModule(
        namespace: String,
        moduleWriter: Writable,
    ): RustCrate {
        val parts = namespace.split("::")
        require(parts.size > 2) { "Cannot create root modules using withNonRootModule" }
        require(parts[0] == "crate") { "Namespace must start with crate::" }

        val fileName = "src/" + parts.filterIndexed { index, _ -> index > 0 }.joinToString("/") + ".rs"
        inner.useFileWriter(fileName, namespace, moduleWriter)
        return this
    }

    /**
     * Create a new file directly
     */
    fun withFile(filename: String, fileWriter: Writable) {
        inner.useFileWriter(filename) {
            fileWriter(it)
        }
    }
}

val ErrorsModule = RustModule.public("error", documentation = "All error types that operations can return.")
val OperationsModule = RustModule.public("operation", documentation = "All operations that this crate can perform.")
val ModelsModule = RustModule.public("model", documentation = "Data structures used by operation inputs/outputs.")
val InputsModule = RustModule.public("input", documentation = "Input structures for operations.")
val OutputsModule = RustModule.public("output", documentation = "Output structures for operations.")
val ConfigModule = RustModule.public("config", documentation = "Client configuration.")

/**
 * Allowlist of modules that will be exposed publicly in generated crates
 */
val DefaultPublicModules = setOf(
    ErrorsModule,
    OperationsModule,
    ModelsModule,
    InputsModule,
    OutputsModule,
    ConfigModule,
).associateBy { it.name }

/**
 * Finalize all the writers by:
 * - inlining inline dependencies that have been used
 * - generating (and writing) a Cargo.toml based on the settings & the required dependencies
 */
fun WriterDelegator<RustWriter>.finalize(
    settings: CoreRustSettings,
    model: Model,
    manifestCustomizations: ManifestCustomizations,
    libRsCustomizations: List<LibRsCustomization>,
    modules: List<RustModule>,
    features: List<Feature>,
    requireDocs: Boolean,
) {
    this.useFileWriter("src/lib.rs", "crate::lib") {
        LibRsGenerator(settings, model, modules, libRsCustomizations, requireDocs).render(it)
    }
    val cargoDependencies = mergeDependencyFeatures(
        this.dependencies.map { RustDependency.fromSymbolDependency(it) }
            .filterIsInstance<CargoDependency>().distinct(),
    )
    this.useFileWriter("Cargo.toml") {
        val cargoToml = CargoTomlGenerator(
            settings,
            it,
            manifestCustomizations,
            cargoDependencies,
            features,
        )
        cargoToml.render()
    }
    flushWriters()
}

private fun CargoDependency.mergeWith(other: CargoDependency): CargoDependency {
    check(key == other.key)
    return copy(
        features = features + other.features,
        optional = optional && other.optional,
    )
}

fun mergeDependencyFeatures(cargoDependencies: List<CargoDependency>): List<CargoDependency> =
    cargoDependencies.groupBy { it.key }
        .mapValues { group -> group.value.reduce { acc, next -> acc.mergeWith(next) } }
        .values
        .toList()
        .sortedBy { it.name }
