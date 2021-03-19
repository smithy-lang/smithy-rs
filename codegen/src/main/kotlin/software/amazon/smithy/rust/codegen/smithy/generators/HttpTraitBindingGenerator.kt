/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.HttpBinding
import software.amazon.smithy.model.knowledge.HttpBindingIndex
import software.amazon.smithy.model.pattern.SmithyPattern
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.HttpTrait
import software.amazon.smithy.model.traits.MediaTypeTrait
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.util.dq

fun HttpTrait.uriFormatString(): String {
    val base = uri.segments.joinToString("/", prefix = "/") {
        when {
            it.isLabel -> "{${it.content}}"
            else -> it.content
        }
    }
    // TODO: support query literals
    return base.dq()
}

/**
 * HttpTraitBindingGenerator
 *
 * Generates methods to serialize and deserialize requests/responses based on the HTTP trait. Specifically:
 * 1. `fn update_http_request(builder: http::request::Builder) -> Builder`
 *
 * This method takes a builder (perhaps pre configured with some headers) from the caller and sets the HTTP
 * headers & URL based on the HTTP trait implementation.
 *
 * More work is required to implement the entirety of https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html
 * Specifically:
 * TODO: httpPrefixHeaders; 4h
 * TODO: Deserialization of all fields; 1w
 */
class HttpTraitBindingGenerator(
    val model: Model,
    private val symbolProvider: SymbolProvider,
    private val runtimeConfig: RuntimeConfig,
    private val writer: RustWriter,
    private val shape: OperationShape,
    private val inputShape: StructureShape,
    private val httpTrait: HttpTrait
) {
    // TODO: make defaultTimestampFormat configurable
    private val defaultTimestampFormat = TimestampFormatTrait.Format.EPOCH_SECONDS
    private val index = HttpBindingIndex.of(model)
    private val buildError = runtimeConfig.operationBuildError()

    /**
     * Generates `update_http_builder` and all necessary dependency functions into the impl block provided by
     * [implBlockWriter]. The specific behavior is configured by [httpTrait].
     */
    fun renderUpdateHttpBuilder(implBlockWriter: RustWriter) {
        uriBase(implBlockWriter)
        val hasHeaders = addHeaders(implBlockWriter)
        val hasQuery = uriQuery(implBlockWriter)
        implBlockWriter.rustBlock(
            "fn update_http_builder(&self, builder: #1T) -> Result<#1T, #2T>",
            RuntimeType.HttpRequestBuilder,
            buildError
        ) {
            write("let mut uri = String::new();")
            write("self.uri_base(&mut uri);")
            if (hasQuery) {
                write("self.uri_query(&mut uri);")
            }
            if (hasHeaders) {
                write("let builder = self.add_headers(builder)?;")
            }
            write("Ok(builder.method(${httpTrait.method.dq()}).uri(uri))")
        }
    }

    /** Header Generation **/

    /**
     * If the protocol sets headers, generate a function to add headers to a request.
     * Returns `true` if headers were generated and false if are not required.
     */
    private fun addHeaders(writer: RustWriter): Boolean {
        val headers = index.getRequestBindings(shape, HttpBinding.Location.HEADER)
        val prefixHeaders = index.getRequestBindings(
            shape,
            HttpBinding.Location.PREFIX_HEADERS
        )
        val buildErrorT = runtimeConfig.operationBuildError()

        if (headers.isEmpty() && prefixHeaders.isEmpty()) {
            return false
        }
        writer.rustBlock(
            "fn add_headers(&self, mut builder: #1T) -> Result<#1T, #2T>",
            RuntimeType.HttpRequestBuilder,
            buildErrorT
        ) {
            headers.forEach { httpBinding -> renderHeaders(httpBinding) }
            prefixHeaders.forEach { httpBinding ->
                renderPrefixHeaders(httpBinding)
            }
            rust("Ok(builder)")
        }
        return true
    }

    private fun RustWriter.renderPrefixHeaders(httpBinding: HttpBinding) {
        val memberShape = httpBinding.member
        val memberType = model.expectShape(memberShape.target)
        val memberSymbol = symbolProvider.toSymbol(memberShape)
        val memberName = symbolProvider.toMemberName(memberShape)
        val target = when (memberType) {
            is CollectionShape -> model.expectShape(memberType.member.target)
            is MapShape -> model.expectShape(memberType.value.target)
            else -> TODO("unexpected member for prefix headers: $memberType")
        }
        ifSet(memberType, memberSymbol, "&self.$memberName") { field ->
            rustTemplate(
                """
                for (k, v) in $field {
                    use std::str::FromStr;
                    let header_name = http::header::HeaderName::from_str(&format!("{}{}", ${httpBinding.locationName.dq()}, &k)).map_err(|err| {
                        #{build_error}::InvalidField { field: ${memberName.dq()}, details: format!("`{}` cannot be used as a header name: {}", k, err)}
                    })?;
                    use std::convert::TryFrom;
                    let header_value = ${headerFmtFun(target, memberShape, "v")};
                    let header_value = http::header::HeaderValue::try_from(header_value).map_err(|err| {
                        #{build_error}::InvalidField {
                            field: ${memberName.dq()},
                            details: format!("`{}` cannot be used as a header value: {}", ${redactIfNecessary(memberShape, model,"v")}, err)}
                    })?;
                    builder = builder.header(header_name, header_value);
                }

            """,
                "build_error" to runtimeConfig.operationBuildError()
            )
        }
    }

    private fun RustWriter.renderHeaders(httpBinding: HttpBinding) {
        val memberShape = httpBinding.member
        val memberType = model.expectShape(memberShape.target)
        val memberSymbol = symbolProvider.toSymbol(memberShape)
        val memberName = symbolProvider.toMemberName(memberShape)
        ifSet(memberType, memberSymbol, "&self.$memberName") { field ->
            ListForEach(memberType, field) { innerField, targetId ->
                val innerMemberType = model.expectShape(targetId)
                val formatted = headerFmtFun(innerMemberType, memberShape, innerField)
                val safeName = safeName("formatted")
                write("let $safeName = $formatted;")
                rustBlock("if !$safeName.is_empty()") {
                    rustTemplate(
                        """
                        use std::convert::TryFrom;
                        let header_value = $safeName;
                        let header_value = http::header::HeaderValue::try_from(&*header_value).map_err(|err| {
                            #{build_error}::InvalidField { field: ${memberName.dq()}, details: format!("`{}` cannot be used as a header value: {}", &${
                        redactIfNecessary(
                            memberShape,
                            model,
                            "header_value"
                        )
                        }, err)}
                        })?;
                        builder = builder.header(${httpBinding.locationName.dq()}, header_value);
                    """,
                        "build_error" to runtimeConfig.operationBuildError()
                    )
                }
            }
        }
    }

    /**
     * Format [member] in the when used as an HTTP header
     */
    private fun headerFmtFun(target: Shape, member: MemberShape, targetName: String): String {
        return when {
            target.isStringShape -> {
                /*val func = */ if (target.hasTrait(MediaTypeTrait::class.java)) {
                    val func = writer.format(RuntimeType.Base64Encode(runtimeConfig))
                    "$func(&${writer.useAs(target, targetName)})"
                } else {
                    writer.useAs(target, targetName)
                    // writer.format(RuntimeType.QueryFormat(runtimeConfig, "fmt_string"))
                }
            }
            target.isTimestampShape -> {
                val timestampFormat =
                    index.determineTimestampFormat(member, HttpBinding.Location.HEADER, defaultTimestampFormat)
                val timestampFormatType = RuntimeType.TimestampFormat(runtimeConfig, timestampFormat)
                "$targetName.fmt(${writer.format(timestampFormatType)})"
            }
            target.isListShape || target.isMemberShape -> {
                throw IllegalArgumentException("lists should be handled at a higher level")
            }
            else -> {
                val func = writer.format(RuntimeType.QueryFormat(runtimeConfig, "fmt_default"))
                "$func(&$targetName)"
            }
        }
    }

    /** URI Generation **/

    /**
     * Generate a function to build the request URI
     */
    private fun uriBase(writer: RustWriter) {
        val formatString = httpTrait.uriFormatString()
        val args = httpTrait.uri.labels.map { label ->
            val member = inputShape.getMember(label.content).get()
            "${label.content} = ${labelFmtFun(model.expectShape(member.target), member, label)}"
        }
        val combinedArgs = listOf(formatString, *args.toTypedArray())
        writer.addImport(RuntimeType.stdfmt.member("Write").toSymbol(), null)
        writer.rustBlock("fn uri_base(&self, output: &mut String)") {
            write("write!(output, ${combinedArgs.joinToString(", ")}).expect(\"formatting should succeed\")")
        }
    }

    /**
     * When needed, generate a function to build a query string
     */
    private fun uriQuery(writer: RustWriter): Boolean {
        // Don't bother generating the function if we aren't going to make a query string
        val queryParams = index.getRequestBindings(shape, HttpBinding.Location.QUERY)
        if (queryParams.isEmpty()) {
            return false
        }
        writer.rustBlock("fn uri_query(&self, output: &mut String)") {
            write("let mut params = Vec::new();")

            queryParams.forEach { param ->
                val memberShape = param.member
                val memberSymbol = symbolProvider.toSymbol(memberShape)
                val memberName = symbolProvider.toMemberName(memberShape)
                val outerTarget = model.expectShape(memberShape.target)
                ifSet(outerTarget, memberSymbol, "&self.$memberName") { field ->
                    ListForEach(outerTarget, field) { innerField, targetId ->
                        val target = model.expectShape(targetId)
                        write(
                            "params.push((${param.locationName.dq()}, ${
                            paramFmtFun(
                                target,
                                memberShape,
                                innerField
                            )
                            }));"
                        )
                    }
                }
            }
            write("#T(params, output)", RuntimeType.QueryFormat(runtimeConfig, "write"))
        }
        return true
    }

    /**
     * Format [member] when used as a queryParam
     */
    private fun paramFmtFun(target: Shape, member: MemberShape, targetName: String): String {
        return when {
            target.isStringShape -> {
                val func = writer.format(RuntimeType.QueryFormat(runtimeConfig, "fmt_string"))
                "$func(&${writer.useAs(target, targetName)})"
            }
            target.isTimestampShape -> {
                val timestampFormat =
                    index.determineTimestampFormat(member, HttpBinding.Location.QUERY, defaultTimestampFormat)
                val timestampFormatType = RuntimeType.TimestampFormat(runtimeConfig, timestampFormat)
                val func = writer.format(RuntimeType.QueryFormat(runtimeConfig, "fmt_timestamp"))
                "$func($targetName, ${writer.format(timestampFormatType)})"
            }
            target.isListShape || target.isMemberShape -> {
                throw IllegalArgumentException("lists should be handled at a higher level")
            }
            else -> {
                val func = writer.format(RuntimeType.QueryFormat(runtimeConfig, "fmt_default"))
                "$func(&$targetName)"
            }
        }
    }

    /**
     * Format [member] when used as an HTTP Label (`/bucket/{key}`)
     */
    private fun labelFmtFun(target: Shape, member: MemberShape, label: SmithyPattern.Segment): String {
        val memberName = symbolProvider.toMemberName(member)
        return when {
            target.isStringShape -> {
                val func = writer.format(RuntimeType.LabelFormat(runtimeConfig, "fmt_string"))
                "$func(&self.$memberName, ${label.isGreedyLabel})"
            }
            target.isTimestampShape -> {
                val timestampFormat =
                    index.determineTimestampFormat(member, HttpBinding.Location.LABEL, defaultTimestampFormat)
                val timestampFormatType = RuntimeType.TimestampFormat(runtimeConfig, timestampFormat)
                val func = writer.format(RuntimeType.LabelFormat(runtimeConfig, "fmt_timestamp"))
                "$func(&self.$memberName, ${writer.format(timestampFormatType)})"
            }
            else -> {
                val func = writer.format(RuntimeType.LabelFormat(runtimeConfig, "fmt_default"))
                "$func(&self.$memberName)"
            }
        }
    }

    /** End URI generation **/
}
