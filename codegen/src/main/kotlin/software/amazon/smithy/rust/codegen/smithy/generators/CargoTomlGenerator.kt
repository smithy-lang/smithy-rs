/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import com.moandjiezana.toml.TomlWriter
import software.amazon.smithy.rust.codegen.lang.CargoDependency
import software.amazon.smithy.rust.codegen.lang.Compile
import software.amazon.smithy.rust.codegen.lang.Dev
import software.amazon.smithy.rust.codegen.smithy.RustSettings
import software.amazon.smithy.utils.CodeWriter

class CargoTomlGenerator(private val settings: RustSettings, private val writer: CodeWriter, private val dependencies: List<CargoDependency>) {
    fun render() {
        val cargoToml = mapOf(
            "package" to mapOf(
                "name" to settings.moduleName,
                "version" to settings.moduleVersion,
                "description" to settings.moduleDescription,
                "authors" to settings.moduleAuthors,
                "edition" to "2018"
            ),
            "dependencies" to dependencies.filter { it.scope == Compile }.map { it.name to it.toMap() }.toMap(),
            "dev-dependencies" to dependencies.filter { it.scope == Dev }.map { it.name to it.toMap() }.toMap()
        )
        writer.writeWithNoFormatting(TomlWriter().write(cargoToml))
    }
}
