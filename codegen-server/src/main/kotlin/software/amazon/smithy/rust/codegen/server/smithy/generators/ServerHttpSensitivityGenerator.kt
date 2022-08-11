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
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.withBlock
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.getTrait
import software.amazon.smithy.rust.codegen.util.hasTrait
import java.util.*

internal fun findUriGreedyLabelPosition(uriPattern: UriPattern): Int? {
    return uriPattern
        .greedyLabel
        .orElse(null)
        ?.let { uriPattern.toString().indexOf("$it") }
}

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
    runtimeConfig: RuntimeConfig,
) {
    private val codegenScope = arrayOf(
        "SmithyHttpServer" to ServerCargoDependency.SmithyHttpServer(runtimeConfig).asType(),
        "Http" to CargoDependency.Http.asType(),
    )

    // Models the ways headers can be bound and sensitive
    sealed class HeaderSensitivity(
        // The values of [trait|sensitive] ~> [trait|httpHeader]
        val headerKeys: List<String>,
    ) {
        // The case where map[trait|httpPrefixHeaders] > [id|member = value] is not sensitive
        class NotSensitiveMapValue(
            headerKeys: List<String>,
            // The value of map[trait|httpPrefixHeaders] > [id|member = key], null if it's not sensitive
            val prefixHeader: String?,
        ) : HeaderSensitivity(headerKeys)

        // The case where map[trait|httpPrefixHeaders] > [id|member = value] is sensitive
        class SensitiveMapValue(
            headerKeys: List<String>,
            // Is map[trait|httpQueryParams] > [id|member = key] sensitive?
            val keySensitive: Boolean,
            // What is the value of map[trait|httpQueryParams]?
            val prefixHeader: String,
        ) : HeaderSensitivity(headerKeys)

        // Is there anything to redact?
        fun hasRedactions(): Boolean {
            return when (this) {
                is NotSensitiveMapValue -> {
                    prefixHeader != null || headerKeys.isNotEmpty()
                }
                is SensitiveMapValue -> {
                    true
                }
            }
        }
    }

    internal fun findHeaderSensitivity(rootShape: Shape): HeaderSensitivity {
        // httpHeader bindings
        // [trait|sensitive] ~> [trait|httpHeader]
        val headerKeys = findSensitiveBoundTrait<HttpHeaderTrait>(rootShape).map { it.value }.distinct()

        // httpPrefixHeaders bindings
        // [trait|sensitive] ~> [trait|httpPrefixHeaders]
        // All prefix keys and values are sensitive
        val prefixSuffixA = findSensitiveBoundTrait<HttpPrefixHeadersTrait>(rootShape).map { it.value }.singleOrNull()
        if (prefixSuffixA != null) {
            return HeaderSensitivity.SensitiveMapValue(headerKeys, true, prefixSuffixA)
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
            .singleOrNull() ?: return HeaderSensitivity.NotSensitiveMapValue(headerKeys, null)

        // Find map[trait|httpPrefixHeaders] > member[trait|sensitive]
        val mapMembers: List<MemberShape> =
            Walker(model)
                .walkShapes(httpPrefixMember) {
                    // Do not traverse upwards
                    it.direction == RelationshipDirection.DIRECTED
                }
                .filter {
                    it.hasTrait<SensitiveTrait>()
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
            HeaderSensitivity.SensitiveMapValue(headerKeys, keySensitive, httpPrefixName)
        } else {
            HeaderSensitivity.NotSensitiveMapValue(headerKeys, httpPrefixName)
        }
    }

    internal fun renderHeaderClosure(writer: RustWriter, headerSensitivity: HeaderSensitivity) {
        writer.rustBlockTemplate("|name: &#{Http}::header::HeaderName|", *codegenScope) {
            rust("##[allow(unused_variables)]")
            rust("let name = name.as_str();")

            if (headerSensitivity.headerKeys.isEmpty()) {
                rust("let name_match = false;")
            } else {
                val matches = headerSensitivity.headerKeys.joinToString("|") { it.dq() }
                rust("let name_match = matches!(name, $matches);")
            }

            when (headerSensitivity) {
                is HeaderSensitivity.NotSensitiveMapValue -> {
                    headerSensitivity.prefixHeader?.let {
                        rust("let starts_with = name.starts_with(${it.dq()});")
                        rust("let key_suffix = if starts_with { Some(${it.length}) } else { None };")
                    } ?: rust("let key_suffix = None;")
                    rust("let value = name_match;")
                }
                is HeaderSensitivity.SensitiveMapValue -> {
                    val prefixHeader = headerSensitivity.prefixHeader
                    rust("let starts_with = name.starts_with(${prefixHeader.dq()});")
                    if (headerSensitivity.keySensitive) {
                        rust("let key_suffix = if starts_with { Some(${prefixHeader.length}) } else { None };")
                    } else {
                        rust("let key_suffix = None;")
                    }
                    rust("let value = name_match || starts_with;")
                }
            }

            rustBlockTemplate("#{SmithyHttpServer}::logging::sensitivity::headers::HeaderMarker", *codegenScope) {
                rust("value, key_suffix")
            }
        }
    }

    // Models the ways query strings can be bound and sensitive
    sealed class QuerySensitivity(
        // Are all keys sensitive?
        val allKeysSensitive: Boolean,
    ) {
        // The case where map[trait|httpQueryParams] > [id|member = value] is not sensitive
        class NotSensitiveMapValue(
            // The values of [trait|sensitive] ~> [trait|httpQuery]
            val queryKeys: List<String>,
            allKeysSensitive: Boolean,
        ) : QuerySensitivity(allKeysSensitive)

        // The case where map[trait|httpQueryParams] > [id|member = value] is sensitive
        class SensitiveMapValue(allKeysSensitive: Boolean) : QuerySensitivity(allKeysSensitive)

        // Is there anything to redact?
        fun hasRedactions(): Boolean {
            return when (this) {
                is NotSensitiveMapValue -> {
                    allKeysSensitive || queryKeys.isNotEmpty()
                }
                is SensitiveMapValue -> {
                    true
                }
            }
        }
    }

    internal fun findQuerySensitivity(rootShape: Shape): QuerySensitivity {
        // httpQueryParams bindings
        // [trait|sensitive] ~> [trait|httpQueryParams]
        // Both keys/values are sensitive
        val allSensitive = findSensitiveBoundTrait<HttpQueryParamsTrait>(rootShape).isNotEmpty()

        if (allSensitive) {
            return QuerySensitivity.SensitiveMapValue(true)
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
            return QuerySensitivity.SensitiveMapValue(keysSensitive)
        }

        // httpQuery bindings
        // [trait|sensitive] ~> [trait|httpQuery]
        val queries = findSensitiveBoundTrait<HttpQueryTrait>(rootShape).map { it.value }.distinct()

        return QuerySensitivity.NotSensitiveMapValue(queries, keysSensitive)
    }

    internal fun renderQueryClosure(writer: RustWriter, querySensitivity: QuerySensitivity) {
        writer.rustBlockTemplate("|name: &str|", *codegenScope) {
            rust("let key = ${querySensitivity.allKeysSensitive};")

            when (querySensitivity) {
                is QuerySensitivity.SensitiveMapValue -> {
                    rust("let value = true;")
                }
                is QuerySensitivity.NotSensitiveMapValue -> {
                    if (querySensitivity.queryKeys.isEmpty()) {
                        rust("let value = false;")
                    } else {
                        val matches = querySensitivity.queryKeys.joinToString("|") { it.dq() }
                        rust("let value = matches!(name, $matches);")
                    }
                }
            }
            rustTemplate("#{SmithyHttpServer}::logging::sensitivity::uri::QueryMarker { key, value }", *codegenScope)
        }
    }

    internal fun renderLabelClosure(writer: RustWriter, indexes: List<Int>) {
        writer.rustBlock("|index: usize|") {
            withBlock("matches!(index,", ")") {
                val matches = indexes.joinToString("|") { "$it" }
                rust(matches)
            }
        }
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
                it.hasTrait<A>() || (it.asMemberShape().isPresent()
                        && model.expectShape(it.asMemberShape().get().getTarget()).hasTrait<A>())
            }
            .flatMap {
                Walker(model)
                    .walkShapes(it) {
                        // Do not traverse upwards
                        it.direction == RelationshipDirection.DIRECTED
                    }
                    .filter {
                        it.hasTrait<B>()
                    }.mapNotNull {
                        it as? MemberShape
                    }
            }
    }

    // Find member shapes with trait `T` contained in a shape enjoying `SensitiveTrait`.
    // [trait|sensitive] ~> [trait|T]
    internal inline fun <reified T : Trait> findSensitiveBound(rootShape: Shape): List<MemberShape> {
        return findTraitInterval<SensitiveTrait, T>(rootShape)
    }

    // Find trait `T` contained in a shape enjoying `SensitiveTrait`.
    private inline fun <reified T : Trait> findSensitiveBoundTrait(rootShape: Shape): List<T> {
        return findSensitiveBound<T>(rootShape).map {
            val trait = it.getTrait<T>()
            checkNotNull(trait) { "trait shouldn't be null because of the null checked previously" }
        }
    }

    internal fun findUriLabelIndexes(uriPattern: UriPattern, rootShape: Shape): List<Int> {
        val uriLabels: Map<String, Int> = uriPattern
            .segments
            .withIndex()
            .filter { (_, segment) -> segment.isLabel }.associate { (index, segment) -> Pair(segment.content, index) }
        return findSensitiveBound<HttpLabelTrait>(rootShape).mapNotNull { uriLabels[it.memberName] }
    }

    sealed class LabelSensitivity {
        class Normal(val indexes: List<Int>) : LabelSensitivity()
        class Greedy(val suffixPosition: Int) : LabelSensitivity()
    }

    private fun findLabel(uriPattern: UriPattern, rootShape: Shape): LabelSensitivity {
        return findUriGreedyLabelPosition(uriPattern)?.let { LabelSensitivity.Greedy(it) } ?: findUriLabelIndexes(uriPattern, rootShape).let { LabelSensitivity.Normal(it) }
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

    fun renderRequestFmt(writer: RustWriter) {
        writer.rustTemplate("#{SmithyHttpServer}::logging::sensitivity::RequestFmt::new()", *codegenScope)

        // Sensitivity only applies when http trait is applied to the operation
        val httpTrait = operation.getTrait<HttpTrait>() ?: return
        val inputShape = input() ?: return

        // httpHeader/httpPrefixHeaders bindings
        val headerSensitivity = findHeaderSensitivity(inputShape)
        if (headerSensitivity.hasRedactions()) {
            writer.withBlock(".header(", ")") {
                renderHeaderClosure(writer, headerSensitivity)
            }
        }

        // httpLabel bindings
        when (val label = findLabel(httpTrait.uri, inputShape)) {
            is LabelSensitivity.Normal -> {
                if (label.indexes.isNotEmpty()) {
                    writer.withBlock(".label(", ")") {
                        renderLabelClosure(writer, label.indexes)
                    }
                }
            }
            is LabelSensitivity.Greedy -> {
                writer.rust(".greedy_label(${label.suffixPosition})")
            }
        }

        // httpQuery/httpQueryParams bindings
        val querySensitivity = findQuerySensitivity(inputShape)
        if (querySensitivity.hasRedactions()) {
            writer.withBlock(".query(", ")") {
                renderQueryClosure(writer, querySensitivity)
            }
        }
    }

    fun renderResponseFmt(writer: RustWriter) {
        writer.rustTemplate("#{SmithyHttpServer}::logging::sensitivity::ResponseFmt::new()", *codegenScope)

        // Sensitivity only applies when HTTP trait is applied to the operation
        operation.getTrait<HttpTrait>() ?: return
        val outputShape = output() ?: return

        // httpHeader/httpPrefixHeaders bindings
        val headerSensitivity = findHeaderSensitivity(outputShape)
        if (headerSensitivity.hasRedactions()) {
            writer.withBlock(".header(", ")") {
                renderHeaderClosure(writer, headerSensitivity)
            }
        }

        // Status code bindings
        val hasResponseStatusCode = findSensitiveBoundTrait<HttpResponseCodeTrait>(outputShape).isNotEmpty()
        if (hasResponseStatusCode) {
            writer.rust(".status_code()")
        }
    }
}
