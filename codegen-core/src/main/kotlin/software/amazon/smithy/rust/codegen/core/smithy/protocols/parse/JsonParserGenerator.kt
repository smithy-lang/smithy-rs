/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols.parse

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.BooleanShape
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.DocumentShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.NumberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.TimestampShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.model.traits.SparseTrait
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.escape
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.rustlang.withBlockTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.canUseDefault
import software.amazon.smithy.rust.codegen.core.smithy.customize.NamedCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.Section
import software.amazon.smithy.rust.codegen.core.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.renderUnknownVariant
import software.amazon.smithy.rust.codegen.core.smithy.generators.setterName
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.smithy.isRustBoxed
import software.amazon.smithy.rust.codegen.core.smithy.protocols.HttpBindingResolver
import software.amazon.smithy.rust.codegen.core.smithy.protocols.HttpLocation
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolFunctions
import software.amazon.smithy.rust.codegen.core.util.PANIC
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.isTargetUnit
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.utils.StringUtils

/**
 * Class describing a JSON parser section that can be used in a customization.
 */
sealed class JsonParserSection(name: String) : Section(name) {
    data class BeforeBoxingDeserializedMember(val shape: MemberShape) :
        JsonParserSection("BeforeBoxingDeserializedMember")

    data class AfterTimestampDeserializedMember(val shape: MemberShape) :
        JsonParserSection("AfterTimestampDeserializedMember")

    data class AfterBlobDeserializedMember(val shape: MemberShape) : JsonParserSection("AfterBlobDeserializedMember")

    data class AfterDocumentDeserializedMember(val shape: MemberShape) :
        JsonParserSection("AfterDocumentDeserializedMember")

    /**
     * Represents a customization point at the beginning of union deserialization, before any token
     * processing occurs.
     */
    data class BeforeUnionDeserialize(val shape: UnionShape) :
        JsonParserSection("BeforeUnionDeserialize")
}

/**
 * Customization for the JSON parser.
 */
typealias JsonParserCustomization = NamedCustomization<JsonParserSection>

class JsonParserGenerator(
    private val codegenContext: CodegenContext,
    private val httpBindingResolver: HttpBindingResolver,
    /** Function that maps a MemberShape into a JSON field name */
    private val jsonName: (MemberShape) -> String,
    /**
     * Whether we should parse a value for a shape into its associated unconstrained type. For example, when the shape
     * is a `StructureShape`, we should construct and return a builder instead of building into the final `struct` the
     * user gets. This is only relevant for the server, that parses the incoming request and only after enforces
     * constraint traits.
     *
     * The function returns a data class that signals the return symbol that should be parsed, and whether it's
     * unconstrained or not.
     */
    private val returnSymbolToParse: (Shape) -> ReturnSymbolToParse = { shape ->
        ReturnSymbolToParse(codegenContext.symbolProvider.toSymbol(shape), false)
    },
    private val customizations: List<JsonParserCustomization> = listOf(),
    smithyJson: RuntimeType = RuntimeType.smithyJson(codegenContext.runtimeConfig),
) : StructuredDataParserGenerator {
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenTarget = codegenContext.target
    private val protocolFunctions = ProtocolFunctions(codegenContext)
    private val builderInstantiator = codegenContext.builderInstantiator()
    private val codegenScope =
        arrayOf(
            "Error" to smithyJson.resolve("deserialize::error::DeserializeError"),
            "expect_blob_or_null" to smithyJson.resolve("deserialize::token::expect_blob_or_null"),
            "expect_bool_or_null" to smithyJson.resolve("deserialize::token::expect_bool_or_null"),
            "expect_document" to smithyJson.resolve("deserialize::token::expect_document"),
            "expect_number_or_null" to smithyJson.resolve("deserialize::token::expect_number_or_null"),
            "expect_start_array" to smithyJson.resolve("deserialize::token::expect_start_array"),
            "expect_start_object" to smithyJson.resolve("deserialize::token::expect_start_object"),
            "expect_string_or_null" to smithyJson.resolve("deserialize::token::expect_string_or_null"),
            "expect_timestamp_or_null" to smithyJson.resolve("deserialize::token::expect_timestamp_or_null"),
            "json_token_iter" to smithyJson.resolve("deserialize::json_token_iter"),
            "Peekable" to RuntimeType.std.resolve("iter::Peekable"),
            "skip_value" to smithyJson.resolve("deserialize::token::skip_value"),
            "skip_to_end" to smithyJson.resolve("deserialize::token::skip_to_end"),
            "Token" to smithyJson.resolve("deserialize::Token"),
            "or_empty" to orEmptyJson(),
            *preludeScope,
        )

    /**
     * Reusable structure parser implementation that can be used to generate parsing code for
     * operation, error and structure shapes.
     * We still generate the parser symbol even if there are no included members because the server
     * generation requires parsers for all input structures.
     */
    private fun structureParser(
        shape: Shape,
        builderSymbol: Symbol,
        includedMembers: List<MemberShape>,
        fnNameSuffix: String? = null,
    ): RuntimeType {
        return protocolFunctions.deserializeFn(shape, fnNameSuffix) { fnName ->
            val unusedMut = if (includedMembers.isEmpty()) "##[allow(unused_mut)] " else ""
            rustBlockTemplate(
                "pub(crate) fn $fnName(value: &[u8], ${unusedMut}mut builder: #{Builder}) -> Result<#{Builder}, #{Error}>",
                "Builder" to builderSymbol,
                *codegenScope,
            ) {
                rustTemplate(
                    """
                    let mut tokens_owned = #{json_token_iter}(#{or_empty}(value)).peekable();
                    let tokens = &mut tokens_owned;
                    #{expect_start_object}(tokens.next())?;
                    """,
                    *codegenScope,
                )
                deserializeStructInner(includedMembers)
                expectEndOfTokenStream()
                rust("Ok(builder)")
            }
        }
    }

    override fun payloadParser(member: MemberShape): RuntimeType {
        val shape = model.expectShape(member.target)
        val returnSymbolToParse = returnSymbolToParse(shape)
        check(shape is UnionShape || shape is StructureShape || shape is DocumentShape) {
            "Payload parser should only be used on structure shapes, union shapes, and document shapes."
        }
        return protocolFunctions.deserializeFn(shape, fnNameSuffix = "payload") { fnName ->
            rustBlockTemplate(
                "pub(crate) fn $fnName(input: &[u8]) -> Result<#{ReturnType}, #{Error}>",
                *codegenScope,
                "ReturnType" to returnSymbolToParse.symbol,
            ) {
                val input =
                    if (shape is DocumentShape) {
                        "input"
                    } else {
                        "#{or_empty}(input)"
                    }

                rustTemplate(
                    """
                    let mut tokens_owned = #{json_token_iter}($input).peekable();
                    let tokens = &mut tokens_owned;
                    """,
                    *codegenScope,
                )
                rust("let result =")
                deserializeMember(member)
                rustTemplate(".ok_or_else(|| #{Error}::custom(\"expected payload member value\"));", *codegenScope)
                expectEndOfTokenStream()
                rust("result")
            }
        }
    }

    override fun operationParser(operationShape: OperationShape): RuntimeType? {
        // Don't generate an operation JSON deserializer if there is no JSON body
        val httpDocumentMembers = httpBindingResolver.responseMembers(operationShape, HttpLocation.DOCUMENT)
        if (httpDocumentMembers.isEmpty()) {
            return null
        }
        val outputShape = operationShape.outputShape(model)
        return structureParser(operationShape, symbolProvider.symbolForBuilder(outputShape), httpDocumentMembers)
    }

    override fun errorParser(errorShape: StructureShape): RuntimeType? {
        if (errorShape.members().isEmpty()) {
            return null
        }
        return structureParser(
            errorShape,
            symbolProvider.symbolForBuilder(errorShape),
            errorShape.members().toList(),
            fnNameSuffix = "json_err",
        )
    }

    private fun orEmptyJson(): RuntimeType =
        ProtocolFunctions.crossOperationFn("or_empty_doc") {
            rust(
                """
                pub(crate) fn or_empty_doc(data: &[u8]) -> &[u8] {
                    if data.is_empty() {
                        b"{}"
                    } else {
                        data
                    }
                }
                """,
            )
        }

    override fun serverInputParser(operationShape: OperationShape): RuntimeType? {
        val includedMembers = httpBindingResolver.requestMembers(operationShape, HttpLocation.DOCUMENT)
        if (includedMembers.isEmpty()) {
            return null
        }
        val inputShape = operationShape.inputShape(model)
        return structureParser(operationShape, symbolProvider.symbolForBuilder(inputShape), includedMembers)
    }

    private fun RustWriter.expectEndOfTokenStream() {
        rustBlock("if tokens.next().is_some()") {
            rustTemplate(
                "return Err(#{Error}::custom(\"found more JSON tokens after completing parsing\"));",
                *codegenScope,
            )
        }
    }

    private fun RustWriter.deserializeStructInner(members: Collection<MemberShape>) {
        objectKeyLoop(hasMembers = members.isNotEmpty()) {
            rustBlock("match key.to_unescaped()?.as_ref()") {
                for (member in members) {
                    rustBlock("${jsonName(member).dq()} =>") {
                        when (codegenTarget) {
                            CodegenTarget.CLIENT -> {
                                withBlock("builder = builder.${member.setterName()}(", ");") {
                                    deserializeMember(member)
                                }
                            }

                            CodegenTarget.SERVER -> {
                                if (symbolProvider.toSymbol(member).isOptional()) {
                                    withBlock("builder = builder.${member.setterName()}(", ");") {
                                        deserializeMember(member)
                                    }
                                } else {
                                    rust("if let Some(v) = ")
                                    deserializeMember(member)
                                    rust(
                                        """
                                        {
                                            builder = builder.${member.setterName()}(v);
                                        }
                                        """,
                                    )
                                }
                            }
                        }
                    }
                }
                rustTemplate("_ => #{skip_value}(tokens)?", *codegenScope)
            }
        }
    }

    private fun RustWriter.deserializeMember(memberShape: MemberShape) {
        when (val target = model.expectShape(memberShape.target)) {
            is StringShape -> deserializeString(target)
            is BooleanShape -> rustTemplate("#{expect_bool_or_null}(tokens.next())?", *codegenScope)
            is NumberShape -> deserializeNumber(target)
            is BlobShape -> deserializeBlob(memberShape)
            is TimestampShape -> deserializeTimestamp(memberShape)
            is CollectionShape -> deserializeCollection(target)
            is MapShape -> deserializeMap(target)
            is StructureShape -> deserializeStruct(target)
            is UnionShape -> deserializeUnion(target)
            is DocumentShape -> deserializeDocument(memberShape)
            else -> PANIC("unexpected shape: $target")
        }
        val symbol = symbolProvider.toSymbol(memberShape)
        if (symbol.isRustBoxed()) {
            for (customization in customizations) {
                customization.section(JsonParserSection.BeforeBoxingDeserializedMember(memberShape))(this)
            }
            rust(".map(Box::new)")
        }
    }

    private fun RustWriter.deserializeDocument(member: MemberShape) {
        rustTemplate("Some(#{expect_document}(tokens)?)", *codegenScope)
        for (customization in customizations) {
            customization.section(JsonParserSection.AfterDocumentDeserializedMember(member))(this)
        }
    }

    private fun RustWriter.deserializeBlob(member: MemberShape) {
        rustTemplate(
            "#{expect_blob_or_null}(tokens.next())?",
            *codegenScope,
        )
        for (customization in customizations) {
            customization.section(JsonParserSection.AfterBlobDeserializedMember(member))(this)
        }
    }

    private fun RustWriter.deserializeStringInner(
        target: StringShape,
        escapedStrName: String,
    ) {
        withBlock("$escapedStrName.to_unescaped().map(|u|", ")") {
            when (target.hasTrait<EnumTrait>()) {
                true -> {
                    if (returnSymbolToParse(target).isUnconstrained) {
                        rust("u.into_owned()")
                    } else {
                        rust("#T::from(u.as_ref())", symbolProvider.toSymbol(target))
                    }
                }
                false -> rust("u.into_owned()")
            }
        }
    }

    private fun RustWriter.deserializeString(target: StringShape) {
        withBlockTemplate("#{expect_string_or_null}(tokens.next())?.map(|s|", ").transpose()?", *codegenScope) {
            deserializeStringInner(target, "s")
        }
    }

    private fun RustWriter.deserializeNumber(target: NumberShape) {
        if (target.isFloatShape) {
            rustTemplate("#{expect_number_or_null}(tokens.next())?.map(|v| v.to_f32_lossy())", *codegenScope)
        } else if (target.isDoubleShape) {
            rustTemplate("#{expect_number_or_null}(tokens.next())?.map(|v| v.to_f64_lossy())", *codegenScope)
        } else {
            rustTemplate(
                """
                #{expect_number_or_null}(tokens.next())?
                    .map(#{NumberType}::try_from)
                    .transpose()?
                """,
                "NumberType" to returnSymbolToParse(target).symbol,
                *codegenScope,
            )
        }
    }

    private fun RustWriter.deserializeTimestamp(member: MemberShape) {
        val timestampFormat =
            httpBindingResolver.timestampFormat(
                member, HttpLocation.DOCUMENT,
                TimestampFormatTrait.Format.EPOCH_SECONDS, model,
            )
        val timestampFormatType = RuntimeType.parseTimestampFormat(codegenTarget, runtimeConfig, timestampFormat)
        rustTemplate(
            "#{expect_timestamp_or_null}(tokens.next(), #{T})?",
            "T" to timestampFormatType, *codegenScope,
        )
        for (customization in customizations) {
            customization.section(JsonParserSection.AfterTimestampDeserializedMember(member))(this)
        }
    }

    private fun RustWriter.deserializeCollection(shape: CollectionShape) {
        val isSparse = shape.hasTrait<SparseTrait>()
        val (returnSymbol, returnUnconstrainedType) = returnSymbolToParse(shape)
        val parser =
            protocolFunctions.deserializeFn(shape) { fnName ->
                rustBlockTemplate(
                    """
                    pub(crate) fn $fnName<'a, I>(tokens: &mut #{Peekable}<I>) -> Result<Option<#{ReturnType}>, #{Error}>
                        where I: Iterator<Item = Result<#{Token}<'a>, #{Error}>>
                    """,
                    "ReturnType" to returnSymbol,
                    *codegenScope,
                ) {
                    startArrayOrNull {
                        rust("let mut items = Vec::new();")
                        rustBlock("loop") {
                            rustBlock("match tokens.peek()") {
                                rustBlockTemplate("Some(Ok(#{Token}::EndArray { .. })) =>", *codegenScope) {
                                    rust("tokens.next().transpose().unwrap(); break;")
                                }
                                rustBlock("_ => ") {
                                    if (isSparse) {
                                        withBlock("items.push(", ");") {
                                            deserializeMember(shape.member)
                                        }
                                    } else {
                                        withBlock("let value =", ";") {
                                            deserializeMember(shape.member)
                                        }
                                        rust(
                                            """
                                            if let Some(value) = value {
                                                items.push(value);
                                            }
                                            """,
                                        )
                                        codegenTarget.ifServer {
                                            rustTemplate(
                                                """
                                                else {
                                                    return Err(#{Error}::custom("dense list cannot contain null values"));
                                                }
                                                """,
                                                *codegenScope,
                                            )
                                        }
                                    }
                                }
                            }
                        }
                        if (returnUnconstrainedType) {
                            rust("Ok(Some(#{T}(items)))", returnSymbol)
                        } else {
                            rust("Ok(Some(items))")
                        }
                    }
                }
            }
        rust("#T(tokens)?", parser)
    }

    private fun RustWriter.deserializeMap(shape: MapShape) {
        val keyTarget = model.expectShape(shape.key.target, StringShape::class.java)
        val isSparse = shape.hasTrait<SparseTrait>()
        val returnSymbolToParse = returnSymbolToParse(shape)
        val parser =
            protocolFunctions.deserializeFn(shape) { fnName ->
                rustBlockTemplate(
                    """
                    pub(crate) fn $fnName<'a, I>(tokens: &mut #{Peekable}<I>) -> Result<Option<#{ReturnType}>, #{Error}>
                        where I: Iterator<Item = Result<#{Token}<'a>, #{Error}>>
                    """,
                    "ReturnType" to returnSymbolToParse.symbol,
                    *codegenScope,
                ) {
                    startObjectOrNull {
                        rust("let mut map = #T::new();", RuntimeType.HashMap)
                        objectKeyLoop(hasMembers = true) {
                            withBlock("let key =", "?;") {
                                deserializeStringInner(keyTarget, "key")
                            }
                            withBlock("let value =", ";") {
                                deserializeMember(shape.value)
                            }
                            if (isSparse) {
                                rust("map.insert(key, value);")
                            } else {
                                codegenTarget.ifServer {
                                    rustTemplate(
                                        """
                                        match value {
                                            Some(value) => { map.insert(key, value); }
                                            None => return Err(#{Error}::custom("dense map cannot contain null values"))
                                            }""",
                                        *codegenScope,
                                    )
                                }
                                codegenTarget.ifClient {
                                    rustTemplate(
                                        """
                                        if let Some(value) = value {
                                            map.insert(key, value);
                                        }
                                        """,
                                    )
                                }
                            }
                        }
                        if (returnSymbolToParse.isUnconstrained) {
                            rust("Ok(Some(#{T}(map)))", returnSymbolToParse.symbol)
                        } else {
                            rust("Ok(Some(map))")
                        }
                    }
                }
            }
        rust("#T(tokens)?", parser)
    }

    private fun RustWriter.deserializeStruct(shape: StructureShape) {
        val returnSymbolToParse = returnSymbolToParse(shape)
        val nestedParser =
            protocolFunctions.deserializeFn(shape) { fnName ->
                rustBlockTemplate(
                    """
                    pub(crate) fn $fnName<'a, I>(tokens: &mut #{Peekable}<I>) -> Result<Option<#{ReturnType}>, #{Error}>
                        where I: Iterator<Item = Result<#{Token}<'a>, #{Error}>>
                    """,
                    "ReturnType" to returnSymbolToParse.symbol,
                    *codegenScope,
                ) {
                    startObjectOrNull {
                        Attribute.AllowUnusedMut.render(this)
                        rustTemplate(
                            "let mut builder = #{Builder}::default();",
                            *codegenScope,
                            "Builder" to symbolProvider.symbolForBuilder(shape),
                        )
                        deserializeStructInner(shape.members())
                        val builder =
                            builderInstantiator.finalizeBuilder(
                                "builder", shape,
                            ) {
                                rustTemplate(
                                    """|err|#{Error}::custom_source("Response was invalid", err)""", *codegenScope,
                                )
                            }
                        rust("Ok(Some(#T))", builder)
                    }
                }
            }
        rust("#T(tokens)?", nestedParser)
    }

    private fun RustWriter.deserializeUnion(shape: UnionShape) {
        val returnSymbolToParse = returnSymbolToParse(shape)
        val nestedParser =
            protocolFunctions.deserializeFn(shape) { fnName ->
                rustBlockTemplate(
                    """
                    pub(crate) fn $fnName<'a, I>(tokens: &mut #{Peekable}<I>) -> Result<Option<#{Shape}>, #{Error}>
                        where I: Iterator<Item = Result<#{Token}<'a>, #{Error}>>
                    """,
                    *codegenScope,
                    "Shape" to returnSymbolToParse.symbol,
                ) {
                    // Apply any custom union deserialization logic before processing tokens.
                    // This allows for customization of how union variants are handled,
                    // particularly their discrimination mechanism.
                    for (customization in customizations) {
                        customization.section(JsonParserSection.BeforeUnionDeserialize(shape))(this)
                    }
                    rust("let mut variant = None;")
                    val checkValueSet = !shape.members().all { it.isTargetUnit() } && !codegenTarget.renderUnknownVariant()
                    rustBlock("match tokens.next().transpose()?") {
                        rustBlockTemplate(
                            """
                            Some(#{Token}::ValueNull { .. }) => return Ok(None),
                            Some(#{Token}::StartObject { .. }) =>
                            """,
                            *codegenScope,
                        ) {
                            objectKeyLoop(hasMembers = shape.members().isNotEmpty()) {
                                rustTemplate(
                                    """
                                    if let #{Some}(#{Ok}(#{Token}::ValueNull { .. })) = tokens.peek() {
                                        let _ = tokens.next().expect("peek returned a token")?;
                                        continue;
                                    }
                                    """,
                                    *codegenScope,
                                )
                                rustTemplate(
                                    """
                                    let key = key.to_unescaped()?;
                                    if key == "__type" {
                                        #{skip_value}(tokens)?;
                                        continue
                                    }
                                    if variant.is_some() {
                                        return Err(#{Error}::custom("encountered mixed variants in union"));
                                    }
                                    """,
                                    *codegenScope,
                                )
                                withBlock("variant = match key.as_ref() {", "};") {
                                    for (member in shape.members()) {
                                        val variantName = symbolProvider.toMemberName(member)
                                        rustBlock("${jsonName(member).dq()} =>") {
                                            if (member.isTargetUnit()) {
                                                rustTemplate(
                                                    """
                                                    #{skip_value}(tokens)?;
                                                    Some(#{Union}::$variantName)
                                                    """,
                                                    "Union" to returnSymbolToParse.symbol, *codegenScope,
                                                )
                                            } else {
                                                withBlock("Some(#T::$variantName(", "))", returnSymbolToParse.symbol) {
                                                    deserializeMember(member)
                                                    unwrapOrDefaultOrError(member, checkValueSet)
                                                }
                                            }
                                        }
                                    }
                                    when (codegenTarget.renderUnknownVariant()) {
                                        // In client mode, resolve an unknown union variant to the unknown variant.
                                        true ->
                                            rustTemplate(
                                                """
                                                _ => {
                                                  #{skip_value}(tokens)?;
                                                  Some(#{Union}::${UnionGenerator.UNKNOWN_VARIANT_NAME})
                                                }
                                                """,
                                                "Union" to returnSymbolToParse.symbol,
                                                *codegenScope,
                                            )
                                        // In server mode, use strict parsing.
                                        // Consultation: https://github.com/awslabs/smithy/issues/1222
                                        false ->
                                            rustTemplate(
                                                """variant => return Err(#{Error}::custom(format!("unexpected union variant: {}", variant)))""",
                                                *codegenScope,
                                            )
                                    }
                                }
                            }
                        }
                        rustTemplate(
                            """_ => return Err(#{Error}::custom("expected start object or null"))""",
                            *codegenScope,
                        )
                    }
                    // If we've gotten to the point where the union had a `{ ... }` section, we can't return None
                    // anymore. If we didn't parse a union at this point, this is an error.
                    rustTemplate(
                        """
                        if variant.is_none() {
                            return Err(#{Error}::custom("Union did not contain a valid variant."))
                        }
                        """,
                        *codegenScope,
                    )
                    rust("Ok(variant)")
                }
            }
        rust("#T(tokens)?", nestedParser)
    }

    private fun RustWriter.unwrapOrDefaultOrError(
        member: MemberShape,
        checkValueSet: Boolean,
    ) {
        if (symbolProvider.toSymbol(member).canUseDefault() && !checkValueSet) {
            rust(".unwrap_or_default()")
        } else {
            rustTemplate(
                ".ok_or_else(|| #{Error}::custom(\"value for '${escape(member.memberName)}' cannot be null\"))?",
                *codegenScope,
            )
        }
    }

    private fun RustWriter.objectKeyLoop(
        hasMembers: Boolean,
        inner: Writable,
    ) {
        if (!hasMembers) {
            rustTemplate("#{skip_to_end}(tokens)?;", *codegenScope)
        } else {
            rustBlock("loop") {
                rustBlock("match tokens.next().transpose()?") {
                    rustBlockTemplate(
                        """
                        Some(#{Token}::EndObject { .. }) => break,
                        Some(#{Token}::ObjectKey { key, .. }) =>
                        """,
                        *codegenScope,
                    ) {
                        inner()
                    }
                    rustTemplate(
                        """other => return Err(#{Error}::custom(format!("expected object key or end object, found: {:?}", other)))""",
                        *codegenScope,
                    )
                }
            }
        }
    }

    private fun RustWriter.startArrayOrNull(inner: Writable) = startOrNull("array", inner)

    private fun RustWriter.startObjectOrNull(inner: Writable) = startOrNull("object", inner)

    private fun RustWriter.startOrNull(
        objectOrArray: String,
        inner: Writable,
    ) {
        rustBlockTemplate("match tokens.next().transpose()?", *codegenScope) {
            rustBlockTemplate(
                """
                Some(#{Token}::ValueNull { .. }) => Ok(None),
                Some(#{Token}::Start${StringUtils.capitalize(objectOrArray)} { .. }) =>
                """,
                *codegenScope,
            ) {
                inner()
            }
            rustBlockTemplate("_ =>") {
                rustTemplate(
                    "Err(#{Error}::custom(\"expected start $objectOrArray or null\"))",
                    *codegenScope,
                )
            }
        }
    }
}
