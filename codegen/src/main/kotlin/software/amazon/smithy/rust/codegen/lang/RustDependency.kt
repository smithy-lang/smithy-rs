/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.lang

import software.amazon.smithy.codegen.core.SymbolDependency
import software.amazon.smithy.codegen.core.SymbolDependencyContainer
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.util.dq

sealed class DependencyScope
object Dev : DependencyScope()
object Compile : DependencyScope()

sealed class DependencyLocation
data class CratesIo(val version: String) : DependencyLocation()
data class Local(val basePath: String) : DependencyLocation()

data class RustDependency(
    val name: String,
    val location: DependencyLocation,
    val scope: DependencyScope = Compile,
    val features: List<String> = listOf()
) : SymbolDependencyContainer {
    override fun getDependencies(): List<SymbolDependency> {
        return listOf(
            SymbolDependency
                .builder()
                .packageName(name).version(version())
                // We rely on retrieving the structured dependency from the symbol later
                .putProperty(PropertyKey, this).build()
        )
    }

    private fun version(): String = when (location) {
        is CratesIo -> location.version
        is Local -> "local"
    }

    override fun toString(): String {
        val attribs = mutableListOf<String>()
        with(location) {
            attribs.add(
                when (this) {
                    is CratesIo -> """version = ${version.dq()}"""
                    is Local -> {
                        val fullPath = "$basePath/$name"
                        """path = ${fullPath.dq()}"""
                    }
                }
            )
        }
        with(features) {
            if (!isEmpty()) {
                attribs.add("features = [${joinToString(",") { it.dq() }}]")
            }
        }
        return "$name = { ${attribs.joinToString(",")} }"
    }

    companion object {
        val Http: RustDependency = RustDependency("http", CratesIo("0.2"))
        fun SmithyTypes(runtimeConfig: RuntimeConfig) =
            RustDependency("${runtimeConfig.cratePrefix}-types", Local(runtimeConfig.relativePath))

        fun SmithyHttp(runtimeConfig: RuntimeConfig) = RustDependency(
            "${runtimeConfig.cratePrefix}-http", Local(runtimeConfig.relativePath)
        )

        fun ProtocolTestHelpers(runtimeConfig: RuntimeConfig) = RustDependency(
            "protocol-test-helpers", Local(runtimeConfig.relativePath), scope = Dev
        )

        private val PropertyKey = "rustdep"

        fun fromSymbolDependency(symbolDependency: SymbolDependency) =
            symbolDependency.getProperty(PropertyKey, RustDependency::class.java).get()
    }
}
