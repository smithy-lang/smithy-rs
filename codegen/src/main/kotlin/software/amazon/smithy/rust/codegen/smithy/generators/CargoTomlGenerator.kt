/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.rust.codegen.lang.CargoDependency
import software.amazon.smithy.rust.codegen.lang.Compile
import software.amazon.smithy.rust.codegen.lang.Dev
import software.amazon.smithy.rust.codegen.smithy.RustSettings
import software.amazon.smithy.utils.CodeWriter

class CargoTomlGenerator(private val settings: RustSettings, private val writer: CodeWriter, private val dependencies: List<CargoDependency>) {
    fun render() {
        writer.write("[package]")
        writer.write("""name = "${settings.moduleName}"""")
        writer.write("""version = "${settings.moduleVersion}"""")
        writer.write("""authors = ["TODO@todo.com"]""")
        // TODO: make edition configurable
        writer.write("""edition = "2018"""")

        writer.insertTrailingNewline()

        val compileDependencies = dependencies.filter { it.scope == Compile }
        val devDependencies = dependencies.filter { it.scope == Dev }
        if (compileDependencies.isNotEmpty()) {
            writer.write("[dependencies]")
            compileDependencies.forEach {
                writer.write(it.toString())
            }
        }

        if (devDependencies.isNotEmpty()) {
            writer.write("[dev-dependencies]")
            devDependencies.forEach {
                writer.write(it.toString())
            }
        }
    }
}
