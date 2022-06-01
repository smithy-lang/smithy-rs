/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.protocols.parse

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
import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.escape
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.withBlock
import software.amazon.smithy.rust.codegen.rustlang.withBlockTemplate
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.UnconstrainedShapeSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.canReachConstrainedShape
import software.amazon.smithy.rust.codegen.smithy.canUseDefault
import software.amazon.smithy.rust.codegen.smithy.generators.CodegenTarget
import software.amazon.smithy.rust.codegen.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.builderSymbol
import software.amazon.smithy.rust.codegen.smithy.generators.deserializerBuilderSetterName
import software.amazon.smithy.rust.codegen.smithy.generators.renderUnknownVariant
import software.amazon.smithy.rust.codegen.smithy.isOptional
import software.amazon.smithy.rust.codegen.smithy.isRustBoxed
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpBindingResolver
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpLocation
import software.amazon.smithy.rust.codegen.smithy.protocols.deserializeFunctionName
import software.amazon.smithy.rust.codegen.smithy.targetCanReachConstrainedShape
import software.amazon.smithy.rust.codegen.util.PANIC
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.hasTrait
import software.amazon.smithy.rust.codegen.util.inputShape
import software.amazon.smithy.rust.codegen.util.outputShape
import software.amazon.smithy.utils.StringUtils

// TODO: Separate commit: Make all functions pub(crate). If the functions have in their type signature a pub(crate) type,
//  and the function is declared `pub`, Rust will complain, even if the json_deser module is not `pub`.

class JsonParserGenerator(
    codegenContext: CodegenContext,
    private val httpBindingResolver: HttpBindingResolver,
    /** Function that maps a MemberShape into a JSON field name */
    private val jsonName: (MemberShape) -> String,
) : StructuredDataParserGenerator {
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val unconstrainedShapeSymbolProvider = UnconstrainedShapeSymbolProvider(symbolProvider, model, codegenContext.serviceShape)
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenTarget = codegenContext.target
    private val smithyJson = CargoDependency.smithyJson(runtimeConfig).asType()
    private val jsonDeserModule = RustModule.private("json_deser")
    private val codegenScope = arrayOf(
        "Error" to smithyJson.member("deserialize::Error"),
        "ErrorReason" to smithyJson.member("deserialize::ErrorReason"),
        "expect_blob_or_null" to smithyJson.member("deserialize::token::expect_blob_or_null"),
        "expect_bool_or_null" to smithyJson.member("deserialize::token::expect_bool_or_null"),
        "expect_document" to smithyJson.member("deserialize::token::expect_document"),
        "expect_number_or_null" to smithyJson.member("deserialize::token::expect_number_or_null"),
        "expect_start_array" to smithyJson.member("deserialize::token::expect_start_array"),
        "expect_start_object" to smithyJson.member("deserialize::token::expect_start_object"),
        "expect_string_or_null" to smithyJson.member("deserialize::token::expect_string_or_null"),
        "expect_timestamp_or_null" to smithyJson.member("deserialize::token::expect_timestamp_or_null"),
        "json_token_iter" to smithyJson.member("deserialize::json_token_iter"),
        "Peekable" to RuntimeType.std.member("iter::Peekable"),
        "skip_value" to smithyJson.member("deserialize::token::skip_value"),
        "skip_to_end" to smithyJson.member("deserialize::token::skip_to_end"),
        "Token" to smithyJson.member("deserialize::Token"),
        "or_empty" to orEmptyJson(),
    )

    /**
     * Reusable structure parser implementation that can be used to generate parsing code for
     * operation, error and structure shapes.
     * We still generate the parser symbol even if there are no included members because the server
     * generation requires parsers for all input structures.
     */
    private fun structureParser(
        fnName: String,
        structureShape: StructureShape,
        includedMembers: List<MemberShape>
    ): RuntimeType {
        return RuntimeType.forInlineFun(fnName, jsonDeserModule) {
            val unusedMut = if (includedMembers.isEmpty()) "##[allow(unused_mut)] " else ""
            it.rustBlockTemplate(
                "pub(crate) fn $fnName(value: &[u8], ${unusedMut}mut builder: #{Builder}) -> Result<#{Builder}, #{Error}>",
                "Builder" to structureShape.builderSymbol(symbolProvider),
                *codegenScope
            ) {
                rustTemplate(
                    """
                    let mut tokens_owned = #{json_token_iter}(#{or_empty}(value)).peekable();
                    let tokens = &mut tokens_owned;
                    #{expect_start_object}(tokens.next())?;
                    """,
                    *codegenScope
                )
                deserializeStructInner(includedMembers)
                expectEndOfTokenStream()
                rust("Ok(builder)")
            }
        }
    }

    override fun payloadParser(member: MemberShape): RuntimeType {
        val shape = model.expectShape(member.target)
        check(shape is UnionShape || shape is StructureShape || shape is DocumentShape) { "payload parser should only be used on structures & unions" }
        val fnName = symbolProvider.deserializeFunctionName(shape) + "_payload"
        return RuntimeType.forInlineFun(fnName, jsonDeserModule) {
            it.rustBlockTemplate(
                "pub(crate) fn $fnName(input: &[u8]) -> Result<#{Shape}, #{Error}>",
                *codegenScope,
                "Shape" to symbolProvider.toSymbol(shape)
            ) {
                val input = if (shape is DocumentShape) {
                    "input"
                } else {
                    "#{or_empty}(input)"
                }

                rustTemplate(
                    """
                    let mut tokens_owned = #{json_token_iter}($input).peekable();
                    let tokens = &mut tokens_owned;
                    """,
                    *codegenScope
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
        val fnName = symbolProvider.deserializeFunctionName(operationShape)
        return structureParser(fnName, outputShape, httpDocumentMembers)
    }

    override fun errorParser(errorShape: StructureShape): RuntimeType? {
        if (errorShape.members().isEmpty()) {
            return null
        }
        val fnName = symbolProvider.deserializeFunctionName(errorShape) + "_json_err"
        return structureParser(fnName, errorShape, errorShape.members().toList())
    }

    private fun orEmptyJson(): RuntimeType = RuntimeType.forInlineFun("or_empty_doc", jsonDeserModule) {
        it.rust(
            """
            pub(crate) fn or_empty_doc(data: &[u8]) -> &[u8] {
                if data.is_empty() {
                    b"{}"
                } else {
                    data
                }
            }
            """
        )
    }

    override fun serverInputParser(operationShape: OperationShape): RuntimeType? {
        val includedMembers = httpBindingResolver.requestMembers(operationShape, HttpLocation.DOCUMENT)
        if (includedMembers.isEmpty()) {
            return null
        }
        val inputShape = operationShape.inputShape(model)
        val fnName = symbolProvider.deserializeFunctionName(operationShape)
        return structureParser(fnName, inputShape, includedMembers)
    }

    private fun RustWriter.expectEndOfTokenStream() {
        rustBlock("if tokens.next().is_some()") {
            rustTemplate(
                "return Err(#{Error}::custom(\"found more JSON tokens after completing parsing\"));",
                *codegenScope
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
                                withBlock(
                                    "builder = builder.${
                                        member.deserializerBuilderSetterName(model, symbolProvider, codegenTarget)
                                    }(", ");"
                                ) {
                                    deserializeMember(member)
                                }
                            }
                            CodegenTarget.SERVER -> {
                                if (symbolProvider.toSymbol(member).isOptional()) {
                                    withBlock(
                                        "builder = builder.${
                                            member.deserializerBuilderSetterName(model, symbolProvider, codegenTarget)
                                        }(", ");"
                                    ) {
                                        deserializeMember(member)
                                    }
                                } else {
                                    rust("if let Some(v) = ")
                                    deserializeMember(member)
                                    rust(
                                        """
                                        {
                                            builder = builder.${
                                                member.deserializerBuilderSetterName(model, symbolProvider, codegenTarget)
                                            }(v);
                                        }
                                        """
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
            is BlobShape -> rustTemplate("#{expect_blob_or_null}(tokens.next())?", *codegenScope)
            is TimestampShape -> deserializeTimestamp(memberShape)
            is CollectionShape -> deserializeCollection(target)
            is MapShape -> deserializeMap(target)
            is StructureShape -> deserializeStruct(target)
            is UnionShape -> deserializeUnion(target)
            is DocumentShape -> rustTemplate("Some(#{expect_document}(tokens)?)", *codegenScope)
            else -> PANIC("unexpected shape: $target")
        }
        val symbol = symbolProvider.toSymbol(memberShape)
        if (symbol.isRustBoxed()) {
            if (codegenTarget == CodegenTarget.SERVER && memberShape.targetCanReachConstrainedShape(model, symbolProvider)) {
                // Before boxing, convert into `MaybeConstrained` if the target can reach a constrained shape.
                rust(".map(|x| x.into())")
            }
            rust(".map(Box::new)")
        }
    }

    private fun RustWriter.deserializeStringInner(target: StringShape, escapedStrName: String) {
        withBlock("$escapedStrName.to_unescaped().map(|u|", ")") {
            when (target.hasTrait<EnumTrait>()) {
                true -> {
                    if (parseUnconstrainedShape(target)) {
                        rust("u.into_owned()")
                    } else {
                        rust("#T::from(u.as_ref())", symbolProvider.toSymbol(target))
                    }
                }
                else -> rust("u.into_owned()")
            }
        }
    }

    private fun RustWriter.deserializeString(target: StringShape) {
        withBlockTemplate("#{expect_string_or_null}(tokens.next())?.map(|s|", ").transpose()?", *codegenScope) {
            deserializeStringInner(target, "s")
        }
    }

    private fun RustWriter.deserializeNumber(target: NumberShape) {
        val symbol = symbolProvider.toSymbol(target)
        rustTemplate("#{expect_number_or_null}(tokens.next())?.map(|v| v.to_#{T}())", "T" to symbol, *codegenScope)
    }

    private fun RustWriter.deserializeTimestamp(member: MemberShape) {
        val timestampFormat =
            httpBindingResolver.timestampFormat(
                member, HttpLocation.DOCUMENT,
                TimestampFormatTrait.Format.EPOCH_SECONDS
            )
        val timestampFormatType = RuntimeType.TimestampFormat(runtimeConfig, timestampFormat)
        rustTemplate("#{expect_timestamp_or_null}(tokens.next(), #{T})?", "T" to timestampFormatType, *codegenScope)
    }

    private fun RustWriter.deserializeCollection(shape: CollectionShape) {
        val fnName = symbolProvider.deserializeFunctionName(shape)
        val isSparse = shape.hasTrait<SparseTrait>()
        val returnUnconstrainedType = parseUnconstrainedShape(shape)
        val returnType = if (returnUnconstrainedType) {
            unconstrainedShapeSymbolProvider.toSymbol(shape)
        } else {
            symbolProvider.toSymbol(shape)
        }
        val parser = RuntimeType.forInlineFun(fnName, jsonDeserModule) {
            // Allow non-snake-case since some SDK models have lists with names prefixed with `__listOf__`,
            // which become `__list_of__`, and the Rust compiler warning doesn't like multiple adjacent underscores.
            it.rustBlockTemplate(
                """
                ##[allow(non_snake_case)]
                pub(crate) fn $fnName<'a, I>(tokens: &mut #{Peekable}<I>) -> Result<Option<#{ReturnType}>, #{Error}>
                    where I: Iterator<Item = Result<#{Token}<'a>, #{Error}>>
                """,
                "ReturnType" to returnType,
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
                                    rustBlock("if let Some(value) = value") {
                                        rust("items.push(value);")
                                    }
                                }
                            }
                        }
                    }
                    if (returnUnconstrainedType) {
                        rust("Ok(Some(#{T}(items)))", returnType)
                    } else {
                        rust("Ok(Some(items))")
                    }
                }
            }
        }
        rust("#T(tokens)?", parser)
    }

    private fun RustWriter.deserializeMap(shape: MapShape) {
        val keyTarget = model.expectShape(shape.key.target) as StringShape
        val fnName = symbolProvider.deserializeFunctionName(shape)
        val isSparse = shape.hasTrait<SparseTrait>()
        val returnUnconstrainedType = parseUnconstrainedShape(shape)
        val returnType = if (returnUnconstrainedType) {
            unconstrainedShapeSymbolProvider.toSymbol(shape)
        } else {
            symbolProvider.toSymbol(shape)
        }
        val parser = RuntimeType.forInlineFun(fnName, jsonDeserModule) {
            // Allow non-snake-case since some SDK models have maps with names prefixed with `__mapOf__`,
            // which become `__map_of__`, and the Rust compiler warning doesn't like multiple adjacent underscores.
            it.rustBlockTemplate(
                """
                ##[allow(non_snake_case)]
                pub(crate) fn $fnName<'a, I>(tokens: &mut #{Peekable}<I>) -> Result<Option<#{ReturnType}>, #{Error}>
                    where I: Iterator<Item = Result<#{Token}<'a>, #{Error}>>
                """,
                "ReturnType" to returnType,
                *codegenScope,
            ) {
                startObjectOrNull {
                    rust("let mut map = #T::new();", RustType.HashMap.RuntimeType)
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
                            rustBlock("if let Some(value) = value") {
                                rust("map.insert(key, value);")
                            }
                        }
                    }
                    if (returnUnconstrainedType) {
                        rust("Ok(Some(#{T}(map)))", returnType)
                    } else {
                        rust("Ok(Some(map))")
                    }
                }
            }
        }
        rust("#T(tokens)?", parser)
    }

    private fun RustWriter.deserializeStruct(shape: StructureShape) {
        val fnName = symbolProvider.deserializeFunctionName(shape)
        val symbol = symbolProvider.toSymbol(shape)
        val returnBuilder = codegenTarget == CodegenTarget.SERVER && shape.canReachConstrainedShape(model, symbolProvider)
        val returnType = if (returnBuilder) {
            unconstrainedShapeSymbolProvider.toSymbol(shape)
        } else {
            symbol
        }
        val nestedParser = RuntimeType.forInlineFun(fnName, jsonDeserModule) {
            it.rustBlockTemplate(
                """
                pub(crate) fn $fnName<'a, I>(tokens: &mut #{Peekable}<I>) -> Result<Option<#{ReturnType}>, #{Error}>
                    where I: Iterator<Item = Result<#{Token}<'a>, #{Error}>>
                """,
                "ReturnType" to returnType,
                *codegenScope,
            ) {
                startObjectOrNull {
                    Attribute.AllowUnusedMut.render(this)
                    rustTemplate("let mut builder = #{Shape}::builder();", *codegenScope, "Shape" to symbol)
                    deserializeStructInner(shape.members())
                    // Only call `build()` if the builder is not fallible. Otherwise, return the builder.
                    if (returnBuilder) {
                        rust("Ok(Some(builder))")
                    } else {
                        rust("Ok(Some(builder.build()))")
                    }
                }
            }
        }
        rust("#T(tokens)?", nestedParser)
    }

    private fun RustWriter.deserializeUnion(shape: UnionShape) {
        val fnName = symbolProvider.deserializeFunctionName(shape)
        val symbol = if (parseUnconstrainedShape(shape)) {
            unconstrainedShapeSymbolProvider.toSymbol(shape)
        } else {
            symbolProvider.toSymbol(shape)
        }
        val nestedParser = RuntimeType.forInlineFun(fnName, jsonDeserModule) {
            it.rustBlockTemplate(
                """
                pub(crate) fn $fnName<'a, I>(tokens: &mut #{Peekable}<I>) -> Result<Option<#{Shape}>, #{Error}>
                    where I: Iterator<Item = Result<#{Token}<'a>, #{Error}>>
                """,
                *codegenScope,
                "Shape" to symbol
            ) {
                rust("let mut variant = None;")
                rustBlock("match tokens.next().transpose()?") {
                    rustBlockTemplate(
                        """
                        Some(#{Token}::ValueNull { .. }) => return Ok(None),
                        Some(#{Token}::StartObject { .. }) =>
                        """,
                        *codegenScope
                    ) {
                        objectKeyLoop(hasMembers = shape.members().isNotEmpty()) {
                            rustTemplate(
                                """
                                if variant.is_some() {
                                    return Err(#{Error}::custom("encountered mixed variants in union"));
                                }
                                """,
                                *codegenScope
                            )
                            withBlock("variant = match key.to_unescaped()?.as_ref() {", "};") {
                                for (member in shape.members()) {
                                    val variantName = symbolProvider.toMemberName(member)
                                    rustBlock("${jsonName(member).dq()} =>") {
                                        withBlock("Some(#T::$variantName(", "))", symbol) {
                                            deserializeMember(member)
                                            unwrapOrDefaultOrError(member)
                                        }
                                    }
                                }
                                when (codegenTarget.renderUnknownVariant()) {
                                    // In client mode, resolve an unknown union variant to the unknown variant.
                                    true -> rustTemplate(
                                        """
                                        _ => {
                                          #{skip_value}(tokens)?;
                                          Some(#{Union}::${UnionGenerator.UnknownVariantName})
                                        }
                                        """,
                                        "Union" to symbol, *codegenScope
                                    )
                                    // In server mode, use strict parsing.
                                    // Consultation: https://github.com/awslabs/smithy/issues/1222
                                    false -> rustTemplate(
                                        """variant => return Err(#{Error}::custom(format!("unexpected union variant: {}", variant)))""",
                                        *codegenScope
                                    )
                                }
                            }
                        }
                    }
                    rustTemplate(
                        """_ => return Err(#{Error}::custom("expected start object or null"))""",
                        *codegenScope
                    )
                }
                rust("Ok(variant)")
            }
        }
        rust("#T(tokens)?", nestedParser)
    }

    private fun RustWriter.unwrapOrDefaultOrError(member: MemberShape) {
        if (symbolProvider.toSymbol(member).canUseDefault()) {
            rust(".unwrap_or_default()")
        } else {
            rustTemplate(
                ".ok_or_else(|| #{Error}::custom(\"value for '${escape(member.memberName)}' cannot be null\"))?",
                *codegenScope
            )
        }
    }

    private fun RustWriter.objectKeyLoop(hasMembers: Boolean, inner: RustWriter.() -> Unit) {
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
                        *codegenScope
                    ) {
                        inner()
                    }
                    rustTemplate(
                        """other => return Err(#{Error}::custom(format!("expected object key or end object, found: {:?}", other)))""",
                        *codegenScope
                    )
                }
            }
        }
    }

    private fun RustWriter.startArrayOrNull(inner: RustWriter.() -> Unit) = startOrNull("array", inner)
    private fun RustWriter.startObjectOrNull(inner: RustWriter.() -> Unit) = startOrNull("object", inner)
    private fun RustWriter.startOrNull(objectOrArray: String, inner: RustWriter.() -> Unit) {
        rustBlockTemplate("match tokens.next().transpose()?", *codegenScope) {
            rustBlockTemplate(
                """
                Some(#{Token}::ValueNull { .. }) => Ok(None),
                Some(#{Token}::Start${StringUtils.capitalize(objectOrArray)} { .. }) =>
                """,
                *codegenScope
            ) {
                inner()
            }
            rustBlockTemplate("_ =>") {
                rustTemplate(
                    "Err(#{Error}::custom(\"expected start $objectOrArray or null\"))",
                    *codegenScope
                )
            }
        }
    }

    /**
     * Whether we should parse a value for a shape into its associated unconstrained type. For example, when the shape
     * is a `StructureShape`, we should construct and return a builder instead of building into the final `struct` the
     * user gets. This is only relevant for the server, that parses the incoming request and only after enforces
     * constraint traits.
     */
    private fun parseUnconstrainedShape(shape: Shape) =
        codegenTarget == CodegenTarget.SERVER && shape.canReachConstrainedShape(model, symbolProvider)
}
