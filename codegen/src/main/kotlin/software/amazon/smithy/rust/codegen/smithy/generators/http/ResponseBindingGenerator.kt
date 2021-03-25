/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators.http

import software.amazon.smithy.model.knowledge.HttpBinding
import software.amazon.smithy.model.knowledge.HttpBindingIndex
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.DocumentShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.model.traits.MediaTypeTrait
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.render
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.rustlang.withBlock
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.rustType
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.toSnakeCase

class ResponseBindingGenerator(protocolConfig: ProtocolConfig, private val operationShape: OperationShape) {
    private val runtimeConfig = protocolConfig.runtimeConfig
    private val symbolProvider = protocolConfig.symbolProvider
    private val model = protocolConfig.model
    private val index = HttpBindingIndex.of(model)
    private val headerUtil = CargoDependency.SmithyHttp(runtimeConfig).asType().member("header")
    private val defaultTimestampFormat = TimestampFormatTrait.Format.EPOCH_SECONDS
    private val instant = RuntimeType.Instant(runtimeConfig).toSymbol().rustType()

    /**
     * Generate a function to deserialize [binding] from HTTP headers
     *
     * The name of the resulting function is returned as a String
     *
     * Generates a function like:
     * ```rust
     * fn parse_foo(headers: &http::HeaderMap) -> Result<Option<String>, ParseError> {
     *   ...
     * }
     * ```
     */
    fun generateDeserializeHeaderFn(binding: HttpBinding, writer: RustWriter): String {
        val outputT = symbolProvider.toSymbol(binding.member)
        val fnName = "parse_from_header_${binding.memberName.toSnakeCase()}"
        writer.rustBlock(
            "fn $fnName(headers: &#T::HeaderMap) -> Result<#T, #T::ParseError>",
            RuntimeType.http,
            outputT,
            headerUtil
        ) {
            deserializeFromHeader(binding)
        }
        return fnName
    }

    /**
     * Generate a function to deserialize `[binding]` from the response payload
     */
    fun generateDeserializePayloadFn(
        binding: HttpBinding,
        errorT: RuntimeType,
        rustWriter: RustWriter,
        structuredHandler: RustWriter.(String) -> Unit
    ): String {
        check(binding.location == HttpBinding.Location.PAYLOAD)
        val outputT = symbolProvider.toSymbol(binding.member)
        val fnName = "parse_from_payload_${binding.memberName.toSnakeCase()}"
        rustWriter.rustBlock("fn $fnName(body: &[u8]) -> Result<#T, #T>", outputT, errorT) {
            deserializePayloadBody(binding, errorT, structuredHandler)
        }
        return fnName
    }

    private fun RustWriter.deserializePayloadBody(
        binding: HttpBinding,
        errorSymbol: RuntimeType,
        structuredHandler: RustWriter.(String) -> Unit
    ) {
        val member = binding.member
        val targetShape = model.expectShape(member.target)
        // There is an unfortunate bit of dual behavior caused by an empty body causing the output to be `None` instead
        // of an empty instance of the response type.
        withBlock("(!body.is_empty()).then(||{", "}).transpose()") {
            when (targetShape) {
                is StructureShape, is UnionShape -> this.structuredHandler("body")
                is StringShape -> {
                    rustTemplate(
                        "let body_str = std::str::from_utf8(&body).map_err(#{error_symbol}::unhandled)?;",
                        "error_symbol" to errorSymbol
                    )
                    if (targetShape.hasTrait(EnumTrait::class.java)) {
                        rust(
                            "Ok(#T::from(body_str))",
                            symbolProvider.toSymbol(targetShape)
                        )
                    } else {
                        rust("Ok(body_str.to_string())")
                    }
                }
                is BlobShape -> rust(
                    "Ok(#T::new(body))",
                    RuntimeType.Blob(runtimeConfig)
                )
                is DocumentShape -> rust("""unimplemented!()""")
                else -> TODO("unexpected shape: $targetShape")
            }
        }
    }

    /** Parse a value from a header
     * This function produces an expression which produces the precise output type required by the output shape
     */
    private fun RustWriter.deserializeFromHeader(binding: HttpBinding) {
        check(binding.location == HttpBinding.Location.HEADER)
        val targetType = model.expectShape(binding.member.target)
        val rustType = symbolProvider.toSymbol(targetType).rustType().stripOuter<RustType.Option>()
        val (coreType, coreShape) = if (targetType is CollectionShape) {
            rustType.stripOuter<RustType.Container>() to model.expectShape(targetType.member.target)
        } else {
            rustType to targetType
        }
        val parsedValue = safeName()
        if (coreType == instant) {
            val timestampFormat =
                index.determineTimestampFormat(
                    binding.member,
                    HttpBinding.Location.HEADER,
                    defaultTimestampFormat
                )
            val timestampFormatType = RuntimeType.TimestampFormat(runtimeConfig, timestampFormat)
            rust(
                "let $parsedValue: Vec<${coreType.render(true)}> = #T::many_dates(&headers, ${binding.locationName.dq()}, #T)?;",
                headerUtil,
                timestampFormatType
            )
        } else {
            rust(
                "let $parsedValue: Vec<${coreType.render(true)}> = #T::read_many(&headers, ${binding.locationName.dq()})?;",
                headerUtil
            )
            if (coreShape.hasTrait(MediaTypeTrait::class.java)) {
                rustTemplate(
                    """let $parsedValue: Result<Vec<_>, _> = $parsedValue
                        .iter().map(|s|
                            #{base_64_decode}(s).map_err(|_|#{header}::ParseError)
                            .and_then(|bytes|String::from_utf8(bytes).map_err(|_|#{header}::ParseError))
                        ).collect();""",
                    "base_64_decode" to RuntimeType.Base64Decode(runtimeConfig),
                    "header" to headerUtil
                )
                rust("let $parsedValue = $parsedValue?;")
            }
        }
        when (rustType) {
            is RustType.Vec ->
                rust(
                    """
                Ok(if !$parsedValue.is_empty() {
                    Some($parsedValue.into())
                } else {
                    None
                })
                """
                )
            is RustType.HashSet ->
                rust(
                    """
                Ok(if !$parsedValue.is_empty() {
                    Some($parsedValue.into_iter().collect())
                } else {
                    None
                })
                """
                )
            else ->
                rustTemplate(
                    """
                    if $parsedValue.len() > 1 {
                        Err(#{header_util}::ParseError)
                    } else {
                        let mut $parsedValue = $parsedValue;
                        Ok($parsedValue.pop())
                    }
                """,
                    "header_util" to headerUtil
                )
        }
    }
}
