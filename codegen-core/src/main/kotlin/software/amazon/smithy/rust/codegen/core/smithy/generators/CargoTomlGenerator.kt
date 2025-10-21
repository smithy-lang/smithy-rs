/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import com.moandjiezana.toml.TomlWriter
import software.amazon.smithy.rust.codegen.core.Version
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.DependencyScope
import software.amazon.smithy.rust.codegen.core.rustlang.Feature
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.smithy.CoreRustSettings
import software.amazon.smithy.rust.codegen.core.util.deepMergeWith

/**
 * Customizations to apply to the generated Cargo.toml file.
 *
 * This is a nested map of key/value that represents the properties in a crate manifest.
 * For example, the following
 *
 * ```kotlin
 * mapOf(
 *     "package" to mapOf(
 *         "name" to "foo",
 *         "version" to "1.0.0",
 *     )
 * )
 * ```
 *
 * is equivalent to
 *
 * ```toml
 * [package]
 * name = "foo"
 * version = "1.0.0"
 * ```
 */
typealias ManifestCustomizations = Map<String, Any?>

/**
 * Generates the crate manifest Cargo.toml file.
 */
class CargoTomlGenerator(
    private val moduleName: String,
    private val moduleVersion: String,
    private val moduleAuthors: List<String>,
    private val moduleDescription: String?,
    private val moduleLicense: String?,
    private val moduleRepository: String?,
    private val minimumSupportedRustVersion: String?,
    private val protocolId: String?,
    private val writer: RustWriter,
    private val manifestCustomizations: ManifestCustomizations = emptyMap(),
    private val dependencies: List<CargoDependency> = emptyList(),
    private val features: List<Feature> = emptyList(),
) {
    constructor(
        settings: CoreRustSettings,
        protocolId: String?,
        writer: RustWriter,
        manifestCustomizations: ManifestCustomizations,
        dependencies: List<CargoDependency>,
        features: List<Feature>,
    ) : this(
        settings.moduleName,
        settings.moduleVersion,
        settings.moduleAuthors,
        settings.moduleDescription,
        settings.license,
        settings.moduleRepository,
        settings.minimumSupportedRustVersion,
        protocolId,
        writer,
        manifestCustomizations,
        dependencies,
        features,
    )

    fun render() {
        val cargoFeatures = features.map { it.name to it.deps }.toMutableList()
        if (features.isNotEmpty()) {
            cargoFeatures.add("default" to features.filter { it.default }.map { it.name })
        }

        val cargoToml =
            mapOf(
                "package" to
                    listOfNotNull(
                        "name" to moduleName,
                        "version" to moduleVersion,
                        "authors" to moduleAuthors,
                        moduleDescription?.let { "description" to it },
                        "edition" to "2021",
                        "license" to moduleLicense,
                        "repository" to moduleRepository,
                        minimumSupportedRustVersion?.let { "rust-version" to it },
                        "metadata" to
                            listOfNotNull(
                                "smithy" to
                                    listOfNotNull(
                                        "codegen-version" to Version.fromDefaultResource().gitHash,
                                        "protocol" to protocolId,
                                    ).toMap(),
                            ).toMap(),
                    ).toMap(),
                "dependencies" to
                    dependencies.filter { it.scope == DependencyScope.Compile }
                        .associate { it.name to it.toMap() },
                "build-dependencies" to
                    dependencies.filter { it.scope == DependencyScope.Build }
                        .associate { it.name to it.toMap() },
                "dev-dependencies" to
                    dependencies.filter { it.scope == DependencyScope.Dev }
                        .associate { it.name to it.toMap() },
                "features" to cargoFeatures.toMap(),
            ).deepMergeWith(manifestCustomizations)

        writer.writeWithNoFormatting(TomlWriter().write(cargoToml))
    }
}
