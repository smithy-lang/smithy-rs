/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.CratesIo
import software.amazon.smithy.rust.codegen.rustlang.DependencyLocation
import software.amazon.smithy.rust.codegen.rustlang.InlineDependency
import software.amazon.smithy.rust.codegen.rustlang.Local
import software.amazon.smithy.rust.codegen.rustlang.RustDependency
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.util.orNull
import java.util.Optional

/**
 * Location of the runtime crates (aws-smithy-http, aws-smithy-types etc.)
 *
 * This can be configured via the `runtimeConfig.version` field in smithy-build.json
 */
data class RuntimeCrateLocation(val path: String?, val version: String?) {
    init {
        check(path != null || version != null) {
            "path ($path) or version ($version) must not be null"
        }
    }

    companion object {
        fun Path(path: String) = RuntimeCrateLocation(path, null)
    }
}

fun RuntimeCrateLocation.crateLocation(): DependencyLocation = when (this.path) {
    null -> CratesIo(this.version!!)
    else -> Local(this.path, this.version)
}

fun defaultRuntimeCrateVersion(): String {
    // generated as part of the build, see codegen/build.gradle.kts
    try {
        return object {}.javaClass.getResource("runtime-crate-version.txt")?.readText()
            ?: throw CodegenException("sdk-version.txt does not exist")
    } catch (ex: Exception) {
        throw CodegenException("failed to load sdk-version.txt which sets the default client-runtime version", ex)
    }
}

/**
 * Prefix & crate location for the runtime crates.
 */
data class RuntimeConfig(
    val cratePrefix: String = "aws-smithy",
    val runtimeCrateLocation: RuntimeCrateLocation = RuntimeCrateLocation.Path("../")
) {
    companion object {

        /**
         * Load a `RuntimeConfig` from an [ObjectNode] (JSON)
         */
        fun fromNode(node: Optional<ObjectNode>): RuntimeConfig {
            return if (node.isPresent) {
                val resolvedVersion = when (val configuredVersion = node.get().getStringMember("version").orNull()?.value) {
                    "DEFAULT" -> defaultRuntimeCrateVersion()
                    null -> null
                    else -> configuredVersion
                }
                val path = node.get().getStringMember("relativePath").orNull()?.value
                val runtimeCrateLocation = RuntimeCrateLocation(path = path, version = resolvedVersion)
                RuntimeConfig(
                    node.get().getStringMemberOrDefault("cratePrefix", "aws-smithy"),
                    runtimeCrateLocation = runtimeCrateLocation
                )
            } else {
                RuntimeConfig()
            }
        }
    }

    val crateSrcPrefix: String = cratePrefix.replace("-", "_")

    fun runtimeCrate(runtimeCrateName: String, optional: Boolean = false): CargoDependency =
        CargoDependency("$cratePrefix-$runtimeCrateName", runtimeCrateLocation.crateLocation(), optional = optional)
}

/**
 * `RuntimeType` captures all necessary information to render a type into a Rust file:
 * - [name]: What type is this?
 * - [dependency]: What other crates, if any, are required to use this type?
 * - [namespace]: Where can we find this type.
 *
 * For example:
 *
 * `http::header::HeaderName`
 *  ------------  ----------
 *      |           |
 *  [namespace]   [name]
 *
 *  This type would have a [CargoDependency] pointing to the `http` crate.
 *
 *  By grouping all of this information, when we render a type into a [RustWriter], we can not only render a fully qualified
 *  name, but also ensure that we automatically add any dependencies **as they are used**.
 */
data class RuntimeType(val name: String?, val dependency: RustDependency?, val namespace: String) {
    /**
     * Convert this [RuntimeType] into a [Symbol].
     *
     * This is not commonly required, but is occasionally useful when you want to force an import without referencing a type
     * (e.g. when bringing a trait into scope). See [CodegenWriter.addUseImports].
     */
    fun toSymbol(): Symbol {
        val builder = Symbol.builder().name(name).namespace(namespace, "::")
            .rustType(RustType.Opaque(name ?: "", namespace = namespace))

        dependency?.run { builder.addDependency(this) }
        return builder.build()
    }

    /**
     * Create a new [RuntimeType] with a nested name.
     *
     * # Example
     * ```kotlin
     * val http = CargoDependency.http.member("Request")
     * ```
     */
    fun member(member: String): RuntimeType {
        val newName = name?.let { "$name::$member" } ?: member
        return copy(name = newName)
    }

    /**
     * Returns the fully qualified name for this type
     */
    fun fullyQualifiedName(): String {
        val postFix = name?.let { "::$name" } ?: ""
        return "$namespace$postFix"
    }

    /**
     * The companion object contains commonly used RuntimeTypes
     */
    companion object {
        fun errorKind(runtimeConfig: RuntimeConfig) = RuntimeType(
            "ErrorKind",
            dependency = CargoDependency.SmithyTypes(runtimeConfig),
            namespace = "${runtimeConfig.crateSrcPrefix}_types::retry"
        )

        fun provideErrorKind(runtimeConfig: RuntimeConfig) = RuntimeType(
            "ProvideErrorKind",
            dependency = CargoDependency.SmithyTypes(runtimeConfig),
            namespace = "${runtimeConfig.crateSrcPrefix}_types::retry"
        )

        val std = RuntimeType(null, dependency = null, namespace = "std")
        val stdfmt = std.member("fmt")

        val AsRef = RuntimeType("AsRef", dependency = null, namespace = "std::convert")
        val ByteSlab = RuntimeType("Vec<u8>", dependency = null, namespace = "std::vec")
        val Clone = std.member("clone::Clone")
        val Debug = stdfmt.member("Debug")
        val Default: RuntimeType = RuntimeType("Default", dependency = null, namespace = "std::default")
        val From = RuntimeType("From", dependency = null, namespace = "std::convert")
        val Infallible = RuntimeType("Infallible", dependency = null, namespace = "std::convert")
        val PartialEq = std.member("cmp::PartialEq")
        val StdError = RuntimeType("Error", dependency = null, namespace = "std::error")
        val String = RuntimeType("String", dependency = null, namespace = "std::string")

        fun DateTime(runtimeConfig: RuntimeConfig) =
            RuntimeType("DateTime", CargoDependency.SmithyTypes(runtimeConfig), "${runtimeConfig.crateSrcPrefix}_types")

        fun GenericError(runtimeConfig: RuntimeConfig) =
            RuntimeType("Error", CargoDependency.SmithyTypes(runtimeConfig), "${runtimeConfig.crateSrcPrefix}_types")

        fun Blob(runtimeConfig: RuntimeConfig) =
            RuntimeType("Blob", CargoDependency.SmithyTypes(runtimeConfig), "${runtimeConfig.crateSrcPrefix}_types")

        fun Document(runtimeConfig: RuntimeConfig): RuntimeType =
            RuntimeType("Document", CargoDependency.SmithyTypes(runtimeConfig), "${runtimeConfig.crateSrcPrefix}_types")

        fun LabelFormat(runtimeConfig: RuntimeConfig, func: String) =
            RuntimeType(func, CargoDependency.SmithyHttp(runtimeConfig), "${runtimeConfig.crateSrcPrefix}_http::label")

        fun QueryFormat(runtimeConfig: RuntimeConfig, func: String) =
            RuntimeType(func, CargoDependency.SmithyHttp(runtimeConfig), "${runtimeConfig.crateSrcPrefix}_http::query")

        fun Base64Encode(runtimeConfig: RuntimeConfig): RuntimeType =
            RuntimeType(
                "encode",
                CargoDependency.SmithyTypes(runtimeConfig),
                "${runtimeConfig.crateSrcPrefix}_types::base64"
            )

        fun Base64Decode(runtimeConfig: RuntimeConfig): RuntimeType =
            RuntimeType(
                "decode",
                CargoDependency.SmithyTypes(runtimeConfig),
                "${runtimeConfig.crateSrcPrefix}_types::base64"
            )

        fun TimestampFormat(runtimeConfig: RuntimeConfig, format: TimestampFormatTrait.Format): RuntimeType {
            val timestampFormat = when (format) {
                TimestampFormatTrait.Format.EPOCH_SECONDS -> "EpochSeconds"
                TimestampFormatTrait.Format.DATE_TIME -> "DateTime"
                TimestampFormatTrait.Format.HTTP_DATE -> "HttpDate"
                TimestampFormatTrait.Format.UNKNOWN -> TODO()
            }
            return RuntimeType(
                timestampFormat,
                CargoDependency.SmithyTypes(runtimeConfig),
                "${runtimeConfig.crateSrcPrefix}_types::date_time::Format"
            )
        }

        fun ProtocolTestHelper(runtimeConfig: RuntimeConfig, func: String): RuntimeType =
            RuntimeType(
                func, CargoDependency.SmithyProtocolTestHelpers(runtimeConfig), "aws_smithy_protocol_test"
            )

        val http = CargoDependency.Http.asType()
        fun Http(path: String): RuntimeType =
            RuntimeType(name = path, dependency = CargoDependency.Http, namespace = "http")

        val HttpRequestBuilder = Http("request::Builder")
        val HttpResponseBuilder = Http("response::Builder")

        val Hyper = CargoDependency.Hyper.asType()

        fun eventStreamReceiver(runtimeConfig: RuntimeConfig): RuntimeType =
            RuntimeType(
                "Receiver",
                dependency = CargoDependency.SmithyHttp(runtimeConfig),
                "aws_smithy_http::event_stream"
            )

        fun jsonErrors(runtimeConfig: RuntimeConfig) =
            forInlineDependency(InlineDependency.jsonErrors(runtimeConfig))

        val IdempotencyToken by lazy { forInlineDependency(InlineDependency.idempotencyToken()) }

        val Config = RuntimeType("config", null, "crate")

        fun operation(runtimeConfig: RuntimeConfig) = RuntimeType(
            "Operation",
            dependency = CargoDependency.SmithyHttp(runtimeConfig),
            namespace = "aws_smithy_http::operation"
        )

        fun operationModule(runtimeConfig: RuntimeConfig) = RuntimeType(
            null,
            dependency = CargoDependency.SmithyHttp(runtimeConfig),
            namespace = "aws_smithy_http::operation"
        )

        fun sdkBody(runtimeConfig: RuntimeConfig): RuntimeType =
            RuntimeType("SdkBody", dependency = CargoDependency.SmithyHttp(runtimeConfig), "aws_smithy_http::body")

        fun parseStrictResponse(runtimeConfig: RuntimeConfig) = RuntimeType(
            "ParseStrictResponse",
            dependency = CargoDependency.SmithyHttp(runtimeConfig),
            namespace = "aws_smithy_http::response"
        )

        val Bytes = RuntimeType("Bytes", dependency = CargoDependency.Bytes, namespace = "bytes")

        fun forInlineDependency(inlineDependency: InlineDependency) =
            RuntimeType(inlineDependency.name, inlineDependency, namespace = "crate")

        fun forInlineFun(name: String, module: RustModule, func: (RustWriter) -> Unit) = RuntimeType(
            name = name,
            dependency = InlineDependency(name, module, listOf(), func),
            namespace = "crate::${module.name}"
        )

        fun byteStream(runtimeConfig: RuntimeConfig) =
            CargoDependency.SmithyHttp(runtimeConfig).asType().member("byte_stream::ByteStream")

        fun parseResponse(runtimeConfig: RuntimeConfig) = RuntimeType(
            "ParseHttpResponse",
            dependency = CargoDependency.SmithyHttp(runtimeConfig),
            namespace = "aws_smithy_http::response"
        )

        fun ec2QueryErrors(runtimeConfig: RuntimeConfig) =
            forInlineDependency(InlineDependency.ec2QueryErrors(runtimeConfig))

        fun wrappedXmlErrors(runtimeConfig: RuntimeConfig) =
            forInlineDependency(InlineDependency.wrappedXmlErrors(runtimeConfig))

        fun unwrappedXmlErrors(runtimeConfig: RuntimeConfig) =
            forInlineDependency(InlineDependency.unwrappedXmlErrors(runtimeConfig))
    }
}
