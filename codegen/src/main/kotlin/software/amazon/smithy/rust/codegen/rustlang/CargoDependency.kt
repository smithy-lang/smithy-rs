/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.rustlang

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
    open fun dependencies(): List<RustDependency> = listOf()
    override fun getDependencies(): List<SymbolDependency> {
        return listOf(
            SymbolDependency
                .builder()
                .packageName(name).version(version())
                // We rely on retrieving the structured dependency from the symbol later
                .putProperty(PropertyKey, this).build()
        ) + dependencies().flatMap { it.dependencies }
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
class InlineDependency(
    name: String,
    val module: String,
    val extraDependencies: List<RustDependency> = listOf(),
    val renderer: (RustWriter) -> Unit
) : RustDependency(name) {
    override fun version(): String {
        return renderer(RustWriter.forModule("_")).hashCode().toString()
    }

    override fun dependencies(): List<RustDependency> {
        return extraDependencies
    }

    fun key() = "$module::$name"

    companion object {
        fun forRustFile(
            name: String,
            vararg additionalDependencies: RustDependency
        ): InlineDependency {
            val module = name
            val filename = "$name.rs"
            // The inline crate is loaded as a dependency on the runtime classpath
            val rustFile = this::class.java.getResource("/inlineable/src/$filename")
            check(rustFile != null) { "Rust file $filename was missing from the resource bundle!" }
            return InlineDependency(name, module, additionalDependencies.toList()) { writer ->
                writer.raw(rustFile.readText())
            }
        }

        fun awsJsonErrors(runtimeConfig: RuntimeConfig) =
            forRustFile("aws_json_errors", CargoDependency.Http, CargoDependency.SmithyTypes(runtimeConfig))

        fun docJson() = forRustFile("doc_json", CargoDependency.Serde)
        fun instantEpoch() = forRustFile("instant_epoch", CargoDependency.Serde)
        fun instantHttpDate() =
            forRustFile("instant_httpdate", CargoDependency.Serde)

        fun instant8601() = forRustFile("instant_iso8601", CargoDependency.Serde)

        fun idempotencyToken() =
            forRustFile("idempotency_token", CargoDependency.Rand)

        fun blobSerde(runtimeConfig: RuntimeConfig) = forRustFile(
            "blob_serde",
            CargoDependency.Serde,
            CargoDependency.SmithyHttp(runtimeConfig)
        )
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

    fun toMap(): Map<String, Any> {
        val attribs = mutableMapOf<String, Any>()
        with(location) {
            when (this) {
                is CratesIo -> attribs["version"] = version
                is Local -> {
                    val fullPath = "$basePath/$name"
                    attribs["path"] = fullPath
                }
            }
        }
        with(features) {
            if (!isEmpty()) {
                attribs["features"] = this
            }
        }
        return attribs
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
        val Bytes: RustDependency = CargoDependency("bytes", CratesIo("1"))
        val Rand: CargoDependency = CargoDependency("rand", CratesIo("0.7"))
        val Http: CargoDependency = CargoDependency("http", CratesIo("0.2"))
        // WIP being refactored
        fun OperationWip(runtimeConfig: RuntimeConfig): CargoDependency = CargoDependency("operationwip", Local(runtimeConfig.relativePath))

        fun SmithyTypes(runtimeConfig: RuntimeConfig) =
            CargoDependency("${runtimeConfig.cratePrefix}-types", Local(runtimeConfig.relativePath))

        fun SmithyHttp(runtimeConfig: RuntimeConfig) = CargoDependency(
            "${runtimeConfig.cratePrefix}-http", Local(runtimeConfig.relativePath)
        )

        fun ProtocolTestHelpers(runtimeConfig: RuntimeConfig) = CargoDependency(
            "protocol-test-helpers", Local(runtimeConfig.relativePath), scope = Dev
        )

        val SerdeJson: CargoDependency =
            CargoDependency("serde_json", CratesIo("1"), features = listOf("float_roundtrip"))
        val Serde = CargoDependency("serde", CratesIo("1"), features = listOf("derive"))
    }
}
