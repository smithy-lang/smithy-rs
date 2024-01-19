/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize

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
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticOutputTrait
import software.amazon.smithy.rust.codegen.core.util.UNREACHABLE
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.expectTrait
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.isTargetUnit
import software.amazon.smithy.rust.codegen.core.util.outputShape

// TODO Cleanup commented and unused code.

/**
 * Class describing a JSON serializer section that can be used in a customization.
 */
sealed class CborSerializerSection(name: String) : Section(name) {
    /**
     * Mutate the serializer prior to serializing any structure members. Eg: this can be used to inject `__type`
     * to record the error type in the case of an error structure.
     */
    data class BeforeSerializingStructureMembers(val structureShape: StructureShape, val encoderBindingName: String) :
        CborSerializerSection("ServerError")

    /** Manipulate the serializer context for a map prior to it being serialized. **/
    data class BeforeIteratingOverMapOrCollection(val shape: Shape, val context: CborSerializerGenerator.Context<Shape>) :
        CborSerializerSection("BeforeIteratingOverMapOrCollection")
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
        /** Path in the JSON to get here, used for errors */
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
            fun collectionMember(context: Context<CollectionShape>, itemName: String): MemberContext =
                MemberContext(
                    "encoder",
                    ValueExpression.Reference(itemName),
                    context.shape.member,
                    writeNulls = true,
                )

            fun mapMember(context: Context<MapShape>, key: String, value: String): MemberContext =
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
            private fun encodeKeyExpression(name: String): String =
                "encoder.str(${name.dq()})"
        }
    }

    // Specialized since it holds a JsonObjectWriter expression rather than a JsonValueWriter
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
    // TODO Cleanup
    private val codegenScope = arrayOf(
        "String" to RuntimeType.String,
        "Error" to runtimeConfig.serializationError(),
        "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
        "Encoder" to RuntimeType.smithyCbor(runtimeConfig).resolve("Encoder"),
        "ByteSlab" to RuntimeType.ByteSlab,
    )
    private val serializerUtil = SerializerUtil(model)

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
        val suffix = when (error) {
            true -> "error"
            else -> "output"
        }
        return protocolFunctions.serializeFn(structureShape, fnNameSuffix = suffix) { fnName ->
            rustBlockTemplate(
                "pub fn $fnName(value: &#{target}) -> Result<Vec<u8>, #{Error}>",
                *codegenScope,
                "target" to symbolProvider.toSymbol(structureShape),
            ) {
                rustTemplate("let mut encoder = #{Encoder}::new(Vec::new());", *codegenScope)
                // Open a scope in which we can safely shadow the `encoder` variable to bind it to a mutable reference.
                rustBlock("") {
                    rust("let encoder = &mut encoder;")
                    serializeStructure(StructContext("value", structureShape), includedMembers)
                }
                rust("Ok(encoder.into_writer())")
            }
        }
    }

    // TODO
    override fun payloadSerializer(member: MemberShape): RuntimeType {
        val target = model.expectShape(member.target)
        return protocolFunctions.serializeFn(member, fnNameSuffix = "payload") { fnName ->
            rustBlockTemplate(
                "pub fn $fnName(input: &#{target}) -> std::result::Result<#{ByteSlab}, #{Error}>",
                *codegenScope,
                "target" to symbolProvider.toSymbol(target),
            ) {
                rust("let mut out = String::new();")
                rustTemplate("let mut object = #{JsonObjectWriter}::new(&mut out);", *codegenScope)
                when (target) {
                    is StructureShape -> serializeStructure(StructContext("input", target))
                    is UnionShape -> serializeUnion(Context(ValueExpression.Reference("input"), target))
                    else -> throw IllegalStateException("json payloadSerializer only supports structs and unions")
                }
                rust("object.finish();")
                rustTemplate("Ok(out.into_bytes())", *codegenScope)
            }
        }
    }

    // TODO Unclear whether we'll need this.
    override fun unsetStructure(structure: StructureShape): RuntimeType =
        ProtocolFunctions.crossOperationFn("rest_json_unsetpayload") { fnName ->
            rustTemplate(
                """
                pub fn $fnName() -> #{ByteSlab} {
                    b"{}"[..].into()
                }
                """,
                *codegenScope,
            )
        }

    override fun unsetUnion(union: UnionShape): RuntimeType {
        // TODO
        TODO("Not yet implemented")
    }

    override fun operationInputSerializer(operationShape: OperationShape): RuntimeType? {
        // Don't generate an operation CBOR serializer if there is no CBOR body.
        val httpDocumentMembers = httpBindingResolver.requestMembers(operationShape, HttpLocation.DOCUMENT)
        if (httpDocumentMembers.isEmpty()) {
            return null
        }

        val inputShape = operationShape.inputShape(model)
        return protocolFunctions.serializeFn(operationShape, fnNameSuffix = "input") { fnName ->
            rustBlockTemplate(
                "pub fn $fnName(input: &#{target}) -> Result<#{SdkBody}, #{Error}>",
                *codegenScope, "target" to symbolProvider.toSymbol(inputShape),
            ) {
                rust("let mut out = String::new();")
                rustTemplate("let mut object = #{JsonObjectWriter}::new(&mut out);", *codegenScope)
                serializeStructure(StructContext("input", inputShape), httpDocumentMembers)
                rust("object.finish();")
                rustTemplate("Ok(#{SdkBody}::from(out))", *codegenScope)
            }
        }
    }

    override fun documentSerializer(): RuntimeType {
        return ProtocolFunctions.crossOperationFn("serialize_document") { fnName ->
            rustTemplate(
                """
                pub fn $fnName(input: &#{Document}) -> #{ByteSlab} {
                    let mut out = String::new();
                    #{JsonValueWriter}::new(&mut out).document(input);
                    out.into_bytes()
                }
                """,
                "Document" to RuntimeType.document(runtimeConfig), *codegenScope,
            )
        }
    }

    override fun operationOutputSerializer(operationShape: OperationShape): RuntimeType? {
        // Don't generate an operation JSON serializer if there was no operation output shape in the
        // original (untransformed) model.
        val syntheticOutputTrait = operationShape.outputShape(model).expectTrait<SyntheticOutputTrait>()
        if (syntheticOutputTrait.originalId == null) {
            return null
        }

        // TODO
        // Note that, unlike the client, we serialize an empty JSON document `"{}"` if the operation output shape is
        // empty (has no members).
        // The client instead serializes an empty payload `""` in _both_ these scenarios:
        //     1. there is no operation input shape; and
        //     2. the operation input shape is empty (has no members).
        // The first case gets reduced to the second, because all operations get a synthetic input shape with
        // the [OperationNormalizer] transformation.
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
        // TODO Need to inject `__type` when serializing errors.
        val structureSerializer = protocolFunctions.serializeFn(context.shape) { fnName ->
            rustBlockTemplate(
                "pub fn $fnName(encoder: &mut #{Encoder}, ##[allow(unused)] input: &#{StructureSymbol}) -> Result<(), #{Error}>",
                "StructureSymbol" to symbolProvider.toSymbol(context.shape),
                *codegenScope,
            ) {
                // TODO If all members are non-`Option`-al, we know AOT the map's size and can use `.map()`
                //  instead of `.begin_map()` for efficiency. Add test.
                rust("encoder.begin_map();")
                for (customization in customizations) {
                    customization.section(CborSerializerSection.BeforeSerializingStructureMembers(context.shape, "encoder"))(this)
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
                    serializeMemberValue(context, targetShape)
                }
                if (context.writeNulls) {
                    rustBlock("else") {
                        rust("${context.encoderBindingName}.null();")
                    }
                }
            }
        } else {
            with(serializerUtil) {
                ignoreZeroValues(context.shape, context.valueExpression) {
                    serializeMemberValue(context, targetShape)
                }
            }
        }
    }

    private fun RustWriter.serializeMemberValue(context: MemberContext, target: Shape) {
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

            // TODO Document shapes have not been specced out yet.
            // is DocumentShape -> rust("$encoder.document(${value.asRef()});")

            // Aggregate shapes: https://smithy.io/2.0/spec/aggregate-types.html
            else -> {
                // This condition is equivalent to `containerShape !is CollectionShape`.
                if (containerShape is StructureShape || containerShape is UnionShape || containerShape is MapShape) {
                    rust("$encoder;") // Encode the member key.
                }
                when (target) {
                    is StructureShape -> serializeStructure(StructContext(value.name, target))
                    is CollectionShape -> serializeCollection(Context(value, target))
                    is MapShape -> serializeMap(Context(value, target))
                    is UnionShape -> serializeUnion(Context(value, target))
                    else -> UNREACHABLE("Smithy added a new aggregate shape: $target")
                }
            }
        }
    }

    private fun RustWriter.serializeCollection(context: Context<CollectionShape>) {
        // `.expect()` safety: `From<u64> for usize` is not in the standard library, but the conversion should be
        // infallible (unless we ever have 128-bit machines I guess).
        // See https://users.rust-lang.org/t/cant-convert-usize-to-u64/6243.
        // TODO Point to a `static` to not inflate the binary.
        for (customization in customizations) {
            customization.section(CborSerializerSection.BeforeIteratingOverMapOrCollection(context.shape, context))(this)
        }
        rust(
            """
            encoder.array(
                (${context.valueExpression.asRef()}).len().try_into().expect("`usize` to `u64` conversion failed")
            );
            """
        )
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
        rust(
            """
            encoder.map(
                (${context.valueExpression.asRef()}).len().try_into().expect("`usize` to `u64` conversion failed")
            );
            """
        )
        rustBlock("for ($keyName, $valueName) in ${context.valueExpression.asRef()}") {
            val keyExpression = "$keyName.as_str()"
            serializeMember(MemberContext.mapMember(context, keyExpression, valueName))
        }
    }

    private fun RustWriter.serializeUnion(context: Context<UnionShape>) {
        val unionSymbol = symbolProvider.toSymbol(context.shape)
        val unionSerializer = protocolFunctions.serializeFn(context.shape) { fnName ->
            rustBlockTemplate(
                "pub fn $fnName(encoder: &mut #{Encoder}, input: &#{UnionSymbol}) -> Result<(), #{Error}>",
                "UnionSymbol" to unionSymbol,
                *codegenScope,
            ) {
                // A union is serialized identically as a `structure` shape, but only a single member can be set to a
                // non-null value.
                rust("encoder.map(1);")
                rustBlock("match input") {
                    for (member in context.shape.members()) {
                        val variantName = if (member.isTargetUnit()) {
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
                            "#{Union}::${UnionGenerator.UnknownVariantName} => return Err(#{Error}::unknown_variant(${unionSymbol.name.dq()}))",
                            "Union" to unionSymbol,
                            *codegenScope,
                        )
                    }
                }
                rust("Ok(())")
            }
        }
        rust("#T(encoder, ${context.valueExpression.asRef()})?;", unionSerializer)
    }
}
