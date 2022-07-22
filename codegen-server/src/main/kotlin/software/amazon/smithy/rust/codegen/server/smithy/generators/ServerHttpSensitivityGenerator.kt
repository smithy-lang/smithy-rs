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
    private val inputShape = operation.inputShape(model)
    private val outputShape = operation.outputShape(model)

    sealed class HeaderSensitivity(
        // Sensitive suffix of header key
        val prefixHeader: String?,
    ) {
        // List of keys whose values are sensitive
        class SpecificHeaderValues(val headerKeys: List<String>, prefixHeader: String?) : HeaderSensitivity(prefixHeader)

        // All values are sensitive
        class AllHeaderValues(prefixHeader: String?) : HeaderSensitivity(prefixHeader)

        fun isIdentity(): Boolean {
            return when (this) {
                is SpecificHeaderValues -> {
                    prefixHeader == null && headerKeys.isEmpty()
                }
                is AllHeaderValues -> {
                    false
                }
            }
        }
    }

    internal fun findHeaderSensitivity(rootShape: Shape): HeaderSensitivity {
        // httpPrefixHeaders bindings
        // [trait|sensitive] ~> [trait|httpPrefixHeaders]
        // All prefix keys and values are sensitive
        val prefixSuffixA = findSensitiveBoundTrait<HttpPrefixHeadersTrait>(rootShape).map { it.value }.singleOrNull()
        if (prefixSuffixA != null) {
            return HeaderSensitivity.AllHeaderValues(prefixSuffixA)
        }

        // Find httpPrefixHeaders trait
        // member[trait|httpPrefixHeaders]
        val httpPrefixMember: MemberShape? = Walker(model)
            .walkShapes(rootShape) {
                // Do not traverse upwards or beyond a httpPrefixHeaders trait
                it.direction == RelationshipDirection.DIRECTED && it.shape.getTrait<HttpPrefixHeadersTrait>() == null
            }
            .filter {
                it.getTrait<HttpPrefixHeadersTrait>() != null
            }
            .map { it as? MemberShape }
            .singleOrNull()

        // httpHeader bindings
        // [trait|sensitive] ~> [trait|httpHeader]
        // Specific values are sensitive from keys...
        val headerKeys = findSensitiveBoundTrait<HttpHeaderTrait>(rootShape).map { it.value }.distinct()

        // ...and if there are no sensitive keys...
        if (httpPrefixMember == null) {
            return HeaderSensitivity.SpecificHeaderValues(headerKeys, null)
        }

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

        val init: Pair<String?, Boolean> = Pair(null, false)
        val (prefixSuffixB, valuesSensitive) = mapMembers.fold(init) { (key, value), it ->
            // httpPrefixHeaders name
            val httpPrefixName = httpPrefixMember.getTrait<HttpPrefixHeadersTrait>()!!.value
            Pair(
                key ?: if (it.memberName == "key") { httpPrefixName } else { null },
                value || it.memberName == "value"
            )
        }

        return if (valuesSensitive) {
            // All values are sensitive
            HeaderSensitivity.AllHeaderValues(prefixSuffixB)
        } else {
            HeaderSensitivity.SpecificHeaderValues(headerKeys, prefixSuffixB)
        }
    }

    internal fun renderHeaderClosure(writer: RustWriter, headerSensitivity: HeaderSensitivity) {
        writer.rustBlockTemplate("|name: &#{Http}::header::HeaderName|", *codegenScope) {
            rust("let name = name.as_str();")

            headerSensitivity.prefixHeader?.let {
                rust("let key_suffix = name.starts_with(\"$it\").then_some(${it.length});")
            } ?: rust("let key_suffix = None;")

            when (headerSensitivity) {
                is HeaderSensitivity.AllHeaderValues -> {
                    rust("let value = true;")
                }
                is HeaderSensitivity.SpecificHeaderValues -> {
                    if (headerSensitivity.headerKeys.isEmpty()) {
                        rust("let value = false;")
                    } else {
                        val matches = headerSensitivity.headerKeys.joinToString("|") { "\"$it\"" }
                        rust("let value = matches!(name, $matches);")
                    }
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

    internal fun findQuerySensitivity(): QuerySensitivity {
        // httpQueryParams bindings
        // [trait|sensitive] ~> [trait|httpQueryParams]
        // Both keys/values are sensitive
        val allSensitive = findSensitiveBoundTrait<HttpQueryParamsTrait>(inputShape).isNotEmpty()

        if (allSensitive) {
            return QuerySensitivity.AllValues(true)
        }

        // Sensitive trait can exist within the httpQueryParams map
        // [trait|httpQueryParams] ~> map > member [trait|sensitive]
        // Keys/values may be sensitive
        val mapMembers = findTraitInterval<HttpQueryParamsTrait, SensitiveTrait>(inputShape)
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
        val queries = findSensitiveBoundTrait<HttpQueryTrait>(inputShape).map { it.value }.distinct()

        return QuerySensitivity.SpecificValues(queries, keysSensitive)
    }

    internal fun renderQueryClosure(writer: RustWriter, querySensitivity: QuerySensitivity) {
        writer.rustBlockTemplate("|name: &str|", *codegenScope) {
            rust("let keys = ${querySensitivity.allKeysSensitive};")

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

    internal fun findUriLabelIndexes(uriPattern: UriPattern): List<Int> {
        val uriLabels: Map<String, Int> = uriPattern
            .segments
            .withIndex()
            .filter { (_, segment) -> segment.isLabel }.associate { (index, segment) -> Pair(segment.content, index) }
        return findSensitiveBound<HttpLabelTrait>(inputShape).mapNotNull { uriLabels[it.memberName] }
    }

    sealed class LabelSensitivity {
        class Normal(val indexes: List<Int>) : LabelSensitivity()
        class Greedy(val suffixPosition: Int) : LabelSensitivity()
    }

    private fun findLabel(uriPattern: UriPattern): LabelSensitivity {
        return findUriGreedyLabelPosition(uriPattern)?.let { LabelSensitivity.Greedy(it) } ?: findUriLabelIndexes(uriPattern).let { LabelSensitivity.Normal(it) }
    }

    fun renderRequestFmt(writer: RustWriter) {
        writer.withBlockTemplate("#{SmithyHttpServer}::logging::sensitivity::RequestFmt::new()", ";", *codegenScope) {
            // Sensitivity only applies when http trait is applied to the operation
            val httpTrait = operation.getTrait<HttpTrait>() ?: return@withBlockTemplate

            // httpHeader/httpPrefixHeaders bindings
            val headerSensitivity = findHeaderSensitivity(inputShape)
            if (!headerSensitivity.isIdentity()) {
                withBlock(".header(", ")") {
                    renderHeaderClosure(writer, headerSensitivity)
                }
            }

            // httpLabel bindings
            when (val label = findLabel(httpTrait.uri)) {
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
            val querySensitivity = findQuerySensitivity()
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
