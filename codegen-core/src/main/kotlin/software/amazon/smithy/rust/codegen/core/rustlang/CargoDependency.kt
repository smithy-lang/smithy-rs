/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.rustlang

import software.amazon.smithy.codegen.core.SymbolDependency
import software.amazon.smithy.codegen.core.SymbolDependencyContainer
import software.amazon.smithy.rust.codegen.core.smithy.ConstrainedModule
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.dq
import java.nio.file.Path

sealed class DependencyScope {
    object Dev : DependencyScope()
    object Compile : DependencyScope()
}

sealed class DependencyLocation
data class CratesIo(val version: String) : DependencyLocation()
data class Local(val basePath: String, val version: String? = null) : DependencyLocation()

sealed class RustDependency(open val name: String) : SymbolDependencyContainer {
    abstract fun version(): String
    open fun dependencies(): List<RustDependency> = listOf()
    override fun getDependencies(): List<SymbolDependency> {
        return listOf(
            SymbolDependency
                .builder()
                .packageName(name).version(version())
                // We rely on retrieving the structured dependency from the symbol later
                .putProperty(PropertyKey, this).build(),
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
 * [software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.forInlineFun]
 *
 * InlineDependencies are created as private modules within the main crate. This is useful for any code that
 * doesn't need to exist in a shared crate, but must still be generated exactly once during codegen.
 *
 * CodegenVisitor de-duplicates inline dependencies by (module, name) during code generation.
 */
class InlineDependency(
    name: String,
    val module: RustModule,
    private val extraDependencies: List<RustDependency> = listOf(),
    val renderer: Writable,
) : RustDependency(name) {
    override fun version(): String {
        // just need a version that won't crash
        return renderer.hashCode().toString()
    }

    override fun dependencies(): List<RustDependency> {
        return extraDependencies
    }

    fun key() = "${module.fullyQualifiedPath()}::$name"

    companion object {
        fun forRustFile(
            module: RustModule.LeafModule,
            resourcePath: String,
            vararg additionalDependencies: RustDependency,
        ): InlineDependency {
            // The inline crate is loaded as a dependency on the runtime classpath
            val rustFile = this::class.java.getResource(resourcePath)
            check(rustFile != null) { "Rust file $resourcePath was missing from the resource bundle!" }
            return InlineDependency(module.name, module, additionalDependencies.toList()) {
                raw(rustFile.readText())
            }
        }

        private fun forInlineableRustFile(name: String, vararg additionalDependencies: RustDependency) =
            forRustFile(RustModule.private(name), "/inlineable/src/$name.rs", *additionalDependencies)

        fun jsonErrors(runtimeConfig: RuntimeConfig) =
            forInlineableRustFile(
                "json_errors",
                CargoDependency.smithyJson(runtimeConfig),
                CargoDependency.Bytes,
                CargoDependency.Http,
            )

        fun idempotencyToken() =
            forInlineableRustFile("idempotency_token", CargoDependency.FastRand)

        fun ec2QueryErrors(runtimeConfig: RuntimeConfig): InlineDependency =
            forInlineableRustFile("ec2_query_errors", CargoDependency.smithyXml(runtimeConfig))

        fun wrappedXmlErrors(runtimeConfig: RuntimeConfig): InlineDependency =
            forInlineableRustFile("rest_xml_wrapped_errors", CargoDependency.smithyXml(runtimeConfig))

        fun unwrappedXmlErrors(runtimeConfig: RuntimeConfig): InlineDependency =
            forInlineableRustFile("rest_xml_unwrapped_errors", CargoDependency.smithyXml(runtimeConfig))

        fun constrained(): InlineDependency =
            InlineDependency.forRustFile(ConstrainedModule, "/inlineable/src/constrained.rs")
    }
}

fun InlineDependency.toType() = RuntimeType(module.fullyQualifiedPath(), this)

data class Feature(val name: String, val default: Boolean, val deps: List<String>)

/**
 * A dependency on an internal or external Cargo Crate
 */
data class CargoDependency(
    override val name: String,
    private val location: DependencyLocation,
    val scope: DependencyScope = DependencyScope.Compile,
    val optional: Boolean = false,
    val features: Set<String> = emptySet(),
    val rustName: String = name.replace("-", "_"),
) : RustDependency(name) {
    val key: Triple<String, DependencyLocation, DependencyScope> get() = Triple(name, location, scope)

    fun withFeature(feature: String): CargoDependency {
        return copy(features = features.toMutableSet().apply { add(feature) })
    }

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
                    val fullPath = Path.of("$basePath/$name")
                    attribs["path"] = fullPath.normalize().toString()
                    version?.also { attribs["version"] = version }
                }
            }
        }
        with(features) {
            if (!isEmpty()) {
                attribs["features"] = this
            }
        }
        if (optional) {
            attribs["optional"] = true
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
                },
            )
        }
        with(features) {
            if (!isEmpty()) {
                attribs.add("features = [${joinToString(",") { it.dq() }}]")
            }
        }
        return "$name = { ${attribs.joinToString(",")} }"
    }

    fun toType(): RuntimeType {
        return RuntimeType(rustName, this)
    }

    companion object {
        val OnceCell: CargoDependency = CargoDependency("once_cell", CratesIo("1.16"))
        val Url: CargoDependency = CargoDependency("url", CratesIo("2.3.1"))
        val Bytes: CargoDependency = CargoDependency("bytes", CratesIo("1.0.0"))
        val BytesUtils: CargoDependency = CargoDependency("bytes-utils", CratesIo("0.1.0"))
        val FastRand: CargoDependency = CargoDependency("fastrand", CratesIo("1.8.0"))
        val Hex: CargoDependency = CargoDependency("hex", CratesIo("0.4.3"))
        val Http: CargoDependency = CargoDependency("http", CratesIo("0.2.0"))
        val HttpBody: CargoDependency = CargoDependency("http-body", CratesIo("0.4.4"))
        val Hyper: CargoDependency = CargoDependency("hyper", CratesIo("0.14.12"))
        val HyperWithStream: CargoDependency = Hyper.withFeature("stream")
        val LazyStatic: CargoDependency = CargoDependency("lazy_static", CratesIo("1.4.0"))
        val Md5: CargoDependency = CargoDependency("md-5", CratesIo("0.10.0"), rustName = "md5")
        val PercentEncoding: CargoDependency = CargoDependency("percent-encoding", CratesIo("2.0.0"))
        val Regex: CargoDependency = CargoDependency("regex", CratesIo("1.5.5"))
        val Ring: CargoDependency = CargoDependency("ring", CratesIo("0.16.0"))
        val TokioStream: CargoDependency = CargoDependency("tokio-stream", CratesIo("0.1.7"))
        val Tower: CargoDependency = CargoDependency("tower", CratesIo("0.4"))
        val Tracing: CargoDependency = CargoDependency("tracing", CratesIo("0.1"))
        val Serde = CargoDependency("serde", CratesIo("1.0"), features = setOf("derive"))
        fun SmithyTypes(runtimeConfig: RuntimeConfig) = runtimeConfig.runtimeCrate("types")
        fun SmithyClient(runtimeConfig: RuntimeConfig) = runtimeConfig.runtimeCrate("client")
        fun SmithyChecksums(runtimeConfig: RuntimeConfig) = runtimeConfig.runtimeCrate("checksums")
        fun SmithyAsync(runtimeConfig: RuntimeConfig) = runtimeConfig.runtimeCrate("async")
        fun SmithyEventStream(runtimeConfig: RuntimeConfig) = runtimeConfig.runtimeCrate("eventstream")
        fun SmithyHttp(runtimeConfig: RuntimeConfig) = runtimeConfig.runtimeCrate("http")
        fun SmithyHttpTower(runtimeConfig: RuntimeConfig) = runtimeConfig.runtimeCrate("http-tower")
        fun SmithyProtocolTestHelpers(runtimeConfig: RuntimeConfig) =
            runtimeConfig.runtimeCrate("protocol-test", scope = DependencyScope.Dev)
        fun smithyJson(runtimeConfig: RuntimeConfig): CargoDependency = runtimeConfig.runtimeCrate("json")
        fun smithyQuery(runtimeConfig: RuntimeConfig): CargoDependency = runtimeConfig.runtimeCrate("query")
        fun smithyXml(runtimeConfig: RuntimeConfig): CargoDependency = runtimeConfig.runtimeCrate("xml")

        // Test-only dependencies
        val AsyncStd: CargoDependency = CargoDependency("async-std", CratesIo("1.12.0"), DependencyScope.Dev)
        val AsyncStream: CargoDependency = CargoDependency("async-stream", CratesIo("0.3.0"), DependencyScope.Dev)
        val Criterion: CargoDependency = CargoDependency("criterion", CratesIo("0.4.0"), DependencyScope.Dev)
        val FuturesCore: CargoDependency = CargoDependency("futures-core", CratesIo("0.3.25"), DependencyScope.Dev)
        val FuturesUtil: CargoDependency = CargoDependency("futures-util", CratesIo("0.3.25"), DependencyScope.Dev)
        val HdrHistogram: CargoDependency = CargoDependency("hdrhistogram", CratesIo("7.5.2"), DependencyScope.Dev)
        val Hound: CargoDependency = CargoDependency("hound", CratesIo("3.4.0"), DependencyScope.Dev)
        val PrettyAssertions: CargoDependency =
            CargoDependency("pretty_assertions", CratesIo("1.3.0"), DependencyScope.Dev)
        val SerdeJson: CargoDependency = CargoDependency("serde_json", CratesIo("1.0.0"), DependencyScope.Dev)
        val Smol: CargoDependency = CargoDependency("smol", CratesIo("1.2.0"), DependencyScope.Dev)
        val TempFile: CargoDependency = CargoDependency("tempfile", CratesIo("3.2.0"), DependencyScope.Dev)
        val Tokio: CargoDependency =
            CargoDependency("tokio", CratesIo("1.8.4"), DependencyScope.Dev, features = setOf("macros", "test-util", "rt-multi-thread"))
        val TracingAppender: CargoDependency = CargoDependency(
            "tracing-appender",
            CratesIo("0.2.2"),
            DependencyScope.Dev,
        )
        val TracingSubscriber: CargoDependency = CargoDependency(
            "tracing-subscriber",
            CratesIo("0.3.16"),
            DependencyScope.Dev,
            features = setOf("env-filter", "json"),
        )

    }
}
