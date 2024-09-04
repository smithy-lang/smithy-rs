/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize

import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.BooleanShape
import software.amazon.smithy.model.shapes.ByteShape
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.DocumentShape
import software.amazon.smithy.model.shapes.DoubleShape
import software.amazon.smithy.model.shapes.FloatShape
import software.amazon.smithy.model.shapes.IntegerShape
import software.amazon.smithy.model.shapes.LongShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.ShortShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.TimestampShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.customize.NamedCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.Section
import software.amazon.smithy.rust.codegen.core.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.renderUnknownVariant
import software.amazon.smithy.rust.codegen.core.smithy.generators.serializationError
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.smithy.protocols.HttpBindingResolver
import software.amazon.smithy.rust.codegen.core.smithy.protocols.HttpLocation
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolFunctions
import software.amazon.smithy.rust.codegen.core.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.core.util.UNREACHABLE
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.isTargetUnit
import software.amazon.smithy.rust.codegen.core.util.isUnit
import software.amazon.smithy.rust.codegen.core.util.outputShape

/**
 * Class describing a CBOR serializer section that can be used in a customization.
 */
sealed class CborSerializerSection(name: String) : Section(name) {
    /**
     * Mutate the serializer prior to serializing any structure members. Eg: this can be used to inject `__type`
     * to record the error type in the case of an error structure.
     */
    data class BeforeSerializingStructureMembers(
        val structureShape: StructureShape,
        val encoderBindingName: String,
    ) : CborSerializerSection("BeforeSerializingStructureMembers")

    /** Manipulate the serializer context for a map prior to it being serialized. **/
    data class BeforeIteratingOverMapOrCollection(val shape: Shape, val context: CborSerializerGenerator.Context<Shape>) :
        CborSerializerSection("BeforeIteratingOverMapOrCollection")

    /** Manipulate the serializer context for a non-null member prior to it being serialized. **/
    data class BeforeSerializingNonNullMember(val shape: Shape, val context: CborSerializerGenerator.MemberContext) :
        CborSerializerSection("BeforeSerializingNonNullMember")
}

/**
 * Customization for the CBOR serializer.
 */
typealias CborSerializerCustomization = NamedCustomization<CborSerializerSection>

class CborSerializerGenerator(
    codegenContext: CodegenContext,
    private val httpBindingResolver: HttpBindingResolver,
    private val customizations: List<CborSerializerCustomization> = listOf(),
) : StructuredDataSerializerGenerator {
    data class Context<out T : Shape>(
        /** Expression representing the value to write to the encoder */
        var valueExpression: ValueExpression,
        /** Shape to serialize */
        val shape: T,
    )

    data class MemberContext(
        /** Name for the variable bound to the encoder object **/
        val encoderBindingName: String,
        /** Expression representing the value to write to the `Encoder` */
        var valueExpression: ValueExpression,
        val shape: MemberShape,
        /** Whether to serialize null values if the type is optional */
        val writeNulls: Boolean = false,
    ) {
        companion object {
            fun collectionMember(
                context: Context<CollectionShape>,
                itemName: String,
            ): MemberContext =
                MemberContext(
                    "encoder",
                    ValueExpression.Reference(itemName),
                    context.shape.member,
                    writeNulls = true,
                )

            fun mapMember(
                context: Context<MapShape>,
                key: String,
                value: String,
            ): MemberContext =
                MemberContext(
                    "encoder.str($key)",
                    ValueExpression.Reference(value),
                    context.shape.value,
                    writeNulls = true,
                )

            fun structMember(
                context: StructContext,
                member: MemberShape,
                symProvider: RustSymbolProvider,
            ): MemberContext =
                MemberContext(
                    encodeKeyExpression(member.memberName),
                    ValueExpression.Value("${context.localName}.${symProvider.toMemberName(member)}"),
                    member,
                )

            fun unionMember(
                variantReference: String,
                member: MemberShape,
            ): MemberContext =
                MemberContext(
                    encodeKeyExpression(member.memberName),
                    ValueExpression.Reference(variantReference),
                    member,
                )

            /** Returns an expression to encode a key member **/
            private fun encodeKeyExpression(name: String): String = "encoder.str(${name.dq()})"
        }
    }

    data class StructContext(
        /** Name of the variable that holds the struct */
        val localName: String,
        val shape: StructureShape,
    )

    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val codegenTarget = codegenContext.target
    private val runtimeConfig = codegenContext.runtimeConfig
    private val protocolFunctions = ProtocolFunctions(codegenContext)

    private val codegenScope =
        arrayOf(
            *preludeScope,
            "Error" to runtimeConfig.serializationError(),
            "Encoder" to RuntimeType.smithyCbor(runtimeConfig).resolve("Encoder"),
            "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
        )
    private val serializerUtil = SerializerUtil(model, symbolProvider)

    /**
     * Reusable structure serializer implementation that can be used to generate serializing code for
     * operation outputs or errors.
     * This function is only used by the server, the client uses directly [serializeStructure].
     */
    private fun serverSerializer(
        structureShape: StructureShape,
        includedMembers: List<MemberShape>,
        error: Boolean,
    ): RuntimeType {
        val suffix =
            when (error) {
                true -> "error"
                else -> "output"
            }
        return protocolFunctions.serializeFn(structureShape, fnNameSuffix = suffix) { fnName ->
            rustBlockTemplate(
                "pub fn $fnName(value: &#{target}) -> #{Result}<#{Vec}<u8>, #{Error}>",
                *codegenScope,
                "target" to symbolProvider.toSymbol(structureShape),
            ) {
                rustTemplate("let mut encoder = #{Encoder}::new(#{Vec}::new());", *codegenScope)
                // Open a scope in which we can safely shadow the `encoder` variable to bind it to a mutable reference.
                rustBlock("") {
                    rust("let encoder = &mut encoder;")
                    serializeStructure(
                        StructContext("value", structureShape),
                        includedMembers,
                    )
                }
                rustTemplate("#{Ok}(encoder.into_writer())", *codegenScope)
            }
        }
    }

    override fun payloadSerializer(member: MemberShape): RuntimeType {
        val target = model.expectShape(member.target)
        return protocolFunctions.serializeFn(member, fnNameSuffix = "payload") { fnName ->
            rustBlockTemplate(
                "pub fn $fnName(input: &#{target}) -> std::result::Result<#{Vec}<u8>, #{Error}>",
                *codegenScope,
                "target" to symbolProvider.toSymbol(target),
            ) {
                rustTemplate("let mut encoder = #{Encoder}::new(#{Vec}::new());", *codegenScope)
                rustBlock("") {
                    rust("let encoder = &mut encoder;")
                    when (target) {
                        is StructureShape -> serializeStructure(StructContext("input", target))
                        is UnionShape -> serializeUnion(Context(ValueExpression.Reference("input"), target))
                        else -> throw IllegalStateException("CBOR payloadSerializer only supports structs and unions")
                    }
                }
                rustTemplate("#{Ok}(encoder.into_writer())", *codegenScope)
            }
        }
    }

    override fun unsetStructure(structure: StructureShape): RuntimeType =
        UNREACHABLE("Only clients use this method when serializing an `@httpPayload`. No protocol using CBOR supports this trait, so we don't need to implement this")

    override fun unsetUnion(union: UnionShape): RuntimeType =
        UNREACHABLE("Only clients use this method when serializing an `@httpPayload`. No protocol using CBOR supports this trait, so we don't need to implement this")

    override fun operationInputSerializer(operationShape: OperationShape): RuntimeType? {
        // Don't generate an operation CBOR serializer if there was no operation input shape in the
        // original (untransformed) model.
        if (!OperationNormalizer.hadUserModeledOperationInput(operationShape, model)) {
            return null
        }

        val httpDocumentMembers = httpBindingResolver.requestMembers(operationShape, HttpLocation.DOCUMENT)
        val inputShape = operationShape.inputShape(model)
        return protocolFunctions.serializeFn(operationShape, fnNameSuffix = "input") { fnName ->
            rustBlockTemplate(
                "pub fn $fnName(input: &#{target}) -> Result<#{SdkBody}, #{Error}>",
                *codegenScope, "target" to symbolProvider.toSymbol(inputShape),
            ) {
                rustTemplate("let mut encoder = #{Encoder}::new(Vec::new());", *codegenScope)
                // Open a scope in which we can safely shadow the `encoder` variable to bind it to a mutable reference
                // which doesn't require us to pass `&mut encoder` where requested.
                rustBlock("") {
                    rust("let encoder = &mut encoder;")
                    serializeStructure(StructContext("input", inputShape), httpDocumentMembers)
                }
                rustTemplate("Ok(#{SdkBody}::from(encoder.into_writer()))", *codegenScope)
            }
        }
    }

    override fun documentSerializer(): RuntimeType =
        UNREACHABLE("No protocol using CBOR supports `document` shapes, so we don't need to implement this")

    override fun operationOutputSerializer(operationShape: OperationShape): RuntimeType? {
        // Don't generate an operation CBOR serializer if there was no operation output shape in the
        // original (untransformed) model.
        if (!OperationNormalizer.hadUserModeledOperationOutput(operationShape, model)) {
            return null
        }

        val httpDocumentMembers = httpBindingResolver.responseMembers(operationShape, HttpLocation.DOCUMENT)
        val outputShape = operationShape.outputShape(model)
        return serverSerializer(outputShape, httpDocumentMembers, error = false)
    }

    override fun serverErrorSerializer(shape: ShapeId): RuntimeType {
        val errorShape = model.expectShape(shape, StructureShape::class.java)
        val includedMembers =
            httpBindingResolver.errorResponseBindings(shape).filter { it.location == HttpLocation.DOCUMENT }
                .map { it.member }
        return serverSerializer(errorShape, includedMembers, error = true)
    }

    private fun RustWriter.serializeStructure(
        context: StructContext,
        includedMembers: List<MemberShape>? = null,
    ) {
        if (context.shape.isUnit()) {
            rust(
                """
                encoder.begin_map();
                encoder.end();
                """,
            )
            return
        }

        val structureSerializer =
            protocolFunctions.serializeFn(context.shape) { fnName ->
                rustBlockTemplate(
                    "pub fn $fnName(encoder: &mut #{Encoder}, ##[allow(unused)] input: &#{StructureSymbol}) -> #{Result}<(), #{Error}>",
                    "StructureSymbol" to symbolProvider.toSymbol(context.shape),
                    *codegenScope,
                ) {
                    // TODO(https://github.com/smithy-lang/smithy-rs/issues/3745) If all members are non-`Option`-al,
                    //  we know AOT the map's size and can use `.map()` instead of `.begin_map()` for efficiency.
                    rust("encoder.begin_map();")
                    for (customization in customizations) {
                        customization.section(
                            CborSerializerSection.BeforeSerializingStructureMembers(
                                context.shape,
                                "encoder",
                            ),
                        )(this)
                    }
                    context.copy(localName = "input").also { inner ->
                        val members = includedMembers ?: inner.shape.members()
                        for (member in members) {
                            serializeMember(MemberContext.structMember(inner, member, symbolProvider))
                        }
                    }
                    rust("encoder.end();")
                    rust("Ok(())")
                }
            }
        rust("#T(encoder, ${context.localName})?;", structureSerializer)
    }

    private fun RustWriter.serializeMember(context: MemberContext) {
        val targetShape = model.expectShape(context.shape.target)
        if (symbolProvider.toSymbol(context.shape).isOptional()) {
            safeName().also { local ->
                rustBlock("if let Some($local) = ${context.valueExpression.asRef()}") {
                    context.valueExpression = ValueExpression.Reference(local)
                    resolveValueExpressionForConstrainedType(targetShape, context)
                    serializeMemberValue(context, targetShape)
                }
                if (context.writeNulls) {
                    rustBlock("else") {
                        rust("${context.encoderBindingName}.null();")
                    }
                }
            }
        } else {
            resolveValueExpressionForConstrainedType(targetShape, context)
            with(serializerUtil) {
                ignoreDefaultsForNumbersAndBools(context.shape, context.valueExpression) {
                    serializeMemberValue(context, targetShape)
                }
            }
        }
    }

    private fun RustWriter.resolveValueExpressionForConstrainedType(
        targetShape: Shape,
        context: MemberContext,
    ) {
        for (customization in customizations) {
            customization.section(
                CborSerializerSection.BeforeSerializingNonNullMember(
                    targetShape,
                    context,
                ),
            )(this)
        }
    }

    private fun RustWriter.serializeMemberValue(
        context: MemberContext,
        target: Shape,
    ) {
        val encoder = context.encoderBindingName
        val value = context.valueExpression
        val containerShape = model.expectShape(context.shape.container)

        when (target) {
            // Simple shapes: https://smithy.io/2.0/spec/simple-types.html
            is BlobShape -> rust("$encoder.blob(${value.asRef()});")
            is BooleanShape -> rust("$encoder.boolean(${value.asValue()});")

            is StringShape -> rust("$encoder.str(${value.name}.as_str());")

            is ByteShape -> rust("$encoder.byte(${value.asValue()});")
            is ShortShape -> rust("$encoder.short(${value.asValue()});")
            is IntegerShape -> rust("$encoder.integer(${value.asValue()});")
            is LongShape -> rust("$encoder.long(${value.asValue()});")

            is FloatShape -> rust("$encoder.float(${value.asValue()});")
            is DoubleShape -> rust("$encoder.double(${value.asValue()});")

            is TimestampShape -> rust("$encoder.timestamp(${value.asRef()});")

            is DocumentShape -> UNREACHABLE("Smithy RPC v2 CBOR does not support `document` shapes")

            // Aggregate shapes: https://smithy.io/2.0/spec/aggregate-types.html
            else -> {
                // This condition is equivalent to `containerShape !is CollectionShape`.
                if (containerShape is StructureShape || containerShape is UnionShape || containerShape is MapShape) {
                    rust("$encoder;") // Encode the member key.
                }
                when (target) {
                    is StructureShape -> serializeStructure(StructContext(value.asRef(), target))
                    is CollectionShape -> serializeCollection(Context(value, target))
                    is MapShape -> serializeMap(Context(value, target))
                    is UnionShape -> serializeUnion(Context(value, target))
                    else -> UNREACHABLE("Smithy added a new aggregate shape: $target")
                }
            }
        }
    }

    private fun RustWriter.serializeCollection(context: Context<CollectionShape>) {
        for (customization in customizations) {
            customization.section(CborSerializerSection.BeforeIteratingOverMapOrCollection(context.shape, context))(this)
        }
        rust("encoder.array((${context.valueExpression.asValue()}).len());")
        val itemName = safeName("item")
        rustBlock("for $itemName in ${context.valueExpression.asRef()}") {
            serializeMember(MemberContext.collectionMember(context, itemName))
        }
    }

    private fun RustWriter.serializeMap(context: Context<MapShape>) {
        val keyName = safeName("key")
        val valueName = safeName("value")
        for (customization in customizations) {
            customization.section(CborSerializerSection.BeforeIteratingOverMapOrCollection(context.shape, context))(this)
        }
        rust("encoder.map((${context.valueExpression.asValue()}).len());")
        rustBlock("for ($keyName, $valueName) in ${context.valueExpression.asRef()}") {
            val keyExpression = "$keyName.as_str()"
            serializeMember(MemberContext.mapMember(context, keyExpression, valueName))
        }
    }

    private fun RustWriter.serializeUnion(context: Context<UnionShape>) {
        val unionSymbol = symbolProvider.toSymbol(context.shape)
        val unionSerializer =
            protocolFunctions.serializeFn(context.shape) { fnName ->
                rustBlockTemplate(
                    "pub fn $fnName(encoder: &mut #{Encoder}, input: &#{UnionSymbol}) -> #{Result}<(), #{Error}>",
                    "UnionSymbol" to unionSymbol,
                    *codegenScope,
                ) {
                    // A union is serialized identically as a `structure` shape, but only a single member can be set to a
                    // non-null value.
                    rust("encoder.map(1);")
                    rustBlock("match input") {
                        for (member in context.shape.members()) {
                            val variantName =
                                if (member.isTargetUnit()) {
                                    symbolProvider.toMemberName(member)
                                } else {
                                    "${symbolProvider.toMemberName(member)}(inner)"
                                }
                            rustBlock("#T::$variantName =>", unionSymbol) {
                                serializeMember(MemberContext.unionMember("inner", member))
                            }
                        }
                        if (codegenTarget.renderUnknownVariant()) {
                            rustTemplate(
                                "#{Union}::${UnionGenerator.UNKNOWN_VARIANT_NAME} => return #{Err}(#{Error}::unknown_variant(${unionSymbol.name.dq()}))",
                                "Union" to unionSymbol,
                                *codegenScope,
                            )
                        }
                    }
                    rustTemplate("#{Ok}(())", *codegenScope)
                }
            }
        rust("#T(encoder, ${context.valueExpression.asRef()})?;", unionSerializer)
    }
}
