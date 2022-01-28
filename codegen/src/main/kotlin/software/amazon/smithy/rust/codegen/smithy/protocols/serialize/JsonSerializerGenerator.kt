/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.protocols.serialize

import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.BooleanShape
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.DocumentShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.NumberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.TimestampShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.model.traits.TimestampFormatTrait.Format.EPOCH_SECONDS
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.withBlock
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.renderUnknownVariant
import software.amazon.smithy.rust.codegen.smithy.generators.serializationError
import software.amazon.smithy.rust.codegen.smithy.isOptional
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpBindingResolver
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpLocation
import software.amazon.smithy.rust.codegen.smithy.protocols.serializeFunctionName
import software.amazon.smithy.rust.codegen.smithy.rustType
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.hasTrait
import software.amazon.smithy.rust.codegen.util.inputShape
import software.amazon.smithy.rust.codegen.util.outputShape

class JsonSerializerGenerator(
    codegenContext: CodegenContext,
    private val httpBindingResolver: HttpBindingResolver,
    /** Function that maps a MemberShape into a JSON field name */
    private val jsonName: (MemberShape) -> String,
) : StructuredDataSerializerGenerator {
    private data class Context<T : Shape>(
        /** Expression that retrieves a JsonValueWriter from either a JsonObjectWriter or JsonArrayWriter */
        val writerExpression: String,
        /** Expression representing the value to write to the JsonValueWriter */
        val valueExpression: ValueExpression,
        /** Path in the JSON to get here, used for errors */
        val shape: T,
    )

    private data class MemberContext(
        /** Expression that retrieves a JsonValueWriter from either a JsonObjectWriter or JsonArrayWriter */
        val writerExpression: String,
        /** Expression representing the value to write to the JsonValueWriter */
        val valueExpression: ValueExpression,
        val shape: MemberShape,
        /** Whether or not to serialize null values if the type is optional */
        val writeNulls: Boolean = false,
    ) {
        companion object {
            fun collectionMember(context: Context<CollectionShape>, itemName: String): MemberContext =
                MemberContext(
                    "${context.writerExpression}.value()",
                    ValueExpression.Reference(itemName),
                    context.shape.member,
                    writeNulls = true
                )

            fun mapMember(context: Context<MapShape>, key: String, value: String): MemberContext =
                MemberContext(
                    "${context.writerExpression}.key($key)",
                    ValueExpression.Reference(value),
                    context.shape.value,
                    writeNulls = true
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
                    member
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
                    member
                )

            /** Returns an expression to get a JsonValueWriter from a JsonObjectWriter */
            private fun objectValueWriterExpression(objectWriterName: String, jsonName: String): String =
                "$objectWriterName.key(${jsonName.dq()})"
        }
    }

    // Specialized since it holds a JsonObjectWriter expression rather than a JsonValueWriter
    private data class StructContext(
        /** Name of the JsonObjectWriter */
        val objectName: String,
        /** Name of the variable that holds the struct */
        val localName: String,
        val shape: StructureShape,
    )

    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val mode = codegenContext.mode
    private val runtimeConfig = codegenContext.runtimeConfig
    private val smithyTypes = CargoDependency.SmithyTypes(runtimeConfig).asType()
    private val smithyJson = CargoDependency.smithyJson(runtimeConfig).asType()
    private val codegenScope = arrayOf(
        "String" to RuntimeType.String,
        "Error" to runtimeConfig.serializationError(),
        "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
        "JsonObjectWriter" to smithyJson.member("serialize::JsonObjectWriter"),
        "JsonValueWriter" to smithyJson.member("serialize::JsonValueWriter"),
    )
    private val serializerUtil = SerializerUtil(model)
    private val operationSerModule = RustModule.private("operation_ser")
    private val jsonSerModule = RustModule.private("json_ser")

    /**
     * Reusable structure serializer implementation that can be used to generate serializing code for
     * operation, error and structure shapes.
     * We still generate the serializer symbol even if there are no included members because the server
     * generation requires serializers for all output/error structures.
     */
    private fun structureSerializer(
        fnName: String,
        structureShape: StructureShape,
        includedMembers: List<MemberShape>
    ): RuntimeType {
        return RuntimeType.forInlineFun(fnName, operationSerModule) {
            it.rustBlockTemplate(
                "pub fn $fnName(value: &#{target}) -> Result<String, #{Error}>",
                *codegenScope, "target" to symbolProvider.toSymbol(structureShape)
            ) {
                rust("let mut out = String::new();")
                rustTemplate("let mut object = #{JsonObjectWriter}::new(&mut out);", *codegenScope)
                serializeStructure(StructContext("object", "value", structureShape), includedMembers)

                // All Smithy specs for AWS protocols that serialize to JSON
                // contain:
                //
                // > Error responses in <protocol> are serialized
                // > identically to standard responses with one additional
                // > component to distinguish which error is contained. The
                // > component MUST be one of the following: an additional header
                // > with the name `X-Amzn-Errortype`, a body field with the name
                // > code, or a body field named `__type`.
                // > The value of this component SHOULD contain the error's
                // > shape name.
                //
                // *Some* Smithy specs for AWS protocols that serialize to JSON
                // additionally contain:
                //
                // > New server-side protocol implementations SHOULD use a body
                // > field named `__type`.
                //
                // Since our server implementation is recent, we choose to
                // implement this latter behavior.
                if (structureShape.hasTrait<ErrorTrait>()) {
                    rust("""object.key("__type").string("${structureShape.id.name}");""")
                }
                rust("object.finish();")
                rustTemplate("Ok(out)", *codegenScope)
            }
        }
    }

    override fun payloadSerializer(member: MemberShape): RuntimeType {
        val fnName = symbolProvider.serializeFunctionName(member)
        val target = model.expectShape(member.target)
        return RuntimeType.forInlineFun(fnName, operationSerModule) { writer ->
            writer.rustBlockTemplate(
                "pub fn $fnName(input: &#{target}) -> std::result::Result<std::vec::Vec<u8>, #{Error}>",
                *codegenScope,
                "target" to symbolProvider.toSymbol(target)
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

    override fun unsetStructure(structure: StructureShape): RuntimeType {
        return RuntimeType.forInlineFun("rest_json_unsetpayload", operationSerModule) { writer ->
            writer.rustTemplate(
                """
                pub fn rest_json_unsetpayload() -> std::vec::Vec<u8> {
                    b"{}"[..].into()
                }
                """
            )
        }
    }

    override fun operationSerializer(operationShape: OperationShape): RuntimeType? {
        // Don't generate an operation JSON serializer if there is no JSON body
        val httpDocumentMembers = httpBindingResolver.requestMembers(operationShape, HttpLocation.DOCUMENT)
        if (httpDocumentMembers.isEmpty()) {
            return null
        }

        val inputShape = operationShape.inputShape(model)
        val fnName = symbolProvider.serializeFunctionName(operationShape)
        return RuntimeType.forInlineFun(fnName, operationSerModule) {
            it.rustBlockTemplate(
                "pub fn $fnName(input: &#{target}) -> Result<#{SdkBody}, #{Error}>",
                *codegenScope, "target" to symbolProvider.toSymbol(inputShape)
            ) {
                rust("let mut out = String::new();")
                rustTemplate("let mut object = #{JsonObjectWriter}::new(&mut out);", *codegenScope)
                serializeStructure(StructContext("object", "input", inputShape), httpDocumentMembers)
                rust("object.finish();")
                rustTemplate("Ok(#{SdkBody}::from(out))", *codegenScope)
            }
        }
    }

    override fun documentSerializer(): RuntimeType {
        val fnName = "serialize_document"
        return RuntimeType.forInlineFun(fnName, operationSerModule) {
            it.rustTemplate(
                """
                pub fn $fnName(input: &#{Document}) -> Result<#{SdkBody}, #{Error}> {
                    let mut out = String::new();
                    #{JsonValueWriter}::new(&mut out).document(input);
                    Ok(#{SdkBody}::from(out))
                }
                """,
                "Document" to RuntimeType.Document(runtimeConfig), *codegenScope
            )
        }
    }

    override fun serverOutputSerializer(operationShape: OperationShape): RuntimeType {
        val outputShape = operationShape.outputShape(model)
        val includedMembers = httpBindingResolver.responseMembers(operationShape, HttpLocation.DOCUMENT)
        val fnName = symbolProvider.serializeFunctionName(outputShape)
        return structureSerializer(fnName, outputShape, includedMembers)
    }

    override fun serverErrorSerializer(shape: ShapeId): RuntimeType {
        val errorShape = model.expectShape(shape, StructureShape::class.java)
        val includedMembers =
            httpBindingResolver.errorResponseBindings(shape).filter { it.location == HttpLocation.DOCUMENT }
                .map { it.member }
        val fnName = symbolProvider.serializeFunctionName(errorShape)
        return structureSerializer(fnName, errorShape, includedMembers)
    }

    private fun RustWriter.serializeStructure(
        context: StructContext,
        includedMembers: List<MemberShape>? = null,
    ) {
        val fnName = symbolProvider.serializeFunctionName(context.shape)
        val structureSymbol = symbolProvider.toSymbol(context.shape)
        val structureSerializer = RuntimeType.forInlineFun(fnName, jsonSerModule) { writer ->
            writer.rustBlockTemplate(
                "pub fn $fnName(object: &mut #{JsonObjectWriter}, input: &#{Input}) -> Result<(), #{Error}>",
                "Input" to structureSymbol,
                *codegenScope,
            ) {
                context.copy(objectName = "object", localName = "input").also { inner ->
                    val members = includedMembers ?: inner.shape.members()
                    if (members.isEmpty()) {
                        rust("let (_, _) = (object, input);") // Suppress unused argument warnings
                    }
                    for (member in members) {
                        serializeMember(MemberContext.structMember(inner, member, symbolProvider, jsonName))
                    }
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
                    val innerContext = context.copy(valueExpression = ValueExpression.Reference(local))
                    serializeMemberValue(innerContext, targetShape)
                }
                if (context.writeNulls) {
                    rustBlock("else") {
                        rust("${context.writerExpression}.null();")
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
        val writer = context.writerExpression
        val value = context.valueExpression
        when (target) {
            is StringShape -> when (target.hasTrait<EnumTrait>()) {
                true -> rust("$writer.string(${value.name}.as_str());")
                false -> rust("$writer.string(${value.name});")
            }
            is BooleanShape -> rust("$writer.boolean(${value.asValue()});")
            is NumberShape -> {
                val numberType = when (symbolProvider.toSymbol(target).rustType()) {
                    is RustType.Float -> "Float"
                    // NegInt takes an i64 while PosInt takes u64. We need this to be signed here
                    is RustType.Integer -> "NegInt"
                    else -> throw IllegalStateException("unreachable")
                }
                rust(
                    "$writer.number(##[allow(clippy::useless_conversion)]#T::$numberType((${value.asValue()}).into()));",
                    smithyTypes.member("Number")
                )
            }
            is BlobShape -> rust(
                "$writer.string_unchecked(&#T(${value.name}));",
                RuntimeType.Base64Encode(runtimeConfig)
            )
            is TimestampShape -> {
                val timestampFormat =
                    httpBindingResolver.timestampFormat(context.shape, HttpLocation.DOCUMENT, EPOCH_SECONDS)
                val timestampFormatType = RuntimeType.TimestampFormat(runtimeConfig, timestampFormat)
                rust("$writer.date_time(${value.name}, #T)?;", timestampFormatType)
            }
            is CollectionShape -> jsonArrayWriter(context) { arrayName ->
                serializeCollection(Context(arrayName, context.valueExpression, target))
            }
            is MapShape -> jsonObjectWriter(context) { objectName ->
                serializeMap(Context(objectName, context.valueExpression, target))
            }
            is StructureShape -> jsonObjectWriter(context) { objectName ->
                serializeStructure(StructContext(objectName, context.valueExpression.name, target))
            }
            is UnionShape -> jsonObjectWriter(context) { objectName ->
                serializeUnion(Context(objectName, context.valueExpression, target))
            }
            is DocumentShape -> rust("$writer.document(${value.asRef()});")
            else -> TODO(target.toString())
        }
    }

    private fun RustWriter.jsonArrayWriter(context: MemberContext, inner: RustWriter.(String) -> Unit) {
        safeName("array").also { arrayName ->
            rust("let mut $arrayName = ${context.writerExpression}.start_array();")
            inner(arrayName)
            rust("$arrayName.finish();")
        }
    }

    private fun RustWriter.jsonObjectWriter(context: MemberContext, inner: RustWriter.(String) -> Unit) {
        safeName("object").also { objectName ->
            rust("let mut $objectName = ${context.writerExpression}.start_object();")
            inner(objectName)
            rust("$objectName.finish();")
        }
    }

    private fun RustWriter.serializeCollection(context: Context<CollectionShape>) {
        val itemName = safeName("item")
        rustBlock("for $itemName in ${context.valueExpression.asRef()}") {
            serializeMember(MemberContext.collectionMember(context, itemName))
        }
    }

    private fun RustWriter.serializeMap(context: Context<MapShape>) {
        val keyName = safeName("key")
        val valueName = safeName("value")
        rustBlock("for ($keyName, $valueName) in ${context.valueExpression.asRef()}") {
            val keyTarget = model.expectShape(context.shape.key.target)
            val keyExpression = when (keyTarget.hasTrait<EnumTrait>()) {
                true -> "$keyName.as_str()"
                else -> keyName
            }
            serializeMember(MemberContext.mapMember(context, keyExpression, valueName))
        }
    }

    private fun RustWriter.serializeUnion(context: Context<UnionShape>) {
        val fnName = symbolProvider.serializeFunctionName(context.shape)
        val unionSymbol = symbolProvider.toSymbol(context.shape)
        val unionSerializer = RuntimeType.forInlineFun(fnName, jsonSerModule) { writer ->
            writer.rustBlockTemplate(
                "pub fn $fnName(${context.writerExpression}: &mut #{JsonObjectWriter}, input: &#{Input}) -> Result<(), #{Error}>",
                "Input" to unionSymbol,
                *codegenScope,
            ) {
                rustBlock("match input") {
                    for (member in context.shape.members()) {
                        val variantName = symbolProvider.toMemberName(member)
                        withBlock("#T::$variantName(inner) => {", "},", unionSymbol) {
                            serializeMember(MemberContext.unionMember(context, "inner", member, jsonName))
                        }
                    }
                    if (mode.renderUnknownVariant()) {
                        rustTemplate(
                            "#{Union}::${UnionGenerator.UnknownVariantName} => return Err(#{Error}::unknown_variant(${unionSymbol.name.dq()}))",
                            "Union" to unionSymbol,
                            *codegenScope
                        )
                    }
                }
                rust("Ok(())")
            }
        }
        rust("#T(&mut ${context.writerExpression}, ${context.valueExpression.asRef()})?;", unionSerializer)
    }
}
