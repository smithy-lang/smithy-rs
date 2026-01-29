/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.pattern.UriPattern
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.HttpHeaderTrait
import software.amazon.smithy.model.traits.HttpLabelTrait
import software.amazon.smithy.model.traits.HttpPrefixHeadersTrait
import software.amazon.smithy.model.traits.HttpQueryParamsTrait
import software.amazon.smithy.model.traits.HttpQueryTrait
import software.amazon.smithy.model.traits.HttpTrait
import software.amazon.smithy.model.traits.SensitiveTrait
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.plus
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.orNull
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import java.util.Optional

/** Models the ways status codes can be bound and sensitive. */
class StatusCodeSensitivity(private val sensitive: Boolean, runtimeConfig: RuntimeConfig) {
    private val codegenScope =
        arrayOf(
            "SmithyHttpServer" to ServerCargoDependency.smithyHttpServer(runtimeConfig).toType(),
        )

    /** Returns the type of the `MakeFmt`. */
    fun type(): Writable =
        writable {
            if (sensitive) {
                rustTemplate("#{SmithyHttpServer}::instrumentation::MakeSensitive", *codegenScope)
            } else {
                rustTemplate("#{SmithyHttpServer}::instrumentation::MakeIdentity", *codegenScope)
            }
        }

    /** Returns the setter. */
    fun setter(): Writable =
        writable {
            if (sensitive) {
                rust(".status_code()")
            }
        }
}

/** Represents the information needed to specify the position of a greedy label. */
data class GreedyLabel(
    // The segment index the greedy label.
    val segmentIndex: Int,
    // The number of characters from the end of the URI the greedy label terminates.
    val endOffset: Int,
)

/** Models the ways labels can be bound and sensitive. */
class LabelSensitivity(internal val labelIndexes: List<Int>, internal val greedyLabel: GreedyLabel?, runtimeConfig: RuntimeConfig) {
    private val codegenScope =
        arrayOf("SmithyHttpServer" to ServerCargoDependency.smithyHttpServer(runtimeConfig).toType())

    /** Returns the closure used during construction. */
    fun closure(): Writable =
        writable {
            if (labelIndexes.isNotEmpty()) {
                rustTemplate(
                    """
                    {
                        |index: usize| matches!(index, ${labelIndexes.joinToString("|")})
                    } as fn(usize) -> bool
                    """,
                    *codegenScope,
                )
            } else {
                rust("{ |_index: usize| false }  as fn(usize) -> bool")
            }
        }

    private fun hasRedactions(): Boolean = labelIndexes.isNotEmpty() || greedyLabel != null

    /** Returns the type of the `MakeFmt`. */
    fun type(): Writable =
        if (hasRedactions()) {
            writable {
                rustTemplate("#{SmithyHttpServer}::instrumentation::sensitivity::uri::MakeLabel<fn(usize) -> bool>", *codegenScope)
            }
        } else {
            writable {
                rustTemplate("#{SmithyHttpServer}::instrumentation::MakeIdentity", *codegenScope)
            }
        }

    /** Returns the value of the `GreedyLabel`. */
    private fun greedyLabelStruct(): Writable =
        writable {
            if (greedyLabel != null) {
                rustTemplate(
                    """
                Some(#{SmithyHttpServer}::instrumentation::sensitivity::uri::GreedyLabel::new(${greedyLabel.segmentIndex}, ${greedyLabel.endOffset}))""",
                    *codegenScope,
                )
            } else {
                rust("None")
            }
        }

    /** Returns the setter enclosing the closure or suffix position. */
    fun setter(): Writable =
        if (hasRedactions()) {
            writable {
                rustTemplate(".label(#{Closure:W}, #{GreedyLabel:W})", "Closure" to closure(), "GreedyLabel" to greedyLabelStruct())
            }
        } else {
            writable { }
        }
}

/** Models the ways headers can be bound and sensitive */
sealed class HeaderSensitivity(
    /** The values of the sensitive `httpHeaders`. */
    val headerKeys: List<String>,
    runtimeConfig: RuntimeConfig,
) {
    private val codegenScope =
        arrayOf(
            "SmithyHttpServer" to ServerCargoDependency.smithyHttpServer(runtimeConfig).toType(),
            "Http" to RuntimeType.http(runtimeConfig),
        )

    /** The case where `prefixHeaders` value is not sensitive. */
    class NotSensitiveMapValue(
        headerKeys: List<String>,
        /** The value of `prefixHeaders`, null if it's not sensitive. */
        val prefixHeader: String?,
        runtimeConfig: RuntimeConfig,
    ) : HeaderSensitivity(headerKeys, runtimeConfig)

    /** The case where `prefixHeaders` value is sensitive. */
    class SensitiveMapValue(
        headerKeys: List<String>,
        /** Is the `prefixHeaders` key sensitive? */
        val keySensitive: Boolean,
        /** The value of `prefixHeaders`. */
        val prefixHeader: String,
        runtimeConfig: RuntimeConfig,
    ) : HeaderSensitivity(headerKeys, runtimeConfig)

    /** Is there anything to redact? */
    internal fun hasRedactions(): Boolean =
        headerKeys.isNotEmpty() ||
            when (this) {
                is NotSensitiveMapValue -> prefixHeader != null
                is SensitiveMapValue -> true
            }

    /** Returns the type of the `MakeDebug`. */
    fun type(): Writable =
        writable {
            if (hasRedactions()) {
                rustTemplate("#{SmithyHttpServer}::instrumentation::sensitivity::headers::MakeHeaders<fn(&#{Http}::header::HeaderName) -> #{SmithyHttpServer}::instrumentation::sensitivity::headers::HeaderMarker>", *codegenScope)
            } else {
                rustTemplate("#{SmithyHttpServer}::instrumentation::MakeIdentity", *codegenScope)
            }
        }

    /** Returns the closure used during construction. */
    internal fun closure(): Writable {
        val nameMatch =
            if (headerKeys.isEmpty()) {
                writable {
                    rust("false")
                }
            } else {
                writable {
                    val matches = headerKeys.joinToString("|") { it.dq() }
                    rust("matches!(name.as_str(), $matches)")
                }
            }

        val suffixAndValue =
            when (this) {
                is NotSensitiveMapValue ->
                    writable {
                        prefixHeader?.let {
                            rust(
                                """
                                let starts_with = name.as_str().starts_with("$it");
                                let key_suffix = if starts_with { Some(${it.length}) } else { None };
                                """,
                            )
                        } ?: rust("let key_suffix = None;")
                        rust("let value = name_match;")
                    }
                is SensitiveMapValue ->
                    writable {
                        rust("let starts_with = name.as_str().starts_with(${prefixHeader.dq()});")
                        if (keySensitive) {
                            rust("let key_suffix = if starts_with { Some(${prefixHeader.length}) } else { None };")
                        } else {
                            rust("let key_suffix = None;")
                        }
                        rust("let value = name_match || starts_with;")
                    }
            }

        return writable {
            rustTemplate(
                """
                {
                    |name: &#{Http}::header::HeaderName| {
                        let name_match = #{NameMatch:W};
                        #{SuffixAndValue:W}
                        #{SmithyHttpServer}::instrumentation::sensitivity::headers::HeaderMarker { key_suffix, value }
                    }
                } as fn(&_) -> _
                """,
                "NameMatch" to nameMatch,
                "SuffixAndValue" to suffixAndValue,
                *codegenScope,
            )
        }
    }

    /** Returns the setter enclosing the closure. */
    fun setter(): Writable =
        writable {
            if (hasRedactions()) {
                rustTemplate(".header(#{Closure:W})", "Closure" to closure())
            }
        }
}

/** Models the ways query strings can be bound and sensitive. */
sealed class QuerySensitivity(
    /** Are all keys sensitive? */
    val allKeysSensitive: Boolean,
    runtimeConfig: RuntimeConfig,
) {
    private val codegenScope =
        arrayOf("SmithyHttpServer" to ServerCargoDependency.smithyHttpServer(runtimeConfig).toType())

    /** The case where the `httpQueryParams` value is not sensitive. */
    class NotSensitiveMapValue(
        /** The values of `httpQuery`. */
        val queryKeys: List<String>,
        allKeysSensitive: Boolean, runtimeConfig: RuntimeConfig,
    ) : QuerySensitivity(allKeysSensitive, runtimeConfig)

    /** The case where `httpQueryParams` value is sensitive. */
    class SensitiveMapValue(allKeysSensitive: Boolean, runtimeConfig: RuntimeConfig) : QuerySensitivity(allKeysSensitive, runtimeConfig)

    /** Is there anything to redact? */
    internal fun hasRedactions(): Boolean =
        when (this) {
            is NotSensitiveMapValue -> allKeysSensitive || queryKeys.isNotEmpty()
            is SensitiveMapValue -> true
        }

    /** Returns the type of the `MakeFmt`. */
    fun type(): Writable =
        writable {
            if (hasRedactions()) {
                rustTemplate("#{SmithyHttpServer}::instrumentation::sensitivity::uri::MakeQuery<fn(&str) -> #{SmithyHttpServer}::instrumentation::sensitivity::uri::QueryMarker>", *codegenScope)
            } else {
                rustTemplate("#{SmithyHttpServer}::instrumentation::MakeIdentity", *codegenScope)
            }
        }

    /** Returns the closure used during construction. */
    internal fun closure(): Writable {
        val value =
            when (this) {
                is SensitiveMapValue ->
                    writable {
                        rust("true")
                    }
                is NotSensitiveMapValue ->
                    if (queryKeys.isEmpty()) {
                        writable {
                            rust("false;")
                        }
                    } else {
                        writable {
                            val matches = queryKeys.joinToString("|") { it.dq() }
                            rust("matches!(name, $matches);")
                        }
                    }
            }

        return writable {
            rustTemplate(
                """
                {
                    |name: &str| {
                        let key = $allKeysSensitive;
                        let value = #{Value:W};
                        #{SmithyHttpServer}::instrumentation::sensitivity::uri::QueryMarker { key, value }
                    }
                } as fn(&_) -> _
                """,
                "Value" to value,
                *codegenScope,
            )
        }
    }

    /** Returns the setter enclosing the closure. */
    fun setters(): Writable =
        writable {
            if (hasRedactions()) {
                rustTemplate(".query(#{Closure:W})", "Closure" to closure())
            }
        }
}

/** Represents a `RequestFmt` or `ResponseFmt` type and value. */
data class MakeFmt(
    val type: Writable,
    val value: Writable,
)

/**
 * A code generator responsible for using a `Model` and a chosen `OperationShape` to produce Rust closures marking
 * parts of the request/response HTTP as sensitive.
 *
 * These closures are provided to `RequestFmt` and `ResponseFmt` constructors, which in turn are provided to
 * `InstrumentedOperation` to configure logging. These structures can be found in `aws_smithy_http_server::instrumentation`.
 *
 * See [Logging in the Presence of Sensitive Data](https://github.com/smithy-lang/smithy-rs/blob/main/design/src/rfcs/rfc0018_logging_sensitive.md)
 * for more details.
 */
class ServerHttpSensitivityGenerator(
    private val model: Model,
    private val operation: OperationShape,
    private val runtimeConfig: RuntimeConfig,
) {
    private val codegenScope =
        arrayOf(
            "SmithyHttpServer" to ServerCargoDependency.smithyHttpServer(runtimeConfig).toType(),
            "Http" to RuntimeType.http(runtimeConfig),
        )

    /** Constructs `StatusCodeSensitivity` of a `Shape` */
    private fun findStatusCodeSensitivity(rootShape: Shape): StatusCodeSensitivity {
        // Is root shape sensitive?
        val rootSensitive = rootShape.hasTrait<SensitiveTrait>()

        // Find all sensitive `httpResponseCode` bindings in the `rootShape`.
        val isSensitive =
            rootShape
                .members()
                .filter { it.hasTrait<HttpHeaderTrait>() }
                .any { rootSensitive || it.getMemberTrait(model, SensitiveTrait::class.java).isPresent }

        return StatusCodeSensitivity(isSensitive, runtimeConfig)
    }

    /** Constructs `HeaderSensitivity` of a `Shape` */
    internal fun findHeaderSensitivity(rootShape: Shape): HeaderSensitivity {
        // Find all `httpHeader` bindings in the `rootShape`.
        val headerKeys = rootShape.members().mapNotNull { member -> member.getTrait<HttpHeaderTrait>()?.let { trait -> Pair(member, trait.value) } }.distinct()

        // Find `httpPrefixHeaders` trait.
        val prefixHeader = rootShape.members().mapNotNull { member -> member.getTrait<HttpPrefixHeadersTrait>()?.let { trait -> Pair(member, trait.value) } }.singleOrNull()

        // Is `rootShape` sensitive and does `httpPrefixHeaders` exist?
        val rootSensitive = rootShape.hasTrait<SensitiveTrait>()
        if (rootSensitive) {
            if (prefixHeader != null) {
                return HeaderSensitivity.SensitiveMapValue(
                    headerKeys.map { it.second }, true,
                    prefixHeader.second, runtimeConfig,
                )
            }
        }

        // Which headers are sensitive?
        val sensitiveHeaders =
            headerKeys
                .filter { (member, _) -> rootSensitive || member.getMemberTrait(model, SensitiveTrait::class.java).orNull() != null }
                .map { (_, name) -> name }

        return if (prefixHeader != null) {
            // Get the `httpPrefixHeader` map.
            val prefixHeaderMap = model.getShape(prefixHeader.first.target).orNull()?.asMapShape()?.orNull()

            // Check whether key and value are sensitive.
            val isKeySensitive = prefixHeaderMap?.key?.getMemberTrait(model, SensitiveTrait::class.java)?.orNull() != null
            val isValueSensitive = prefixHeaderMap?.value?.getMemberTrait(model, SensitiveTrait::class.java)?.orNull() != null

            if (isValueSensitive) {
                HeaderSensitivity.SensitiveMapValue(sensitiveHeaders, isKeySensitive, prefixHeader.second, runtimeConfig)
            } else if (isKeySensitive) {
                HeaderSensitivity.NotSensitiveMapValue(sensitiveHeaders, prefixHeader.second, runtimeConfig)
            } else {
                HeaderSensitivity.NotSensitiveMapValue(sensitiveHeaders, null, runtimeConfig)
            }
        } else {
            HeaderSensitivity.NotSensitiveMapValue(sensitiveHeaders, null, runtimeConfig)
        }
    }

    /** Constructs `QuerySensitivity` of a `Shape` */
    internal fun findQuerySensitivity(rootShape: Shape): QuerySensitivity {
        // Find `httpQueryParams` trait.
        val queryParams = rootShape.members().singleOrNull { member -> member.hasTrait<HttpQueryParamsTrait>() }

        // Is `rootShape` sensitive and does `httpQueryParams` exist?
        val rootSensitive = rootShape.hasTrait<SensitiveTrait>()
        if (rootSensitive) {
            if (queryParams != null) {
                return QuerySensitivity.SensitiveMapValue(true, runtimeConfig)
            }
        }

        // Find all `httpQuery` bindings in the `rootShape`.
        val queryKeys = rootShape.members().mapNotNull { member -> member.getTrait<HttpQueryTrait>()?.let { trait -> Pair(member, trait.value) } }.distinct()

        // Which queries are sensitive?
        val sensitiveQueries =
            queryKeys
                .filter { (member, _) -> rootSensitive || member.getMemberTrait(model, SensitiveTrait::class.java).orNull() != null }
                .map { (_, name) -> name }

        return if (queryParams != null) {
            // Get the `httpQueryParams` map.
            val queryParamsMap = model.getShape(queryParams.target).orNull()?.asMapShape()?.orNull()

            // Check whether key and value are sensitive.
            val isKeySensitive = queryParamsMap?.key?.getMemberTrait(model, SensitiveTrait::class.java)?.orNull() != null
            val isValueSensitive = queryParamsMap?.value?.getMemberTrait(model, SensitiveTrait::class.java)?.orNull() != null

            if (isValueSensitive) {
                QuerySensitivity.SensitiveMapValue(isKeySensitive, runtimeConfig)
            } else {
                QuerySensitivity.NotSensitiveMapValue(sensitiveQueries, isKeySensitive, runtimeConfig)
            }
        } else {
            QuerySensitivity.NotSensitiveMapValue(sensitiveQueries, false, runtimeConfig)
        }
    }

    /** Constructs `LabelSensitivity` of a `Shape` */
    internal fun findLabelSensitivity(
        uriPattern: UriPattern,
        rootShape: Shape,
    ): LabelSensitivity {
        // Is root shape sensitive?
        val rootSensitive = rootShape.hasTrait<SensitiveTrait>()

        // Find `httpLabel` trait which are also sensitive.
        val httpLabels =
            rootShape
                .members()
                .filter { it.hasTrait<HttpLabelTrait>() }
                .filter { rootSensitive || it.getMemberTrait(model, SensitiveTrait::class.java).orNull() != null }

        val labelIndexes =
            httpLabels
                .mapNotNull { member ->
                    uriPattern
                        .segments
                        .withIndex()
                        .find { (_, segment) ->
                            segment.isLabel && !segment.isGreedyLabel && segment.content == member.memberName
                        }
                }
                .map { (index, _) -> index }

        val greedyLabel =
            httpLabels.mapNotNull { member ->
                uriPattern
                    .segments
                    .withIndex()
                    .find { (_, segment) -> segment.isGreedyLabel && segment.content == member.memberName }
            }
                .singleOrNull()
                ?.let { (index, _) ->
                    val remainder =
                        uriPattern
                            .segments
                            .drop(index + 1)
                            .sumOf { it.content.length + 1 }
                    GreedyLabel(index, remainder)
                }

        return LabelSensitivity(labelIndexes, greedyLabel, runtimeConfig)
    }

    private fun getShape(shape: Optional<ShapeId>): Shape? {
        return shape.orElse(null)?.let { model.getShape(it).orElse(null) }
    }

    internal fun input(): Shape? {
        return getShape(operation.input)
    }

    internal fun output(): Shape? {
        return getShape(operation.output)
    }

    private fun defaultRequestFmt(): MakeFmt {
        val type =
            writable {
                rustTemplate("#{SmithyHttpServer}::instrumentation::sensitivity::DefaultRequestFmt", *codegenScope)
            }
        val value =
            writable {
                rustTemplate("#{SmithyHttpServer}::instrumentation::sensitivity::RequestFmt::new()", *codegenScope)
            }
        return MakeFmt(type, value)
    }

    fun requestFmt(): MakeFmt {
        // Sensitivity only applies when http trait is applied to the operation
        val httpTrait = operation.getTrait<HttpTrait>() ?: return defaultRequestFmt()
        val inputShape = input() ?: return defaultRequestFmt()

        // httpHeader/httpPrefixHeaders bindings
        val headerSensitivity = findHeaderSensitivity(inputShape)

        // httpLabel bindings
        val labelSensitivity = findLabelSensitivity(httpTrait.uri, inputShape)

        // httpQuery/httpQueryParams bindings
        val querySensitivity = findQuerySensitivity(inputShape)

        val type =
            writable {
                rustTemplate(
                    """
                    #{SmithyHttpServer}::instrumentation::sensitivity::RequestFmt<
                        #{HeaderType:W},
                        #{SmithyHttpServer}::instrumentation::sensitivity::uri::MakeUri<
                            #{LabelType:W},
                            #{QueryType:W}
                        >
                    >
                    """,
                    "HeaderType" to headerSensitivity.type(),
                    "LabelType" to labelSensitivity.type(),
                    "QueryType" to querySensitivity.type(),
                    *codegenScope,
                )
            }

        val value =
            writable {
                rustTemplate("#{SmithyHttpServer}::instrumentation::sensitivity::RequestFmt::new()", *codegenScope)
            } + headerSensitivity.setter() + labelSensitivity.setter() + querySensitivity.setters()

        return MakeFmt(type, value)
    }

    private fun defaultResponseFmt(): MakeFmt {
        val type =
            writable {
                rustTemplate("#{SmithyHttpServer}::instrumentation::sensitivity::DefaultResponseFmt", *codegenScope)
            }

        val value =
            writable {
                rustTemplate("#{SmithyHttpServer}::instrumentation::sensitivity::ResponseFmt::new()", *codegenScope)
            }
        return MakeFmt(type, value)
    }

    fun responseFmt(): MakeFmt {
        // Sensitivity only applies when HTTP trait is applied to the operation
        operation.getTrait<HttpTrait>() ?: return defaultResponseFmt()
        val outputShape = output() ?: return defaultResponseFmt()

        // httpHeader/httpPrefixHeaders bindings
        val headerSensitivity = findHeaderSensitivity(outputShape)

        // Status code bindings
        val statusCodeSensitivity = findStatusCodeSensitivity(outputShape)

        val type =
            writable {
                rustTemplate(
                    "#{SmithyHttpServer}::instrumentation::sensitivity::ResponseFmt<#{HeaderType:W}, #{StatusType:W}>",
                    "HeaderType" to headerSensitivity.type(),
                    "StatusType" to statusCodeSensitivity.type(),
                    *codegenScope,
                )
            }

        val value =
            writable {
                rustTemplate("#{SmithyHttpServer}::instrumentation::sensitivity::ResponseFmt::new()", *codegenScope)
            } + headerSensitivity.setter() + statusCodeSensitivity.setter()

        return MakeFmt(type, value)
    }
}
