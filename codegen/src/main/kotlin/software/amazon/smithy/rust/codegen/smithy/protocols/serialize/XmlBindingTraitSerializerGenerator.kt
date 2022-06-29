/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.protocols.serialize

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.BooleanShape
import software.amazon.smithy.model.shapes.CollectionShape
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
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.model.traits.XmlFlattenedTrait
import software.amazon.smithy.model.traits.XmlNamespaceTrait
import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.autoDeref
import software.amazon.smithy.rust.codegen.rustlang.render
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.rustlang.withBlock
import software.amazon.smithy.rust.codegen.smithy.CoreCodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.renderUnknownVariant
import software.amazon.smithy.rust.codegen.smithy.generators.serializationError
import software.amazon.smithy.rust.codegen.smithy.isOptional
import software.amazon.smithy.rust.codegen.smithy.letIf
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpBindingResolver
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpLocation
import software.amazon.smithy.rust.codegen.smithy.protocols.XmlMemberIndex
import software.amazon.smithy.rust.codegen.smithy.protocols.XmlNameIndex
import software.amazon.smithy.rust.codegen.smithy.protocols.serializeFunctionName
import software.amazon.smithy.rust.codegen.smithy.rustType
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.getTrait
import software.amazon.smithy.rust.codegen.util.hasTrait
import software.amazon.smithy.rust.codegen.util.inputShape
import software.amazon.smithy.rust.codegen.util.outputShape

class XmlBindingTraitSerializerGenerator(
    coreCodegenContext: CoreCodegenContext,
    private val httpBindingResolver: HttpBindingResolver
) : StructuredDataSerializerGenerator {
    private val symbolProvider = coreCodegenContext.symbolProvider
    private val runtimeConfig = coreCodegenContext.runtimeConfig
    private val model = coreCodegenContext.model
    private val smithyXml = CargoDependency.smithyXml(runtimeConfig).asType()
    private val target = coreCodegenContext.target
    private val codegenScope =
        arrayOf(
            "XmlWriter" to smithyXml.member("encode::XmlWriter"),
            "ElementWriter" to smithyXml.member("encode::ElWriter"),
            "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
            "Error" to runtimeConfig.serializationError()
        )
    private val operationSerModule = RustModule.private("operation_ser")
    private val xmlSerModule = RustModule.private("xml_ser")

    private val xmlIndex = XmlNameIndex.of(model)
    private val rootNamespace = coreCodegenContext.serviceShape.getTrait<XmlNamespaceTrait>()
    private val util = SerializerUtil(model)

    sealed class Ctx {
        abstract val input: String

        data class Element(val elementWriter: String, override val input: String) : Ctx()

        data class Scope(val scopeWriter: String, override val input: String) : Ctx()

        companion object {
            // Kotlin doesn't have a "This" type
            @Suppress("UNCHECKED_CAST")
            fun <T : Ctx> updateInput(input: T, newInput: String): T = when (input) {
                is Element -> input.copy(input = newInput) as T
                is Scope -> input.copy(input = newInput) as T
                else -> TODO()
            }
        }
    }

    private fun Ctx.Element.scopedTo(member: MemberShape) =
        this.copy(input = "$input.${symbolProvider.toMemberName(member)}")

    private fun Ctx.Scope.scopedTo(member: MemberShape) =
        this.copy(input = "$input.${symbolProvider.toMemberName(member)}")

    override fun operationInputSerializer(operationShape: OperationShape): RuntimeType? {
        val fnName = symbolProvider.serializeFunctionName(operationShape)
        val inputShape = operationShape.inputShape(model)
        val xmlMembers = operationShape.requestBodyMembers()
        if (xmlMembers.isEmpty()) {
            return null
        }
        val operationXmlName = xmlIndex.operationInputShapeName(operationShape)
            ?: throw CodegenException("operation must have a name if it has members")
        return RuntimeType.forInlineFun(fnName, operationSerModule) {
            it.rustBlockTemplate(
                "pub fn $fnName(input: &#{target}) -> Result<#{SdkBody}, #{Error}>",
                *codegenScope, "target" to symbolProvider.toSymbol(inputShape)
            ) {
                rust("let mut out = String::new();")
                // create a scope for writer. This ensure that writer has been dropped before returning the
                // string and ensures that all closing tags get written
                rustBlock("") {
                    rustTemplate(
                        """
                        let mut writer = #{XmlWriter}::new(&mut out);
                        ##[allow(unused_mut)]
                        let mut root = writer.start_el(${operationXmlName.dq()})${inputShape.xmlNamespace(root = true).apply()};
                        """,
                        *codegenScope
                    )
                    serializeStructure(inputShape, xmlMembers, Ctx.Element("root", "input"))
                }
                rustTemplate("Ok(#{SdkBody}::from(out))", *codegenScope)
            }
        }
    }

    override fun documentSerializer(): RuntimeType {
        TODO("RestXML does not support document types")
    }

    override fun payloadSerializer(member: MemberShape): RuntimeType {
        val fnName = symbolProvider.serializeFunctionName(member)
        val target = model.expectShape(member.target)
        return RuntimeType.forInlineFun(fnName, xmlSerModule) {
            val t = symbolProvider.toSymbol(member).rustType().stripOuter<RustType.Option>().render(true)
            it.rustBlockTemplate(
                "pub fn $fnName(input: &$t) -> std::result::Result<std::vec::Vec<u8>, #{Error}>",
                *codegenScope
            ) {
                rust("let mut out = String::new();")
                // create a scope for writer. This ensure that writer has been dropped before returning the
                // string and ensures that all closing tags get written
                rustBlock("") {
                    rustTemplate(
                        """
                        let mut writer = #{XmlWriter}::new(&mut out);
                        ##[allow(unused_mut)]
                        let mut root = writer.start_el(${xmlIndex.payloadShapeName(member).dq()})${
                        target.xmlNamespace(root = true).apply()
                        };
                        """,
                        *codegenScope
                    )
                    when (target) {
                        is StructureShape -> serializeStructure(
                            target,
                            XmlMemberIndex.fromMembers(target.members().toList()),
                            Ctx.Element("root", "input")
                        )
                        is UnionShape -> serializeUnion(target, Ctx.Element("root", "input"))
                        else -> throw IllegalStateException("xml payloadSerializer only supports structs and unions")
                    }
                }
                rustTemplate("Ok(out.into_bytes())", *codegenScope)
            }
        }
    }

    override fun unsetStructure(structure: StructureShape): RuntimeType {
        val fnName = "rest_xml_unset_payload"
        return RuntimeType.forInlineFun(fnName, operationSerModule) { writer ->
            writer.rustTemplate(
                """
                pub fn $fnName() -> #{ByteSlab} {
                    Vec::new()
                }
                """,
                "ByteSlab" to RuntimeType.ByteSlab
            )
        }
    }

    override fun operationOutputSerializer(operationShape: OperationShape): RuntimeType? {
        val fnName = symbolProvider.serializeFunctionName(operationShape)
        val outputShape = operationShape.outputShape(model)
        val xmlMembers = operationShape.responseBodyMembers()
        if (xmlMembers.isEmpty()) {
            return null
        }
        val operationXmlName = xmlIndex.operationOutputShapeName(operationShape)
            ?: throw CodegenException("operation must have a name if it has members")
        return RuntimeType.forInlineFun(fnName, operationSerModule) {
            it.rustBlockTemplate(
                "pub fn $fnName(output: &#{target}) -> Result<String, #{Error}>",
                *codegenScope, "target" to symbolProvider.toSymbol(outputShape)
            ) {
                rust("let mut out = String::new();")
                // create a scope for writer. This ensure that writer has been dropped before returning the
                // string and ensures that all closing tags get written
                rustBlock("") {
                    rustTemplate(
                        """
                        let mut writer = #{XmlWriter}::new(&mut out);
                        ##[allow(unused_mut)]
                        let mut root = writer.start_el(${operationXmlName.dq()})${outputShape.xmlNamespace(root = true).apply()};
                        """,
                        *codegenScope
                    )
                    serializeStructure(outputShape, xmlMembers, Ctx.Element("root", "output"))
                }
                rustTemplate("Ok(out)", *codegenScope)
            }
        }
    }

    override fun serverErrorSerializer(shape: ShapeId): RuntimeType {
        val errorShape = model.expectShape(shape, StructureShape::class.java)
        val xmlMembers = httpBindingResolver.errorResponseBindings(shape)
            .filter { it.location == HttpLocation.DOCUMENT }
            .map { it.member }
        val fnName = symbolProvider.serializeFunctionName(errorShape)
        return RuntimeType.forInlineFun(fnName, operationSerModule) {
            it.rustBlockTemplate(
                "pub fn $fnName(error: &#{target}) -> Result<String, #{Error}>",
                *codegenScope, "target" to symbolProvider.toSymbol(errorShape)
            ) {
                rust("let mut out = String::new();")
                // create a scope for writer. This ensure that writer has been dropped before returning the
                // string and ensures that all closing tags get written
                rustBlock("") {
                    rustTemplate(
                        """
                        let mut writer = #{XmlWriter}::new(&mut out);
                        ##[allow(unused_mut)]
                        let mut root = writer.start_el("Error")${errorShape.xmlNamespace(root = true).apply()};
                        """,
                        *codegenScope
                    )
                    serializeStructure(
                        errorShape,
                        XmlMemberIndex.fromMembers(xmlMembers),
                        Ctx.Element("root", "error")
                    )
                }
                rustTemplate("Ok(out)", *codegenScope)
            }
        }
    }

    private fun XmlNamespaceTrait?.apply(): String {
        this ?: return ""
        val prefix = prefix.map { prefix -> "Some(${prefix.dq()})" }.orElse("None")
        return ".write_ns(${uri.dq()}, $prefix)"
    }

    private fun RustWriter.structureInner(members: XmlMemberIndex, ctx: Ctx.Element) {
        if (members.attributeMembers.isNotEmpty()) {
            rust("let mut ${ctx.elementWriter} = ${ctx.elementWriter};")
        }
        members.attributeMembers.forEach { member ->
            handleOptional(member, ctx.scopedTo(member)) { ctx ->
                withBlock("${ctx.elementWriter}.write_attribute(${xmlIndex.memberName(member).dq()},", ");") {
                    serializeRawMember(member, ctx.input)
                }
            }
        }
        Attribute.AllowUnusedMut.render(this)
        rust("let mut scope = ${ctx.elementWriter}.finish();")
        val scopeCtx = Ctx.Scope("scope", ctx.input)
        members.dataMembers.forEach { member ->
            serializeMember(member, scopeCtx.scopedTo(member), null)
        }
        rust("scope.finish();")
    }

    private fun RustWriter.serializeRawMember(member: MemberShape, input: String) {
        when (val shape = model.expectShape(member.target)) {
            is StringShape -> if (shape.hasTrait<EnumTrait>()) {
                rust("$input.as_str()")
            } else {
                rust("$input.as_ref()")
            }
            is BooleanShape, is NumberShape -> {
                rust(
                    "#T::from(${autoDeref(input)}).encode()",
                    CargoDependency.SmithyTypes(runtimeConfig).asType().member("primitive::Encoder")
                )
            }
            is BlobShape -> rust("#T($input.as_ref()).as_ref()", RuntimeType.Base64Encode(runtimeConfig))
            is TimestampShape -> {
                val timestampFormat =
                    httpBindingResolver.timestampFormat(
                        member,
                        HttpLocation.DOCUMENT,
                        TimestampFormatTrait.Format.DATE_TIME
                    )
                val timestampFormatType = RuntimeType.TimestampFormat(runtimeConfig, timestampFormat)
                rust("$input.fmt(#T)?.as_ref()", timestampFormatType)
            }
            else -> TODO(member.toString())
        }
    }

    @Suppress("NAME_SHADOWING")
    private fun RustWriter.serializeMember(memberShape: MemberShape, ctx: Ctx.Scope, rootNameOverride: String? = null) {
        val target = model.expectShape(memberShape.target)
        val xmlName = rootNameOverride ?: xmlIndex.memberName(memberShape)
        val ns = memberShape.xmlNamespace(root = false).apply()
        handleOptional(memberShape, ctx) { ctx ->
            when (target) {
                is StringShape, is BooleanShape, is NumberShape, is TimestampShape, is BlobShape -> {
                    rust("let mut inner_writer = ${ctx.scopeWriter}.start_el(${xmlName.dq()})$ns.finish();")
                    withBlock("inner_writer.data(", ");") {
                        serializeRawMember(memberShape, ctx.input)
                    }
                }
                is CollectionShape -> if (memberShape.hasTrait<XmlFlattenedTrait>()) {
                    serializeFlatList(memberShape, target, ctx)
                } else {
                    rust("let mut inner_writer = ${ctx.scopeWriter}.start_el(${xmlName.dq()})$ns.finish();")
                    serializeList(target, Ctx.Scope("inner_writer", ctx.input))
                }
                is MapShape -> if (memberShape.hasTrait<XmlFlattenedTrait>()) {
                    serializeMap(target, xmlIndex.memberName(memberShape), ctx)
                } else {
                    rust("let mut inner_writer = ${ctx.scopeWriter}.start_el(${xmlName.dq()})$ns.finish();")
                    serializeMap(target, "entry", Ctx.Scope("inner_writer", ctx.input))
                }
                is StructureShape -> {
                    rust("let inner_writer = ${ctx.scopeWriter}.start_el(${xmlName.dq()})$ns;")
                    serializeStructure(
                        target,
                        XmlMemberIndex.fromMembers(target.members().toList()),
                        Ctx.Element("inner_writer", ctx.input)
                    )
                }
                is UnionShape -> {
                    rust("let inner_writer = ${ctx.scopeWriter}.start_el(${xmlName.dq()})$ns;")
                    serializeUnion(target, Ctx.Element("inner_writer", ctx.input))
                }
                else -> TODO(target.toString())
            }
        }
    }

    private fun RustWriter.serializeStructure(
        structureShape: StructureShape,
        members: XmlMemberIndex,
        ctx: Ctx.Element
    ) {
        val structureSymbol = symbolProvider.toSymbol(structureShape)
        val fnName = symbolProvider.serializeFunctionName(structureShape)
        val structureSerializer = RuntimeType.forInlineFun(fnName, xmlSerModule) {
            it.rustBlockTemplate(
                "pub fn $fnName(input: &#{Input}, writer: #{ElementWriter}) -> Result<(), #{Error}>",
                "Input" to structureSymbol,
                *codegenScope
            ) {
                if (!members.isNotEmpty()) {
                    // removed unused warning if there are no fields we're going to read
                    rust("let _ = input;")
                }
                structureInner(members, Ctx.Element("writer", "&input"))
                rust("Ok(())")
            }
        }
        rust("#T(${ctx.input}, ${ctx.elementWriter})?", structureSerializer)
    }

    private fun RustWriter.serializeUnion(unionShape: UnionShape, ctx: Ctx.Element) {
        val fnName = symbolProvider.serializeFunctionName(unionShape)
        val unionSymbol = symbolProvider.toSymbol(unionShape)
        val structureSerializer = RuntimeType.forInlineFun(fnName, xmlSerModule) {
            it.rustBlockTemplate(
                "pub fn $fnName(input: &#{Input}, writer: #{ElementWriter}) -> Result<(), #{Error}>",
                "Input" to unionSymbol,
                *codegenScope
            ) {
                rust("let mut scope_writer = writer.finish();")
                rustBlock("match input") {
                    val members = unionShape.members()
                    members.forEach { member ->
                        val variantName = symbolProvider.toMemberName(member)
                        withBlock("#T::$variantName(inner) =>", ",", unionSymbol) {
                            serializeMember(member, Ctx.Scope("scope_writer", "inner"))
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
        rust("#T(${ctx.input}, ${ctx.elementWriter})?", structureSerializer)
    }

    private fun RustWriter.serializeList(listShape: CollectionShape, ctx: Ctx.Scope) {
        val itemName = safeName("list_item")
        rustBlock("for $itemName in ${ctx.input}") {
            serializeMember(listShape.member, ctx.copy(input = itemName))
        }
    }

    private fun RustWriter.serializeFlatList(member: MemberShape, listShape: CollectionShape, ctx: Ctx.Scope) {
        val itemName = safeName("list_item")
        rustBlock("for $itemName in ${ctx.input}") {
            serializeMember(listShape.member, ctx.copy(input = itemName), xmlIndex.memberName(member))
        }
    }

    private fun RustWriter.serializeMap(mapShape: MapShape, entryName: String, ctx: Ctx.Scope) {
        val key = safeName("key")
        val value = safeName("value")
        rustBlock("for ($key, $value) in ${ctx.input}") {
            rust("""let mut entry = ${ctx.scopeWriter}.start_el(${entryName.dq()}).finish();""")
            serializeMember(mapShape.key, ctx.copy(scopeWriter = "entry", input = key))
            serializeMember(mapShape.value, ctx.copy(scopeWriter = "entry", input = value))
        }
    }

    /**
     * If [member] is an optional shape, generate code like:
     * ```rust
     * if let Some(inner) = member {
     *   // .. BLOCK
     * }
     * ```
     *
     * If [member] is not an optional shape, generate code like:
     * `{ .. Block }`
     *
     * [inner] is passed a new `ctx` object to use for code generation which handles the
     * potentially new name of the input.
     */
    private fun <T : Ctx> RustWriter.handleOptional(
        member: MemberShape,
        ctx: T,
        inner: RustWriter.(T) -> Unit
    ) {
        val memberSymbol = symbolProvider.toSymbol(member)
        if (memberSymbol.isOptional()) {
            val tmp = safeName()
            rustBlock("if let Some($tmp) = ${ctx.input}") {
                inner(Ctx.updateInput(ctx, tmp))
            }
        } else {
            with(util) {
                ignoreZeroValues(member, ValueExpression.Value(autoDeref(ctx.input))) {
                    inner(ctx)
                }
            }
        }
    }

    private fun OperationShape.requestBodyMembers(): XmlMemberIndex =
        XmlMemberIndex.fromMembers(httpBindingResolver.requestMembers(this, HttpLocation.DOCUMENT))

    private fun OperationShape.responseBodyMembers(): XmlMemberIndex =
        XmlMemberIndex.fromMembers(httpBindingResolver.responseMembers(this, HttpLocation.DOCUMENT))

    private fun Shape.xmlNamespace(root: Boolean): XmlNamespaceTrait? {
        return this.getTrait<XmlNamespaceTrait>().letIf(root) { it ?: rootNamespace }
    }
}
