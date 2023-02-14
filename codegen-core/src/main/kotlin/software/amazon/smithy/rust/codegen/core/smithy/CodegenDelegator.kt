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
import software.amazon.smithy.rust.codegen.core.rustlang.DependencyScope
import software.amazon.smithy.rust.codegen.core.rustlang.Feature
import software.amazon.smithy.rust.codegen.core.rustlang.InlineDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
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
    private val symbolProvider: SymbolProvider,
    coreCodegenConfig: CoreCodegenConfig,
) {
    private val inner = WriterDelegator(fileManifest, symbolProvider, RustWriter.factory(coreCodegenConfig.debugMode))
    private val features: MutableSet<Feature> = mutableSetOf()

    // used to ensure we never create accidentally discard docs / incorrectly create modules with incorrect visibility
    private var duplicateModuleWarningSystem: MutableMap<String, RustModule.LeafModule> = mutableMapOf()

    /**
     * Write into the module that this shape is [locatedIn]
     */
    fun useShapeWriter(shape: Shape, f: Writable) {
        symbolProvider.toSymbol(shape).module().also { module ->
            check(!module.isInline()) {
                "Cannot use useShapeWriter with inline modules—use [RustWriter.withInlineModule] instead"
            }
            withModule(module, f)
        }
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
        inner.finalize(
            settings,
            model,
            manifestCustomizations,
            libRsCustomizations,
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

    private fun checkDups(module: RustModule.LeafModule) {
        duplicateModuleWarningSystem[module.fullyQualifiedPath()]?.also { preexistingModule ->
            check(module == preexistingModule) {
                "Duplicate modules with differing properties were created! This will lead to non-deterministic behavior." +
                    "\n Previous module: $preexistingModule." +
                    "\n      New module: $module"
            }
        }
        duplicateModuleWarningSystem[module.fullyQualifiedPath()] = module
    }

    /**
     * Create a new module directly. The resulting module will be placed in `src/<modulename>.rs`
     */
    fun withModule(
        module: RustModule,
        moduleWriter: Writable,
    ): RustCrate {
        when (module) {
            is RustModule.LibRs -> lib { moduleWriter(this) }
            is RustModule.LeafModule -> {
                checkDups(module)
                // Create a dependency which adds the mod statement for this module. This will be added to the writer
                // so that _usage_ of this module will generate _exactly one_ `mod <name>` with the correct modifiers.
                val modStatement = RuntimeType.forInlineFun("mod_" + module.fullyQualifiedPath(), module.parent) {
                    module.renderModStatement(this)
                }
                val path = module.fullyQualifiedPath().split("::").drop(1).joinToString("/")
                inner.useFileWriter("src/$path.rs", module.fullyQualifiedPath()) { writer ->
                    moduleWriter(writer)
                    writer.addDependency(modStatement.dependency)
                }
            }
        }
        return this
    }

    /**
     * Returns the module for a given Shape.
     */
    fun moduleFor(shape: Shape, moduleWriter: Writable): RustCrate = symbolProvider.toSymbol(shape).let { symbol ->
        check(symbol.rustType() is RustType.Opaque) {
            "`moduleFor` should only be called for opaque Rust types. It doesn't make sense " +
                "to use it for other RustType variants that are mostly std library types."
        }
        withModule(
            (symbolProvider as RustSymbolProvider).config.moduleProvider.moduleForShape(symbol.name, shape),
            moduleWriter,
        )
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

// TODO(https://github.com/awslabs/smithy-rs/issues/2341): Remove unconstrained/constrained from codegen-core
val UnconstrainedModule =
    RustModule.private("unconstrained", "Unconstrained types for constrained shapes.")
val ConstrainedModule =
    RustModule.private("constrained", "Constrained types for constrained shapes.")

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
    features: List<Feature>,
    requireDocs: Boolean,
) {
    this.useFileWriter("src/lib.rs", "crate::lib") {
        LibRsGenerator(settings, model, libRsCustomizations, requireDocs).render(it)
    }
    val cargoDependencies =

        this.dependencies.map { RustDependency.fromSymbolDependency(it) }
            .filterIsInstance<CargoDependency>().distinct()
            .mergeDependencyFeatures()
            .mergeIdenticalTestDependencies()
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

internal fun List<CargoDependency>.mergeDependencyFeatures(): List<CargoDependency> =
    this.groupBy { it.key }
        .mapValues { group -> group.value.reduce { acc, next -> acc.mergeWith(next) } }
        .values
        .toList()
        .sortedBy { it.name }

/**
 * If the same dependency exists both in prod and test scope, remove it from the test scope.
 */
internal fun List<CargoDependency>.mergeIdenticalTestDependencies(): List<CargoDependency> {
    val compileDeps =
        this.filter { it.scope == DependencyScope.Compile }.toSet()

    return this.filterNot { it.scope == DependencyScope.Dev && compileDeps.contains(it.copy(scope = DependencyScope.Compile)) }
}
