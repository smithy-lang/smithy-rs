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
import software.amazon.smithy.rust.codegen.rustlang.withBlockTemplate
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.util.getTrait
import software.amazon.smithy.rust.codegen.util.inputShape
import software.amazon.smithy.rust.codegen.util.outputShape
import java.util.*

internal fun findUriGreedyLabelPosition(uriPattern: UriPattern): Int? {
    return uriPattern
        .greedyLabel
        .orElse(null)
        ?.let { uriPattern.toString().indexOf("$it") }
}

class ServerHttpSensitivityGenerator(
    private val model: Model,
    private val operation: OperationShape,
    runtimeConfig: RuntimeConfig
) {
    private val codegenScope = arrayOf(
        "SmithyHttpServer" to ServerCargoDependency.SmithyHttpServer(runtimeConfig).asType(),
        "Http" to CargoDependency.Http.asType(),
    )
    private val outputShape = operation.output.orElse(null)?.let { model.getShape(it).orElse(null) }

    sealed class HeaderSensitivity(
        // List of keys whose values are sensitive
        val headerKeys: List<String>,
    ) {
        // httpQueryParams map > [id|member = value] is not sensitive?
        class NotMapValue(headerKeys: List<String>, val prefixHeader: String?) : HeaderSensitivity(headerKeys)

        // httpQueryParams map > [id|member = value] is sensitive?
        class MapValue(headerKeys: List<String>, val prefixHeader: String) : HeaderSensitivity(headerKeys)

        fun isIdentity(): Boolean {
            return when (this) {
                is NotMapValue -> {
                    prefixHeader == null && headerKeys.isEmpty()
                }
                is MapValue -> {
                    false
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
            return HeaderSensitivity.MapValue(headerKeys, prefixSuffixA)
        }

        // Find httpPrefixHeaders trait
        // member[trait|httpPrefixHeaders]
        val httpPrefixMember: MemberShape = Walker(model)
            .walkShapes(rootShape) {
                // Do not traverse upwards or beyond a httpPrefixHeaders trait
                it.direction == RelationshipDirection.DIRECTED && it.shape.getTrait<HttpPrefixHeadersTrait>() == null
            }
            .filter {
                it.getTrait<HttpPrefixHeadersTrait>() != null
            }
            .map { it as? MemberShape }
            .singleOrNull() ?: return HeaderSensitivity.NotMapValue(headerKeys, null)

        // Find map > members of member httpPrefixHeaders trait
        val mapMembers: List<MemberShape> =
            Walker(model)
                .walkShapes(httpPrefixMember) {
                    // Do not traverse upwards
                    it.direction == RelationshipDirection.DIRECTED
                }
                .filter {
                    it.getTrait<SensitiveTrait>() != null
                }.mapNotNull {
                    it as? MemberShape
                }

        // httpPrefixHeaders name
        val httpPrefixName = httpPrefixMember.getTrait<HttpPrefixHeadersTrait>()!!.value

        val init: Pair<String?, Boolean> = Pair(null, false)
        val (prefixSuffixB, valuesSensitive) = mapMembers.fold(init) { (key, value), it ->
            Pair(
                key ?: if (it.memberName == "key") { httpPrefixName } else { null },
                value || it.memberName == "value"
            )
        }

        return if (valuesSensitive) {
            // All values are sensitive
            HeaderSensitivity.MapValue(headerKeys, httpPrefixName)
        } else {
            HeaderSensitivity.NotMapValue(headerKeys, prefixSuffixB)
        }
    }

    internal fun renderHeaderClosure(writer: RustWriter, headerSensitivity: HeaderSensitivity) {
        writer.rustBlockTemplate("|name: &#{Http}::header::HeaderName|", *codegenScope) {
            rust("let name = name.as_str();")

            if (headerSensitivity.headerKeys.isEmpty()) {
                rust("let name_match = false;")
            } else {
                val matches = headerSensitivity.headerKeys.joinToString("|") { "\"$it\"" }
                rust("let name_match = matches!(name, $matches);")
            }

            when (headerSensitivity) {
                is HeaderSensitivity.NotMapValue -> {
                    headerSensitivity.prefixHeader?.let {
                        rust("let key_suffix = name.starts_with(\"$it\").then_some(${it.length});")
                    } ?: run {
                        rust("let _ = name;")
                        rust("let key_suffix = None;")
                    }
                    rust("let value = name_match;")
                }
                is HeaderSensitivity.MapValue -> {
                    val prefixHeader = headerSensitivity.prefixHeader
                    rust("let key_suffix = name.starts_with(\"$prefixHeader\").then_some(${prefixHeader.length});")
                    rust("let value = name_match || key_suffix.is_some();")
                }
            }

            rustBlockTemplate("#{SmithyHttpServer}::logging::sensitivity::headers::HeaderMarker", *codegenScope) {
                rust("value, key_suffix")
            }
        }
    }

    sealed class QuerySensitivity(
        // Are all keys sensitive?
        val allKeysSensitive: Boolean
    ) {
        // List of keys marking which values are sensitive
        class SpecificValues(val queryKeys: List<String>, allKeysSensitive: Boolean) : QuerySensitivity(allKeysSensitive)

        // All values are sensitive
        class AllValues(allKeysSensitive: Boolean) : QuerySensitivity(allKeysSensitive)

        fun isIdentity(): Boolean {
            return when (this) {
                is SpecificValues -> {
                    !allKeysSensitive && queryKeys.isEmpty()
                }
                is AllValues -> {
                    false
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
            return QuerySensitivity.AllValues(true)
        }

        // Sensitive trait can exist within the httpQueryParams map
        // [trait|httpQueryParams] ~> map > member [trait|sensitive]
        // Keys/values may be sensitive
        val mapMembers = findTraitInterval<HttpQueryParamsTrait, SensitiveTrait>(rootShape)
        val (keysSensitive, valuesSensitive) = mapMembers.fold(Pair(false, false)) { (key, value), it ->
            Pair(
                key || it.memberName == "key",
                value || it.memberName == "value"
            )
        }

        if (valuesSensitive) {
            return QuerySensitivity.AllValues(keysSensitive)
        }

        // httpQuery bindings
        // [trait|sensitive] ~> [trait|httpQuery]
        val queries = findSensitiveBoundTrait<HttpQueryTrait>(rootShape).map { it.value }.distinct()

        return QuerySensitivity.SpecificValues(queries, keysSensitive)
    }

    internal fun renderQueryClosure(writer: RustWriter, querySensitivity: QuerySensitivity) {
        writer.rustBlockTemplate("|name: &str|", *codegenScope) {
            rust("let key = ${querySensitivity.allKeysSensitive};")

            when (querySensitivity) {
                is QuerySensitivity.AllValues -> {
                    rust("let value = true;")
                }
                is QuerySensitivity.SpecificValues -> {
                    if (querySensitivity.queryKeys.isEmpty()) {
                        rust("let value = false;")
                    } else {
                        val matches = querySensitivity.queryKeys.joinToString("|") { "\"$it\"" }
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
                // Do not traverse upwards or beyond a A trait
                it.direction == RelationshipDirection.DIRECTED && it.shape.getTrait<A>() == null
            }
            .filter {
                it.getTrait<A>() != null
            }
            .flatMap {
                Walker(model)
                    .walkShapes(it) {
                        // Do not traverse upwards
                        it.direction == RelationshipDirection.DIRECTED
                    }
                    .filter {
                        it.getTrait<B>() != null
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
    internal inline fun <reified T : Trait> findSensitiveBoundTrait(rootShape: Shape): List<T> {
        return findSensitiveBound<T>(rootShape).mapNotNull { it.getTrait<T>() }
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
        writer.withBlockTemplate("#{SmithyHttpServer}::logging::sensitivity::RequestFmt::new()", ";", *codegenScope) {
            // Sensitivity only applies when http trait is applied to the operation
            val httpTrait = operation.getTrait<HttpTrait>() ?: return@withBlockTemplate
            val inputShape = input() ?: return@withBlockTemplate

            // httpHeader/httpPrefixHeaders bindings
            val headerSensitivity = findHeaderSensitivity(inputShape)
            if (!headerSensitivity.isIdentity()) {
                withBlock(".header(", ")") {
                    renderHeaderClosure(writer, headerSensitivity)
                }
            }

            // httpLabel bindings
            when (val label = findLabel(httpTrait.uri, inputShape)) {
                is LabelSensitivity.Normal -> {
                    if (label.indexes.isNotEmpty()) {
                        withBlock(".label(", ")") {
                            renderLabelClosure(writer, label.indexes)
                        }
                    }
                }
                is LabelSensitivity.Greedy -> {
                    rust(".greedy_label(${label.suffixPosition})")
                }
            }

            // httpQuery/httpQueryParams bindings
            val querySensitivity = findQuerySensitivity(inputShape)
            if (!querySensitivity.isIdentity()) {
                withBlock(".query(", ")") {
                    renderQueryClosure(writer, querySensitivity)
                }
            }
        }
    }

    fun renderResponseFmt(writer: RustWriter) {
        writer.withBlockTemplate("#{SmithyHttpServer}::logging::sensitivity::ResponseFmt::new()", ";", *codegenScope) {
            // Sensitivity only applies when HTTP trait is applied to the operation
            operation.getTrait<HttpTrait>() ?: return@withBlockTemplate
            val outputShape = output() ?: return@withBlockTemplate

            // httpHeader/httpPrefixHeaders bindings
            val headerSensitivity = findHeaderSensitivity(outputShape)
            if (!headerSensitivity.isIdentity()) {
                withBlock(".header(", ")") {
                    renderHeaderClosure(writer, headerSensitivity)
                }
            }

            // Status code bindings
            val hasResponseStatusCode = findSensitiveBoundTrait<HttpResponseCodeTrait>(outputShape).isNotEmpty()
            if (hasResponseStatusCode) {
                rust(".status_code()")
            }
        }
    }
}
