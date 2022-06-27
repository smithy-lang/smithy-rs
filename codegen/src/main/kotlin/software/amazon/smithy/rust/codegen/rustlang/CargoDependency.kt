/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.rustlang

import software.amazon.smithy.codegen.core.SymbolDependency
import software.amazon.smithy.codegen.core.SymbolDependencyContainer
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.util.dq
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
    val module: RustModule,
    val extraDependencies: List<RustDependency> = listOf(),
    val renderer: (RustWriter) -> Unit
) : RustDependency(name) {
    override fun version(): String {
        // just need a version that won't crash
        return renderer.hashCode().toString()
    }

    override fun dependencies(): List<RustDependency> {
        return extraDependencies
    }

    fun key() = "${module.name}::$name"

    companion object {
        fun forRustFile(
            name: String,
            baseDir: String,
            vararg additionalDependencies: RustDependency
        ): InlineDependency = forRustFile(name, baseDir, visibility = Visibility.PRIVATE, *additionalDependencies)

        fun forRustFile(
            name: String,
            baseDir: String,
            visibility: Visibility,
            vararg additionalDependencies: RustDependency
        ): InlineDependency {
            val module = RustModule.default(name, visibility)
            val filename = if (name.endsWith(".rs")) { name } else { "$name.rs" }
            // The inline crate is loaded as a dependency on the runtime classpath
            val rustFile = this::class.java.getResource("/$baseDir/src/$filename")
            check(rustFile != null) { "Rust file /$baseDir/src/$filename was missing from the resource bundle!" }
            return InlineDependency(name, module, additionalDependencies.toList()) { writer ->
                writer.raw(rustFile.readText())
            }
        }

        fun forRustFile(name: String, vararg additionalDependencies: RustDependency) =
            forRustFile(name, "inlineable", *additionalDependencies)

        fun eventStream(runtimeConfig: RuntimeConfig) =
            forRustFile("event_stream", CargoDependency.SmithyEventStream(runtimeConfig))

        fun jsonErrors(runtimeConfig: RuntimeConfig) =
            forRustFile("json_errors", CargoDependency.Http, CargoDependency.SmithyTypes(runtimeConfig))

        fun idempotencyToken() =
            forRustFile("idempotency_token", CargoDependency.FastRand)

        fun ec2QueryErrors(runtimeConfig: RuntimeConfig): InlineDependency =
            forRustFile("ec2_query_errors", CargoDependency.smithyXml(runtimeConfig))

        fun wrappedXmlErrors(runtimeConfig: RuntimeConfig): InlineDependency =
            forRustFile("rest_xml_wrapped_errors", CargoDependency.smithyXml(runtimeConfig))

        fun unwrappedXmlErrors(runtimeConfig: RuntimeConfig): InlineDependency =
            forRustFile("rest_xml_unwrapped_errors", CargoDependency.smithyXml(runtimeConfig))
    }
}

fun CargoDependency.asType(): RuntimeType = RuntimeType(null, dependency = this, namespace = rustName)

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
    val rustName: String = name.replace("-", "_")
) : RustDependency(name) {
    val key: Triple<String, DependencyLocation, DependencyScope> get() = Triple(name, location, scope)

    fun canMergeWith(other: CargoDependency): Boolean = key == other.key

    fun withFeature(feature: String): CargoDependency {
        return copy(features = features.toMutableSet().apply { add(feature) })
    }

    override fun version(): String = when (location) {
        is CratesIo -> location.version
        is Local -> "local"
    }

    fun rustName(name: String): RuntimeType = RuntimeType(name, this, this.rustName)

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
        val Bytes: CargoDependency = CargoDependency("bytes", CratesIo("1"))
        val BytesUtils: CargoDependency = CargoDependency("bytes-utils", CratesIo("0.1.1"))
        val FastRand: CargoDependency = CargoDependency("fastrand", CratesIo("1"))
        val Hex: CargoDependency = CargoDependency("hex", CratesIo("0.4.3"))
        val HttpBody: CargoDependency = CargoDependency("http-body", CratesIo("0.4.4"))
        val Http: CargoDependency = CargoDependency("http", CratesIo("0.2"))
        val Hyper: CargoDependency = CargoDependency("hyper", CratesIo("0.14"))
        val HyperWithStream: CargoDependency = Hyper.withFeature("stream")
        val LazyStatic: CargoDependency = CargoDependency("lazy_static", CratesIo("1.4"))
        val Md5: CargoDependency = CargoDependency("md-5", CratesIo("0.10"), rustName = "md5")
        val PercentEncoding: CargoDependency = CargoDependency("percent-encoding", CratesIo("2"))
        val PrettyAssertions: CargoDependency = CargoDependency("pretty_assertions", CratesIo("1"), scope = DependencyScope.Dev)
        val Regex: CargoDependency = CargoDependency("regex", CratesIo("1"))
        val Ring: CargoDependency = CargoDependency("ring", CratesIo("0.16"))
        val TempFile: CargoDependency = CargoDependency("temp-file", CratesIo("0.1.6"), scope = DependencyScope.Dev)
        val TokioStream: CargoDependency = CargoDependency("tokio-stream", CratesIo("0.1.7"))
        val Tower: CargoDependency = CargoDependency("tower", CratesIo("0.4"))
        val Tracing: CargoDependency = CargoDependency("tracing", CratesIo("0.1"))

        fun SmithyTypes(runtimeConfig: RuntimeConfig) = runtimeConfig.runtimeCrate("types")
        fun SmithyClient(runtimeConfig: RuntimeConfig) = runtimeConfig.runtimeCrate("client")
        fun SmithyAsync(runtimeConfig: RuntimeConfig) = runtimeConfig.runtimeCrate("async")
        fun SmithyEventStream(runtimeConfig: RuntimeConfig) = runtimeConfig.runtimeCrate("eventstream")
        fun SmithyHttp(runtimeConfig: RuntimeConfig) = runtimeConfig.runtimeCrate("http")
        fun SmithyHttpTower(runtimeConfig: RuntimeConfig) = runtimeConfig.runtimeCrate("http-tower")
        fun SmithyProtocolTestHelpers(runtimeConfig: RuntimeConfig) =
            runtimeConfig.runtimeCrate("protocol-test").copy(scope = DependencyScope.Dev)
        fun smithyJson(runtimeConfig: RuntimeConfig): CargoDependency = runtimeConfig.runtimeCrate("json")
        fun smithyQuery(runtimeConfig: RuntimeConfig): CargoDependency = runtimeConfig.runtimeCrate("query")
        fun smithyXml(runtimeConfig: RuntimeConfig): CargoDependency = runtimeConfig.runtimeCrate("xml")
    }
}
