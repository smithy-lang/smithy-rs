/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.protocols.parsers

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
import software.amazon.smithy.model.traits.SparseTrait
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
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.expectMember
import software.amazon.smithy.rust.codegen.util.getTrait
import software.amazon.smithy.rust.codegen.util.hasTrait
import software.amazon.smithy.rust.codegen.util.inputShape
import software.amazon.smithy.rust.codegen.util.toPascalCase

private data class SimpleContext<T : Shape>(
    /** Name of the JsonObjectWriter or JsonArrayWriter */
    val writerName: String,
    val valueExpression: ValueExpression,
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
        MemberContext(
            objectName,
            MemberDestination.Object(),
            ValueExpression.Value("$localName.${symbolProvider.toMemberName(member)}"),
            member
        )
}

private sealed class MemberDestination {
    // Add unused parameter so that Kotlin generates equals/hashCode for us
    data class Array(private val unused: Int = 0) : MemberDestination()
    data class Object(val keyNameOverride: String? = null) : MemberDestination()
}

private sealed class ValueExpression {
    abstract val name: String

    data class Reference(override val name: String) : ValueExpression()
    data class Value(override val name: String) : ValueExpression()

    fun asValue(): String = when (this) {
        is Reference -> "*$name"
        is Value -> name
    }

    fun asRef(): String = when (this) {
        is Reference -> name
        is Value -> "&$name"
    }
}

private data class MemberContext(
    /** Name of the JsonObjectWriter or JsonArrayWriter */
    val writerName: String,
    val destination: MemberDestination,
    val valueExpression: ValueExpression,
    val shape: MemberShape,
) {
    val keyExpression: String = when (destination) {
        is MemberDestination.Object ->
            destination.keyNameOverride ?: (shape.getTrait<JsonNameTrait>()?.value ?: shape.memberName).dq()
        is MemberDestination.Array -> ""
    }

    /** Generates an expression that serializes a null value */
    fun writeNull(w: RustWriter, key: String) = when (destination) {
        is MemberDestination.Object -> w.rust("$writerName.key($key).null();")
        is MemberDestination.Array -> w.rust("$writerName.null();")
    }

    /** Generates an expression that serializes the given [value] expression to the object/array */
    fun writeValue(w: RustWriter, writerFn: JsonWriterFn, key: String, value: String) = when (destination) {
        is MemberDestination.Object -> w.rust("$writerName.key($key).$writerFn($value);")
        is MemberDestination.Array -> w.rust("$writerName.$writerFn($value);")
    }

    /** Generates an expression that serializes the given [inner] expression to the object/array */
    fun writeInner(w: RustWriter, writerFn: JsonWriterFn, key: String, inner: RustWriter.() -> Unit) {
        val keyExpression = when (destination) {
            is MemberDestination.Object -> ".key($key)"
            is MemberDestination.Array -> ""
        }
        w.withBlock("$writerName$keyExpression.$writerFn(", ");") {
            inner(w)
        }
    }

    /** Generates a mutable declaration for serializing a new object */
    fun writeStartObject(w: RustWriter, decl: String, key: String) = when (destination) {
        is MemberDestination.Object -> w.rust("let mut $decl = $writerName.key($key).start_object();")
        is MemberDestination.Array -> w.rust("let mut $decl = $writerName.start_object();")
    }

    /** Generates a mutable declaration for serializing a new array */
    fun writeStartArray(w: RustWriter, decl: String, key: String) = when (destination) {
        is MemberDestination.Object -> w.rust("let mut $decl = $writerName.key($key).start_array();")
        is MemberDestination.Array -> w.rust("let mut $decl = $writerName.start_array();")
    }
}

private enum class JsonWriterFn {
    BOOLEAN,
    DOCUMENT,
    INSTANT,
    NUMBER,
    STRING,
    STRING_UNCHECKED;

    override fun toString(): String = name.toLowerCase()
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
        "JsonValueWriter" to smithyJson.member("serialize::JsonValueWriter"),
    )
    private val httpIndex = HttpBindingIndex.of(model)

    override fun payloadSerializer(member: MemberShape): RuntimeType {
        val fnName = SerializeFunctionNamer.name(model, symbolProvider, member)
        val target = model.expectShape(member.target, StructureShape::class.java)
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

        // Don't generate an operation JSON serializer if there is no JSON body
        val httpBindings = httpIndex.getRequestBindings(operationShape)
        val hasDocumentHttpBindings = httpBindings
            .filter { it.value.location == HttpBinding.Location.DOCUMENT }
            .keys.map { inputShape.expectMember(it) }
            .isNotEmpty()
        if (inputShape.members().isEmpty() || httpBindings.isNotEmpty() && !hasDocumentHttpBindings) {
            return null
        }

        val fnName = SerializeFunctionNamer.name(model, symbolProvider, operationShape)
        return RuntimeType.forInlineFun(fnName, "operation_ser") {
            it.rustBlockTemplate(
                "pub fn $fnName(input: &#{target}) -> Result<#{SdkBody}, #{Error}>",
                *codegenScope, "target" to symbolProvider.toSymbol(inputShape)
            ) {
                rust("let mut out = String::new();")
                rustTemplate("let mut object = #{JsonObjectWriter}::new(&mut out);", *codegenScope)
                serializeStructure(StructContext("object", "input", inputShape, symbolProvider))
                rust("object.finish();")
                rustTemplate("Ok(#{SdkBody}::from(out))", *codegenScope)
            }
        }
    }

    override fun documentSerializer(): RuntimeType {
        val fnName = "serialize_document"
        return RuntimeType.forInlineFun(fnName, "operation_ser") {
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

    private fun RustWriter.serializeStructure(context: StructContext) {
        val fnName = SerializeFunctionNamer.name(model, symbolProvider, context.shape)
        val structureSymbol = symbolProvider.toSymbol(context.shape)
        val structureSerializer = RuntimeType.forInlineFun(fnName, "json_ser") { writer ->
            writer.rustBlockTemplate(
                "pub fn $fnName(object: &mut #{JsonObjectWriter}, input: &#{Input})",
                "Input" to structureSymbol,
                *codegenScope,
            ) {
                context.copy(objectName = "object", localName = "input").also { inner ->
                    if (inner.shape.members().isEmpty()) {
                        rust("let (_, _) = (object, input);") // Suppress unused argument warnings
                    }
                    for (member in inner.shape.members()) {
                        serializeMember(inner.member(member))
                    }
                }
            }
        }
        rust("#T(&mut ${context.objectName}, ${context.localName});", structureSerializer)
    }

    private fun RustWriter.serializeMember(context: MemberContext) {
        handleOptional(context) { inner ->
            serializeMemberValue(inner, model.expectShape(context.shape.target))
        }
    }

    private fun RustWriter.serializeSparseMember(context: MemberContext) {
        handleOptional(context, elseBlock = { context.writeNull(this, context.keyExpression) }) { inner ->
            serializeMemberValue(inner, model.expectShape(context.shape.target))
        }
    }

    private fun RustWriter.serializeMemberValue(context: MemberContext, target: Shape) {
        val key = context.keyExpression
        val value = context.valueExpression
        when (target) {
            is StringShape -> when (target.hasTrait<EnumTrait>()) {
                true -> context.writeValue(this, JsonWriterFn.STRING, key, "${value.name}.as_str()")
                false -> context.writeValue(this, JsonWriterFn.STRING, key, value.name)
            }
            is BooleanShape -> context.writeValue(this, JsonWriterFn.BOOLEAN, key, value.asValue())
            is NumberShape -> {
                val numberType = when (symbolProvider.toSymbol(target).rustType()) {
                    is RustType.Float -> "Float"
                    is RustType.Integer -> "NegInt"
                    else -> throw IllegalStateException("unreachable")
                }
                context.writeInner(this, JsonWriterFn.NUMBER, key) {
                    rustInline("#T::$numberType((${value.asValue()}).into())", smithyTypes.member("Number"))
                }
            }
            is BlobShape -> context.writeInner(this, JsonWriterFn.STRING_UNCHECKED, key) {
                rustInline("&#T(${value.name})", RuntimeType.Base64Encode(runtimeConfig))
            }
            is TimestampShape -> {
                val timestampFormat =
                    httpIndex.determineTimestampFormat(context.shape, HttpBinding.Location.DOCUMENT, EPOCH_SECONDS)
                val timestampFormatType = RuntimeType.TimestampFormat(runtimeConfig, timestampFormat)
                context.writeInner(this, JsonWriterFn.INSTANT, key) {
                    rustInline("${value.name}, #T", timestampFormatType)
                }
            }
            is CollectionShape -> jsonArrayWriter(context) { arrayName ->
                serializeCollection(SimpleContext(arrayName, context.valueExpression, target))
            }
            is MapShape -> jsonObjectWriter(context) { objectName ->
                serializeMap(SimpleContext(objectName, context.valueExpression, target))
            }
            is StructureShape -> jsonObjectWriter(context) { objectName ->
                serializeStructure(StructContext(objectName, context.valueExpression.name, target, symbolProvider))
            }
            is UnionShape -> jsonObjectWriter(context) { objectName ->
                serializeUnion(SimpleContext(objectName, context.valueExpression, target))
            }
            is DocumentShape -> context.writeValue(this, JsonWriterFn.DOCUMENT, key, value.asRef())
            else -> TODO(target.toString())
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
        rustBlock("for $itemName in ${context.valueExpression.asRef()}") {
            val memberContext = MemberContext(
                context.writerName,
                MemberDestination.Array(),
                ValueExpression.Reference(itemName),
                context.shape.member
            )
            when (context.shape.hasTrait<SparseTrait>()) {
                true -> serializeSparseMember(memberContext)
                else -> serializeMember(memberContext)
            }
        }
    }

    private fun RustWriter.serializeMap(context: SimpleContext<MapShape>) {
        val keyName = safeName("key")
        val valueName = safeName("value")
        val valueShape = context.shape.value
        rustBlock("for ($keyName, $valueName) in ${context.valueExpression.asRef()}") {
            val memberContext = MemberContext(
                context.writerName,
                MemberDestination.Object(keyNameOverride = keyName),
                ValueExpression.Reference(valueName),
                valueShape
            )
            when (context.shape.hasTrait<SparseTrait>()) {
                true -> serializeSparseMember(memberContext)
                else -> serializeMember(memberContext)
            }
        }
    }

    private fun RustWriter.serializeUnion(context: SimpleContext<UnionShape>) {
        val fnName = SerializeFunctionNamer.name(model, symbolProvider, context.shape)
        val unionSymbol = symbolProvider.toSymbol(context.shape)
        val unionSerializer = RuntimeType.forInlineFun(fnName, "json_ser") { writer ->
            writer.rustBlockTemplate(
                "pub fn $fnName(${context.writerName}: &mut #{JsonObjectWriter}, input: &#{Input})",
                "Input" to unionSymbol,
                *codegenScope,
            ) {
                rustBlock("match input") {
                    for (member in context.shape.members()) {
                        val variantName = member.memberName.toPascalCase()
                        withBlock("#T::$variantName(inner) => {", "},", unionSymbol) {
                            serializeMember(
                                MemberContext(
                                    context.writerName,
                                    MemberDestination.Object(),
                                    ValueExpression.Reference("inner"),
                                    member
                                )
                            )
                        }
                    }
                }
            }
        }
        rust("#T(&mut ${context.writerName}, ${context.valueExpression.asRef()});", unionSerializer)
    }

    private fun RustWriter.handleOptional(
        context: MemberContext,
        elseBlock: (RustWriter.(MemberContext) -> Unit)? = null,
        inner: RustWriter.(MemberContext) -> Unit,
    ) {
        if (symbolProvider.toSymbol(context.shape).isOptional()) {
            safeName().also { localDecl ->
                val innerContext = context.copy(valueExpression = ValueExpression.Reference(localDecl))
                rustBlock("if let Some($localDecl) = ${context.valueExpression.asRef()}") {
                    inner(innerContext)
                }
                if (elseBlock != null) {
                    rustBlock("else") {
                        elseBlock(innerContext)
                    }
                }
            }
        } else {
            inner(context)
        }
    }
}
