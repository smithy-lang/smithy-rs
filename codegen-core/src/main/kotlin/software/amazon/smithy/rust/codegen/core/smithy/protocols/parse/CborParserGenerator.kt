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
import software.amazon.smithy.model.traits.SparseTrait
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.conditionalBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.customize.NamedCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.Section
import software.amazon.smithy.rust.codegen.core.smithy.generators.setterName
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.smithy.isRustBoxed
import software.amazon.smithy.rust.codegen.core.smithy.protocols.HttpBindingResolver
import software.amazon.smithy.rust.codegen.core.smithy.protocols.HttpLocation
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolFunctions
import software.amazon.smithy.rust.codegen.core.util.PANIC
import software.amazon.smithy.rust.codegen.core.util.UNREACHABLE
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.outputShape

/**
 * Class describing a CBOR parser section that can be used in a customization.
 */
sealed class CborParserSection(name: String) : Section(name) {
    data class BeforeBoxingDeserializedMember(val shape: MemberShape) : CborParserSection("BeforeBoxingDeserializedMember")
}

/**
 * Customization for the CBOR parser.
 */
typealias CborParserCustomization = NamedCustomization<CborParserSection>

// TODO Add a `CborParserGeneratorTest` a la `CborSerializerGeneratorTest`.
class CborParserGenerator(
    private val codegenContext: CodegenContext,
    private val httpBindingResolver: HttpBindingResolver,
    /** See docs for this parameter in [JsonParserGenerator]. */
    private val returnSymbolToParse: (Shape) -> ReturnSymbolToParse = { shape ->
        ReturnSymbolToParse(codegenContext.symbolProvider.toSymbol(shape), false)
    },
    private val customizations: List<CborParserCustomization> = listOf(),
) : StructuredDataParserGenerator {
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val runtimeConfig = codegenContext.runtimeConfig
    // TODO Use?
    private val codegenTarget = codegenContext.target
    private val smithyCbor = CargoDependency.smithyCbor(runtimeConfig).toType()
    private val protocolFunctions = ProtocolFunctions(codegenContext)
    private val codegenScope = arrayOf(
        "SmithyCbor" to smithyCbor,
        "Decoder" to smithyCbor.resolve("Decoder"),
        "Error" to smithyCbor.resolve("decode::DeserializeError"),
        "HashMap" to RuntimeType.HashMap,
        "Vec" to RuntimeType.Vec,
    )

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
            ) -> Result<#{ListSymbol}, #{Error}>
            """,
            *codegenScope,
            "ListSymbol" to listSymbol,
        ) {
            if (isSparseList) {
                withBlock("let res = ", ";") {
                    deserializeMember(memberShape)
                }
                rust(
                    """
                    let value = match res {
                        Ok(value) => Some(value),
                        Err(_e) => {
                            let _v = decoder.null()?;
                            None
                        }
                    };
                    """
                )
            } else {
                withBlock("let value = ", "?;") {
                    deserializeMember(memberShape)
                }
            }

            if (returnUnconstrainedType) {
                rust("list.0.push(value);")
            } else {
                rust("list.push(value);")
            }

            rust("Ok(list)")
        }
    }

    // TODO DRY out with lists
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
            ) -> Result<#{MapSymbol}, #{Error}>
            """,
            *codegenScope,
            "MapSymbol" to mapSymbol,
        ) {
            withBlock("let key = ", "?;") {
                deserializeString(keyTarget)
            }
            if (isSparseMap) {
                withBlock("let res = ", ";") {
                    deserializeMember(valueShape)
                }
                rust(
                    """
                    let value = match res {
                        Ok(value) => Some(value),
                        Err(e) => {
                            let _v = decoder.null()?;
                            None
                        }
                    };
                    """
                )
            } else {
                withBlock("let value = ", "?;") {
                    deserializeMember(valueShape)
                }
            }

            if (returnUnconstrainedType) {
                rust("map.0.insert(key, value);")
            } else {
                rust("map.insert(key, value);")
            }

            rust("Ok(map)")
        }
    }

    private fun structurePairParserFnWritable(builderSymbol: Symbol, includedMembers: Collection<MemberShape>) = writable {
        rustBlockTemplate(
            """
            fn pair(
                mut builder: #{Builder},
                decoder: &mut #{Decoder}
            ) -> Result<#{Builder}, #{Error}>
            """,
            *codegenScope,
            "Builder" to builderSymbol,
        ) {
            withBlock("builder = match decoder.str()? {", "};") {
                for (member in includedMembers) {
                    rustBlock("${member.memberName.dq()} =>") {
                        withBlock("builder.${member.setterName()}(", ")") {
                            conditionalBlock("Some(", ")", symbolProvider.toSymbol(member).isOptional()) {
                                val symbol = symbolProvider.toSymbol(member)
                                if (symbol.isRustBoxed()) {
                                    rustBlock("") {
                                        withBlock("let v = ", "?;") {
                                            deserializeMember(member)
                                        }
                                        for (customization in customizations) {
                                            customization.section(CborParserSection.BeforeBoxingDeserializedMember(member))(this)
                                        }
                                        rust("Box::new(v)")
                                    }
                                } else {
                                    deserializeMember(member)
                                    rust("?")
                                }
                            }
                        }
                    }
                }

                // TODO Skip like in JSON or reject? I think we should reject in JSON too. Cut issue.
                rust("_ => { todo!() }")
            }
            rust("Ok(builder)")
        }
    }

    private fun unionPairParserFnWritable(shape: UnionShape) = writable {
        val returnSymbolToParse = returnSymbolToParse(shape)
        // TODO Test with unit variants
        // TODO Test with all unit variants
        rustBlockTemplate(
            """
            fn pair(
                decoder: &mut #{Decoder}
            ) -> Result<#{UnionSymbol}, #{Error}>
            """,
            *codegenScope,
            "UnionSymbol" to returnSymbolToParse.symbol,
        ) {
            withBlock("Ok(match decoder.str()? {", "})") {
                for (member in shape.members()) {
                    val variantName = symbolProvider.toMemberName(member)

                    withBlock("${member.memberName.dq()} => #T::$variantName(", "?),", returnSymbolToParse.symbol) {
                        deserializeMember(member)
                    }
                }
                // TODO Test client mode (parse unknown variant) and server mode (reject unknown variant).
                // In client mode, resolve an unknown union variant to the unknown variant.
                // In server mode, use strict parsing.
                // Consultation: https://github.com/awslabs/smithy/issues/1222
                rust("_ => { todo!() }")
            }
        }
    }

    private fun decodeStructureMapLoopWritable() = writable {
        rustTemplate(
            """
            match decoder.map()? {
                None => loop {
                    match decoder.datatype()? {
                        #{SmithyCbor}::data::Type::Break => {
                            decoder.skip()?;
                            break;
                        }
                        _ => {
                            builder = pair(builder, decoder)?;
                        }
                    };
                },
                Some(n) => {
                    for _ in 0..n {
                        builder = pair(builder, decoder)?;
                    }
                }
            };
            """,
            *codegenScope,
        )
    }

    // TODO This should be DRYed up with `decodeStructureMapLoopWritable`.
    private fun decodeMapLoopWritable() = writable {
        rustTemplate(
            """
            match decoder.map()? {
                None => loop {
                    match decoder.datatype()? {
                        #{SmithyCbor}::data::Type::Break => {
                            decoder.skip()?;
                            break;
                        }
                        _ => {
                            map = pair(map, decoder)?;
                        }
                    };
                },
                Some(n) => {
                    for _ in 0..n {
                        map = pair(map, decoder)?;
                    }
                }
            };
            """,
            *codegenScope,
        )
    }

    // TODO This should be DRYed up with `decodeStructureMapLoopWritable`.
    private fun decodeListLoop() = writable {
        rustTemplate(
            """
            match decoder.list()? {
                None => loop {
                    match decoder.datatype()? {
                        #{SmithyCbor}::data::Type::Break => {
                            decoder.skip()?;
                            break;
                        }
                        _ => {
                            list = member(list, decoder)?;
                        }
                    };
                },
                Some(n) => {
                    for _ in 0..n {
                        list = member(list, decoder)?;
                    }
                }
            };
            """,
            *codegenScope,
        )
    }

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
            // TODO Test no members.
//            val unusedMut = if (includedMembers.isEmpty()) "##[allow(unused_mut)] " else ""
            // TODO Assert token stream ended.
            rustTemplate(
                """
                pub(crate) fn $fnName(value: &[u8], mut builder: #{Builder}) -> Result<#{Builder}, #{Error}> {
                    #{StructurePairParserFn:W}
                    
                    let decoder = &mut #{Decoder}::new(value);
                    
                    #{DecodeStructureMapLoop:W}

                    if decoder.position() != value.len() {
                        todo!()
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
        UNREACHABLE("No protocol using CBOR serialization supports payload binding")
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
            fnNameSuffix = "json_err",
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

    private fun RustWriter.deserializeMember(memberShape: MemberShape) {
        when (val target = model.expectShape(memberShape.target)) {
            // Simple shapes: https://smithy.io/2.0/spec/simple-types.html
            is BlobShape -> rust("decoder.blob()")
            is BooleanShape -> rust("decoder.boolean()")

            is StringShape -> deserializeString(target)

            is ByteShape -> rust("decoder.byte()")
            is ShortShape -> rust("decoder.short()")
            is IntegerShape -> rust("decoder.integer()")
            is LongShape -> rust("decoder.long()")

            is FloatShape -> rust("decoder.float()")
            is DoubleShape -> rust("decoder.double()")

            is TimestampShape -> rust("decoder.timestamp()")

            // TODO Document shapes have not been specced out yet.
            // is DocumentShape -> rustTemplate("Some(#{expect_document}(tokens)?)", *codegenScope)

            // Aggregate shapes: https://smithy.io/2.0/spec/aggregate-types.html
            is StructureShape -> deserializeStruct(target)
            is CollectionShape -> deserializeCollection(target)
            is MapShape -> deserializeMap(target)
            is UnionShape -> deserializeUnion(target)
            else -> PANIC("unexpected shape: $target")
        }
        // TODO Boxing
//        val symbol = symbolProvider.toSymbol(memberShape)
//        if (symbol.isRustBoxed()) {
//            for (customization in customizations) {
//                customization.section(JsonParserSection.BeforeBoxingDeserializedMember(memberShape))(this)
//            }
//            rust(".map(Box::new)")
//        }
    }

    private fun RustWriter.deserializeString(target: StringShape, bubbleUp: Boolean = true) {
        // TODO Handle enum shapes
        rust("decoder.string()")
    }

    private fun RustWriter.deserializeCollection(shape: CollectionShape) {
        val (returnSymbol, returnUnconstrainedType) = returnSymbolToParse(shape)

        // TODO Test `@sparse` and non-@sparse lists.
        //      - Clients should insert only non-null values in non-`@sparse` list.
        //      - Servers should reject upon encountering first null value in non-`@sparse` list.
        //      - Both clients and servers should insert null values in `@sparse` list.

        val parser = protocolFunctions.deserializeFn(shape) { fnName ->
            val initContainerWritable = writable {
                withBlock("let mut list = ", ";") {
                    conditionalBlock("#{T}(", ")", conditional = returnUnconstrainedType, returnSymbol) {
                        rustTemplate("#{Vec}::new()", *codegenScope)
                    }
                }
            }

            rustTemplate(
                """
                pub(crate) fn $fnName(decoder: &mut #{Decoder}) -> Result<#{ReturnType}, #{Error}> {
                    #{ListMemberParserFn:W}
                    
                    #{InitContainerWritable:W}
                    
                    #{DecodeListLoop:W}
                
                    Ok(list)
                }
                """,
                "ReturnType" to returnSymbol,
                "ListMemberParserFn" to listMemberParserFn(
                    returnSymbol,
                    isSparseList = shape.hasTrait<SparseTrait>(),
                    shape.member,
                    returnUnconstrainedType = returnUnconstrainedType,
                ),
                "InitContainerWritable" to initContainerWritable,
                "DecodeListLoop" to decodeListLoop(),
                *codegenScope,
            )
        }
        rust("#T(decoder)", parser)
    }

    private fun RustWriter.deserializeMap(shape: MapShape) {
        val keyTarget = model.expectShape(shape.key.target, StringShape::class.java)
        val (returnSymbol, returnUnconstrainedType) = returnSymbolToParse(shape)

        // TODO Test `@sparse` and non-@sparse maps.
        //      - Clients should insert only non-null values in non-`@sparse` map.
        //      - Servers should reject upon encountering first null value in non-`@sparse` map.
        //      - Both clients and servers should insert null values in `@sparse` map.

        val parser = protocolFunctions.deserializeFn(shape) { fnName ->
            val initContainerWritable = writable {
                withBlock("let mut map = ", ";") {
                    conditionalBlock("#{T}(", ")", conditional = returnUnconstrainedType, returnSymbol) {
                        rustTemplate("#{HashMap}::new()", *codegenScope)
                    }
                }
            }

            rustTemplate(
                """
                pub(crate) fn $fnName(decoder: &mut #{Decoder}) -> Result<#{ReturnType}, #{Error}> {
                    #{MapPairParserFn:W}
                    
                    #{InitContainerWritable:W}
                    
                    #{DecodeMapLoop:W}
                
                    Ok(map)
                }
                """,
                "ReturnType" to returnSymbol,
                "MapPairParserFn" to mapPairParserFnWritable(
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
        val parser = protocolFunctions.deserializeFn(shape) { fnName ->
            rustBlockTemplate(
                "pub(crate) fn $fnName(decoder: &mut #{Decoder}) -> Result<#{ReturnType}, #{Error}>",
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
                    rust("Ok(builder.build())")
                }
            }
        }
        rust("#T(decoder)", parser)
    }

    private fun RustWriter.deserializeUnion(shape: UnionShape) {
        val returnSymbolToParse = returnSymbolToParse(shape)
        val parser = protocolFunctions.deserializeFn(shape) { fnName ->
            rustTemplate(
                """
                pub(crate) fn $fnName(decoder: &mut #{Decoder}) -> Result<#{UnionSymbol}, #{Error}> {
                    #{UnionPairParserFnWritable}
                    
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
                *codegenScope,
            )
        }
        rust("#T(decoder)", parser)
    }
}
