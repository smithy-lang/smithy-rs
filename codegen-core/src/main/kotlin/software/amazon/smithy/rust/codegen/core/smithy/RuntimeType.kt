/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.core.Version
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.CratesIo
import software.amazon.smithy.rust.codegen.core.rustlang.DependencyLocation
import software.amazon.smithy.rust.codegen.core.rustlang.DependencyScope
import software.amazon.smithy.rust.codegen.core.rustlang.InlineDependency
import software.amazon.smithy.rust.codegen.core.rustlang.Local
import software.amazon.smithy.rust.codegen.core.rustlang.RustDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustInlineTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.util.orNull
import java.util.Optional

/**
 * Location of the runtime crates (aws-smithy-http, aws-smithy-types etc.)
 *
 * This can be configured via the `runtimeConfig.versions` field in smithy-build.json
 */
data class RuntimeCrateLocation(val path: String?, val versions: CrateVersionMap) {
    companion object {
        fun path(path: String) = RuntimeCrateLocation(path, CrateVersionMap(emptyMap()))
    }
}

fun RuntimeCrateLocation.crateLocation(crateName: String): DependencyLocation {
    val version = crateName.let { versions.map[crateName] } ?: Version.crateVersion(crateName)
    return when (this.path) {
        // CratesIo needs an exact version. However, for local runtime crates we do not
        // provide a detected version unless the user explicitly sets one via the `versions` map.
        null -> CratesIo(version)
        else -> Local(this.path)
    }
}

/**
 * A mapping from crate name to a user-specified version.
 */
@JvmInline
value class CrateVersionMap(
    val map: Map<String, String>,
)

/**
 * HTTP version to use for code generation.
 *
 * This determines which versions of http/http-body/hyper crates to use,
 * as well as which smithy-http-server crate to use (legacy vs current).
 */
enum class HttpVersion {
    /** HTTP 0.x: http@0.2, http-body@0.4, hyper@0.14, aws-smithy-legacy-http-server */
    Http0x,

    /** HTTP 1.x: http@1, http-body@1, hyper@1, aws-smithy-http-server@1 */
    Http1x,
}

/**
 * Prefix & crate location for the runtime crates.
 */
data class RuntimeConfig(
    val cratePrefix: String = "aws",
    val runtimeCrateLocation: RuntimeCrateLocation = RuntimeCrateLocation.path("../"),
    /**
     * HTTP version to use for code generation. Defaults to Http1x as that is the default for Clients..
     */
    val httpVersion: HttpVersion = HttpVersion.Http1x,
) {
    companion object {
        /**
         * Load a `RuntimeConfig` from an [ObjectNode] (JSON)
         */
        fun fromNode(maybeNode: Optional<ObjectNode>): RuntimeConfig {
            val node = maybeNode.orElse(Node.objectNode())
            val crateVersionMap =
                node.getObjectMember("versions")
                    .orElse(Node.objectNode())
                    .members
                    .entries
                    .let { members ->
                        val map =
                            members.associate {
                                it.key.toString() to it.value.expectStringNode().value
                            }
                        CrateVersionMap(map)
                    }
            val path = node.getStringMember("relativePath").orNull()?.value
            val runtimeCrateLocation = RuntimeCrateLocation(path = path, versions = crateVersionMap)
            return RuntimeConfig(
                node.getStringMemberOrDefault("cratePrefix", "aws"),
                runtimeCrateLocation = runtimeCrateLocation,
            )
        }
    }

    fun smithyRuntimeCrate(
        runtimeCrateName: String,
        optional: Boolean = false,
        scope: DependencyScope = DependencyScope.Compile,
    ): CargoDependency {
        val crateName = "$cratePrefix-$runtimeCrateName"
        return CargoDependency(
            crateName,
            runtimeCrateLocation.crateLocation(crateName),
            optional = optional,
            scope = scope,
        )
    }
}

/**
 * `RuntimeType` captures all necessary information to render a type into a Rust file:
 * - [fullyQualifiedName]: What type is this?
 * - [namespace]: Where can we find this type.
 * - [dependency]: What other crates, if any, are required to use this type?
 *
 * For example:
 *
 *  * `RuntimeType("header::HeaderName", CargoDependency.Http)`, when passed to a [RustWriter] would appear as such:
 *
 * `http::header::HeaderName`
 *  ------------  ----------
 *       |            |
 * `[namespace]` `[fullyQualifiedName]`
 *
 *  This type would have a [CargoDependency] pointing to the `http` crate. Writing it multiple times would still only
 *  add the dependency once.
 */
data class RuntimeType(val path: String, val dependency: RustDependency? = null) {
    val name: String
    val namespace: String

    init {
        val splitPath = path.split("::").toMutableList()
        // get the name at the end
        this.name = splitPath.removeLast()
        // get all parts that aren't the name at the end
        this.namespace = splitPath.joinToString("::")
    }

    /**
     * Get a writable for this `RuntimeType`
     */
    val writable =
        writable {
            rustInlineTemplate(
                "#{this:T}",
                "this" to this@RuntimeType,
            )
        }

    fun toDevDependencyType(): RuntimeType = copy(dependency = dependency?.toDevDependency())

    /**
     * Convert this [RuntimeType] into a [Symbol].
     *
     * This is not commonly required, but is occasionally useful when you want to force an import without referencing a type
     * (e.g. when bringing a trait into scope). See [CodegenWriter.addUseImports].
     */
    fun toSymbol(): Symbol {
        val builder =
            Symbol.builder()
                .name(name)
                .namespace(namespace, "::")
                .rustType(RustType.Opaque(name, namespace))

        dependency?.run { builder.addDependency(this) }
        return builder.build()
    }

    /**
     * Create a new [RuntimeType] with a nested path.
     *
     * # Example
     * ```kotlin
     * val http = CargoDependency.http.resolve("Request")
     * ```
     */
    fun resolve(subPath: String): RuntimeType = copy(path = "$path::$subPath")

    /**
     * Returns the fully qualified name for this type
     */
    fun fullyQualifiedName(): String = path

    fun render(fullyQualified: Boolean = true): String {
        return if (fullyQualified) {
            fullyQualifiedName()
        } else {
            name
        }
    }

    /**
     * The companion object contains commonly used RuntimeTypes
     */
    companion object {
        /**
         * Scope that contains all Rust prelude types, but not macros or functions.
         *
         * Prelude docs: https://doc.rust-lang.org/std/prelude/index.html#prelude-contents
         */
        val preludeScope by lazy {
            arrayOf(
                // Rust 1.0
                "Copy" to std.resolve("marker::Copy"),
                "Send" to Send,
                "Sized" to std.resolve("marker::Sized"),
                "Sync" to Sync,
                "Unpin" to std.resolve("marker::Unpin"),
                "Drop" to std.resolve("ops::Drop"),
                "Fn" to std.resolve("ops::Fn"),
                "FnMut" to std.resolve("ops::FnMut"),
                "FnOnce" to std.resolve("ops::FnOnce"),
                "Box" to Box,
                "ToOwned" to std.resolve("borrow::ToOwned"),
                "Clone" to Clone,
                "PartialEq" to std.resolve("cmp::PartialEq"),
                "PartialOrd" to std.resolve("cmp::PartialOrd"),
                "Eq" to Eq,
                "Ord" to Ord,
                "AsRef" to AsRef,
                "AsMut" to std.resolve("convert::AsMut"),
                "Into" to Into,
                "From" to From,
                "Default" to Default,
                "Iterator" to std.resolve("iter::Iterator"),
                "Extend" to std.resolve("iter::Extend"),
                "IntoIterator" to std.resolve("iter::IntoIterator"),
                "DoubleEndedIterator" to std.resolve("iter::DoubleEndedIterator"),
                "ExactSizeIterator" to std.resolve("iter::ExactSizeIterator"),
                "Option" to Option,
                "Some" to Option.resolve("Some"),
                "None" to Option.resolve("None"),
                "Result" to std.resolve("result::Result"),
                "Ok" to std.resolve("result::Result::Ok"),
                "Err" to std.resolve("result::Result::Err"),
                "String" to String,
                "ToString" to std.resolve("string::ToString"),
                "Vec" to Vec,
                // 2021 Edition
                "TryFrom" to std.resolve("convert::TryFrom"),
                "TryInto" to std.resolve("convert::TryInto"),
                "FromIterator" to std.resolve("iter::FromIterator"),
            )
        }

        // stdlib types
        val std = RuntimeType("::std")
        val stdCmp = std.resolve("cmp")
        val stdFmt = std.resolve("fmt")
        val stdConvert = std.resolve("convert")
        val Arc = std.resolve("sync::Arc")
        val AsRef = stdConvert.resolve("AsRef")
        val Bool = std.resolve("primitive::bool")
        val Box = std.resolve("boxed::Box")
        val ByteSlab = std.resolve("vec::Vec<u8>")
        val Clone = std.resolve("clone::Clone")
        val Cow = std.resolve("borrow::Cow")
        val Debug = stdFmt.resolve("Debug")
        val Default = std.resolve("default::Default")
        val Display = stdFmt.resolve("Display")
        val Duration = std.resolve("time::Duration")
        val Eq = stdCmp.resolve("Eq")
        val From = stdConvert.resolve("From")
        val Hash = std.resolve("hash::Hash")
        val HashMap = std.resolve("collections::HashMap")
        val Into = stdConvert.resolve("Into")
        val Option = std.resolve("option::Option")
        val Ord = stdCmp.resolve("Ord")
        val PartialEq = stdCmp.resolve("PartialEq")
        val PartialOrd = stdCmp.resolve("PartialOrd")
        val Phantom = std.resolve("marker::PhantomData")
        val Send = std.resolve("marker::Send")
        val StdError = std.resolve("error::Error")
        val String = std.resolve("string::String")
        val Sync = std.resolve("marker::Sync")
        val TryFrom = stdConvert.resolve("TryFrom")
        val U64 = std.resolve("primitive::u64")
        val Vec = std.resolve("vec::Vec")

        // primitive types
        val StaticStr = RuntimeType("&'static str")

        // Http0x types
        val Http = CargoDependency.Http.toType()
        val HttpBody = CargoDependency.HttpBody.toType()
        val HttpRequest = Http.resolve("Request")
        val HttpRequestBuilder = Http.resolve("request::Builder")
        val HttpResponse = Http.resolve("Response")
        val HttpResponseBuilder = Http.resolve("response::Builder")

        // Http1x types
        val Http1x = CargoDependency.Http1x.toType()
        val HttpBody1x = CargoDependency.HttpBody1x.toType()
        val HttpRequestBuilder1x = Http1x.resolve("request::Builder")

        /**
         * Returns the appropriate http crate based on HTTP version.
         *
         * For HTTP 1.x: returns http@1.x crate
         * For HTTP 0.x: returns http@0.2.x crate
         */
        fun httpForConfig(runtimeConfig: RuntimeConfig): RuntimeType =
            when (runtimeConfig.httpVersion) {
                HttpVersion.Http1x -> Http1x
                HttpVersion.Http0x -> Http
            }

        /**
         * Returns the appropriate http::request::Builder based on HTTP version.
         *
         * For HTTP 1.x: returns http@1.x request::Builder
         * For HTTP 0.x: returns http@0.2.x request::Builder
         */
        fun httpRequestBuilderForConfig(runtimeConfig: RuntimeConfig): RuntimeType =
            httpForConfig(runtimeConfig).resolve("request::Builder")

        /**
         * Returns the appropriate http::response::Builder based on HTTP version.
         *
         * For HTTP 1.x: returns http@1.x response::Builder
         * For HTTP 0.x: returns http@0.2.x response::Builder
         */
        fun httpResponseBuilderForConfig(runtimeConfig: RuntimeConfig): RuntimeType =
            httpForConfig(runtimeConfig).resolve("response::Builder")

        /**
         * Returns the appropriate http-body crate module based on HTTP version.
         *
         * For HTTP 1.x: returns http-body@1.x crate
         * For HTTP 0.x: returns http-body@0.4.x crate
         */
        fun httpBodyForConfig(runtimeConfig: RuntimeConfig): RuntimeType =
            when (runtimeConfig.httpVersion) {
                HttpVersion.Http1x -> HttpBody1x
                HttpVersion.Http0x -> HttpBody
            }

        fun hyperForConfig(runtimeConfig: RuntimeConfig) = CargoDependency.hyper(runtimeConfig).toType()

        // external cargo dependency types
        val Bytes = CargoDependency.Bytes.toType().resolve("Bytes")
        val FastRand = CargoDependency.FastRand.toType()
        val Hyper = CargoDependency.Hyper.toType()
        val LazyStatic = CargoDependency.LazyStatic.toType()
        val PercentEncoding = CargoDependency.PercentEncoding.toType()
        val PrettyAssertions = CargoDependency.PrettyAssertions.toType()
        val Regex = CargoDependency.Regex.toType()
        val Serde = CargoDependency.Serde.toType()
        val SerdeDeserialize = Serde.resolve("Deserialize")
        val SerdeSerialize = Serde.resolve("Serialize")
        val Tokio = CargoDependency.Tokio.toType()
        val TokioStream = CargoDependency.TokioStream.toType()
        val Tower = CargoDependency.Tower.toType()
        val Tracing = CargoDependency.Tracing.toType()
        val TracingTest = CargoDependency.TracingTest.toType()
        val TracingSubscriber = CargoDependency.TracingSubscriber.toType()

        // codegen types
        val ConstrainedTrait =
            RuntimeType("crate::constrained::Constrained", InlineDependency.constrained())
        val MaybeConstrained =
            RuntimeType("crate::constrained::MaybeConstrained", InlineDependency.constrained())

        // smithy runtime types
        fun smithyAsync(runtimeConfig: RuntimeConfig) = CargoDependency.smithyAsync(runtimeConfig).toType()

        fun smithyCbor(runtimeConfig: RuntimeConfig) = CargoDependency.smithyCbor(runtimeConfig).toType()

        fun smithyChecksums(runtimeConfig: RuntimeConfig) = CargoDependency.smithyChecksums(runtimeConfig).toType()

        fun smithyEventStream(runtimeConfig: RuntimeConfig) = CargoDependency.smithyEventStream(runtimeConfig).toType()

        fun smithyHttp(runtimeConfig: RuntimeConfig) = CargoDependency.smithyHttp(runtimeConfig).toType()

        fun smithyJson(runtimeConfig: RuntimeConfig) = CargoDependency.smithyJson(runtimeConfig).toType()

        fun smithyQuery(runtimeConfig: RuntimeConfig) = CargoDependency.smithyQuery(runtimeConfig).toType()

        fun smithyRuntime(runtimeConfig: RuntimeConfig) = CargoDependency.smithyRuntime(runtimeConfig).toType()

        fun smithyRuntimeApi(runtimeConfig: RuntimeConfig) = CargoDependency.smithyRuntimeApi(runtimeConfig).toType()

        /**
         * Returns smithy-runtime-api with the appropriate HTTP version feature enabled.
         *
         * This is needed when accessing HTTP types from smithy-runtime-api like http::Request.
         * For HTTP 1.x: adds "http-1x" feature
         * For HTTP 0.x: adds "http-02x" feature
         *
         * Use this instead of smithyRuntimeApi() when you need HTTP type re-exports.
         */
        fun smithyRuntimeApiWithHttpFeature(runtimeConfig: RuntimeConfig): RuntimeType =
            when (runtimeConfig.httpVersion) {
                HttpVersion.Http1x -> CargoDependency.smithyRuntimeApi(runtimeConfig).withFeature("http-1x").toType()
                HttpVersion.Http0x -> CargoDependency.smithyRuntimeApi(runtimeConfig).withFeature("http-02x").toType()
            }

        fun smithyRuntimeApiClient(runtimeConfig: RuntimeConfig) =
            CargoDependency.smithyRuntimeApiClient(runtimeConfig).toType()

        fun smithyTypes(runtimeConfig: RuntimeConfig) = CargoDependency.smithyTypes(runtimeConfig).toType()

        fun smithyXml(runtimeConfig: RuntimeConfig) = CargoDependency.smithyXml(runtimeConfig).toType()

        private fun smithyProtocolTest(runtimeConfig: RuntimeConfig) =
            CargoDependency.smithyProtocolTestHelpers(runtimeConfig).toType()

        // smithy runtime type members
        fun base64Decode(runtimeConfig: RuntimeConfig): RuntimeType =
            smithyTypes(runtimeConfig).resolve("base64::decode")

        fun base64Encode(runtimeConfig: RuntimeConfig): RuntimeType =
            smithyTypes(runtimeConfig).resolve("base64::encode")

        fun configBag(runtimeConfig: RuntimeConfig): RuntimeType =
            smithyTypes(runtimeConfig).resolve("config_bag::ConfigBag")

        fun runtimeComponents(runtimeConfig: RuntimeConfig) =
            smithyRuntimeApiClient(runtimeConfig)
                .resolve("client::runtime_components::RuntimeComponents")

        fun runtimeComponentsBuilder(runtimeConfig: RuntimeConfig) =
            smithyRuntimeApiClient(runtimeConfig)
                .resolve("client::runtime_components::RuntimeComponentsBuilder")

        fun runtimePlugins(runtimeConfig: RuntimeConfig): RuntimeType =
            smithyRuntimeApiClient(runtimeConfig)
                .resolve("client::runtime_plugin::RuntimePlugins")

        fun runtimePlugin(runtimeConfig: RuntimeConfig) =
            smithyRuntimeApiClient(runtimeConfig)
                .resolve("client::runtime_plugin::RuntimePlugin")

        fun sharedRuntimePlugin(runtimeConfig: RuntimeConfig) =
            smithyRuntimeApiClient(runtimeConfig)
                .resolve("client::runtime_plugin::SharedRuntimePlugin")

        fun boxError(runtimeConfig: RuntimeConfig): RuntimeType =
            smithyRuntimeApi(runtimeConfig).resolve("box_error::BoxError")

        fun sdkError(runtimeConfig: RuntimeConfig): RuntimeType =
            smithyRuntimeApiClient(runtimeConfig).resolve("client::result::SdkError")

        fun intercept(runtimeConfig: RuntimeConfig): RuntimeType =
            smithyRuntimeApiClient(runtimeConfig).resolve("client::interceptors::Intercept")

        fun interceptorContext(runtimeConfig: RuntimeConfig): RuntimeType =
            smithyRuntimeApiClient(runtimeConfig)
                .resolve("client::interceptors::context::InterceptorContext")

        fun sharedInterceptor(runtimeConfig: RuntimeConfig): RuntimeType =
            smithyRuntimeApiClient(runtimeConfig)
                .resolve("client::interceptors::SharedInterceptor")

        fun afterDeserializationInterceptorContextRef(runtimeConfig: RuntimeConfig): RuntimeType =
            smithyRuntimeApiClient(runtimeConfig).resolve("client::interceptors::context::AfterDeserializationInterceptorContextRef")

        fun beforeSerializationInterceptorContextRef(runtimeConfig: RuntimeConfig): RuntimeType =
            smithyRuntimeApiClient(runtimeConfig).resolve("client::interceptors::context::BeforeSerializationInterceptorContextRef")

        fun beforeSerializationInterceptorContextMut(runtimeConfig: RuntimeConfig): RuntimeType =
            smithyRuntimeApiClient(runtimeConfig).resolve("client::interceptors::context::BeforeSerializationInterceptorContextMut")

        fun beforeDeserializationInterceptorContextRef(runtimeConfig: RuntimeConfig): RuntimeType =
            smithyRuntimeApiClient(runtimeConfig).resolve("client::interceptors::context::BeforeDeserializationInterceptorContextRef")

        fun beforeDeserializationInterceptorContextMut(runtimeConfig: RuntimeConfig): RuntimeType =
            smithyRuntimeApiClient(runtimeConfig).resolve("client::interceptors::context::BeforeDeserializationInterceptorContextMut")

        fun beforeTransmitInterceptorContextRef(runtimeConfig: RuntimeConfig): RuntimeType =
            smithyRuntimeApiClient(runtimeConfig).resolve("client::interceptors::context::BeforeTransmitInterceptorContextRef")

        fun beforeTransmitInterceptorContextMut(runtimeConfig: RuntimeConfig): RuntimeType =
            smithyRuntimeApiClient(runtimeConfig).resolve("client::interceptors::context::BeforeTransmitInterceptorContextMut")

        fun finalizerInterceptorContextRef(runtimeConfig: RuntimeConfig): RuntimeType =
            smithyRuntimeApiClient(runtimeConfig)
                .resolve("client::interceptors::context::FinalizerInterceptorContextRef")

        fun finalizerInterceptorContextMut(runtimeConfig: RuntimeConfig): RuntimeType =
            smithyRuntimeApiClient(runtimeConfig)
                .resolve("client::interceptors::context::FinalizerInterceptorContextMut")

        fun headers(runtimeConfig: RuntimeConfig): RuntimeType =
            smithyRuntimeApi(runtimeConfig).resolve("http::Headers")

        fun blob(runtimeConfig: RuntimeConfig) = smithyTypes(runtimeConfig).resolve("Blob")

        fun byteStream(runtimeConfig: RuntimeConfig) = smithyTypes(runtimeConfig).resolve("byte_stream::ByteStream")

        fun dateTime(runtimeConfig: RuntimeConfig) = smithyTypes(runtimeConfig).resolve("DateTime")

        fun document(runtimeConfig: RuntimeConfig): RuntimeType = smithyTypes(runtimeConfig).resolve("Document")

        fun format(runtimeConfig: RuntimeConfig) = smithyTypes(runtimeConfig).resolve("date_time::Format")

        fun retryErrorKind(runtimeConfig: RuntimeConfig) = smithyTypes(runtimeConfig).resolve("retry::ErrorKind")

        fun eventStreamReceiver(runtimeConfig: RuntimeConfig): RuntimeType =
            smithyHttp(runtimeConfig).resolve("event_stream::Receiver")

        fun eventReceiver(runtimeConfig: RuntimeConfig) =
            forInlineDependency(InlineDependency.eventReceiver(runtimeConfig))
                .resolve("EventReceiver")

        fun eventStreamSender(runtimeConfig: RuntimeConfig): RuntimeType =
            smithyHttp(runtimeConfig).resolve("event_stream::EventStreamSender")

        fun futuresStreamCompatByteStream(runtimeConfig: RuntimeConfig): RuntimeType =
            smithyHttp(runtimeConfig)
                .resolve("futures_stream_adapter::FuturesStreamCompatByteStream")

        fun errorMetadata(runtimeConfig: RuntimeConfig) = smithyTypes(runtimeConfig).resolve("error::ErrorMetadata")

        fun errorMetadataBuilder(runtimeConfig: RuntimeConfig) =
            smithyTypes(runtimeConfig).resolve("error::metadata::Builder")

        fun provideErrorMetadataTrait(runtimeConfig: RuntimeConfig) =
            smithyTypes(runtimeConfig).resolve("error::metadata::ProvideErrorMetadata")

        fun jsonErrors(runtimeConfig: RuntimeConfig) = forInlineDependency(InlineDependency.jsonErrors(runtimeConfig))

        fun awsQueryCompatibleErrors(runtimeConfig: RuntimeConfig) =
            forInlineDependency(InlineDependency.awsQueryCompatibleErrors(runtimeConfig))

        fun labelFormat(
            runtimeConfig: RuntimeConfig,
            func: String,
        ) = smithyHttp(runtimeConfig).resolve("label::$func")

        fun operation(runtimeConfig: RuntimeConfig) = smithyHttp(runtimeConfig).resolve("operation::Operation")

        fun protocolTest(
            runtimeConfig: RuntimeConfig,
            func: String,
        ): RuntimeType = smithyProtocolTest(runtimeConfig).resolve(func)

        fun provideErrorKind(runtimeConfig: RuntimeConfig) =
            smithyTypes(runtimeConfig).resolve("retry::ProvideErrorKind")

        fun queryFormat(
            runtimeConfig: RuntimeConfig,
            func: String,
        ) = smithyHttp(runtimeConfig).resolve("query::$func")

        fun sdkBody(runtimeConfig: RuntimeConfig): RuntimeType = smithyTypes(runtimeConfig).resolve("body::SdkBody")

        fun parseTimestampFormat(
            codegenTarget: CodegenTarget,
            runtimeConfig: RuntimeConfig,
            format: TimestampFormatTrait.Format,
        ): RuntimeType {
            val timestampFormat =
                when (format) {
                    TimestampFormatTrait.Format.EPOCH_SECONDS -> "EpochSeconds"
                    // clients allow offsets, servers do nt
                    TimestampFormatTrait.Format.DATE_TIME ->
                        codegenTarget.ifClient { "DateTimeWithOffset" } ?: "DateTime"

                    TimestampFormatTrait.Format.HTTP_DATE -> "HttpDate"
                    TimestampFormatTrait.Format.UNKNOWN -> TODO()
                }

            return smithyTypes(runtimeConfig).resolve("date_time::Format::$timestampFormat")
        }

        fun serializeTimestampFormat(
            runtimeConfig: RuntimeConfig,
            format: TimestampFormatTrait.Format,
        ): RuntimeType {
            val timestampFormat =
                when (format) {
                    TimestampFormatTrait.Format.EPOCH_SECONDS -> "EpochSeconds"
                    // clients allow offsets, servers do not
                    TimestampFormatTrait.Format.DATE_TIME -> "DateTime"
                    TimestampFormatTrait.Format.HTTP_DATE -> "HttpDate"
                    TimestampFormatTrait.Format.UNKNOWN -> TODO()
                }

            return smithyTypes(runtimeConfig).resolve("date_time::Format::$timestampFormat")
        }

        fun captureRequest(runtimeConfig: RuntimeConfig) =
            CargoDependency.smithyHttpClientTestUtil(runtimeConfig)
                .toType()
                .resolve("test_util::capture_request")

        fun forInlineDependency(inlineDependency: InlineDependency) =
            RuntimeType("crate::${inlineDependency.name}", inlineDependency)

        fun forInlineFun(
            name: String,
            module: RustModule,
            func: Writable,
        ) = RuntimeType(
            "${module.fullyQualifiedPath()}::$name",
            dependency = InlineDependency(name, module, listOf(), func),
        )

        // inlinable types
        fun cborErrors(runtimeConfig: RuntimeConfig) = forInlineDependency(InlineDependency.cborErrors(runtimeConfig))

        fun ec2QueryErrors(runtimeConfig: RuntimeConfig) =
            forInlineDependency(InlineDependency.ec2QueryErrors(runtimeConfig))

        fun wrappedXmlErrors(runtimeConfig: RuntimeConfig) =
            forInlineDependency(InlineDependency.wrappedXmlErrors(runtimeConfig))

        fun unwrappedXmlErrors(runtimeConfig: RuntimeConfig) =
            forInlineDependency(InlineDependency.unwrappedXmlErrors(runtimeConfig))

        fun idempotencyToken(runtimeConfig: RuntimeConfig) =
            forInlineDependency(InlineDependency.idempotencyToken(runtimeConfig))

        fun clientRequestCompression(runtimeConfig: RuntimeConfig) =
            forInlineDependency(InlineDependency.clientRequestCompression(runtimeConfig))
    }
}
