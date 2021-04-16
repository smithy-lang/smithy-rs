/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy

import software.amazon.smithy.build.FileManifest
import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.codegen.core.writer.CodegenWriterDelegator
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.Feature
import software.amazon.smithy.rust.codegen.rustlang.InlineDependency
import software.amazon.smithy.rust.codegen.rustlang.RustDependency
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.smithy.generators.CargoTomlGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsGenerator

open class RustCrate(
    fileManifest: FileManifest,
    symbolProvider: SymbolProvider,
    baseModules: Map<String, RustModule>
) {
    private val inner = CodegenWriterDelegator(fileManifest, symbolProvider, RustWriter.Factory)
    private val modules: MutableMap<String, RustModule> = baseModules.toMutableMap()
    private val features: MutableSet<Feature> = mutableSetOf()
    // Lib.rs is a little special—we need to reorder things at the end
    private val libRs = mutableListOf<Writable>()
    fun useShapeWriter(shape: Shape, f: (RustWriter) -> Unit) {
        inner.useShapeWriter(shape, f)
    }

    fun lib(moduleWriter: (RustWriter) -> Unit) {
        libRs.add(moduleWriter)
    }

    fun addFeature(feature: Feature) = this.features.add(feature)

    fun finalize(settings: RustSettings, libRsCustomizations: List<LibRsCustomization>) {
        injectInlineDependencies()
        val modules = inner.writers.values.mapNotNull { it.module() }.filter { it != "lib" }
            .map { modules[it] ?: RustModule.default(it, false) }
        inner.finalize(settings, libRsCustomizations + libRs, modules, this.features.toList())
    }

    private fun injectInlineDependencies() {
        val unloadedDepdencies = {
            this
                .inner.dependencies
                .map { dep -> RustDependency.fromSymbolDependency(dep) }
                .filterIsInstance<InlineDependency>().distinctBy { it.key() }
                .filter { !modules.contains(it.module) }
        }
        while (unloadedDepdencies().isNotEmpty()) {
            unloadedDepdencies().forEach { dep ->
                this.withModule(RustModule.default(dep.module, false)) {
                    dep.renderer(it)
                }
            }
        }
    }

    fun withModule(
        module: RustModule,
        moduleWriter: (RustWriter) -> Unit
    ): RustCrate {
        val moduleName = module.name
        modules[moduleName] = module
        inner.useFileWriter("src/$moduleName.rs", "crate::$moduleName", moduleWriter)
        return this
    }

    fun withFile(filename: String, fileWriter: (RustWriter) -> Unit) {
        inner.useFileWriter(filename) {
            fileWriter(it)
        }
    }
}

// TODO: this should _probably_ be configurable via RustSettings; 2h
/**
 * Allowlist of modules that will be exposed publicly in generated crates
 */
val DefaultPublicModules =
    setOf("error", "operation", "model", "input", "output", "config").map { it to RustModule.default(it, true) }.toMap()

/**
 * Finalize all the writers by:
 * - inlining inline dependencies that have been used
 * - generating (and writing) a Cargo.toml based on the settings & the required dependencies
 */
fun CodegenWriterDelegator<RustWriter>.finalize(
    settings: RustSettings,
    libRsCustomizations: List<LibRsCustomization>,
    modules: List<RustModule>,
    features: List<Feature>
) {
    this.useFileWriter("src/lib.rs", "crate::lib") { writer ->
        LibRsGenerator(settings.moduleDescription, modules, libRsCustomizations).render(writer)
    }
    val cargoDependencies =
        this.dependencies.map { RustDependency.fromSymbolDependency(it) }.filterIsInstance<CargoDependency>().distinct()
    this.useFileWriter("Cargo.toml") {
        val cargoToml = CargoTomlGenerator(
            settings,
            it,
            cargoDependencies,
            features
        )
        cargoToml.render()
    }
    flushWriters()
}
