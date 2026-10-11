/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators.http

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.HttpBinding
import software.amazon.smithy.model.knowledge.HttpBindingIndex
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.SimpleShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.traits.MediaTypeTrait
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.render
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.makeOptional
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.isPrimitive

/**
 * Renders parsing for one modeled HTTP header value from an iterator of raw byte slices.
 *
 * This is intentionally independent of a concrete Smithy protocol. HTTP header bindings share
 * their value grammar across clients and servers; callers provide the model-derived timestamp
 * default and any target-specific customizations. Policy decisions such as whether a client may
 * skip an unreadable response header remain outside this parser so server request parsing stays
 * strict.
 */
class HttpHeaderValueParserGenerator(
    private val model: Model,
    private val symbolProvider: SymbolProvider,
    private val codegenTarget: CodegenTarget,
    private val runtimeConfig: RuntimeConfig,
    private val index: HttpBindingIndex = HttpBindingIndex.of(model),
    private val defaultTimestampFormat: TimestampFormatTrait.Format = TimestampFormatTrait.Format.EPOCH_SECONDS,
    private val customizations: List<HttpBindingCustomization> = emptyList(),
) {
    constructor(
        codegenContext: CodegenContext,
        symbolProvider: SymbolProvider = codegenContext.symbolProvider,
        defaultTimestampFormat: TimestampFormatTrait.Format = TimestampFormatTrait.Format.EPOCH_SECONDS,
        customizations: List<HttpBindingCustomization> = emptyList(),
    ) : this(
        codegenContext.model,
        symbolProvider,
        codegenContext.target,
        codegenContext.runtimeConfig,
        HttpBindingIndex.of(codegenContext.model),
        defaultTimestampFormat,
        customizations,
    )

    private val headerUtil = RuntimeType.smithyHttp(runtimeConfig).resolve("header")

    val parseError: RuntimeType = headerUtil.resolve("ParseError")

    fun outputType(memberShape: MemberShape): Symbol = symbolProvider.toSymbol(memberShape).makeOptional()

    /**
     * Emits an expression returning `Result<Option<T>, ParseError>`.
     *
     * [headersExpression] must yield an iterator whose items are raw `&[u8]` header values.
     */
    fun RustWriter.renderHeaderValueParser(
        targetShape: Shape,
        memberShape: MemberShape,
        headersExpression: String = "headers",
    ) {
        val rustType = symbolProvider.toSymbol(targetShape).rustType().stripOuter<RustType.Option>()
        // Normally, we go through a flow that looks for `,`s but that's wrong if the output
        // is just a single string (which might include `,`s.).
        // MediaType doesn't include `,` since it's base64, send that through the normal path.
        if (targetShape is StringShape && !targetShape.hasTrait<MediaTypeTrait>()) {
            rust("#T::one_or_none_bytes($headersExpression)", headerUtil)
            return
        }
        val (coreType, coreShape) =
            if (targetShape is CollectionShape) {
                val coreShape = model.expectShape(targetShape.member.target)
                symbolProvider.toSymbol(coreShape).rustType() to coreShape
            } else {
                rustType to targetShape
            }
        val parsedValue = safeName()
        if (coreShape.isTimestampShape()) {
            val timestampFormat =
                index.determineTimestampFormat(
                    memberShape,
                    HttpBinding.Location.HEADER,
                    defaultTimestampFormat,
                )
            val timestampFormatType = RuntimeType.parseTimestampFormat(codegenTarget, runtimeConfig, timestampFormat)
            rust(
                "let $parsedValue: Vec<${coreType.render()}> = #T::many_dates_bytes($headersExpression, #T)?",
                headerUtil,
                timestampFormatType,
            )
            for (customization in customizations) {
                customization.section(HttpBindingSection.AfterDeserializingIntoADateTimeOfHttpHeaders(memberShape))(this)
            }
            rust(";")
        } else if (coreShape.isPrimitive()) {
            rust(
                "let $parsedValue = #T::read_many_primitive_bytes::<${coreType.render()}>($headersExpression)?;",
                headerUtil,
            )
        } else {
            rust(
                "let $parsedValue: Vec<${coreType.render()}> = #T::read_many_from_str_bytes($headersExpression)?;",
                headerUtil,
            )
            if (coreShape.hasTrait<MediaTypeTrait>()) {
                rustTemplate(
                    """
                    let $parsedValue: std::result::Result<Vec<_>, _> = $parsedValue
                        .iter().map(|s|
                            #{base_64_decode}(s).map_err(|_|#{header}::ParseError::new("failed to decode base64"))
                            .and_then(|bytes|String::from_utf8(bytes).map_err(|_|#{header}::ParseError::new("base64 encoded data was not valid utf-8")))
                        ).collect();
                    """,
                    "base_64_decode" to RuntimeType.base64Decode(runtimeConfig),
                    "header" to headerUtil,
                )
                rust("let $parsedValue = $parsedValue?;")
            }
        }
        when (rustType) {
            is RustType.Vec ->
                rust(
                    """
                    Ok(if !$parsedValue.is_empty() {
                        Some($parsedValue)
                    } else {
                        None
                    })
                    """,
                )

            is RustType.HashSet ->
                rust(
                    """
                    Ok(if !$parsedValue.is_empty() {
                        Some($parsedValue.into_iter().collect())
                    } else {
                        None
                    })
                    """,
                )

            else -> {
                if (targetShape is ListShape) {
                    // This is a constrained list shape and we must therefore be generating a server SDK.
                    check(codegenTarget == CodegenTarget.SERVER)
                    check(rustType is RustType.Opaque)
                    rust(
                        """
                        Ok(if !$parsedValue.is_empty() {
                            Some(#T($parsedValue))
                        } else {
                            None
                        })
                        """,
                        symbolProvider.toSymbol(targetShape),
                    )
                } else {
                    check(targetShape is SimpleShape)
                    rustTemplate(
                        """
                        if $parsedValue.len() > 1 {
                            Err(#{header_util}::ParseError::new(format!("expected one item but found {}", $parsedValue.len())))
                        } else {
                            let mut $parsedValue = $parsedValue;
                            Ok($parsedValue.pop())
                        }
                        """,
                        "header_util" to headerUtil,
                    )
                }
            }
        }
    }
}
