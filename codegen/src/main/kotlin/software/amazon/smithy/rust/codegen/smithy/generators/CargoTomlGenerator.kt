/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import com.moandjiezana.toml.TomlWriter
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.DependencyScope
import software.amazon.smithy.rust.codegen.rustlang.Feature
import software.amazon.smithy.rust.codegen.smithy.RustSettings
import software.amazon.smithy.utils.CodeWriter

class CargoTomlGenerator(
    private val settings: RustSettings,
    private val writer: CodeWriter,
    private val dependencies: List<CargoDependency>,
    private val features: List<Feature>
) {
    fun render() {
        val cargoFeatures = features.map { it.name to it.deps }.toMutableList()
        if (features.isNotEmpty()) {
            cargoFeatures.add("default" to features.filter { it.default }.map { it.name })
        }
        val cargoToml = mapOf(
            "package" to mapOf(
                "name" to settings.moduleName,
                "version" to settings.moduleVersion,
                "description" to settings.moduleDescription,
                "authors" to settings.moduleAuthors,
                "license" to settings.license,
                "edition" to "2018",
            ),
            "dependencies" to dependencies.filter { it.scope == DependencyScope.Compile }.map { it.name to it.toMap() }
                .toMap(),
            "dev-dependencies" to dependencies.filter { it.scope == DependencyScope.Dev }.map { it.name to it.toMap() }
                .toMap(),
            "features" to cargoFeatures.toMap()
        )
        writer.writeWithNoFormatting(TomlWriter().write(cargoToml))
    }
}
