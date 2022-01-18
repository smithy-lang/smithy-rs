/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators.http

import software.amazon.smithy.codegen.core.CodegenException
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
import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.autoDeref
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.OperationBuildError
import software.amazon.smithy.rust.codegen.smithy.generators.operationBuildError
import software.amazon.smithy.rust.codegen.smithy.generators.redactIfNecessary
import software.amazon.smithy.rust.codegen.smithy.isOptional
import software.amazon.smithy.rust.codegen.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.util.UNREACHABLE
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.expectMember
import software.amazon.smithy.rust.codegen.util.hasTrait
import software.amazon.smithy.rust.codegen.util.inputShape
import software.amazon.smithy.rust.codegen.util.isPrimitive

fun HttpTrait.uriFormatString(): String {
    return uri.rustFormatString("/", "/")
}

fun SmithyPattern.rustFormatString(prefix: String, separator: String): String {
    val base = segments.joinToString(separator = separator, prefix = prefix) {
        when {
            it.isLabel -> "{${it.content}}"
            else -> it.content
        }
    }
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
 */
class RequestBindingGenerator(
    codegenContext: CodegenContext,
    private val protocol: Protocol,
    private val operationShape: OperationShape,
) {
    private val model = codegenContext.model
    private val inputShape = operationShape.inputShape(model)
    private val symbolProvider = codegenContext.symbolProvider
    private val runtimeConfig = codegenContext.runtimeConfig
    private val httpTrait = protocol.httpBindingResolver.httpTrait(operationShape)
    private val httpBindingGenerator = HttpBindingGenerator(protocol, codegenContext, operationShape)
    private val index = HttpBindingIndex.of(model)
    private val Encoder = CargoDependency.SmithyTypes(runtimeConfig).asType().member("primitive::Encoder")
    private val headerUtil = CargoDependency.SmithyHttp(runtimeConfig).asType().member("header")

    private val codegenScope = arrayOf(
        "BuildError" to runtimeConfig.operationBuildError(),
        "HttpRequestBuilder" to RuntimeType.HttpRequestBuilder,
        "Input" to symbolProvider.toSymbol(inputShape),
    )

    /**
     * Generates `update_http_builder` and all necessary dependency functions into the impl block provided by
     * [implBlockWriter]. The specific behavior is configured by [httpTrait].
     */
    fun renderUpdateHttpBuilder(implBlockWriter: RustWriter) {
        uriBase(implBlockWriter)
        val hasHeaders = addHeaders(implBlockWriter)
        val hasQuery = uriQuery(implBlockWriter)
        Attribute.Custom("allow(clippy::unnecessary_wraps)").render(implBlockWriter)
        implBlockWriter.rustBlockTemplate(
            """
            fn update_http_builder(
                input: &#{Input},
                builder: #{HttpRequestBuilder}
            ) -> std::result::Result<#{HttpRequestBuilder}, #{BuildError}>
            """,
            *codegenScope
        ) {
            write("let mut uri = String::new();")
            write("uri_base(input, &mut uri)?;")
            if (hasQuery) {
                write("uri_query(input, &mut uri)?;")
            }
            if (hasHeaders) {
                write("let builder = add_headers(input, builder)?;")
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
        val headers = index.getRequestBindings(operationShape, HttpBinding.Location.HEADER)
        val prefixHeaders = index.getRequestBindings(
            operationShape,
            HttpBinding.Location.PREFIX_HEADERS
        )

        if (headers.isEmpty() && prefixHeaders.isEmpty()) {
            return false
        }
        writer.rustBlockTemplate(
            """
            fn add_headers(
                _input: &#{Input},
                mut builder: #{HttpRequestBuilder}
            ) -> std::result::Result<#{HttpRequestBuilder}, #{BuildError}>
            """,
            *codegenScope,
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
            else -> UNREACHABLE("unexpected member for prefix headers: $memberType")
        }
        ifSet(memberType, memberSymbol, "&_input.$memberName") { field ->
            val listHeader = memberType is CollectionShape
            rustTemplate(
                """
                for (k, v) in $field {
                    use std::str::FromStr;
                    let header_name = http::header::HeaderName::from_str(&format!("{}{}", ${httpBinding.locationName.dq()}, &k)).map_err(|err| {
                        #{build_error}::InvalidField { field: ${memberName.dq()}, details: format!("`{}` cannot be used as a header name: {}", k, err)}
                    })?;
                    use std::convert::TryFrom;
                    let header_value = ${headerFmtFun(this, target, memberShape, "v", listHeader)};
                    let header_value = http::header::HeaderValue::try_from(&*header_value).map_err(|err| {
                        #{build_error}::InvalidField {
                            field: ${memberName.dq()},
                            details: format!("`{}` cannot be used as a header value: {}", ${
                redactIfNecessary(
                    memberShape,
                    model,
                    "v"
                )
                }, err)}
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
        ifSet(memberType, memberSymbol, "&_input.$memberName") { field ->
            val isListHeader = memberType is CollectionShape
            listForEach(memberType, field) { innerField, targetId ->
                val innerMemberType = model.expectShape(targetId)
                if (innerMemberType.isPrimitive()) {
                    rust("let mut encoder = #T::from(${autoDeref(innerField)});", Encoder)
                }
                val formatted = headerFmtFun(this, innerMemberType, memberShape, innerField, isListHeader)
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
    private fun headerFmtFun(writer: RustWriter, target: Shape, member: MemberShape, targetName: String, isListHeader: Boolean): String {
        fun quoteValue(value: String): String {
            // Timestamp shapes are not quoted in header lists
            return if (isListHeader && !target.isTimestampShape) {
                val quoteFn = writer.format(headerUtil.member("quote_header_value"))
                "$quoteFn($value)"
            } else {
                value
            }
        }
        return when {
            target.isStringShape -> {
                if (target.hasTrait<MediaTypeTrait>()) {
                    val func = writer.format(RuntimeType.Base64Encode(runtimeConfig))
                    "$func(&$targetName)"
                } else {
                    quoteValue("AsRef::<str>::as_ref($targetName)")
                }
            }
            target.isTimestampShape -> {
                val timestampFormat =
                    index.determineTimestampFormat(member, HttpBinding.Location.HEADER, protocol.defaultTimestampFormat)
                val timestampFormatType = RuntimeType.TimestampFormat(runtimeConfig, timestampFormat)
                quoteValue("$targetName.fmt(${writer.format(timestampFormatType)})?")
            }
            target.isListShape || target.isMemberShape -> {
                throw IllegalArgumentException("lists should be handled at a higher level")
            }
            target.isPrimitive() -> {
                "encoder.encode()"
            }
            else -> throw CodegenException("unexpected shape: $target")
        }
    }

    /** URI Generation **/

    /**
     * Generate a function to build the request URI
     */
    private fun uriBase(writer: RustWriter) {
        val formatString = httpTrait.uriFormatString()
        // name of a local variable containing this member's component of the URI
        val local = { member: MemberShape -> symbolProvider.toMemberName(member) }
        val args = httpTrait.uri.labels.map { label ->
            val member = inputShape.expectMember(label.content)
            "${label.content} = ${local(member)}"
        }
        val combinedArgs = listOf(formatString, *args.toTypedArray())
        writer.addImport(RuntimeType.stdfmt.member("Write").toSymbol(), null)
        writer.rustBlockTemplate(
            "fn uri_base(_input: &#{Input}, output: &mut String) -> Result<(), #{BuildError}>",
            *codegenScope
        ) {
            httpTrait.uri.labels.map { label ->
                val member = inputShape.expectMember(label.content)
                serializeLabel(member, label, local(member))
            }
            rust("""write!(output, ${combinedArgs.joinToString(", ")}).expect("formatting should succeed");""")
            rust("Ok(())")
        }
    }

    /**
     * When needed, generate a function to build a query string
     *
     * This function uses aws_smithy_http::query::Query to append params to a query string:
     * ```rust
     *    fn uri_query(input: &Input, mut output: &mut String) {
     *      let mut query = aws_smithy_http::query::Query::new(&mut output);
     *      if let Some(inner_89) = &input.null_value {
     *          query.push_kv("Null", &aws_smithy_http::query::fmt_string(&inner_89));
     *      }
     *      if let Some(inner_90) = &input.empty_string {
     *          query.push_kv("Empty", &aws_smithy_http::query::fmt_string(&inner_90));
     *      }
     *    }
     *  ```
     */
    private fun uriQuery(writer: RustWriter): Boolean {
        // Don't bother generating the function if we aren't going to make a query string
        val dynamicParams = index.getRequestBindings(operationShape, HttpBinding.Location.QUERY)
        val mapParams = index.getRequestBindings(operationShape, HttpBinding.Location.QUERY_PARAMS)
        val literalParams = httpTrait.uri.queryLiterals
        if (dynamicParams.isEmpty() && literalParams.isEmpty()) {
            return false
        }
        val preloadedParams = literalParams.keys + dynamicParams.map { it.locationName }
        writer.rustBlockTemplate(
            "fn uri_query(_input: &#{Input}, mut output: &mut String) -> Result<(), #{BuildError}>",
            *codegenScope
        ) {
            write("let mut query = #T::new(&mut output);", RuntimeType.QueryFormat(runtimeConfig, "Writer"))
            literalParams.forEach { (k, v) ->
                // When `v` is an empty string, no value should be set.
                // this generates a query string like `?k=v&xyz`
                if (v.isEmpty()) {
                    rust("query.push_v(${k.dq()});")
                } else {
                    rust("query.push_kv(${k.dq()}, ${v.dq()});")
                }
            }

            if (mapParams.isNotEmpty()) {
                rust("let protected_params = ${preloadedParams.joinToString(prefix = "[", postfix = "]") { it.dq() }};")
            }
            mapParams.forEach { param ->
                val memberShape = param.member
                val memberSymbol = symbolProvider.toSymbol(memberShape)
                val memberName = symbolProvider.toMemberName(memberShape)
                val targetShape = model.expectShape(memberShape.target, MapShape::class.java)
                val stringFormatter = RuntimeType.QueryFormat(runtimeConfig, "fmt_string")
                ifSet(model.expectShape(param.member.target), memberSymbol, "&_input.$memberName") { field ->
                    rustBlock("for (k, v) in $field") {
                        // if v is a list, generate another level of iteration
                        listForEach(model.expectShape(targetShape.value.target), "v") { innerField, _ ->
                            rustBlock("if !protected_params.contains(&k.as_str())") {
                                rust("query.push_kv(&#1T(k), &#1T($innerField));", stringFormatter)
                            }
                        }
                    }
                }
            }

            dynamicParams.forEach { param ->
                val memberShape = param.member
                val memberSymbol = symbolProvider.toSymbol(memberShape)
                val memberName = symbolProvider.toMemberName(memberShape)
                val outerTarget = model.expectShape(memberShape.target)
                ifSet(outerTarget, memberSymbol, "&_input.$memberName") { field ->
                    // if `param` is a list, generate another level of iteration
                    listForEach(outerTarget, field) { innerField, targetId ->
                        val target = model.expectShape(targetId)
                        rust(
                            "query.push_kv(${param.locationName.dq()}, ${
                            paramFmtFun(writer, target, memberShape, innerField)
                            });"
                        )
                    }
                }
            }
            writer.rust("Ok(())")
        }
        return true
    }

    /**
     * Format [member] when used as a queryParam
     */
    private fun paramFmtFun(writer: RustWriter, target: Shape, member: MemberShape, targetName: String): String {
        return when {
            target.isStringShape -> {
                val func = writer.format(RuntimeType.QueryFormat(runtimeConfig, "fmt_string"))
                "&$func(&$targetName)"
            }
            target.isTimestampShape -> {
                val timestampFormat =
                    index.determineTimestampFormat(member, HttpBinding.Location.QUERY, protocol.defaultTimestampFormat)
                val timestampFormatType = RuntimeType.TimestampFormat(runtimeConfig, timestampFormat)
                val func = writer.format(RuntimeType.QueryFormat(runtimeConfig, "fmt_timestamp"))
                "&$func($targetName, ${writer.format(timestampFormatType)})?"
            }
            target.isListShape || target.isMemberShape -> {
                throw IllegalArgumentException("lists should be handled at a higher level")
            }
            else -> {
                "${writer.format(Encoder)}::from(${autoDeref(targetName)}).encode()"
            }
        }
    }

    private fun RustWriter.serializeLabel(member: MemberShape, label: SmithyPattern.Segment, outputVar: String) {
        val target = model.expectShape(member.target)
        val symbol = symbolProvider.toSymbol(member)
        val buildError = {
            OperationBuildError(runtimeConfig).missingField(
                this,
                symbolProvider.toMemberName(member),
                "cannot be empty or unset"
            )
        }
        val input = safeName("input")
        rust("let $input = &_input.${symbolProvider.toMemberName(member)};")
        if (symbol.isOptional()) {
            rust("let $input = $input.as_ref().ok_or(${buildError()})?;")
        }
        when {
            target.isStringShape -> {
                val func = format(RuntimeType.LabelFormat(runtimeConfig, "fmt_string"))
                rust("let $outputVar = $func($input, ${label.isGreedyLabel});")
            }
            target.isTimestampShape -> {
                val timestampFormat =
                    index.determineTimestampFormat(member, HttpBinding.Location.LABEL, protocol.defaultTimestampFormat)
                val timestampFormatType = RuntimeType.TimestampFormat(runtimeConfig, timestampFormat)
                val func = format(RuntimeType.LabelFormat(runtimeConfig, "fmt_timestamp"))
                rust("let $outputVar = $func($input, ${format(timestampFormatType)})?;")
            }
            else -> {
                rust(
                    "let mut ${outputVar}_encoder = #T::from(${autoDeref(input)}); let $outputVar = ${outputVar}_encoder.encode();",
                    Encoder
                )
            }
        }
        rust(
            """
            if $outputVar.is_empty() {
                return Err(${buildError()})
            }
            """
        )
    }
    /** End URI generation **/
}
