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
import software.amazon.smithy.rust.codegen.core.util.PANIC
import software.amazon.smithy.rust.codegen.core.util.dq
import java.nio.file.Path

enum class DependencyScope {
    Build,
    CfgUnstable,
    Compile,
    Dev,
}

sealed class DependencyLocation

data class CratesIo(val version: String) : DependencyLocation()

data class Local(val basePath: String, val version: String? = null) : DependencyLocation()

sealed class RustDependency(open val name: String) : SymbolDependencyContainer {
    abstract fun version(): String

    open fun dependencies(): List<RustDependency> = listOf()

    override fun getDependencies(): List<SymbolDependency> {
        return listOf(
            SymbolDependency.builder()
                .packageName(name)
                .version(version())
                // We rely on retrieving the structured dependency from the symbol later
                .putProperty(PROPERTY_KEY, this)
                .build(),
        ) + dependencies().flatMap { it.dependencies }
    }

    open fun toDevDependency(): RustDependency =
        when (this) {
            is CargoDependency -> this.toDevDependency()
            is InlineDependency -> PANIC("it does not make sense for an inline dependency to be a dev-dependency")
        }

    companion object {
        private const val PROPERTY_KEY = "rustdep"

        fun fromSymbolDependency(symbolDependency: SymbolDependency) =
            symbolDependency.getProperty(PROPERTY_KEY, RustDependency::class.java).get()
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

    override fun dependencies(): List<RustDependency> = extraDependencies

    fun key() = "${module.fullyQualifiedPath()}::$name"

    companion object {
        fun forRustFile(
            module: RustModule.LeafModule,
            resourcePath: String,
            vararg additionalDependencies: RustDependency,
        ): InlineDependency {
            // The inline crate is loaded as a dependency on the runtime classpath
            val rustFile = this::class.java.getResource(resourcePath)
            check(rustFile != null) {
                "Rust file $resourcePath was missing from the resource bundle!"
            }
            return InlineDependency(module.name, module, additionalDependencies.toList()) {
                raw(rustFile.readText())
            }
        }

        private fun forInlineableRustFile(
            name: String,
            vararg additionalDependencies: RustDependency,
        ) = forRustFile(RustModule.private(name), "/inlineable/src/$name.rs", *additionalDependencies)

        fun eventReceiver(runtimeConfig: RuntimeConfig) =
            forInlineableRustFile(
                "event_receiver",
                CargoDependency.smithyHttp(runtimeConfig),
                CargoDependency.smithyRuntimeApi(runtimeConfig),
                CargoDependency.smithyTypes(runtimeConfig),
            )

        fun jsonErrors(runtimeConfig: RuntimeConfig) =
            forInlineableRustFile(
                "json_errors",
                CargoDependency.smithyJson(runtimeConfig),
                CargoDependency.Bytes,
                CargoDependency.Http,
            )

        fun awsQueryCompatibleErrors(runtimeConfig: RuntimeConfig) =
            forInlineableRustFile(
                "aws_query_compatible_errors",
                CargoDependency.smithyJson(runtimeConfig),
                CargoDependency.Http,
            )

        fun clientRequestCompression(runtimeConfig: RuntimeConfig) =
            forInlineableRustFile(
                "client_request_compression",
                CargoDependency.Http,
                CargoDependency.HttpBody,
                CargoDependency.Tracing,
                CargoDependency.Flate2,
                CargoDependency.Tokio.toDevDependency(),
                CargoDependency.smithyCompression(runtimeConfig)
                    .withFeature("http-body-0-4-x"),
                CargoDependency.smithyRuntimeApiClient(runtimeConfig),
                CargoDependency.smithyTypes(runtimeConfig).withFeature("http-body-0-4-x"),
            )

        fun idempotencyToken(runtimeConfig: RuntimeConfig) =
            forInlineableRustFile(
                "idempotency_token",
                CargoDependency.FastRand,
                CargoDependency.smithyTypes(runtimeConfig),
            )

        fun cborErrors(runtimeConfig: RuntimeConfig): InlineDependency =
            forInlineableRustFile(
                "cbor_errors",
                CargoDependency.smithyCbor(runtimeConfig),
                CargoDependency.smithyRuntimeApi(runtimeConfig),
                CargoDependency.smithyTypes(runtimeConfig),
            )

        fun ec2QueryErrors(runtimeConfig: RuntimeConfig): InlineDependency =
            forInlineableRustFile("ec2_query_errors", CargoDependency.smithyXml(runtimeConfig))

        fun wrappedXmlErrors(runtimeConfig: RuntimeConfig): InlineDependency =
            forInlineableRustFile("rest_xml_wrapped_errors", CargoDependency.smithyXml(runtimeConfig))

        fun unwrappedXmlErrors(runtimeConfig: RuntimeConfig): InlineDependency =
            forInlineableRustFile("rest_xml_unwrapped_errors", CargoDependency.smithyXml(runtimeConfig))

        fun serializationSettings(runtimeConfig: RuntimeConfig): InlineDependency =
            forInlineableRustFile(
                "serialization_settings",
                CargoDependency.Http,
                CargoDependency.smithyHttp(runtimeConfig),
                CargoDependency.smithyTypes(runtimeConfig),
            )

        fun constrained(): InlineDependency = forRustFile(ConstrainedModule, "/inlineable/src/constrained.rs")

        fun sdkFeatureTracker(runtimeConfig: RuntimeConfig): InlineDependency =
            forInlineableRustFile(
                "sdk_feature_tracker",
                CargoDependency.smithyRuntime(runtimeConfig),
            )

        fun cacheable(): InlineDependency =
            forInlineableRustFile(
                "cacheable",
                CargoDependency.Bytes,
            )
    }
}

fun InlineDependency.toType() = RuntimeType(module.fullyQualifiedPath(), this)

data class Feature(val name: String, val default: Boolean, val deps: List<String>)

val DEV_ONLY_FEATURES = setOf("test-util", "legacy-test-util")

/**
 * A dependency on an internal or external Cargo crate
 */
data class CargoDependency(
    override val name: String,
    private val location: DependencyLocation,
    val scope: DependencyScope = DependencyScope.Compile,
    val optional: Boolean = false,
    val features: Set<String> = emptySet(),
    val defaultFeatures: Boolean = true,
    val rustName: String = name.replace("-", "_"),
    val `package`: String? = null,
) : RustDependency(name) {
    val key: Triple<String, DependencyLocation, DependencyScope>
        get() = Triple(name, location, scope)

    init {
        if (scope != DependencyScope.Dev && DEV_ONLY_FEATURES.any { features.contains(it) }) {
            throw IllegalArgumentException("The `test-util` feature cannot be used outside of DependencyScope.Dev")
        }
    }

    fun withFeature(feature: String): CargoDependency {
        return copy(features = features.toMutableSet().apply { add(feature) })
    }

    override fun toDevDependency() = copy(scope = DependencyScope.Dev)

    override fun version(): String =
        when (location) {
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
            if (isNotEmpty()) {
                attribs["features"] = this
            }
        }
        if (optional) {
            attribs["optional"] = true
        }
        if (!defaultFeatures) {
            attribs["default-features"] = false
        }
        if (`package`.isNotNullOrEmpty()) {
            attribs["package"] = `package`.toString()
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
            if (isNotEmpty()) {
                attribs.add("features = [${joinToString(",") { it.dq() }}]")
            }
        }
        attribs.add("scope = $scope")
        return "$name = { ${attribs.joinToString(", ")} }"
    }

    fun toType() = RuntimeType("::$rustName", this)

    companion object {
        val Bytes: CargoDependency = CargoDependency("bytes", CratesIo("1.4.0"))
        val BytesUtils: CargoDependency = CargoDependency("bytes-utils", CratesIo("0.1.0"))
        val FastRand: CargoDependency = CargoDependency("fastrand", CratesIo("2.0.0"))
        val Flate2: CargoDependency = CargoDependency("flate2", CratesIo("1.0.30"))
        val FuturesUtil: CargoDependency =
            CargoDependency("futures-util", CratesIo("0.3.25"), defaultFeatures = false, features = setOf("alloc"))
        val Hex: CargoDependency = CargoDependency("hex", CratesIo("0.4.3"))
        val Hmac: CargoDependency = CargoDependency("hmac", CratesIo("0.12"))
        val LazyStatic: CargoDependency = CargoDependency("lazy_static", CratesIo("1.4.0"))
        val Lru: CargoDependency = CargoDependency("lru", CratesIo("0.12.2"))
        val Md5: CargoDependency = CargoDependency("md-5", CratesIo("0.10.0"), rustName = "md5")
        val PercentEncoding: CargoDependency =
            CargoDependency("percent-encoding", CratesIo("2.0.0"))
        val Regex: CargoDependency = CargoDependency("regex", CratesIo("1.5.5"))
        val RegexLite: CargoDependency = CargoDependency("regex-lite", CratesIo("0.1.5"))
        val Ring: CargoDependency = CargoDependency("ring", CratesIo("0.17.5"))
        val Sha2: CargoDependency = CargoDependency("sha2", CratesIo("0.10"))
        val TokioStream: CargoDependency = CargoDependency("tokio-stream", CratesIo("0.1.7"))
        val Tower: CargoDependency = CargoDependency("tower", CratesIo("0.4"))
        val Tracing: CargoDependency = CargoDependency("tracing", CratesIo("0.1"))
        val Url: CargoDependency = CargoDependency("url", CratesIo("2.3.1"))

        // Test-only dependencies
        val Approx: CargoDependency =
            CargoDependency("approx", CratesIo("0.5.1"), DependencyScope.Dev)
        val AsyncStd: CargoDependency =
            CargoDependency("async-std", CratesIo("1.12.0"), DependencyScope.Dev)
        val AsyncStream: CargoDependency =
            CargoDependency("async-stream", CratesIo("0.3.0"), DependencyScope.Dev)
        val Ciborium: CargoDependency =
            CargoDependency("ciborium", CratesIo("0.2"), DependencyScope.Dev)
        val Criterion: CargoDependency =
            CargoDependency("criterion", CratesIo("0.5.0"), DependencyScope.Dev)
        val FuturesCore: CargoDependency =
            CargoDependency("futures-core", CratesIo("0.3.25"), DependencyScope.Dev)
        val GetRandom: CargoDependency =
            CargoDependency("getrandom", CratesIo("0.3.3"), DependencyScope.Dev)
        val HdrHistogram: CargoDependency =
            CargoDependency("hdrhistogram", CratesIo("7.5.2"), DependencyScope.Dev)
        val Hound: CargoDependency =
            CargoDependency("hound", CratesIo("3.4.0"), DependencyScope.Dev)
        val PrettyAssertions: CargoDependency =
            CargoDependency("pretty_assertions", CratesIo("1.3.0"), DependencyScope.Dev)
        val Proptest: CargoDependency =
            CargoDependency("proptest", CratesIo("1"), DependencyScope.Dev)
        val SerdeJson: CargoDependency =
            CargoDependency("serde_json", CratesIo("1.0.0"), DependencyScope.Dev)
        val Smol: CargoDependency = CargoDependency("smol", CratesIo("1.2.0"), DependencyScope.Dev)
        val TempFile: CargoDependency =
            CargoDependency("tempfile", CratesIo("3.2.0"), DependencyScope.Dev)
        val Tokio: CargoDependency =
            CargoDependency(
                "tokio",
                CratesIo("1.23.1"),
                DependencyScope.Dev,
                features = setOf("macros", "test-util", "rt-multi-thread"),
            )
        val TracingAppender: CargoDependency =
            CargoDependency(
                "tracing-appender",
                CratesIo("0.2.2"),
                DependencyScope.Dev,
            )
        val TracingSubscriber: CargoDependency =
            CargoDependency(
                "tracing-subscriber",
                CratesIo("0.3.16"),
                DependencyScope.Dev,
                features = setOf("env-filter", "json"),
            )
        val TracingTest: CargoDependency =
            CargoDependency(
                "tracing-test",
                CratesIo("0.2.5"),
                DependencyScope.Dev,
                features = setOf("no-env-filter"),
            )

        // Hyper 0.x types
        val Http: CargoDependency = CargoDependency("http", CratesIo("0.2.9"))
        val HttpBody: CargoDependency = CargoDependency("http-body", CratesIo("0.4.4"))
        val Hyper: CargoDependency = CargoDependency("hyper", CratesIo("0.14.26"))
        val HyperWithStream: CargoDependency = Hyper.withFeature("stream")

        // Hyper 1.x types
        val Http1x: CargoDependency = CargoDependency("http-1x", CratesIo("1"), `package` = "http")
        val HttpBody1x: CargoDependency =
            CargoDependency("http-body-1x", CratesIo("1"), `package` = "http-body", optional = true)

        val HttpBodyUtil: CargoDependency = CargoDependency("http-body-util", CratesIo("0.1.3"))

        fun smithyAsync(runtimeConfig: RuntimeConfig) = runtimeConfig.smithyRuntimeCrate("smithy-async")

        fun smithyCbor(runtimeConfig: RuntimeConfig) = runtimeConfig.smithyRuntimeCrate("smithy-cbor")

        fun smithyChecksums(runtimeConfig: RuntimeConfig) = runtimeConfig.smithyRuntimeCrate("smithy-checksums")

        fun smithyCompression(runtimeConfig: RuntimeConfig) = runtimeConfig.smithyRuntimeCrate("smithy-compression")

        fun smithyEventStream(runtimeConfig: RuntimeConfig) = runtimeConfig.smithyRuntimeCrate("smithy-eventstream")

        fun smithyHttp(runtimeConfig: RuntimeConfig) = runtimeConfig.smithyRuntimeCrate("smithy-http")

        fun smithyHttpClient(runtimeConfig: RuntimeConfig) = runtimeConfig.smithyRuntimeCrate("smithy-http-client")

        fun smithyHttpClientTestUtil(runtimeConfig: RuntimeConfig) =
            smithyHttpClient(runtimeConfig).toDevDependency().withFeature("test-util")

        fun smithyJson(runtimeConfig: RuntimeConfig) = runtimeConfig.smithyRuntimeCrate("smithy-json")

        fun smithyProtocolTestHelpers(runtimeConfig: RuntimeConfig) =
            runtimeConfig.smithyRuntimeCrate("smithy-protocol-test", scope = DependencyScope.Dev)

        fun smithyQuery(runtimeConfig: RuntimeConfig) = runtimeConfig.smithyRuntimeCrate("smithy-query")

        fun smithyRuntime(runtimeConfig: RuntimeConfig) =
            runtimeConfig.smithyRuntimeCrate("smithy-runtime").withFeature("client")

        fun smithyRuntimeTestUtil(runtimeConfig: RuntimeConfig) =
            smithyRuntime(runtimeConfig).toDevDependency().withFeature("test-util")

        fun smithyRuntimeApi(runtimeConfig: RuntimeConfig) = runtimeConfig.smithyRuntimeCrate("smithy-runtime-api")

        fun smithyRuntimeApiClient(runtimeConfig: RuntimeConfig) =
            smithyRuntimeApi(runtimeConfig).withFeature("client").withFeature("http-02x")

        fun smithyRuntimeApiTestUtil(runtimeConfig: RuntimeConfig) =
            smithyRuntimeApi(runtimeConfig).toDevDependency().withFeature("test-util")

        fun smithyTypes(runtimeConfig: RuntimeConfig) = runtimeConfig.smithyRuntimeCrate("smithy-types")

        fun smithyXml(runtimeConfig: RuntimeConfig) = runtimeConfig.smithyRuntimeCrate("smithy-xml")

        fun smithyMocks(runtimeConfig: RuntimeConfig) =
            runtimeConfig.smithyRuntimeCrate("smithy-mocks", scope = DependencyScope.Dev)

        // behind feature-gate
        val Serde =
            CargoDependency("serde", CratesIo("1.0"), features = setOf("derive"), scope = DependencyScope.CfgUnstable)
    }
}

private fun String?.isNotNullOrEmpty(): Boolean = !this.isNullOrEmpty()
