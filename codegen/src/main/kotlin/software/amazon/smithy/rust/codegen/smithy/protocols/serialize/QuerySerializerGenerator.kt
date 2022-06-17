/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.protocols.serialize

import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.BooleanShape
import software.amazon.smithy.model.shapes.CollectionShape
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
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.model.traits.XmlNameTrait
import software.amazon.smithy.rust.codegen.rustlang.Attribute
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
import software.amazon.smithy.rust.codegen.smithy.CoreCodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.renderUnknownVariant
import software.amazon.smithy.rust.codegen.smithy.generators.serializationError
import software.amazon.smithy.rust.codegen.smithy.isOptional
import software.amazon.smithy.rust.codegen.smithy.protocols.serializeFunctionName
import software.amazon.smithy.rust.codegen.smithy.rustType
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.getTrait
import software.amazon.smithy.rust.codegen.util.hasTrait
import software.amazon.smithy.rust.codegen.util.inputShape
import software.amazon.smithy.rust.codegen.util.orNull

abstract class QuerySerializerGenerator(coreCodegenContext: CoreCodegenContext) : StructuredDataSerializerGenerator {
    protected data class Context<T : Shape>(
        /** Expression that yields a QueryValueWriter */
        val writerExpression: String,
        /** Expression representing the value to write to the QueryValueWriter */
        val valueExpression: ValueExpression,
        val shape: T,
    )

    protected data class MemberContext(
        /** Expression that yields a QueryValueWriter */
        val writerExpression: String,
        /** Expression representing the value to write to the QueryValueWriter */
        val valueExpression: ValueExpression,
        val shape: MemberShape,
    ) {
        companion object {
            fun structMember(
                context: Context<StructureShape>,
                member: MemberShape,
                symProvider: RustSymbolProvider
            ): MemberContext =
                MemberContext(
                    context.writerExpression,
                    ValueExpression.Value("${context.valueExpression.name}.${symProvider.toMemberName(member)}"),
                    member
                )

            fun unionMember(
                context: Context<UnionShape>,
                variantReference: String,
                member: MemberShape
            ): MemberContext =
                MemberContext(
                    context.writerExpression,
                    ValueExpression.Reference(variantReference),
                    member
                )
        }
    }

    protected val model = coreCodegenContext.model
    protected val symbolProvider = coreCodegenContext.symbolProvider
    protected val runtimeConfig = coreCodegenContext.runtimeConfig
    private val target = coreCodegenContext.target
    private val serviceShape = coreCodegenContext.serviceShape
    private val serializerError = runtimeConfig.serializationError()
    private val smithyTypes = CargoDependency.SmithyTypes(runtimeConfig).asType()
    private val smithyQuery = CargoDependency.smithyQuery(runtimeConfig).asType()
    private val serdeUtil = SerializerUtil(model)
    private val codegenScope = arrayOf(
        "String" to RuntimeType.String,
        "Error" to serializerError,
        "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
        "QueryWriter" to smithyQuery.member("QueryWriter"),
        "QueryValueWriter" to smithyQuery.member("QueryValueWriter"),
    )
    private val operationSerModule = RustModule.private("operation_ser")
    private val querySerModule = RustModule.private("query_ser")

    abstract val protocolName: String
    abstract fun MemberShape.queryKeyName(prioritizedFallback: String? = null): String
    abstract fun MemberShape.isFlattened(): Boolean

    override fun documentSerializer(): RuntimeType {
        TODO("$protocolName doesn't support document types")
    }

    override fun payloadSerializer(member: MemberShape): RuntimeType {
        // TODO(EventStream): Query payload serialization is required for RPC initial message as well as for message
        // frames that have a struct/union type.
        TODO("$protocolName doesn't support payload serialization yet")
    }

    override fun unsetStructure(structure: StructureShape): RuntimeType {
        TODO("AwsQuery doesn't support payload serialization")
    }

    override fun operationInputSerializer(operationShape: OperationShape): RuntimeType? {
        val fnName = symbolProvider.serializeFunctionName(operationShape)
        val inputShape = operationShape.inputShape(model)
        return RuntimeType.forInlineFun(fnName, operationSerModule) { writer ->
            writer.rustBlockTemplate(
                "pub fn $fnName(input: &#{target}) -> Result<#{SdkBody}, #{Error}>",
                *codegenScope, "target" to symbolProvider.toSymbol(inputShape)
            ) {
                val action = operationShape.id.name
                val version = serviceShape.version

                if (inputShape.members().isEmpty()) {
                    rust("let _ = input;")
                }
                rust("let mut out = String::new();")
                Attribute.AllowUnusedMut.render(writer)
                rustTemplate(
                    "let mut writer = #{QueryWriter}::new(&mut out, ${action.dq()}, ${version.dq()});",
                    *codegenScope
                )
                serializeStructureInner(Context("writer", ValueExpression.Reference("input"), inputShape))
                rust("writer.finish();")
                rustTemplate("Ok(#{SdkBody}::from(out))", *codegenScope)
            }
        }
    }

    private fun RustWriter.serializeStructure(context: Context<StructureShape>) {
        val fnName = symbolProvider.serializeFunctionName(context.shape)
        val structureSymbol = symbolProvider.toSymbol(context.shape)
        val structureSerializer = RuntimeType.forInlineFun(fnName, querySerModule) { writer ->
            Attribute.AllowUnusedMut.render(writer)
            writer.rustBlockTemplate(
                "pub fn $fnName(mut writer: #{QueryValueWriter}, input: &#{Input}) -> Result<(), #{Error}>",
                "Input" to structureSymbol,
                *codegenScope
            ) {
                if (context.shape.members().isEmpty()) {
                    rust("let (_, _) = (writer, input);") // Suppress unused argument warnings
                }
                serializeStructureInner(context)
                rust("Ok(())")
            }
        }
        rust("#T(${context.writerExpression}, ${context.valueExpression.name})?;", structureSerializer)
    }

    private fun RustWriter.serializeStructureInner(context: Context<StructureShape>) {
        context.copy(writerExpression = "writer", valueExpression = ValueExpression.Reference("input"))
            .also { inner ->
                for (member in inner.shape.members()) {
                    val memberContext = MemberContext.structMember(inner, member, symbolProvider)
                    structWriter(memberContext) { writerExpression ->
                        serializeMember(memberContext.copy(writerExpression = writerExpression))
                    }
                }
            }
    }

    private fun RustWriter.serializeMember(context: MemberContext) {
        val targetShape = model.expectShape(context.shape.target)
        if (symbolProvider.toSymbol(context.shape).isOptional()) {
            safeName().also { local ->
                rustBlock("if let Some($local) = ${context.valueExpression.asRef()}") {
                    val innerContext = context.copy(valueExpression = ValueExpression.Reference(local))
                    serializeMemberValue(innerContext, targetShape)
                }
            }
        } else {
            with(serdeUtil) {
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
                "$writer.string(&#T(${value.name}));",
                RuntimeType.Base64Encode(runtimeConfig)
            )
            is TimestampShape -> {
                val timestampFormat = determineTimestampFormat(context.shape)
                val timestampFormatType = RuntimeType.TimestampFormat(runtimeConfig, timestampFormat)
                rust("$writer.date_time(${value.name}, #T)?;", timestampFormatType)
            }
            is CollectionShape -> serializeCollection(context, Context(writer, context.valueExpression, target))
            is MapShape -> serializeMap(context, Context(writer, context.valueExpression, target))
            is StructureShape -> serializeStructure(Context(writer, context.valueExpression, target))
            is UnionShape -> structWriter(context) { writerExpression ->
                serializeUnion(Context(writerExpression, context.valueExpression, target))
            }
            else -> TODO(target.toString())
        }
    }

    private fun determineTimestampFormat(shape: MemberShape): TimestampFormatTrait.Format =
        shape.getMemberTrait(model, TimestampFormatTrait::class.java).orNull()?.format
            ?: TimestampFormatTrait.Format.DATE_TIME

    private fun RustWriter.structWriter(context: MemberContext, inner: RustWriter.(String) -> Unit) {
        val prefix = context.shape.queryKeyName()
        safeName("scope").also { scopeName ->
            Attribute.AllowUnusedMut.render(this)
            rust("let mut $scopeName = ${context.writerExpression}.prefix(${prefix.dq()});")
            inner(scopeName)
        }
    }

    private fun RustWriter.serializeCollection(memberContext: MemberContext, context: Context<CollectionShape>) {
        val flat = memberContext.shape.isFlattened()
        val memberOverride = when (val override = context.shape.member.getTrait<XmlNameTrait>()?.value) {
            null -> "None"
            else -> "Some(${override.dq()})"
        }
        val itemName = safeName("item")
        safeName("list").also { listName ->
            rust("let mut $listName = ${context.writerExpression}.start_list($flat, $memberOverride);")
            rustBlock("for $itemName in ${context.valueExpression.asRef()}") {
                val entryName = safeName("entry")
                Attribute.AllowUnusedMut.render(this)
                rust("let mut $entryName = $listName.entry();")
                val targetShape = model.expectShape(context.shape.member.target)
                serializeMemberValue(
                    MemberContext(entryName, ValueExpression.Reference(itemName), context.shape.member),
                    targetShape
                )
            }
            rust("$listName.finish();")
        }
    }

    private fun RustWriter.serializeMap(memberContext: MemberContext, context: Context<MapShape>) {
        val flat = memberContext.shape.isFlattened()
        val entryKeyName = context.shape.key.queryKeyName("key").dq()
        val entryValueName = context.shape.value.queryKeyName("value").dq()
        safeName("map").also { mapName ->
            val keyName = safeName("key")
            val valueName = safeName("value")
            rust("let mut $mapName = ${context.writerExpression}.start_map($flat, $entryKeyName, $entryValueName);")
            rustBlock("for ($keyName, $valueName) in ${context.valueExpression.asRef()}") {
                val keyTarget = model.expectShape(context.shape.key.target)
                val keyExpression = when (keyTarget.hasTrait<EnumTrait>()) {
                    true -> "$keyName.as_str()"
                    else -> keyName
                }
                val entryName = safeName("entry")
                Attribute.AllowUnusedMut.render(this)
                rust("let mut $entryName = $mapName.entry($keyExpression);")
                serializeMember(MemberContext(entryName, ValueExpression.Reference(valueName), context.shape.value))
            }
            rust("$mapName.finish();")
        }
    }

    private fun RustWriter.serializeUnion(context: Context<UnionShape>) {
        val fnName = symbolProvider.serializeFunctionName(context.shape)
        val unionSymbol = symbolProvider.toSymbol(context.shape)
        val unionSerializer = RuntimeType.forInlineFun(fnName, querySerModule) { writer ->
            Attribute.AllowUnusedMut.render(writer)
            writer.rustBlockTemplate(
                "pub fn $fnName(mut writer: #{QueryValueWriter}, input: &#{Input}) -> Result<(), #{Error}>",
                "Input" to unionSymbol,
                *codegenScope,
            ) {
                rustBlock("match input") {
                    for (member in context.shape.members()) {
                        val variantName = symbolProvider.toMemberName(member)
                        withBlock("#T::$variantName(inner) => {", "},", unionSymbol) {
                            serializeMember(
                                MemberContext.unionMember(
                                    context.copy(writerExpression = "writer"),
                                    "inner",
                                    member
                                )
                            )
                        }
                    }
                    if (target.renderUnknownVariant()) {
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
        rust("#T(${context.writerExpression}, ${context.valueExpression.asRef()})?;", unionSerializer)
    }
}
