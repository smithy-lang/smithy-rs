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

sealed class RustDependency(open val name: String) : SymbolDependencyContainer {
    abstract fun version(): String
    override fun getDependencies(): List<SymbolDependency> {
        return listOf(
            SymbolDependency
                .builder()
                .packageName(name).version(version())
                // We rely on retrieving the structured dependency from the symbol later
                .putProperty(PropertyKey, this).build()
        )
    }

    companion object {
        private const val PropertyKey = "rustdep"
        fun fromSymbolDependency(symbolDependency: SymbolDependency) =
            symbolDependency.getProperty(PropertyKey, RustDependency::class.java).get()
    }
}

/**
 * A dependency on a snippet of code
 *
 * InlineDependency should not be instantiated directly, rather, it should be constructed with
 * [software.amazon.smithy.rust.codegen.smithy.RuntimeType.forInlineFun]
 *
 * InlineDependencies are created as private modules within the main crate. This is useful for any code that
 * doesn't need to exist in a shared crate, but must still be generated exactly once during codegen.
 *
 * CodegenVisitor deduplicates inline dependencies by (module, name) during code generation.
 */
class InlineDependency(name: String, val module: String, val renderer: (RustWriter) -> Unit) : RustDependency(name) {
    override fun version(): String {
        return renderer(RustWriter.forModule("_")).hashCode().toString()
    }

    fun key() = "$module::$name"

    companion object {
        fun forRustFile(name: String, module: String, filename: String): InlineDependency {
            val inlineCrate = this::class.java.getResource("/inlineable/src/$filename")
            check(inlineCrate != null)
            return InlineDependency(name, module) { writer ->
                writer.write(inlineCrate.readText())
            }
        }
    }
}

/**
 * A dependency on an internal or external Cargo Crate
 */
data class CargoDependency(
    override val name: String,
    private val location: DependencyLocation,
    val scope: DependencyScope = Compile,
    private val features: List<String> = listOf()
) : RustDependency(name) {

    override fun version(): String = when (location) {
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
        val Random: RustDependency = CargoDependency("rand", CratesIo("0.7.3"))
        val Http: RustDependency = CargoDependency("http", CratesIo("0.2"))
        fun SmithyTypes(runtimeConfig: RuntimeConfig) =
            CargoDependency("${runtimeConfig.cratePrefix}-types", Local(runtimeConfig.relativePath))

        fun SmithyHttp(runtimeConfig: RuntimeConfig) = CargoDependency(
            "${runtimeConfig.cratePrefix}-http", Local(runtimeConfig.relativePath)
        )

        fun ProtocolTestHelpers(runtimeConfig: RuntimeConfig) = CargoDependency(
            "protocol-test-helpers", Local(runtimeConfig.relativePath), scope = Dev
        )

        // TODO: replace this with `InlineDependency`
        fun Inlineable(runtimeConfig: RuntimeConfig) = CargoDependency(
            "inlineable", Local(runtimeConfig.relativePath)
        )

        private val PropertyKey = "rustdep"

        fun fromSymbolDependency(symbolDependency: SymbolDependency) =
            symbolDependency.getProperty(PropertyKey, RustDependency::class.java).get()

        val SerdeJson: CargoDependency = CargoDependency("serde_json", CratesIo("1"))
        val Serde = CargoDependency("serde", CratesIo("1"), features = listOf("derive"))
    }
}
