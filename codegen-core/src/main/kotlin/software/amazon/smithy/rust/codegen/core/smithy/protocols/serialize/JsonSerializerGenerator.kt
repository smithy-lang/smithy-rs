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
import software.amazon.smithy.model.shapes.NumberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.ShortShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.TimestampShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.TimestampFormatTrait.Format.EPOCH_SECONDS
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.rustlang.writable
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
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.isTargetUnit
import software.amazon.smithy.rust.codegen.core.util.outputShape

/**
 * Class describing a JSON serializer section that can be used in a customization.
 */
sealed class JsonSerializerSection(name: String) : Section(name) {
    /** Mutate the server error object prior to finalization. Eg: this can be used to inject `__type` to record the error type. */
    data class ServerError(val structureShape: StructureShape, val jsonObject: String) :
        JsonSerializerSection("ServerError")

    /** Manipulate the serializer context for a map prior to it being serialized. **/
    data class BeforeIteratingOverMapOrCollection(val shape: Shape, val context: JsonSerializerGenerator.Context<Shape>) :
        JsonSerializerSection("BeforeIteratingOverMapOrCollection")

    /** Manipulate the serializer context for a non-null member prior to it being serialized. **/
    data class BeforeSerializingNonNullMember(val shape: Shape, val context: JsonSerializerGenerator.MemberContext) :
        JsonSerializerSection("BeforeSerializingNonNullMember")

    /** Mutate the input object prior to finalization. */
    data class InputStruct(val structureShape: StructureShape, val jsonObject: String) :
        JsonSerializerSection("InputStruct")

    /** Mutate the output object prior to finalization. */
    data class OutputStruct(val structureShape: StructureShape, val jsonObject: String) :
        JsonSerializerSection("OutputStruct")

    /** Allow customizers to perform pre-serialization operations before handling union variants. */
    data class BeforeSerializeUnion(val shape: UnionShape, val jsonObject: String) :
        JsonSerializerSection("BeforeSerializeUnion")
}

/**
 * Customization for the JSON serializer.
 */
typealias JsonSerializerCustomization = NamedCustomization<JsonSerializerSection>

class JsonSerializerGenerator(
    codegenContext: CodegenContext,
    private val httpBindingResolver: HttpBindingResolver,
    /** Function that maps a MemberShape into a JSON field name */
    private val jsonName: (MemberShape) -> String,
    private val customizations: List<JsonSerializerCustomization> = listOf(),
) : StructuredDataSerializerGenerator {
    data class Context<out T : Shape>(
        /** Expression that retrieves a JsonValueWriter from either a JsonObjectWriter or JsonArrayWriter */
        val writerExpression: String,
        /** Expression representing the value to write to the JsonValueWriter */
        var valueExpression: ValueExpression,
        /** Path in the JSON to get here, used for errors */
        val shape: T,
    )

    data class MemberContext(
        /** Expression that retrieves a JsonValueWriter from either a JsonObjectWriter or JsonArrayWriter */
        val writerExpression: String,
        /** Expression representing the value to write to the JsonValueWriter */
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
                    "${context.writerExpression}.value()",
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
                    "${context.writerExpression}.key($key)",
                    ValueExpression.Reference(value),
                    context.shape.value,
                    writeNulls = true,
                )

            fun structMember(
                context: StructContext,
                member: MemberShape,
                symProvider: RustSymbolProvider,
                jsonName: (MemberShape) -> String,
            ): MemberContext =
                MemberContext(
                    objectValueWriterExpression(context.objectName, jsonName(member)),
                    ValueExpression.Value("${context.localName}.${symProvider.toMemberName(member)}"),
                    member,
                )

            fun unionMember(
                context: Context<UnionShape>,
                variantReference: String,
                member: MemberShape,
                jsonName: (MemberShape) -> String,
            ): MemberContext =
                MemberContext(
                    objectValueWriterExpression(context.writerExpression, jsonName(member)),
                    ValueExpression.Reference(variantReference),
                    member,
                )

            /** Returns an expression to get a JsonValueWriter from a JsonObjectWriter */
            private fun objectValueWriterExpression(
                objectWriterName: String,
                jsonName: String,
            ): String = "$objectWriterName.key(${jsonName.dq()})"
        }
    }

    // Specialized since it holds a JsonObjectWriter expression rather than a JsonValueWriter
    data class StructContext(
        /** Name of the JsonObjectWriter */
        val objectName: String,
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
            "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
            "JsonObjectWriter" to RuntimeType.smithyJson(runtimeConfig).resolve("serialize::JsonObjectWriter"),
            "JsonValueWriter" to RuntimeType.smithyJson(runtimeConfig).resolve("serialize::JsonValueWriter"),
            "ByteSlab" to RuntimeType.ByteSlab,
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
        makeSection: (StructureShape, String) -> JsonSerializerSection,
        error: Boolean,
    ): RuntimeType {
        val suffix =
            when (error) {
                true -> "error"
                else -> "output"
            }
        return protocolFunctions.serializeFn(structureShape, fnNameSuffix = suffix) { fnName ->
            rustBlockTemplate(
                "pub fn $fnName(value: &#{target}) -> #{Result}<String, #{Error}>",
                *codegenScope,
                "target" to symbolProvider.toSymbol(structureShape),
            ) {
                rustTemplate(
                    """
                    let mut out = #{String}::new();
                    let mut object = #{JsonObjectWriter}::new(&mut out);
                    """,
                    *codegenScope,
                )
                serializeStructure(StructContext("object", "value", structureShape), includedMembers)
                customizations.forEach { it.section(makeSection(structureShape, "object"))(this) }
                rust(
                    """
                    object.finish();
                    Ok(out)
                    """,
                )
            }
        }
    }

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
                    is StructureShape -> serializeStructure(StructContext("object", "input", target))
                    is UnionShape -> serializeUnion(Context("object", ValueExpression.Reference("input"), target))
                    else -> throw IllegalStateException("json payloadSerializer only supports structs and unions")
                }
                rust("object.finish();")
                rustTemplate("Ok(out.into_bytes())", *codegenScope)
            }
        }
    }

    override fun unsetStructure(structure: StructureShape): RuntimeType =
        ProtocolFunctions.crossOperationFn("rest_json_unset_struct_payload") { fnName ->
            rustTemplate(
                """
                pub fn $fnName() -> #{ByteSlab} {
                    b"{}"[..].into()
                }
                """,
                *codegenScope,
            )
        }

    override fun unsetUnion(union: UnionShape): RuntimeType =
        ProtocolFunctions.crossOperationFn("rest_json_unset_union_payload") { fnName ->
            rustTemplate(
                "pub fn $fnName() -> #{ByteSlab} { #{Vec}::new() }",
                *codegenScope,
            )
        }

    override fun operationInputSerializer(operationShape: OperationShape): RuntimeType? {
        // Don't generate an operation JSON serializer if there is no JSON body.
        val httpDocumentMembers = httpBindingResolver.requestMembers(operationShape, HttpLocation.DOCUMENT)
        if (httpDocumentMembers.isEmpty()) {
            return null
        }

        val inputShape = operationShape.inputShape(model)
        return protocolFunctions.serializeFn(operationShape, fnNameSuffix = "input") { fnName ->
            rustBlockTemplate(
                "pub fn $fnName(input: &#{target}) -> #{Result}<#{SdkBody}, #{Error}>",
                *codegenScope, "target" to symbolProvider.toSymbol(inputShape),
            ) {
                rust("let mut out = String::new();")
                rustTemplate("let mut object = #{JsonObjectWriter}::new(&mut out);", *codegenScope)
                serializeStructure(StructContext("object", "input", inputShape), httpDocumentMembers)
                customizations.forEach { it.section(JsonSerializerSection.InputStruct(inputShape, "object"))(this) }
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
        if (!OperationNormalizer.hadUserModeledOperationOutput(operationShape, model)) {
            return null
        }

        // Note that, unlike the client, we serialize an empty JSON document `"{}"` if the operation output shape is
        // empty (has no members).
        // The client instead serializes an empty payload `""` in _both_ these scenarios:
        //     1. there is no operation input shape; and
        //     2. the operation input shape is empty (has no members).
        // The first case gets reduced to the second, because all operations get a synthetic input shape with
        // the [OperationNormalizer] transformation.
        val httpDocumentMembers = httpBindingResolver.responseMembers(operationShape, HttpLocation.DOCUMENT)

        val outputShape = operationShape.outputShape(model)
        return serverSerializer(outputShape, httpDocumentMembers, JsonSerializerSection::OutputStruct, error = false)
    }

    override fun serverErrorSerializer(shape: ShapeId): RuntimeType {
        val errorShape = model.expectShape(shape, StructureShape::class.java)
        val includedMembers =
            httpBindingResolver.errorResponseBindings(shape).filter { it.location == HttpLocation.DOCUMENT }
                .map { it.member }
        return serverSerializer(errorShape, includedMembers, JsonSerializerSection::ServerError, error = true)
    }

    private fun RustWriter.serializeStructure(
        context: StructContext,
        includedMembers: List<MemberShape>? = null,
    ) {
        val structureSerializer =
            protocolFunctions.serializeFn(context.shape) { fnName ->
                val inner = context.copy(objectName = "object", localName = "input")
                val members = includedMembers ?: inner.shape.members()
                val allowUnusedVariables =
                    writable {
                        if (members.isEmpty()) {
                            Attribute.AllowUnusedVariables.render(this)
                        }
                    }
                rustBlockTemplate(
                    """
                    pub fn $fnName(
                        #{AllowUnusedVariables:W} object: &mut #{JsonObjectWriter},
                        #{AllowUnusedVariables:W} input: &#{StructureSymbol},
                    ) -> #{Result}<(), #{Error}>
                    """,
                    "StructureSymbol" to symbolProvider.toSymbol(context.shape),
                    "AllowUnusedVariables" to allowUnusedVariables,
                    *codegenScope,
                ) {
                    for (member in members) {
                        serializeMember(MemberContext.structMember(inner, member, symbolProvider, jsonName))
                    }
                    rust("Ok(())")
                }
            }
        rust("#T(&mut ${context.objectName}, ${context.localName})?;", structureSerializer)
    }

    private fun RustWriter.serializeMember(context: MemberContext) {
        val targetShape = model.expectShape(context.shape.target)
        if (symbolProvider.toSymbol(context.shape).isOptional()) {
            safeName().also { local ->
                rustBlock("if let Some($local) = ${context.valueExpression.asRef()}") {
                    context.valueExpression = ValueExpression.Reference(local)
                    for (customization in customizations) {
                        customization.section(
                            JsonSerializerSection.BeforeSerializingNonNullMember(
                                targetShape,
                                context,
                            ),
                        )(this)
                    }
                    serializeMemberValue(context, targetShape)
                }
                if (context.writeNulls) {
                    rustBlock("else") {
                        rust("${context.writerExpression}.null();")
                    }
                }
            }
        } else {
            for (customization in customizations) {
                customization.section(JsonSerializerSection.BeforeSerializingNonNullMember(targetShape, context))(
                    this,
                )
            }

            with(serializerUtil) {
                ignoreDefaultsForNumbersAndBools(context.shape, context.valueExpression) {
                    serializeMemberValue(context, targetShape)
                }
            }
        }
    }

    private fun RustWriter.serializeMemberValue(
        context: MemberContext,
        target: Shape,
    ) {
        val writer = context.writerExpression
        val value = context.valueExpression

        when (target) {
            is StringShape -> rust("$writer.string(${value.name}.as_str());")
            is BooleanShape -> rust("$writer.boolean(${value.asValue()});")
            is NumberShape -> {
                val numberType =
                    when (target) {
                        is IntegerShape, is ByteShape, is LongShape, is ShortShape -> "NegInt"
                        is DoubleShape, is FloatShape -> "Float"
                        else -> throw IllegalStateException("unreachable")
                    }
                rust(
                    "$writer.number(##[allow(clippy::useless_conversion)]#T::$numberType((${value.asValue()}).into()));",
                    RuntimeType.smithyTypes(runtimeConfig).resolve("Number"),
                )
            }

            is BlobShape ->
                rust(
                    "$writer.string_unchecked(&#T(${value.asRef()}));",
                    RuntimeType.base64Encode(runtimeConfig),
                )

            is TimestampShape -> {
                val timestampFormat =
                    httpBindingResolver.timestampFormat(context.shape, HttpLocation.DOCUMENT, EPOCH_SECONDS, model)
                val timestampFormatType = RuntimeType.serializeTimestampFormat(runtimeConfig, timestampFormat)
                rustTemplate(
                    "$writer.date_time(${value.asRef()}, #{FormatType})?;",
                    "FormatType" to timestampFormatType,
                )
            }

            is CollectionShape ->
                jsonArrayWriter(context) { arrayName ->
                    serializeCollection(Context(arrayName, value, target))
                }

            is MapShape ->
                jsonObjectWriter(context) { objectName ->
                    serializeMap(Context(objectName, value, target))
                }

            is StructureShape ->
                jsonObjectWriter(context) { objectName ->
                    serializeStructure(StructContext(objectName, value.asRef(), target))
                }

            is UnionShape ->
                jsonObjectWriter(context) { objectName ->
                    serializeUnion(Context(objectName, value, target))
                }

            is DocumentShape -> rust("$writer.document(${value.asRef()});")
            else -> TODO(target.toString())
        }
    }

    private fun RustWriter.jsonArrayWriter(
        context: MemberContext,
        inner: RustWriter.(String) -> Unit,
    ) {
        safeName("array").also { arrayName ->
            rust("let mut $arrayName = ${context.writerExpression}.start_array();")
            inner(arrayName)
            rust("$arrayName.finish();")
        }
    }

    private fun RustWriter.jsonObjectWriter(
        context: MemberContext,
        inner: RustWriter.(String) -> Unit,
    ) {
        safeName("object").also { objectName ->
            rust("##[allow(unused_mut)]")
            rust("let mut $objectName = ${context.writerExpression}.start_object();")
            // We call inner only when context's shape is not the Unit type.
            // If it were, calling inner would generate the following function:
            //
            // ```rust
            // pub fn serialize_structure_crate_model_unit(
            //     object: &mut aws_smithy_json::serialize::JsonObjectWriter,
            //     input: &crate::model::Unit,
            // ) -> Result<(), aws_smithy_http::operation::error::SerializationError> {
            //     let (_, _) = (object, input);
            //     Ok(())
            // }
            // ```
            //
            // However, this would cause a compilation error at a call site because it cannot
            // extract data out of the Unit type that corresponds to the variable "input" above.
            if (!context.shape.isTargetUnit()) {
                inner(objectName)
            }
            rust("$objectName.finish();")
        }
    }

    private fun RustWriter.serializeCollection(context: Context<CollectionShape>) {
        val itemName = safeName("item")
        for (customization in customizations) {
            customization.section(JsonSerializerSection.BeforeIteratingOverMapOrCollection(context.shape, context))(this)
        }
        rustBlock("for $itemName in ${context.valueExpression.asRef()}") {
            serializeMember(MemberContext.collectionMember(context, itemName))
        }
    }

    private fun RustWriter.serializeMap(context: Context<MapShape>) {
        val keyName = safeName("key")
        val valueName = safeName("value")
        for (customization in customizations) {
            customization.section(JsonSerializerSection.BeforeIteratingOverMapOrCollection(context.shape, context))(
                this,
            )
        }
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
                    "pub fn $fnName(${context.writerExpression}: &mut #{JsonObjectWriter}, input: &#{Input}) -> #{Result}<(), #{Error}>",
                    "Input" to unionSymbol,
                    *codegenScope,
                ) {
                    // Allow customizers to perform pre-serialization operations before handling union variants.
                    customizations.forEach {
                        it.section(JsonSerializerSection.BeforeSerializeUnion(context.shape, context.writerExpression))(this)
                    }
                    rustBlock("match input") {
                        for (member in context.shape.members()) {
                            val memberShape = model.expectShape(member.target)
                            val isEmptyStruct =
                                memberShape.isStructureShape &&
                                    memberShape.asStructureShape().get().allMembers.isEmpty()
                            val variantName =
                                if (member.isTargetUnit()) {
                                    "${symbolProvider.toMemberName(member)}"
                                } else if (isEmptyStruct) {
                                    // Empty structures don't use the inner variable
                                    "${symbolProvider.toMemberName(member)}(_inner)"
                                } else {
                                    "${symbolProvider.toMemberName(member)}(inner)"
                                }
                            withBlock("#T::$variantName => {", "},", unionSymbol) {
                                val innerRef = if (isEmptyStruct) "_inner" else "inner"
                                serializeMember(MemberContext.unionMember(context, innerRef, member, jsonName))
                            }
                        }
                        if (codegenTarget.renderUnknownVariant()) {
                            rustTemplate(
                                "#{Union}::${UnionGenerator.UNKNOWN_VARIANT_NAME} => return Err(#{Error}::unknown_variant(${unionSymbol.name.dq()}))",
                                "Union" to unionSymbol,
                                *codegenScope,
                            )
                        }
                    }
                    rust("Ok(())")
                }
            }
        rust("#T(&mut ${context.writerExpression}, ${context.valueExpression.asRef()})?;", unionSerializer)
    }
}
