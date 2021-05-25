/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.protocols.parsers

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.knowledge.HttpBinding
import software.amazon.smithy.model.knowledge.HttpBindingIndex
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
import software.amazon.smithy.model.traits.JsonNameTrait
import software.amazon.smithy.model.traits.TimestampFormatTrait.Format.EPOCH_SECONDS
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustInline
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.withBlock
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.isOptional
import software.amazon.smithy.rust.codegen.smithy.rustType
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.expectTrait
import software.amazon.smithy.rust.codegen.util.getTrait
import software.amazon.smithy.rust.codegen.util.hasTrait
import software.amazon.smithy.rust.codegen.util.inputShape
import software.amazon.smithy.rust.codegen.util.toPascalCase
import software.amazon.smithy.rust.codegen.util.toSnakeCase

private data class SimpleContext<T : Shape>(
    /** Name of the JsonObjectWriter or JsonArrayWriter */
    val writerName: String,
    val localName: String,
    val shape: T,
)

private data class StructContext(
    /** Name of the JsonObjectWriter */
    val objectName: String,
    val localName: String,
    val shape: StructureShape,
    val symbolProvider: RustSymbolProvider,
) {
    fun member(member: MemberShape): MemberContext =
        MemberContext(objectName, "$localName.${symbolProvider.toMemberName(member)}", member, structMember = true)
}

private data class MemberContext(
    /** Name of the JsonObjectWriter or JsonArrayWriter */
    val writerName: String,
    val valueExpression: String,
    val shape: MemberShape,
    /** Whether we're working with a JsonObjectWriter (true) or JsonArrayWriter (false) */
    val structMember: Boolean,
    private val givenKeyExpression: String? = null,
) {
    val wireName: String = shape.getTrait<JsonNameTrait>()?.value ?: shape.memberName
    val keyExpression: String = when (givenKeyExpression) {
        null -> wireName.dq()
        else -> givenKeyExpression
    }

    /** Generates an expression that serializes the given [value] expression to the object/array */
    fun writeValue(w: RustWriter, jsonType: String, key: String, value: String) = when (structMember) {
        true -> w.rust("$writerName.$jsonType($key, $value);")
        else -> w.rust("$writerName.$jsonType($value);")
    }

    /** Generates an expression that serializes the given [inner] expression to the object/array */
    fun writeInner(w: RustWriter, jsonType: String, key: String, inner: RustWriter.() -> Unit) {
        w.rustInline("$writerName.$jsonType(")
        if (structMember) {
            w.writeInline("$key, ")
        }
        inner(w)
        w.write(");")
    }

    /** Generates a mutable declaration for serializing a new object */
    fun writeStartObject(w: RustWriter, decl: String, key: String) = when (structMember) {
        true -> w.rust("let mut $decl = $writerName.start_object($key);")
        else -> w.rust("let mut $decl = $writerName.start_object();")
    }

    /** Generates a mutable declaration for serializing a new array */
    fun writeStartArray(w: RustWriter, decl: String, key: String) = when (structMember) {
        true -> w.rust("let mut $decl = $writerName.start_array($key);")
        else -> w.rust("let mut $decl = $writerName.start_array();")
    }
}

class JsonSerializerGenerator(protocolConfig: ProtocolConfig) : StructuredDataSerializerGenerator {
    private val model = protocolConfig.model
    private val symbolProvider = protocolConfig.symbolProvider
    private val runtimeConfig = protocolConfig.runtimeConfig
    private val serializerError = RuntimeType.SerdeJson("error::Error")
    private val smithyTypes = CargoDependency.SmithyTypes(runtimeConfig).asType()
    private val smithyJson = CargoDependency.smithyJson(runtimeConfig).asType()
    private val codegenScope = arrayOf(
        "String" to RuntimeType.String,
        "Error" to serializerError,
        "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
        "JsonObjectWriter" to smithyJson.member("serialize::JsonObjectWriter"),
    )
    private val httpIndex = HttpBindingIndex.of(model)

    override fun payloadSerializer(member: MemberShape): RuntimeType {
        val target = model.expectShape(member.target, StructureShape::class.java)
        val fnName = "serialize_payload_${target.id.name.toSnakeCase()}_${member.container.name.toSnakeCase()}"
        return RuntimeType.forInlineFun(fnName, "operation_ser") { writer ->
            writer.rustBlockTemplate(
                "pub fn $fnName(input: &#{target}) -> Result<#{SdkBody}, #{Error}>",
                *codegenScope,
                "target" to symbolProvider.toSymbol(target)
            ) {
                rust("let mut out = String::new();")
                rustTemplate("let mut object = #{JsonObjectWriter}::new(&mut out);", *codegenScope)
                serializeStructure(StructContext("object", "input", target, symbolProvider))
                rust("object.finish();")
                rustTemplate("Ok(#{SdkBody}::from(out))", *codegenScope)
            }
        }
    }

    override fun operationSerializer(operationShape: OperationShape): RuntimeType? {
        val inputShape = operationShape.inputShape(model)
        val inputShapeName = inputShape.expectTrait<SyntheticInputTrait>().originalId?.name
            ?: throw CodegenException("operation must have a name if it has members")
        val fnName = "serialize_operation_${inputShapeName.toSnakeCase()}"
        return RuntimeType.forInlineFun(fnName, "operation_ser") {
            it.rustBlockTemplate(
                "pub fn $fnName(input: &#{target}) -> Result<#{SdkBody}, #{Error}>",
                *codegenScope, "target" to symbolProvider.toSymbol(inputShape)
            ) {
                // TODO: Implement operation serialization
                rust("unimplemented!()")
            }
        }
    }

    override fun documentSerializer(): RuntimeType {
        val fnName = "serialize_document"
        return RuntimeType.forInlineFun(fnName, "operation_ser") {
            it.rustTemplate(
                // TODO: Implement document parsing
                """
                pub fn $fnName(input: &#{Document}) -> Result<#{SdkBody}, #{Error}> {
                    unimplemented!();
                }
                """,
                "Document" to RuntimeType.Document(runtimeConfig), *codegenScope
            )
        }
    }

    private fun RustWriter.serializeStructure(context: StructContext) {
        val fnName = "serialize_structure_${context.shape.id.name.toSnakeCase()}"
        val structureSymbol = symbolProvider.toSymbol(context.shape)
        val structureSerializer = RuntimeType.forInlineFun(fnName, "json_ser") { writer ->
            writer.rustBlockTemplate(
                "pub fn $fnName(${context.objectName}: &mut #{JsonObjectWriter}, input: &#{Shape})",
                "Shape" to structureSymbol,
                *codegenScope,
            ) {
                if (context.shape.members().isEmpty()) {
                    rust("let _ = input;") // Suppress an unused argument warning
                }
                for (member in context.shape.members()) {
                    serializeMember(context.member(member))
                }
            }
        }
        rust("#T(${context.objectName.borrowMut()}, ${context.localName});", structureSerializer)
    }

    private fun RustWriter.serializeMember(context: MemberContext) {
        val target = model.expectShape(context.shape.target)
        handleOptional(context) { inner ->
            val key = inner.keyExpression
            val value = inner.valueExpression.borrow()
            when (target) {
                is StringShape -> when (target.hasTrait<EnumTrait>()) {
                    true -> context.writeValue(this, "string", key, "$value.as_str()")
                    false -> context.writeValue(this, "string", key, value)
                }
                is BooleanShape -> context.writeValue(this, "boolean", key, value)
                is NumberShape -> {
                    val numberType = when (symbolProvider.toSymbol(target).rustType()) {
                        is RustType.Float -> "Float"
                        is RustType.Integer -> "NegInt"
                        else -> throw IllegalStateException("unreachable")
                    }
                    context.writeInner(this, "number", key) {
                        rustInline("#T::$numberType(*${inner.valueExpression})", smithyTypes.member("Number"))
                    }
                }
                is BlobShape -> context.writeInner(this, "string_unchecked", key) {
                    rustInline("&#T($value)", RuntimeType.Base64Encode(runtimeConfig))
                }
                is TimestampShape -> {
                    val timestampFormat =
                        httpIndex.determineTimestampFormat(context.shape, HttpBinding.Location.DOCUMENT, EPOCH_SECONDS)
                    val timestampFormatType = RuntimeType.TimestampFormat(runtimeConfig, timestampFormat)
                    context.writeInner(this, "instant", key) {
                        rustInline("$value, #T", timestampFormatType)
                    }
                }
                is CollectionShape -> jsonArrayWriter(inner) { arrayName ->
                    serializeCollection(SimpleContext(arrayName, inner.valueExpression, target))
                }
                is MapShape -> jsonObjectWriter(inner) { objectName ->
                    serializeMap(SimpleContext(objectName, inner.valueExpression, target))
                }
                is StructureShape -> jsonObjectWriter(inner) { objectName ->
                    serializeStructure(StructContext(objectName, inner.valueExpression, target, symbolProvider))
                }
                is UnionShape -> jsonObjectWriter(inner) { objectName ->
                    serializeUnion(SimpleContext(objectName, inner.valueExpression, target))
                }
                is DocumentShape -> {
                    // TODO: Implement document shapes
                }
                else -> TODO(target.toString())
            }
        }
    }

    private fun RustWriter.jsonArrayWriter(context: MemberContext, inner: RustWriter.(String) -> Unit) {
        safeName("array").also { arrayName ->
            context.writeStartArray(this, arrayName, context.keyExpression)
            inner(arrayName)
            rust("$arrayName.finish();")
        }
    }

    private fun RustWriter.jsonObjectWriter(context: MemberContext, inner: RustWriter.(String) -> Unit) {
        safeName("object").also { objectName ->
            context.writeStartObject(this, objectName, context.keyExpression)
            inner(objectName)
            rust("$objectName.finish();")
        }
    }

    private fun RustWriter.serializeCollection(context: SimpleContext<CollectionShape>) {
        val itemName = safeName("item")
        rustBlock("for $itemName in ${context.localName}") {
            serializeMember(MemberContext(context.writerName, itemName, context.shape.member, structMember = false))
        }
    }

    private fun RustWriter.serializeMap(context: SimpleContext<MapShape>) {
        val keyName = safeName("key")
        val valueName = safeName("value")
        val valueShape = context.shape.value
        rustBlock("for ($keyName, $valueName) in ${context.localName}") {
            serializeMember(
                MemberContext(
                    context.writerName,
                    valueName,
                    valueShape,
                    structMember = true,
                    givenKeyExpression = keyName
                )
            )
        }
    }

    private fun RustWriter.serializeUnion(context: SimpleContext<UnionShape>) {
        val fnName = "serialize_union_${context.shape.id.name.toSnakeCase()}"
        val unionSymbol = symbolProvider.toSymbol(context.shape)
        val unionSerializer = RuntimeType.forInlineFun(fnName, "json_ser") { writer ->
            writer.rustBlockTemplate(
                "pub fn $fnName(${context.writerName}: &mut #{JsonObjectWriter}, input: &#{Shape})",
                "Shape" to unionSymbol,
                *codegenScope,
            ) {
                rustBlock("match input") {
                    for (member in context.shape.members()) {
                        val variantName = member.memberName.toPascalCase()
                        withBlock("#T::$variantName(inner) => {", "},", unionSymbol) {
                            serializeMember(MemberContext(context.writerName, "inner", member, true))
                        }
                    }
                }
            }
        }
        rust("#T(${context.writerName.borrowMut()}, ${context.localName});", unionSerializer)
    }

    private fun RustWriter.handleOptional(context: MemberContext, inner: RustWriter.(MemberContext) -> Unit) {
        if (symbolProvider.toSymbol(context.shape).isOptional()) {
            safeName().also { localDecl ->
                rustBlock("if let Some($localDecl) = ${context.valueExpression.borrow()}") {
                    inner(context.copy(valueExpression = localDecl))
                }
            }
        } else {
            inner(context)
        }
    }
}

private fun String.borrow(): String = "&$this"
private fun String.borrowMut(): String = "&mut $this"
