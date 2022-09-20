/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.neighbor.RelationshipDirection
import software.amazon.smithy.model.neighbor.Walker
import software.amazon.smithy.model.pattern.UriPattern
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.HttpHeaderTrait
import software.amazon.smithy.model.traits.HttpLabelTrait
import software.amazon.smithy.model.traits.HttpPrefixHeadersTrait
import software.amazon.smithy.model.traits.HttpQueryParamsTrait
import software.amazon.smithy.model.traits.HttpQueryTrait
import software.amazon.smithy.model.traits.HttpResponseCodeTrait
import software.amazon.smithy.model.traits.HttpTrait
import software.amazon.smithy.model.traits.SensitiveTrait
import software.amazon.smithy.model.traits.Trait
import software.amazon.smithy.rust.codegen.client.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.client.rustlang.Writable
import software.amazon.smithy.rust.codegen.client.rustlang.asType
import software.amazon.smithy.rust.codegen.client.rustlang.plus
import software.amazon.smithy.rust.codegen.client.rustlang.rust
import software.amazon.smithy.rust.codegen.client.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.client.rustlang.writable
import software.amazon.smithy.rust.codegen.client.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import java.util.*

/** Models the ways status codes can be bound and sensitive. */
class StatusCodeSensitivity(private val sensitive: Boolean, runtimeConfig: RuntimeConfig) {
    private val codegenScope = arrayOf(
        "SmithyHttpServer" to ServerCargoDependency.SmithyHttpServer(runtimeConfig).asType(),
    )

    /** Returns the type of the `MakeFmt`. */
    fun type(): Writable = writable {
        if (sensitive) {
            rustTemplate("#{SmithyHttpServer}::logging::MakeSensitive", *codegenScope)
        } else {
            rustTemplate("#{SmithyHttpServer}::logging::MakeIdentity", *codegenScope)
        }
    }

    /** Returns the setter. */
    fun setter(): Writable = writable {
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
    private val codegenScope = arrayOf("SmithyHttpServer" to ServerCargoDependency.SmithyHttpServer(runtimeConfig).asType())

    /** Returns the closure used during construction. */
    fun closure(): Writable = writable {
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
    fun type(): Writable = if (hasRedactions()) writable {
        rustTemplate("#{SmithyHttpServer}::logging::sensitivity::uri::MakeLabel<fn(usize) -> bool>", *codegenScope)
    } else writable {
        rustTemplate("#{SmithyHttpServer}::logging::MakeIdentity", *codegenScope)
    }

    /** Returns the value of the `GreedyLabel`. */
    private fun greedyLabelStruct(): Writable = writable {
        if (greedyLabel != null) {
            rustTemplate(
                """
                Some(#{SmithyHttpServer}::logging::sensitivity::uri::GreedyLabel::new(${greedyLabel.segmentIndex}, ${greedyLabel.endOffset}))""",
                *codegenScope,
            )
        } else {
            rust("None")
        }
    }

    /** Returns the setter enclosing the closure or suffix position. */
    fun setter(): Writable = if (hasRedactions()) writable {
        rustTemplate(".label(#{Closure:W}, #{GreedyLabel:W})", "Closure" to closure(), "GreedyLabel" to greedyLabelStruct())
    } else writable { }
}

/** Models the ways headers can be bound and sensitive */
sealed class HeaderSensitivity(
    // The values of [trait|sensitive] ~> [trait|httpHeader]
    val headerKeys: List<String>,
    runtimeConfig: RuntimeConfig,
) {
    private val codegenScope = arrayOf(
        "SmithyHttpServer" to ServerCargoDependency.SmithyHttpServer(runtimeConfig).asType(),
        "Http" to CargoDependency.Http.asType(),
    )

    // The case where map[trait|httpPrefixHeaders] > [id|member = value] is not sensitive
    class NotSensitiveMapValue(
        headerKeys: List<String>,
        // The value of map[trait|httpPrefixHeaders] > [id|member = key], null if it's not sensitive
        val prefixHeader: String?,
        runtimeConfig: RuntimeConfig,
    ) : HeaderSensitivity(headerKeys, runtimeConfig)

    // The case where map[trait|httpPrefixHeaders] > [id|member = value] is sensitive
    class SensitiveMapValue(
        headerKeys: List<String>,
        // Is map[trait|httpQueryParams] > [id|member = key] sensitive?
        val keySensitive: Boolean,
        // What is the value of map[trait|httpQueryParams]?
        val prefixHeader: String,
        runtimeConfig: RuntimeConfig,
    ) : HeaderSensitivity(headerKeys, runtimeConfig)

    /** Is there anything to redact? */
    internal fun hasRedactions(): Boolean = headerKeys.isNotEmpty() || when (this) {
        is NotSensitiveMapValue -> prefixHeader != null
        is SensitiveMapValue -> true
    }

    /** Returns the type of the `MakeDebug`. */
    fun type(): Writable = writable {
        if (hasRedactions()) {
            rustTemplate("#{SmithyHttpServer}::logging::sensitivity::headers::MakeHeaders<fn(&#{Http}::header::HeaderName) -> #{SmithyHttpServer}::logging::sensitivity::headers::HeaderMarker>", *codegenScope)
        } else {
            rustTemplate("#{SmithyHttpServer}::logging::MakeIdentity", *codegenScope)
        }
    }

    /** Returns the closure used during construction. */
    internal fun closure(): Writable {
        val nameMatch = if (headerKeys.isEmpty()) writable {
            rust("false")
        } else writable {
            val matches = headerKeys.joinToString("|") { it.dq() }
            rust("matches!(name, $matches)")
        }

        val suffixAndValue = when (this) {
            is NotSensitiveMapValue -> writable {
                prefixHeader?.let {
                    rust(
                        """
                        let starts_with = name.starts_with("$it");
                        let key_suffix = if starts_with { Some(${it.length}) } else { None };
                        """,
                    )
                } ?: rust("let key_suffix = None;")
                rust("let value = name_match;")
            }
            is SensitiveMapValue -> writable {
                rust("let starts_with = name.starts_with(${prefixHeader.dq()});")
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
                        ##[allow(unused_variables)]
                        let name = name.as_str();
                        let name_match = #{NameMatch:W};

                        #{SuffixAndValue:W}
                        #{SmithyHttpServer}::logging::sensitivity::headers::HeaderMarker { key_suffix, value }
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
    fun setter(): Writable = writable {
        if (hasRedactions()) {
            rustTemplate(".header(#{Closure:W})", "Closure" to closure())
        }
    }
}

/** Models the ways query strings can be bound and sensitive. */
sealed class QuerySensitivity(
    // Are all keys sensitive?
    val allKeysSensitive: Boolean,
    runtimeConfig: RuntimeConfig,
) {
    private val codegenScope = arrayOf("SmithyHttpServer" to ServerCargoDependency.SmithyHttpServer(runtimeConfig).asType())

    // The case where map[trait|httpQueryParams] > [id|member = value] is not sensitive
    class NotSensitiveMapValue(
        // The values of [trait|sensitive] ~> [trait|httpQuery]
        val queryKeys: List<String>,
        allKeysSensitive: Boolean, runtimeConfig: RuntimeConfig,
    ) : QuerySensitivity(allKeysSensitive, runtimeConfig)

    // The case where map[trait|httpQueryParams] > [id|member = value] is sensitive
    class SensitiveMapValue(allKeysSensitive: Boolean, runtimeConfig: RuntimeConfig) : QuerySensitivity(allKeysSensitive, runtimeConfig)

    /** Is there anything to redact? */
    internal fun hasRedactions(): Boolean = when (this) {
        is NotSensitiveMapValue -> {
            allKeysSensitive || queryKeys.isNotEmpty()
        }
        is SensitiveMapValue -> {
            true
        }
    }

    /** Returns the type of the `MakeFmt`. */
    fun type(): Writable = writable {
        if (hasRedactions()) {
            rustTemplate("#{SmithyHttpServer}::logging::sensitivity::uri::MakeQuery<fn(&str) -> #{SmithyHttpServer}::logging::sensitivity::uri::QueryMarker>", *codegenScope)
        } else {
            rustTemplate("#{SmithyHttpServer}::logging::MakeIdentity", *codegenScope)
        }
    }

    /** Returns the closure used during construction. */
    internal fun closure(): Writable {
        val value = when (this) {
            is SensitiveMapValue -> writable {
                rust("true")
            }
            is NotSensitiveMapValue -> if (queryKeys.isEmpty()) writable {
                rust("false;")
            } else writable {
                val matches = queryKeys.joinToString("|") { it.dq() }
                rust("matches!(name, $matches);")
            }
        }

        return writable {
            rustTemplate(
                """
                {
                    |name: &str| {
                        let key = $allKeysSensitive;
                        let value = #{Value:W};
                        #{SmithyHttpServer}::logging::sensitivity::uri::QueryMarker { key, value }
                    }
                } as fn(&_) -> _
                """,
                "Value" to value,
                *codegenScope,
            )
        }
    }

    /** Returns the setter enclosing the closure. */
    fun setters(): Writable = writable {
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
 * `InstrumentedOperation` to configure logging. These structures can be found in `aws_smithy_http_server::logging`.
 *
 * See [Logging in the Presence of Sensitive Data](https://github.com/awslabs/smithy-rs/blob/main/design/src/rfcs/rfc0018_logging_sensitive.md)
 * for more details.
 */
class ServerHttpSensitivityGenerator(
    private val model: Model,
    private val operation: OperationShape,
    private val runtimeConfig: RuntimeConfig,
) {
    private val codegenScope = arrayOf(
        "SmithyHttpServer" to ServerCargoDependency.SmithyHttpServer(runtimeConfig).asType(),
        "Http" to CargoDependency.Http.asType(),
    )

    /** Constructs `StatusCodeSensitivity` of a `Shape` */
    private fun findStatusCodeSensitivity(rootShape: Shape): StatusCodeSensitivity {
        val isSensitive = findSensitiveBoundTrait<HttpResponseCodeTrait>(rootShape).isNotEmpty()
        return StatusCodeSensitivity(isSensitive, runtimeConfig)
    }

    /** Constructs `HeaderSensitivity` of a `Shape` */
    internal fun findHeaderSensitivity(rootShape: Shape): HeaderSensitivity {
        // httpHeader bindings
        // [trait|sensitive] ~> [trait|httpHeader]
        val headerKeys = findSensitiveBoundTrait<HttpHeaderTrait>(rootShape).map { it.value }.distinct()

        // httpPrefixHeaders bindings
        // [trait|sensitive] ~> [trait|httpPrefixHeaders]
        // All prefix keys and values are sensitive
        val prefixSuffixA = findSensitiveBoundTrait<HttpPrefixHeadersTrait>(rootShape).map { it.value }.singleOrNull()
        if (prefixSuffixA != null) {
            return HeaderSensitivity.SensitiveMapValue(headerKeys, true, prefixSuffixA, runtimeConfig)
        }

        // Find httpPrefixHeaders trait
        // member[trait|httpPrefixHeaders]
        val httpPrefixMember: MemberShape = Walker(model)
            .walkShapes(rootShape) {
                // Do not traverse upwards or beyond a httpPrefixHeaders trait
                it.direction == RelationshipDirection.DIRECTED && !it.shape.hasTrait<HttpPrefixHeadersTrait>()
            }
            .filter {
                it.hasTrait<HttpPrefixHeadersTrait>()
            }
            .map { it as? MemberShape }
            .singleOrNull() ?: return HeaderSensitivity.NotSensitiveMapValue(headerKeys, null, runtimeConfig)

        // Find map[trait|httpPrefixHeaders] > member[trait|sensitive]
        val mapMembers: List<MemberShape> =
            Walker(model)
                .walkShapes(httpPrefixMember) {
                    // Do not traverse upwards
                    it.direction == RelationshipDirection.DIRECTED
                }
                .filter {
                    isDirectedRelationshipSensitive<SensitiveTrait>(it)
                }.mapNotNull {
                    it as? MemberShape
                }

        // httpPrefixHeaders name
        val httpPrefixTrait = httpPrefixMember.getTrait<HttpPrefixHeadersTrait>()
        val httpPrefixName = checkNotNull(httpPrefixTrait) { "httpPrefixTrait shouldn't be null as it was checked above" }.value

        // Are key/value of the httpPrefixHeaders map sensitive?
        val (keySensitive, valuesSensitive) = mapMembers.fold(Pair(false, false)) { (key, value), it ->
            Pair(
                key || it.memberName == "key",
                value || it.memberName == "value",
            )
        }

        return if (valuesSensitive) {
            // All values are sensitive
            HeaderSensitivity.SensitiveMapValue(headerKeys, keySensitive, httpPrefixName, runtimeConfig)
        } else if (keySensitive) {
            // Only keys are sensitive
            HeaderSensitivity.NotSensitiveMapValue(headerKeys, httpPrefixName, runtimeConfig)
        } else {
            // No values are sensitive
            HeaderSensitivity.NotSensitiveMapValue(headerKeys, null, runtimeConfig)
        }
    }

    /** Constructs `QuerySensitivity` of a `Shape` */
    internal fun findQuerySensitivity(rootShape: Shape): QuerySensitivity {
        // httpQueryParams bindings
        // [trait|sensitive] ~> [trait|httpQueryParams]
        // Both keys/values are sensitive
        val allSensitive = findSensitiveBoundTrait<HttpQueryParamsTrait>(rootShape).isNotEmpty()

        if (allSensitive) {
            return QuerySensitivity.SensitiveMapValue(true, runtimeConfig)
        }

        // Sensitive trait can exist within the httpQueryParams map
        // [trait|httpQueryParams] ~> map > member [trait|sensitive]
        // Keys/values may be sensitive
        val mapMembers = findTraitInterval<HttpQueryParamsTrait, SensitiveTrait>(rootShape)
        val (keysSensitive, valuesSensitive) = mapMembers.fold(Pair(false, false)) { (key, value), it ->
            Pair(
                key || it.memberName == "key",
                value || it.memberName == "value",
            )
        }

        if (valuesSensitive) {
            return QuerySensitivity.SensitiveMapValue(keysSensitive, runtimeConfig)
        }

        // httpQuery bindings
        // [trait|sensitive] ~> [trait|httpQuery]
        val queries = findSensitiveBoundTrait<HttpQueryTrait>(rootShape).map { it.value }.distinct()

        return QuerySensitivity.NotSensitiveMapValue(queries, keysSensitive, runtimeConfig)
    }

    /** Constructs `LabelSensitivity` of a `Shape` */
    internal fun findLabelSensitivity(uriPattern: UriPattern, rootShape: Shape): LabelSensitivity {
        val sensitiveLabels = findSensitiveBound<HttpLabelTrait>(rootShape)

        val labelMap: Map<String, Int> = uriPattern
            .segments
            .withIndex()
            .filter { (_, segment) -> segment.isLabel && !segment.isGreedyLabel }.associate { (index, segment) -> Pair(segment.content, index) }
        val labelsIndex = sensitiveLabels.mapNotNull { labelMap[it.memberName] }

        val greedyLabel = uriPattern
            .segments
            .asIterable()
            .withIndex()
            .find { (_, segment) ->
                segment.isGreedyLabel
            }?.let { (index, segment) ->
                // Check if sensitive
                if (sensitiveLabels.find { it.memberName == segment.content } != null) {
                    index
                } else {
                    null
                }
            }?.let { index ->
                val remainingSegments = uriPattern.segments.asIterable().drop(index + 1)
                val suffix = if (remainingSegments.isNotEmpty()) {
                    remainingSegments.joinToString(prefix = "/", separator = "/")
                } else {
                    ""
                }
                GreedyLabel(index, suffix.length)
            }

        return LabelSensitivity(labelsIndex, greedyLabel, runtimeConfig)
    }

    // Find member shapes with trait `B` contained in a shape enjoying `A`.
    // [trait|A] ~> [trait|B]
    internal inline fun <reified A : Trait, reified B : Trait> findTraitInterval(rootShape: Shape): List<MemberShape> {
        return Walker(model)
            .walkShapes(rootShape) {
                // Do not traverse upwards or beyond A trait
                it.direction == RelationshipDirection.DIRECTED && !it.shape.hasTrait<A>()
            }
            .filter {
                isDirectedRelationshipSensitive<A>(it)
            }
            .flatMap {
                Walker(model)
                    .walkShapes(it) {
                        // Do not traverse upwards
                        it.direction == RelationshipDirection.DIRECTED
                    }
                    .filter {
                        isDirectedRelationshipSensitive<B>(it)
                    }.mapNotNull {
                        it as? MemberShape
                    }
            }
    }

    internal inline fun <reified A : Trait> isDirectedRelationshipSensitive(partnerShape: Shape): Boolean {
        return partnerShape.hasTrait<A>() || (
            partnerShape.asMemberShape().isPresent &&
                model.expectShape(partnerShape.asMemberShape().get().target).hasTrait<A>()
            )
    }

    /**
     * Find member shapes with trait `T` contained in a shape enjoying `SensitiveTrait`.
     *
     * [trait|sensitive] ~> [trait|T]
     */
    internal inline fun <reified T : Trait> findSensitiveBound(rootShape: Shape): List<MemberShape> {
        return findTraitInterval<SensitiveTrait, T>(rootShape)
    }

    /** Find trait `T` contained in a shape enjoying `SensitiveTrait`. */
    private inline fun <reified T : Trait> findSensitiveBoundTrait(rootShape: Shape): List<T> {
        return findSensitiveBound<T>(rootShape).map {
            val trait = it.getTrait<T>()
            checkNotNull(trait) { "trait shouldn't be null because of the null checked previously" }
        }
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
        val type = writable {
            rustTemplate("#{SmithyHttpServer}::logging::sensitivity::DefaultRequestFmt", *codegenScope)
        }
        val value = writable {
            rustTemplate("#{SmithyHttpServer}::logging::sensitivity::RequestFmt::new()", *codegenScope)
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

        val type = writable {
            rustTemplate(
                """
                #{SmithyHttpServer}::logging::sensitivity::RequestFmt<
                    #{HeaderType:W},
                    #{SmithyHttpServer}::logging::sensitivity::uri::MakeUri<
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

        val value = writable { rustTemplate("#{SmithyHttpServer}::logging::sensitivity::RequestFmt::new()", *codegenScope) } + headerSensitivity.setter() + labelSensitivity.setter() + querySensitivity.setters()

        return MakeFmt(type, value)
    }

    private fun defaultResponseFmt(): MakeFmt {
        val type = writable {
            rustTemplate("#{SmithyHttpServer}::logging::sensitivity::DefaultResponseFmt", *codegenScope)
        }

        val value = writable {
            rustTemplate("#{SmithyHttpServer}::logging::sensitivity::ResponseFmt::new()", *codegenScope)
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

        val type = writable {
            rustTemplate(
                "#{SmithyHttpServer}::logging::sensitivity::ResponseFmt<#{HeaderType:W}, #{StatusType:W}>",
                "HeaderType" to headerSensitivity.type(),
                "StatusType" to statusCodeSensitivity.type(),
                *codegenScope,
            )
        }

        val value = writable {
            rustTemplate("#{SmithyHttpServer}::logging::sensitivity::ResponseFmt::new()", *codegenScope)
        } + headerSensitivity.setter() + statusCodeSensitivity.setter()

        return MakeFmt(type, value)
    }
}
