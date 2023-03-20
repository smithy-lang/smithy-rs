/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy

import software.amazon.smithy.codegen.core.CodegenException
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

private const val DEFAULT_KEY = "DEFAULT"

/**
 * Location of the runtime crates (aws-smithy-http, aws-smithy-types etc.)
 *
 * This can be configured via the `runtimeConfig.versions` field in smithy-build.json
 */
data class RuntimeCrateLocation(val path: String?, val versions: CrateVersionMap) {
    companion object {
        fun Path(path: String) = RuntimeCrateLocation(path, CrateVersionMap(emptyMap()))
    }
}

fun RuntimeCrateLocation.crateLocation(crateName: String?): DependencyLocation {
    val version = crateName.let { versions.map[crateName] } ?: versions.map[DEFAULT_KEY]
    return when (this.path) {
        // CratesIo needs an exact version. However, for local runtime crates we do not
        // provide a detected version unless the user explicitly sets one via the `versions` map.
        null -> CratesIo(version ?: defaultRuntimeCrateVersion())
        else -> Local(this.path, version)
    }
}

fun defaultRuntimeCrateVersion(): String {
    try {
        return Version.crateVersion()
    } catch (ex: Exception) {
        throw CodegenException("failed to get crate version which sets the default client-runtime version", ex)
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
 * Prefix & crate location for the runtime crates.
 */
data class RuntimeConfig(
    val cratePrefix: String = "aws",
    val runtimeCrateLocation: RuntimeCrateLocation = RuntimeCrateLocation.Path("../"),
) {
    companion object {

        /**
         * Load a `RuntimeConfig` from an [ObjectNode] (JSON)
         */
        fun fromNode(maybeNode: Optional<ObjectNode>): RuntimeConfig {
            val node = maybeNode.orElse(Node.objectNode())
            val crateVersionMap =
                node.getObjectMember("versions").orElse(Node.objectNode()).members.entries.let { members ->
                    val map = members.associate { it.key.toString() to it.value.expectStringNode().value }
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

    val crateSrcPrefix: String = cratePrefix.replace("-", "_")

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
 * - [name]: What type is this?
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
 * `[namespace]`  `[name]`
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
    val writable = writable {
        rustInlineTemplate(
            "#{this:T}",
            "this" to this@RuntimeType,
        )
    }

    /**
     * Convert this [RuntimeType] into a [Symbol].
     *
     * This is not commonly required, but is occasionally useful when you want to force an import without referencing a type
     * (e.g. when bringing a trait into scope). See [CodegenWriter.addUseImports].
     */
    fun toSymbol(): Symbol {
        val builder = Symbol
            .builder()
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
    fun resolve(subPath: String): RuntimeType {
        return copy(path = "$path::$subPath")
    }

    /**
     * Returns the fully qualified name for this type
     */
    fun fullyQualifiedName(): String {
        return path
    }

    /**
     * The companion object contains commonly used RuntimeTypes
     */
    companion object {
        // stdlib types
        val std = RuntimeType("std")
        val stdCmp = std.resolve("cmp")
        val stdFmt = std.resolve("fmt")
        val stdConvert = std.resolve("convert")
        val AsRef = stdConvert.resolve("AsRef")
        val ByteSlab = std.resolve("vec::Vec<u8>")
        val Box = std.resolve("boxed::Box")
        val Clone = std.resolve("clone::Clone")
        val Cow = std.resolve("borrow::Cow")
        val Debug = stdFmt.resolve("Debug")
        val Default = std.resolve("default::Default")
        val Display = stdFmt.resolve("Display")
        val Eq = stdCmp.resolve("Eq")
        val From = stdConvert.resolve("From")
        val Hash = std.resolve("hash::Hash")
        val HashMap = std.resolve("collections::HashMap")
        val Ord = stdCmp.resolve("Ord")
        val Option = std.resolve("option::Option")
        val PartialEq = stdCmp.resolve("PartialEq")
        val PartialOrd = stdCmp.resolve("PartialOrd")
        val Phantom = std.resolve("marker::PhantomData")
        val StdError = std.resolve("error::Error")
        val String = std.resolve("string::String")
        val Bool = std.resolve("primitive::bool")
        val TryFrom = stdConvert.resolve("TryFrom")
        val Vec = std.resolve("vec::Vec")

        // external cargo dependency types
        val Bytes = CargoDependency.Bytes.toType().resolve("Bytes")
        val Http = CargoDependency.Http.toType()
        val HttpBody = CargoDependency.HttpBody.toType()
        val HttpHeaderMap = Http.resolve("HeaderMap")
        val HttpRequest = Http.resolve("Request")
        val HttpRequestBuilder = Http.resolve("request::Builder")
        val HttpResponse = Http.resolve("Response")
        val HttpResponseBuilder = Http.resolve("response::Builder")
        val Hyper = CargoDependency.Hyper.toType()
        val LazyStatic = CargoDependency.LazyStatic.toType()
        val Md5 = CargoDependency.Md5.toType()
        val OnceCell = CargoDependency.OnceCell.toType()
        val PercentEncoding = CargoDependency.PercentEncoding.toType()
        val PrettyAssertions = CargoDependency.PrettyAssertions.toType()
        val Regex = CargoDependency.Regex.toType()
        val Tokio = CargoDependency.Tokio.toType()
        val TokioStream = CargoDependency.TokioStream.toType()
        val Tower = CargoDependency.Tower.toType()
        val Tracing = CargoDependency.Tracing.toType()

        // codegen types
        val ConstrainedTrait = RuntimeType("crate::constrained::Constrained", InlineDependency.constrained())
        val MaybeConstrained = RuntimeType("crate::constrained::MaybeConstrained", InlineDependency.constrained())

        // smithy runtime types
        fun smithyAsync(runtimeConfig: RuntimeConfig) = CargoDependency.smithyAsync(runtimeConfig).toType()
        fun smithyChecksums(runtimeConfig: RuntimeConfig) = CargoDependency.smithyChecksums(runtimeConfig).toType()
        fun smithyClient(runtimeConfig: RuntimeConfig) = CargoDependency.smithyClient(runtimeConfig).toType()
        fun smithyClientTestUtil(runtimeConfig: RuntimeConfig) = CargoDependency.smithyClient(runtimeConfig)
            .copy(features = setOf("test-util"), scope = DependencyScope.Dev).toType()
        fun smithyEventStream(runtimeConfig: RuntimeConfig) = CargoDependency.smithyEventStream(runtimeConfig).toType()
        fun smithyHttp(runtimeConfig: RuntimeConfig) = CargoDependency.smithyHttp(runtimeConfig).toType()
        fun smithyHttpAuth(runtimeConfig: RuntimeConfig) = CargoDependency.smithyHttpAuth(runtimeConfig).toType()
        fun smithyHttpTower(runtimeConfig: RuntimeConfig) = CargoDependency.smithyHttpTower(runtimeConfig).toType()
        fun smithyJson(runtimeConfig: RuntimeConfig) = CargoDependency.smithyJson(runtimeConfig).toType()
        fun smithyQuery(runtimeConfig: RuntimeConfig) = CargoDependency.smithyQuery(runtimeConfig).toType()
        fun smithyRuntime(runtimeConfig: RuntimeConfig) = CargoDependency.smithyRuntime(runtimeConfig).toType()
        fun smithyRuntimeApi(runtimeConfig: RuntimeConfig) = CargoDependency.smithyRuntimeApi(runtimeConfig).toType()
        fun smithyTypes(runtimeConfig: RuntimeConfig) = CargoDependency.smithyTypes(runtimeConfig).toType()
        fun smithyXml(runtimeConfig: RuntimeConfig) = CargoDependency.smithyXml(runtimeConfig).toType()
        private fun smithyProtocolTest(runtimeConfig: RuntimeConfig) =
            CargoDependency.smithyProtocolTestHelpers(runtimeConfig).toType()

        // smithy runtime type members
        fun base64Decode(runtimeConfig: RuntimeConfig): RuntimeType =
            smithyTypes(runtimeConfig).resolve("base64::decode")

        fun base64Encode(runtimeConfig: RuntimeConfig): RuntimeType =
            smithyTypes(runtimeConfig).resolve("base64::encode")

        fun blob(runtimeConfig: RuntimeConfig) = smithyTypes(runtimeConfig).resolve("Blob")
        fun byteStream(runtimeConfig: RuntimeConfig) = smithyHttp(runtimeConfig).resolve("byte_stream::ByteStream")
        fun classifyRetry(runtimeConfig: RuntimeConfig) = smithyHttp(runtimeConfig).resolve("retry::ClassifyRetry")
        fun dateTime(runtimeConfig: RuntimeConfig) = smithyTypes(runtimeConfig).resolve("DateTime")
        fun document(runtimeConfig: RuntimeConfig): RuntimeType = smithyTypes(runtimeConfig).resolve("Document")
        fun retryErrorKind(runtimeConfig: RuntimeConfig) = smithyTypes(runtimeConfig).resolve("retry::ErrorKind")
        fun eventStreamReceiver(runtimeConfig: RuntimeConfig): RuntimeType = smithyHttp(runtimeConfig).resolve("event_stream::Receiver")
        fun eventStreamSender(runtimeConfig: RuntimeConfig): RuntimeType = smithyHttp(runtimeConfig).resolve("event_stream::EventStreamSender")
        fun errorMetadata(runtimeConfig: RuntimeConfig) = smithyTypes(runtimeConfig).resolve("error::ErrorMetadata")
        fun errorMetadataBuilder(runtimeConfig: RuntimeConfig) = smithyTypes(runtimeConfig).resolve("error::metadata::Builder")
        fun provideErrorMetadataTrait(runtimeConfig: RuntimeConfig) = smithyTypes(runtimeConfig).resolve("error::metadata::ProvideErrorMetadata")
        fun unhandledError(runtimeConfig: RuntimeConfig) = smithyTypes(runtimeConfig).resolve("error::Unhandled")
        fun jsonErrors(runtimeConfig: RuntimeConfig) = forInlineDependency(InlineDependency.jsonErrors(runtimeConfig))
        fun awsQueryCompatibleErrors(runtimeConfig: RuntimeConfig) = forInlineDependency(InlineDependency.awsQueryCompatibleErrors(runtimeConfig))
        fun labelFormat(runtimeConfig: RuntimeConfig, func: String) = smithyHttp(runtimeConfig).resolve("label::$func")
        fun operation(runtimeConfig: RuntimeConfig) = smithyHttp(runtimeConfig).resolve("operation::Operation")
        fun operationModule(runtimeConfig: RuntimeConfig) = smithyHttp(runtimeConfig).resolve("operation")
        fun parseHttpResponse(runtimeConfig: RuntimeConfig) =
            smithyHttp(runtimeConfig).resolve("response::ParseHttpResponse")

        fun parseStrictResponse(runtimeConfig: RuntimeConfig) =
            smithyHttp(runtimeConfig).resolve("response::ParseStrictResponse")

        fun protocolTest(runtimeConfig: RuntimeConfig, func: String): RuntimeType =
            smithyProtocolTest(runtimeConfig).resolve(func)

        fun provideErrorKind(runtimeConfig: RuntimeConfig) =
            smithyTypes(runtimeConfig).resolve("retry::ProvideErrorKind")

        fun queryFormat(runtimeConfig: RuntimeConfig, func: String) = smithyHttp(runtimeConfig).resolve("query::$func")
        fun sdkBody(runtimeConfig: RuntimeConfig): RuntimeType = smithyHttp(runtimeConfig).resolve("body::SdkBody")
        fun sdkError(runtimeConfig: RuntimeConfig): RuntimeType = smithyHttp(runtimeConfig).resolve("result::SdkError")
        fun sdkSuccess(runtimeConfig: RuntimeConfig): RuntimeType =
            smithyHttp(runtimeConfig).resolve("result::SdkSuccess")

        fun parseTimestampFormat(
            codegenTarget: CodegenTarget,
            runtimeConfig: RuntimeConfig,
            format: TimestampFormatTrait.Format,
        ): RuntimeType {
            val timestampFormat = when (format) {
                TimestampFormatTrait.Format.EPOCH_SECONDS -> "EpochSeconds"
                // clients allow offsets, servers do nt
                TimestampFormatTrait.Format.DATE_TIME -> codegenTarget.ifClient { "DateTimeWithOffset" } ?: "DateTime"
                TimestampFormatTrait.Format.HTTP_DATE -> "HttpDate"
                TimestampFormatTrait.Format.UNKNOWN -> TODO()
            }

            return smithyTypes(runtimeConfig).resolve("date_time::Format::$timestampFormat")
        }

        fun serializeTimestampFormat(
            runtimeConfig: RuntimeConfig,
            format: TimestampFormatTrait.Format,
        ): RuntimeType {
            val timestampFormat = when (format) {
                TimestampFormatTrait.Format.EPOCH_SECONDS -> "EpochSeconds"
                // clients allow offsets, servers do not
                TimestampFormatTrait.Format.DATE_TIME -> "DateTime"
                TimestampFormatTrait.Format.HTTP_DATE -> "HttpDate"
                TimestampFormatTrait.Format.UNKNOWN -> TODO()
            }

            return smithyTypes(runtimeConfig).resolve("date_time::Format::$timestampFormat")
        }

        fun captureRequest(runtimeConfig: RuntimeConfig) =
            CargoDependency.smithyClientTestUtil(runtimeConfig).toType().resolve("test_connection::capture_request")

        fun forInlineDependency(inlineDependency: InlineDependency) = RuntimeType("crate::${inlineDependency.name}", inlineDependency)

        fun forInlineFun(name: String, module: RustModule, func: Writable) = RuntimeType(
            "${module.fullyQualifiedPath()}::$name",
            dependency = InlineDependency(name, module, listOf(), func),
        )

        // inlinable types
        fun ec2QueryErrors(runtimeConfig: RuntimeConfig) =
            forInlineDependency(InlineDependency.ec2QueryErrors(runtimeConfig))

        fun wrappedXmlErrors(runtimeConfig: RuntimeConfig) =
            forInlineDependency(InlineDependency.wrappedXmlErrors(runtimeConfig))

        fun unwrappedXmlErrors(runtimeConfig: RuntimeConfig) =
            forInlineDependency(InlineDependency.unwrappedXmlErrors(runtimeConfig))

        val IdempotencyToken by lazy { forInlineDependency(InlineDependency.idempotencyToken()) }
    }
}
