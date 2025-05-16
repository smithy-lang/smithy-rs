/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols.parse

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.BooleanShape
import software.amazon.smithy.model.shapes.ByteShape
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.DoubleShape
import software.amazon.smithy.model.shapes.FloatShape
import software.amazon.smithy.model.shapes.IntegerShape
import software.amazon.smithy.model.shapes.LongShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShortShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.TimestampShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.model.traits.SparseTrait
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.conditionalBlock
import software.amazon.smithy.rust.codegen.core.rustlang.join
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.customize.NamedCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.Section
import software.amazon.smithy.rust.codegen.core.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.renderUnknownVariant
import software.amazon.smithy.rust.codegen.core.smithy.generators.setterName
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

/** Class describing a CBOR parser section that can be used in a customization. */
sealed class CborParserSection(name: String) : Section(name) {
    data class BeforeBoxingDeserializedMember(val shape: MemberShape) : CborParserSection("BeforeBoxingDeserializedMember")

    /**
     * Represents a customization point in union deserialization that occurs before decoding the map structure.
     * This allows for custom handling of union variants before the standard map decoding logic is applied.
     * @property shape The union shape being deserialized.
     */
    data class UnionParserBeforeDecodingMap(val shape: UnionShape) : CborParserSection("UnionParserBeforeDecodingMap")
}

/**
 * Customization class for CBOR parser generation that allows modification of union type deserialization behavior.
 * Previously, union variant discrimination was hardcoded to use `decoder.str()`. This has been made more flexible
 * to support different decoder implementations and discrimination methods.
 */
abstract class CborParserCustomization : NamedCustomization<CborParserSection>() {
    /**
     * Allows customization of how union variants are discriminated during deserialization.
     * @param defaultContext The default discrimination context containing decoder symbol and discriminator method.
     * @return UnionVariantDiscriminatorContext that defines how to discriminate union variants.
     */
    open fun getUnionVariantDiscriminator(
        unionShape: UnionShape,
        defaultContext: CborParserGenerator.UnionVariantDiscriminatorContext,
    ) = defaultContext
}

class CborParserGenerator(
    private val codegenContext: CodegenContext,
    private val httpBindingResolver: HttpBindingResolver,
    /** See docs for this parameter in [JsonParserGenerator]. */
    private val returnSymbolToParse: (Shape) -> ReturnSymbolToParse = { shape ->
        ReturnSymbolToParse(codegenContext.symbolProvider.toSymbol(shape), false)
    },
    /** Lambda that controls what to do when seeing a NULL value while parsing an element for a non-sparse collection */
    private val handleNullForNonSparseCollection: (String) -> Writable,
    /** Lambda that determines whether the input to a builder setter needs to be wrapped in `Some` */
    private val shouldWrapBuilderMemberSetterInputWithOption: (MemberShape) -> Boolean = { _ -> true },
    private val customizations: List<CborParserCustomization> = emptyList(),
) : StructuredDataParserGenerator {
    /**
     * Context class that encapsulates the information needed to discriminate union variants during deserialization.
     * @property decoderSymbol The symbol representing the decoder type.
     * @property variantDiscriminatorExpression The method call expression to determine the union variant.
     */
    data class UnionVariantDiscriminatorContext(
        val decoderSymbol: Symbol,
        val variantDiscriminatorExpression: Writable,
    )

    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenTarget = codegenContext.target
    private val smithyCbor = CargoDependency.smithyCbor(runtimeConfig).toType()
    private val protocolFunctions = ProtocolFunctions(codegenContext)
    private val builderInstantiator = codegenContext.builderInstantiator()
    private val codegenScope =
        arrayOf(
            *preludeScope,
            "SmithyCbor" to smithyCbor,
            "Decoder" to smithyCbor.resolve("Decoder"),
            "Error" to smithyCbor.resolve("decode::DeserializeError"),
            "HashMap" to RuntimeType.HashMap,
            *preludeScope,
        )

    private fun handleNullForCollection(
        collectionName: String,
        isSparse: Boolean,
    ) = writable {
        if (isSparse) {
            rustTemplate(
                """
                decoder.null()?;
                #{None}
                """,
                *codegenScope,
            )
        } else {
            rustTemplate(
                "#{handle_null_for_non_sparse_collection:W}",
                "handle_null_for_non_sparse_collection" to
                    handleNullForNonSparseCollection(
                        collectionName,
                    ),
            )
        }
    }

    private fun listMemberParserFn(
        listSymbol: Symbol,
        isSparseList: Boolean,
        memberShape: MemberShape,
        returnUnconstrainedType: Boolean,
    ) = writable {
        rustBlockTemplate(
            """
            fn member(
                mut list: #{ListSymbol},
                decoder: &mut #{Decoder},
            ) -> #{Result}<#{ListSymbol}, #{Error}>
            """,
            *codegenScope,
            "ListSymbol" to listSymbol,
        ) {
            rustTemplate(
                """
                let value = match decoder.datatype()? {
                    #{SmithyCbor}::data::Type::Null => {
                        #{handleNullForCollection:W}
                    }
                    _ => #{DeserializeMember:W}
                };
                """,
                *codegenScope,
                "handleNullForCollection" to handleNullForCollection(CollectionKind.List.decoderMethodName(), isSparseList),
                "DeserializeMember" to
                    writable {
                        conditionalBlock(
                            "Some(", ")", isSparseList,
                        ) {
                            rust("#T?", deserializeMember(memberShape))
                        }
                    },
            )

            if (returnUnconstrainedType) {
                rust("list.0.push(value);")
            } else {
                rust("list.push(value);")
            }

            rust("Ok(list)")
        }
    }

    private fun mapPairParserFnWritable(
        keyTarget: StringShape,
        valueShape: MemberShape,
        isSparseMap: Boolean,
        mapSymbol: Symbol,
        returnUnconstrainedType: Boolean,
    ) = writable {
        rustBlockTemplate(
            """
            fn pair(
                mut map: #{MapSymbol},
                decoder: &mut #{Decoder},
            ) -> #{Result}<#{MapSymbol}, #{Error}>
            """,
            *codegenScope,
            "MapSymbol" to mapSymbol,
        ) {
            val deserializeKeyWritable = deserializeString(keyTarget)
            rustTemplate(
                """
                let key = #{DeserializeKey:W}?;
                """,
                "DeserializeKey" to deserializeKeyWritable,
            )
            rustTemplate(
                """
                let value = match decoder.datatype()? {
                    #{SmithyCbor}::data::Type::Null => {
                        #{handleNullForCollection:W}
                    }
                    _ => #{DeserializeMember:W}
                };
                """,
                *codegenScope,
                "handleNullForCollection" to handleNullForCollection(CollectionKind.Map.decoderMethodName(), isSparseMap),
                "DeserializeMember" to
                    writable {
                        conditionalBlock(
                            "Some(", ")", isSparseMap,
                        ) {
                            rust("#T?", deserializeMember(valueShape))
                        }
                    },
            )
            if (returnUnconstrainedType) {
                rust("map.0.insert(key, value);")
            } else {
                rust("map.insert(key, value);")
            }

            rust("Ok(map)")
        }
    }

    private fun structurePairParserFnWritable(
        builderSymbol: Symbol,
        includedMembers: Collection<MemberShape>,
    ) = writable {
        rustBlockTemplate(
            """
            ##[allow(clippy::match_single_binding)]
            fn pair(
                mut builder: #{Builder},
                decoder: &mut #{Decoder}
            ) -> #{Result}<#{Builder}, #{Error}>
            """,
            *codegenScope,
            "Builder" to builderSymbol,
        ) {
            withBlock("builder = match decoder.str()?.as_ref() {", "};") {
                for (member in includedMembers) {
                    rustBlock("${member.memberName.dq()} =>") {
                        val callBuilderSetMemberFieldWritable =
                            writable {
                                withBlock("builder.${member.setterName()}(", ")") {
                                    conditionalBlock("Some(", ")", shouldWrapBuilderMemberSetterInputWithOption(member)) {
                                        val symbol = symbolProvider.toSymbol(member)
                                        if (symbol.isRustBoxed()) {
                                            rustBlock("") {
                                                rustTemplate(
                                                    "let v = #{DeserializeMember:W}?;",
                                                    "DeserializeMember" to deserializeMember(member),
                                                )

                                                for (customization in customizations) {
                                                    customization.section(
                                                        CborParserSection.BeforeBoxingDeserializedMember(
                                                            member,
                                                        ),
                                                    )(this)
                                                }
                                                rust("Box::new(v)")
                                            }
                                        } else {
                                            rustTemplate(
                                                "#{DeserializeMember:W}?",
                                                "DeserializeMember" to deserializeMember(member),
                                            )
                                        }
                                    }
                                }
                            }

                        if (member.isOptional) {
                            // Call `builder.set_member()` only if the value for the field on the wire is not null.
                            rustTemplate(
                                """
                                #{SmithyCbor}::decode::set_optional(builder, decoder, |builder, decoder| {
                                    Ok(#{MemberSettingWritable:W})
                                })?
                                """,
                                *codegenScope,
                                "MemberSettingWritable" to callBuilderSetMemberFieldWritable,
                            )
                        } else {
                            callBuilderSetMemberFieldWritable.invoke(this)
                        }
                    }
                }

                rust(
                    """
                    _ => {
                        decoder.skip()?;
                        builder
                    }
                    """,
                )
            }
            rust("Ok(builder)")
        }
    }

    private fun unionPairParserFnWritable(shape: UnionShape) =
        writable {
            val returnSymbolToParse = returnSymbolToParse(shape)
            // Get actual decoder type to use and the discriminating function to call to extract
            // the variant of the union that has been encoded in the data.
            val discriminatorContext = getUnionDiscriminatorContext(shape, "Decoder", "decoder.str()?.as_ref()")

            rustBlockTemplate(
                """
                fn pair(
                    decoder: &mut #{DecoderSymbol}
                ) -> #{Result}<#{UnionSymbol}, #{Error}>
                """,
                *codegenScope,
                "DecoderSymbol" to discriminatorContext.decoderSymbol,
                "UnionSymbol" to returnSymbolToParse.symbol,
            ) {
                rustTemplate(
                    """
                    Ok(match #{VariableDiscriminatingExpression} {
                    """,
                    "VariableDiscriminatingExpression" to discriminatorContext.variantDiscriminatorExpression,
                ).run {
                    for (member in shape.members()) {
                        val variantName = symbolProvider.toMemberName(member)

                        if (member.isTargetUnit()) {
                            rust(
                                """
                                ${member.memberName.dq()} => {
                                    decoder.skip()?;
                                    #T::$variantName
                                }
                                """,
                                returnSymbolToParse.symbol,
                            )
                        } else {
                            withBlock("${member.memberName.dq()} => #T::$variantName(", "?),", returnSymbolToParse.symbol) {
                                deserializeMember(member).invoke(this)

                                val symbol = symbolProvider.toSymbol(member)
                                if (symbol.isRustBoxed()) {
                                    rust(".map(Box::new)")
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
                                  decoder.skip()?;
                                  #{Union}::${UnionGenerator.UNKNOWN_VARIANT_NAME}
                                }
                                """,
                                "Union" to returnSymbolToParse.symbol,
                                *codegenScope,
                            )
                        // In server mode, use strict parsing.
                        // Consultation: https://github.com/awslabs/smithy/issues/1222
                        false ->
                            rustTemplate(
                                "variant => return Err(#{Error}::unknown_union_variant(variant, decoder.position()))",
                                *codegenScope,
                            )
                    }
                }
                rust("})")
            }
        }

    private fun getUnionDiscriminatorContext(
        unionShape: UnionShape,
        decoderType: String,
        callMethod: String,
    ): UnionVariantDiscriminatorContext {
        val defaultUnionPairContext =
            UnionVariantDiscriminatorContext(
                smithyCbor.resolve(decoderType).toSymbol(),
                writable { rustTemplate(callMethod) },
            )
        return customizations.fold(defaultUnionPairContext) { context, customization ->
            customization.getUnionVariantDiscriminator(unionShape, context)
        }
    }

    enum class CollectionKind {
        Map,
        List,
        ;

        /** Method to invoke on the decoder to decode this collection kind. **/
        fun decoderMethodName() =
            when (this) {
                Map -> "map"
                List -> "list"
            }
    }

    /**
     * Decode a collection of homogeneous CBOR data items: a map or an array.
     * The first branch of the `match` corresponds to when the collection is encoded using variable-length encoding;
     * the second branch corresponds to fixed-length encoding.
     *
     * https://www.rfc-editor.org/rfc/rfc8949.html#name-indefinite-length-arrays-an
     */
    private fun decodeCollectionLoopWritable(
        collectionKind: CollectionKind,
        variableBindingName: String,
        decodeItemFnName: String,
    ) = writable {
        rustTemplate(
            """
            match decoder.${collectionKind.decoderMethodName()}()? {
                None => loop {
                    match decoder.datatype()? {
                        #{SmithyCbor}::data::Type::Break => {
                            decoder.skip()?;
                            break;
                        }
                        _ => {
                            $variableBindingName = $decodeItemFnName($variableBindingName, decoder)?;
                        }
                    };
                },
                Some(n) => {
                    for _ in 0..n {
                        $variableBindingName = $decodeItemFnName($variableBindingName, decoder)?;
                    }
                }
            };
            """,
            *codegenScope,
        )
    }

    private fun decodeStructureMapLoopWritable() = decodeCollectionLoopWritable(CollectionKind.Map, "builder", "pair")

    private fun decodeMapLoopWritable() = decodeCollectionLoopWritable(CollectionKind.Map, "map", "pair")

    private fun decodeListLoopWritable() = decodeCollectionLoopWritable(CollectionKind.List, "list", "member")

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
            rustTemplate(
                """
                pub(crate) fn $fnName(value: &[u8], mut builder: #{Builder}) -> #{Result}<#{Builder}, #{Error}> {
                    #{StructurePairParserFn:W}

                    let decoder = &mut #{Decoder}::new(value);

                    #{DecodeStructureMapLoop:W}

                    if decoder.position() != value.len() {
                        return Err(#{Error}::expected_end_of_stream(decoder.position()));
                    }

                    Ok(builder)
                }
                """,
                "Builder" to builderSymbol,
                "StructurePairParserFn" to structurePairParserFnWritable(builderSymbol, includedMembers),
                "DecodeStructureMapLoop" to decodeStructureMapLoopWritable(),
                *codegenScope,
            )
        }
    }

    override fun payloadParser(member: MemberShape): RuntimeType {
        val shape = model.expectShape(member.target)
        val returnSymbol = returnSymbolToParse(shape)
        check(shape is UnionShape || shape is StructureShape) {
            "Payload parser should only be used on structure and union shapes."
        }
        return protocolFunctions.deserializeFn(shape, fnNameSuffix = "payload") { fnName ->
            rustTemplate(
                """
                pub(crate) fn $fnName(value: &[u8]) -> #{Result}<#{ReturnType}, #{Error}> {
                    let decoder = &mut #{Decoder}::new(value);
                    #{DeserializeMember}
                }
                """,
                "ReturnType" to returnSymbol.symbol,
                "DeserializeMember" to deserializeMember(member),
                *codegenScope,
            )
        }
    }

    override fun operationParser(operationShape: OperationShape): RuntimeType? {
        // Don't generate an operation CBOR deserializer if there is nothing bound to the HTTP body.
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
            fnNameSuffix = "cbor_err",
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

    private fun deserializeMember(memberShape: MemberShape) =
        writable {
            when (val target = model.expectShape(memberShape.target)) {
                // Simple shapes: https://smithy.io/2.0/spec/simple-types.html
                is BlobShape -> rust("decoder.blob()")
                is BooleanShape -> rust("decoder.boolean()")

                is StringShape -> deserializeString(target).invoke(this)

                is ByteShape -> rust("decoder.byte()")
                is ShortShape -> rust("decoder.short()")
                is IntegerShape -> rust("decoder.integer()")
                is LongShape -> rust("decoder.long()")

                is FloatShape -> rust("decoder.float()")
                is DoubleShape -> rust("decoder.double()")

                is TimestampShape -> rust("decoder.timestamp()")

                // Aggregate shapes: https://smithy.io/2.0/spec/aggregate-types.html
                is StructureShape -> deserializeStruct(target)
                is CollectionShape -> deserializeCollection(target)
                is MapShape -> deserializeMap(target)
                is UnionShape -> deserializeUnion(target)

                // Note that no protocol using CBOR serialization supports `document` shapes.
                else -> PANIC("unexpected shape: $target")
            }
        }

    private fun deserializeString(target: StringShape) =
        writable {
            when (target.hasTrait<EnumTrait>()) {
                true -> {
                    if (this@CborParserGenerator.returnSymbolToParse(target).isUnconstrained) {
                        rust("decoder.string()")
                    } else {
                        rust("decoder.string().map(|s| #T::from(s.as_ref()))", symbolProvider.toSymbol(target))
                    }
                }
                false -> rust("decoder.string()")
            }
        }

    private fun RustWriter.deserializeCollection(shape: CollectionShape) {
        val (returnSymbol, returnUnconstrainedType) = returnSymbolToParse(shape)

        val parser =
            protocolFunctions.deserializeFn(shape) { fnName ->
                val initContainerWritable =
                    writable {
                        withBlock("let mut list = ", ";") {
                            conditionalBlock("#{T}(", ")", conditional = returnUnconstrainedType, returnSymbol) {
                                rustTemplate("#{Vec}::new()", *codegenScope)
                            }
                        }
                    }

                rustTemplate(
                    """
                    pub(crate) fn $fnName(decoder: &mut #{Decoder}) -> #{Result}<#{ReturnType}, #{Error}> {
                        #{ListMemberParserFn:W}

                        #{InitContainerWritable:W}

                        #{DecodeListLoop:W}

                        Ok(list)
                    }
                    """,
                    "ReturnType" to returnSymbol,
                    "ListMemberParserFn" to
                        listMemberParserFn(
                            returnSymbol,
                            isSparseList = shape.hasTrait<SparseTrait>(),
                            shape.member,
                            returnUnconstrainedType = returnUnconstrainedType,
                        ),
                    "InitContainerWritable" to initContainerWritable,
                    "DecodeListLoop" to decodeListLoopWritable(),
                    *codegenScope,
                )
            }
        rust("#T(decoder)", parser)
    }

    private fun RustWriter.deserializeMap(shape: MapShape) {
        val keyTarget = model.expectShape(shape.key.target, StringShape::class.java)
        val (returnSymbol, returnUnconstrainedType) = returnSymbolToParse(shape)

        val parser =
            protocolFunctions.deserializeFn(shape) { fnName ->
                val initContainerWritable =
                    writable {
                        withBlock("let mut map = ", ";") {
                            conditionalBlock("#{T}(", ")", conditional = returnUnconstrainedType, returnSymbol) {
                                rustTemplate("#{HashMap}::new()", *codegenScope)
                            }
                        }
                    }

                rustTemplate(
                    """
                    pub(crate) fn $fnName(decoder: &mut #{Decoder}) -> #{Result}<#{ReturnType}, #{Error}> {
                        #{MapPairParserFn:W}

                        #{InitContainerWritable:W}

                        #{DecodeMapLoop:W}

                        Ok(map)
                    }
                    """,
                    "ReturnType" to returnSymbol,
                    "MapPairParserFn" to
                        mapPairParserFnWritable(
                            keyTarget,
                            shape.value,
                            isSparseMap = shape.hasTrait<SparseTrait>(),
                            returnSymbol,
                            returnUnconstrainedType = returnUnconstrainedType,
                        ),
                    "InitContainerWritable" to initContainerWritable,
                    "DecodeMapLoop" to decodeMapLoopWritable(),
                    *codegenScope,
                )
            }
        rust("#T(decoder)", parser)
    }

    private fun RustWriter.deserializeStruct(shape: StructureShape) {
        val returnSymbolToParse = returnSymbolToParse(shape)
        val parser =
            protocolFunctions.deserializeFn(shape) { fnName ->
                rustBlockTemplate(
                    "pub(crate) fn $fnName(decoder: &mut #{Decoder}) -> #{Result}<#{ReturnType}, #{Error}>",
                    "ReturnType" to returnSymbolToParse.symbol,
                    *codegenScope,
                ) {
                    val builderSymbol = symbolProvider.symbolForBuilder(shape)
                    val includedMembers = shape.members()

                    rustTemplate(
                        """
                        #{StructurePairParserFn:W}

                        let mut builder = #{Builder}::default();

                        #{DecodeStructureMapLoop:W}
                        """,
                        *codegenScope,
                        "StructurePairParserFn" to structurePairParserFnWritable(builderSymbol, includedMembers),
                        "Builder" to builderSymbol,
                        "DecodeStructureMapLoop" to decodeStructureMapLoopWritable(),
                    )

                    // Only call `build()` if the builder is not fallible. Otherwise, return the builder.
                    if (returnSymbolToParse.isUnconstrained) {
                        rust("Ok(builder)")
                    } else {
                        val builder =
                            builderInstantiator.finalizeBuilder(
                                "builder", shape,
                            ) {
                                rustTemplate(
                                    """|err| #{Error}::custom(err.to_string(), decoder.position())""", *codegenScope,
                                )
                            }
                        rust("##[allow(clippy::needless_question_mark)]")
                        rustBlock("") {
                            rust("return Ok(#T);", builder)
                        }
                    }
                }
            }
        rust("#T(decoder)", parser)
    }

    private fun RustWriter.deserializeUnion(shape: UnionShape) {
        val returnSymbolToParse = returnSymbolToParse(shape)
        val beforeDecoderMapCustomization =
            customizations.map { customization ->
                customization.section(
                    CborParserSection.UnionParserBeforeDecodingMap(
                        shape,
                    ),
                )
            }.join("")

        val parser =
            protocolFunctions.deserializeFn(shape) { fnName ->
                rustTemplate(
                    """
                    pub(crate) fn $fnName(decoder: &mut #{Decoder}) -> #{Result}<#{UnionSymbol}, #{Error}> {
                        #{UnionPairParserFnWritable}
                        #{BeforeDecoderMapCustomization:W}

                        match decoder.map()? {
                            None => {
                                let variant = pair(decoder)?;
                                match decoder.datatype()? {
                                    #{SmithyCbor}::data::Type::Break => {
                                        decoder.skip()?;
                                        Ok(variant)
                                    }
                                    ty => Err(
                                        #{Error}::unexpected_union_variant(
                                            ty,
                                            decoder.position(),
                                        ),
                                    ),
                                }
                            }
                            Some(1) => pair(decoder),
                            Some(_) => Err(#{Error}::mixed_union_variants(decoder.position()))
                        }
                    }
                    """,
                    "UnionSymbol" to returnSymbolToParse.symbol,
                    "UnionPairParserFnWritable" to unionPairParserFnWritable(shape),
                    "BeforeDecoderMapCustomization" to beforeDecoderMapCustomization,
                    *codegenScope,
                )
            }
        rust("#T(decoder)", parser)
    }
}
