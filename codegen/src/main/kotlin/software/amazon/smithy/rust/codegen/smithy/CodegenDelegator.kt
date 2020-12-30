/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy

import software.amazon.smithy.codegen.core.writer.CodegenWriterDelegator
import software.amazon.smithy.rust.codegen.lang.CargoDependency
import software.amazon.smithy.rust.codegen.lang.InlineDependency
import software.amazon.smithy.rust.codegen.lang.RustDependency
import software.amazon.smithy.rust.codegen.lang.RustModule
import software.amazon.smithy.rust.codegen.lang.RustWriter
import software.amazon.smithy.rust.codegen.smithy.generators.CargoTomlGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsGenerator

private fun CodegenWriterDelegator<RustWriter>.includedModules(): List<String> = this.writers.values.mapNotNull { it.module() }

// TODO: this should _probably_ be configurable via RustSettings; 2h
/**
 * Allowlist of modules that will be exposed publicly in generated crates
 */
private val PublicModules = setOf("error", "operation", "model", "input", "output")

/**
 * Finalize all the writers by:
 * - inlining inline dependencies that have been used
 * - generating (and writing) a Cargo.toml based on the settings & the required dependencies
 */
fun CodegenWriterDelegator<RustWriter>.finalize(settings: RustSettings) {
    val loadDependencies = { this.dependencies.map { dep -> RustDependency.fromSymbolDependency(dep) } }
    val inlineDependencies = loadDependencies().filterIsInstance<InlineDependency>().distinctBy { it.key() }
    inlineDependencies.forEach { dep ->
        this.useFileWriter("src/${dep.module}.rs", "crate::${dep.module}") {
            dep.renderer(it)
        }
    }
    val newDeps = loadDependencies().filterIsInstance<InlineDependency>().distinctBy { it.key() }
    newDeps.forEach { dep ->
        if (!this.writers.containsKey("src/${dep.module}.rs")) {
            this.useFileWriter("src/${dep.module}.rs", "crate::${dep.module}") {
                dep.renderer(it)
            }
        }
    }
    val cargoDependencies = loadDependencies().filterIsInstance<CargoDependency>().distinct()
    this.useFileWriter("Cargo.toml") {
        val cargoToml = CargoTomlGenerator(
            settings,
            it,
            cargoDependencies,
        )
        cargoToml.render()
    }
    this.useFileWriter("src/lib.rs", "crate::lib") { writer ->
        val includedModules = this.includedModules().toSet().filter { it != "lib" }
        val modules = includedModules.map { moduleName ->
            RustModule.default(moduleName, PublicModules.contains(moduleName))
        }
        LibRsGenerator(settings.moduleDescription, modules).render(writer)
    }
    flushWriters()
}

fun CodegenWriterDelegator<RustWriter>.withModule(
    moduleName: String,
    moduleWriter: RustWriter.() -> Unit
): CodegenWriterDelegator<RustWriter> {
    this.useFileWriter("src/$moduleName.rs", "crate::$moduleName", moduleWriter)
    return this
}
