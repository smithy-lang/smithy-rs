/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.protocols

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.knowledge.HttpBinding
import software.amazon.smithy.model.knowledge.HttpBindingIndex
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.BooleanShape
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.NumberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.TimestampShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.model.traits.XmlAttributeTrait
import software.amazon.smithy.model.traits.XmlFlattenedTrait
import software.amazon.smithy.model.traits.XmlNameTrait
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.conditionalBlock
import software.amazon.smithy.rust.codegen.rustlang.escape
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.withBlock
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.builderSymbol
import software.amazon.smithy.rust.codegen.smithy.generators.setterName
import software.amazon.smithy.rust.codegen.smithy.isBoxed
import software.amazon.smithy.rust.codegen.smithy.isOptional
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticOutputTrait
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.expectMember
import software.amazon.smithy.rust.codegen.util.orNull
import software.amazon.smithy.rust.codegen.util.outputShape
import software.amazon.smithy.rust.codegen.util.toPascalCase
import software.amazon.smithy.rust.codegen.util.toSnakeCase

interface StructuredDataParserGenerator {
    /**
     * Generate a parse function for a given targeted as a payload.
     * Entry point for payload-based parsing.
     * Roughly:
     * ```rust
     * fn parse_my_struct(input: &[u8]) -> Result<MyStruct, XmlError> {
     *      ...
     * }
     * ```
     */
    fun payloadParser(member: MemberShape): RuntimeType

    /** Generate a parser for operation input
     * Because only a subset of fields of the operation may be impacted by the document, a builder is passed
     * through:
     *
     * ```rust
     * fn parse_some_operation(inp: &[u8], builder: my_operation::Builder) -> Result<my_operation::Builder, XmlError> {
     *   ...
     * }
     * ```
     */
    fun operationParser(operationShape: OperationShape): RuntimeType?

    /**
     * Because only a subset of fields of the operation may be impacted by the document, a builder is passed
     * through:
     *
     * ```rust
     * fn parse_some_error(inp: &[u8], builder: my_operation::Builder) -> Result<my_operation::Builder, XmlError> {
     *   ...
     * }
     */
    fun errorParser(errorShape: StructureShape): RuntimeType?

    /**
     * ```rust
     * fn parse_document(inp: &[u8]) -> Result<Document, Error> {
     *   ...
     * }
     * ```
     */
    fun documentParser(operationShape: OperationShape): RuntimeType
}

interface StructuredDataSerializerGenerator {
    /**
     * Generate a parse function for a given targeted as a payload.
     * Entry point for payload-based parsing.
     * Roughly:
     * ```rust
     * ```
     */
    fun payloadSerializer(member: MemberShape): RuntimeType

    /** Generate a serializer for operation input
     * Because only a subset of fields of the operation may be impacted by the document, a builder is passed
     * through:
     *
     * ```rust
     * fn parse_some_operation(inp: &[u8], builder: my_operation::Builder) -> Result<my_operation::Builder, XmlError> {
     *   ...
     * }
     * ```
     */
    fun operationSeralizer(operationShape: OperationShape): RuntimeType?

    /**
     * ```rust
     * fn parse_document(inp: &[u8]) -> Result<Document, Error> {
     *   ...
     * }
     * ```
     */
    fun documentSerializer(): RuntimeType
}

class XmlBindingTraitParserGenerator(protocolConfig: ProtocolConfig, private val xmlErrors: RuntimeType) :
    StructuredDataParserGenerator {

    /**
     * Abstraction to represent an XML element name:
     * `[prefix]:[local]`
     */
    data class XmlName(val local: String, val prefix: String? = null) {
        override fun toString(): String {
            return prefix?.let { "$it:" }.orEmpty() + local
        }

        companion object {
            fun parse(v: String): XmlName {
                val split = v.indexOf(':')
                return if (split == -1) {
                    XmlName(local = v, prefix = null)
                } else {
                    XmlName(v.substring(split + 1), prefix = v.substring(0, split))
                }
            }
        }
    }

    /**
     * Codegeneration Context
     *
     * [tag]: The symbol name of the current tag
     * [accum]: Flattened lists and maps need to be written into an accumulator. When a flattened list / map
     * is possible, `[accum]` contains an expression to mutably access the accumulator. Specifically, this is an
     * option to the collection st. the caller can evaluate `accum.unwrap_or_default()` to get a collection to write
     * data into.
     */
    data class Ctx(val tag: String, val accum: String?)

    private val symbolProvider = protocolConfig.symbolProvider
    private val smithyXml = CargoDependency.smithyXml(protocolConfig.runtimeConfig).asType()
    private val xmlError = smithyXml.member("decode::XmlError")

    private val scopedDecoder = smithyXml.member("decode::ScopedDecoder")
    private val runtimeConfig = protocolConfig.runtimeConfig

    // The symbols we want all the time
    private val codegenScope = arrayOf(
        "Blob" to RuntimeType.Blob(runtimeConfig),
        "Document" to smithyXml.member("decode::Document"),
        "XmlError" to xmlError,
        "next_start_element" to smithyXml.member("decode::next_start_element"),
        "try_data" to smithyXml.member("decode::try_data"),
        "ScopedDecoder" to scopedDecoder
    )
    private val model = protocolConfig.model
    private val index = HttpBindingIndex.of(model)

    /**
     * Generate a parse function for a given targeted as a payload.
     * Entry point for payload-based parsing.
     * Roughly:
     * ```rust
     * fn parse_my_struct(input: &[u8]) -> Result<MyStruct, XmlError> {
     *      ...
     * }
     * ```
     */
    override fun payloadParser(member: MemberShape): RuntimeType {
        val shape = model.expectShape(member.target)
        check(shape is UnionShape || shape is StructureShape) { "payload parser should only be used on structures & unions" }
        val fnName =
            "parse_payload_" + shape.id.name.toString().toSnakeCase() + member.container.name.toString().toSnakeCase()
        return RuntimeType.forInlineFun(fnName, "xml_deser") {
            it.rustBlock(
                "pub fn $fnName(inp: &[u8]) -> Result<#1T, #2T>",
                symbolProvider.toSymbol(shape),
                xmlError
            ) {
                // for payloads, first look at the member trait
                // next, look to see if this structure was renamed

                val shapeName = payloadName(member)
                rustTemplate(
                    """
                    use std::convert::TryFrom;
                    let mut doc = #{Document}::try_from(inp)?;
                    let mut decoder = doc.root_element()?;
                    let start_el = decoder.start_el();
                    if !(${shapeName.compareTo("start_el")}) {
                        return Err(#{XmlError}::custom(format!("invalid root, expected $shapeName got {:?}", start_el)))
                    }
                    """,
                    *codegenScope
                )
                val ctx = Ctx("decoder", accum = null)
                when (shape) {
                    is StructureShape -> {
                        parseStructure(shape, ctx)
                    }
                    is UnionShape -> parseUnion(shape, ctx)
                }
            }
        }
    }

    private fun payloadName(member: MemberShape): XmlName {
        val payloadShape = model.expectShape(member.target)
        val xmlRename = member.getTrait(XmlNameTrait::class.java).orNull()
            ?: payloadShape.getTrait(XmlNameTrait::class.java).orNull()

        return xmlRename?.let { XmlName.parse(it.value) } ?: XmlName(local = payloadShape.id.name)
    }

    /** Generate a parser for operation input
     * Because only a subset of fields of the operation may be impacted by the document, a builder is passed
     * through:
     *
     * ```rust
     * fn parse_some_operation(inp: &[u8], builder: my_operation::Builder) -> Result<my_operation::Builder, XmlError> {
     *   ...
     * }
     * ```
     */
    override fun operationParser(operationShape: OperationShape): RuntimeType? {
        val outputShape = operationShape.outputShape(model)
        val fnName = operationShape.id.name.toString().toSnakeCase() + "_deser_operation"
        val shapeName = operationXmlName(outputShape)
        val members = operationShape.operationXmlMembers()
        if (shapeName == null || !members.isNotEmpty()) {
            return null
        }
        return RuntimeType.forInlineFun(fnName, "xml_deser") {
            it.rustBlock(
                "pub fn $fnName(inp: &[u8], mut builder: #1T) -> Result<#1T, #2T>",
                outputShape.builderSymbol(symbolProvider),
                xmlError
            ) {
                rustTemplate(
                    """
                    use std::convert::TryFrom;
                    let mut doc = #{Document}::try_from(inp)?;
                    let mut decoder = doc.root_element()?;
                    let start_el = decoder.start_el();
                    if !(${shapeName.compareTo("start_el")}) {
                        return Err(#{XmlError}::custom(format!("invalid root, expected $shapeName got {:?}", start_el)))
                    }
                    """,
                    *codegenScope
                )
                parseStructureInner(members, builder = "builder", Ctx(tag = "decoder", accum = null))
                rust("Ok(builder)")
            }
        }
    }

    private fun operationXmlName(outputShape: StructureShape): XmlName? {
        return outputShape.getTrait(XmlNameTrait::class.java).orNull()?.let { XmlName.parse(it.value) }
            ?: outputShape.expectTrait(SyntheticOutputTrait::class.java).originalId?.name?.let {
                XmlName(local = it, prefix = null)
            }
    }

    override fun errorParser(errorShape: StructureShape): RuntimeType {
        val fnName = errorShape.id.name.toString().toSnakeCase()
        return RuntimeType.forInlineFun(fnName, "xml_deser") {
            it.rustBlock(
                "pub fn $fnName(inp: &[u8], mut builder: #1T) -> Result<#1T, #2T>",
                errorShape.builderSymbol(symbolProvider),
                xmlError
            ) {
                rustTemplate(
                    """
                    use std::convert::TryFrom;
                    let mut document = #{Document}::try_from(inp)?;
                    let mut error_decoder = #{xml_errors}::error_scope(&mut document)?;
                    """,
                    *codegenScope,
                    "xml_errors" to xmlErrors
                )
                val members = errorShape.errorXmlMembers()
                parseStructureInner(members, builder = "builder", Ctx(tag = "error_decoder", accum = null))
                rust("Ok(builder)")
            }
        }
    }

    override fun documentParser(operationShape: OperationShape): RuntimeType {
        TODO("Document shapes are not supported by rest XML")
    }

    /**
     * Update a structure builder based on the [members], specifying where to find each member (document vs. attributes)
     */
    private fun RustWriter.parseStructureInner(members: XmlMemberIndex, builder: String, outerCtx: Ctx) {
        members.attributeMembers.forEach { member ->
            val temp = safeName("attrib")
            withBlock("let $temp = ", ";") {
                parseAttributeMember(member, outerCtx)
            }
            rust("$builder.${symbolProvider.toMemberName(member)} = $temp;")
        }
        parseLoop(outerCtx) { ctx ->
            members.dataMembers.forEach { member ->
                case(member) {
                    val temp = safeName()
                    withBlock("let $temp = ", ";") {
                        parseMember(
                            member,
                            ctx.copy(accum = "$builder.${symbolProvider.toMemberName(member)}.take()")
                        )
                    }
                    rust("$builder = $builder.${member.setterName()}($temp);")
                }
            }
        }
    }

    /**
     * The core XML parsing abstraction: A loop that reads through the top level tags at the current scope &
     * generates a match expression
     * When [ignoreUnexpected] is true, unexpected tags are ignored
     */
    private fun RustWriter.parseLoop(ctx: Ctx, ignoreUnexpected: Boolean = true, inner: RustWriter.(Ctx) -> Unit) {
        rustBlock("while let Some(mut tag) = ${ctx.tag}.next_tag()") {
            rustBlock("match tag.start_el()") {
                inner(ctx.copy(tag = "tag"))
                if (ignoreUnexpected) {
                    rust("_ => {}")
                }
            }
        }
    }

    /**
     * Generate an XML parser for a given member
     */
    private fun RustWriter.parseMember(memberShape: MemberShape, ctx: Ctx) {
        val target = model.expectShape(memberShape.target)
        val symbol = symbolProvider.toSymbol(memberShape)
        conditionalBlock("Some(", ")", symbol.isOptional()) {
            conditionalBlock("Box::new(", ")", symbol.isBoxed()) {
                when (target) {
                    is StringShape, is BooleanShape, is NumberShape, is TimestampShape, is BlobShape -> parsePrimitiveInner(
                        memberShape
                    ) {
                        rustTemplate("#{try_data}(&mut ${ctx.tag})?.as_ref()", *codegenScope)
                    }
                    is MapShape -> if (memberShape.isFlattened()) {
                        parseFlatMap(target, ctx)
                    } else {
                        parseMap(target, ctx)
                    }
                    is CollectionShape -> if (memberShape.isFlattened()) {
                        parseFlatList(target, ctx)
                    } else {
                        parseList(target, ctx)
                    }
                    is StructureShape -> {
                        parseStructure(target, ctx)
                    }
                    is UnionShape -> parseUnion(target, ctx)
                    else -> TODO("Unhandled: $target")
                }
                // each internal `parseT` function writes an `Result<T, E>` expression, unwrap those:
                rust("?")
            }
        }
    }

    private fun RustWriter.parseAttributeMember(memberShape: MemberShape, ctx: Ctx) {
        rustBlock("") {
            rustTemplate(
                """let s = ${ctx.tag}
                    .start_el()
                    .attr(${memberShape.xmlName().toString().dq()});""",
                *codegenScope
            )
            rustBlock("match s") {
                rust("None => None,")
                withBlock("Some(s) => Some(", ")") {
                    parsePrimitiveInner(memberShape) {
                        rust("s")
                    }
                    rust("?")
                }
            }
        }
    }

    private fun RustWriter.parseUnion(shape: UnionShape, ctx: Ctx) {
        val fnName = shape.id.name.toString().toSnakeCase() + "_inner"
        val symbol = symbolProvider.toSymbol(shape)
        val nestedParser = RuntimeType.forInlineFun(fnName, "xml_deser") {
            it.rustBlockTemplate(
                "pub fn $fnName(decoder: &mut #{ScopedDecoder}) -> Result<#{Shape}, #{XmlError}>",
                *codegenScope, "Shape" to symbol
            ) {
                val members = shape.members()
                rustTemplate("let mut base: Option<#{Shape}> = None;", *codegenScope)
                parseLoop(Ctx(tag = "decoder", accum = null)) { ctx ->
                    members.forEach { member ->
                        val variantName = member.memberName.toPascalCase()
                        case(member) {
                            val current =
                                """
                                    (match base.take() {
                                        None => None,
                                        Some(${format(symbol)}::$variantName(inner)) => Some(inner),
                                        Some(_) => return Err(#{XmlError}::custom("mixed variants"))
                                    })
                                """
                            withBlock("let tmp = ", ";") {
                                parseMember(member, ctx.copy(accum = current))
                            }
                            rust("base = Some(#T::$variantName(tmp));", symbol)
                        }
                    }
                }
                rustTemplate("""base.ok_or_else(||#{XmlError}::custom("expected union, got nothing"))""", *codegenScope)
            }
        }
        rust("#T(&mut ${ctx.tag})", nestedParser)
    }

    /**
     * The match clause to check if the tag matches a given member
     */
    private fun RustWriter.case(member: MemberShape, inner: RustWriter.() -> Unit) {
        rustBlock(
            "s if ${
            member.xmlName().compareTo("s")
            } /* ${member.memberName} ${escape(member.id.toString())} */ => "
        ) {
            inner()
        }
        rust(",")
    }

    private fun RustWriter.parseStructure(shape: StructureShape, ctx: Ctx) {
        val fnName = shape.id.name.toString().toSnakeCase() + "_inner"
        val symbol = symbolProvider.toSymbol(shape)
        val nestedParser = RuntimeType.forInlineFun(fnName, "xml_deser") {
            it.rustBlockTemplate(
                "pub fn $fnName(decoder: &mut #{ScopedDecoder}) -> Result<#{Shape}, #{XmlError}>",
                *codegenScope, "Shape" to symbol
            ) {
                rustTemplate(
                    """
                    let mut builder = #{Shape}::builder();
                """,
                    *codegenScope, "Shape" to symbol
                )
                val members = shape.xmlMembers()
                parseStructureInner(members, "builder", Ctx(tag = "decoder", accum = null))
                withBlock("Ok(builder.build()", ")") {
                    if (StructureGenerator.fallibleBuilder(shape, symbolProvider)) {
                        rust(""".map_err(|_|{XmlError}::custom("missing field"))?""")
                    }
                }
            }
        }
        rust("#T(&mut ${ctx.tag})", nestedParser)
    }

    private fun RustWriter.parseList(target: CollectionShape, ctx: Ctx) {
        val fnName = "deserialize_${target.member.id.name.toSnakeCase()}"
        val member = target.member
        val listParser = RuntimeType.forInlineFun(fnName, "xml_deser") {
            it.rustBlockTemplate(
                "pub fn $fnName(decoder: &mut #{ScopedDecoder}) -> Result<#{List}, #{XmlError}>",
                *codegenScope,
                "List" to symbolProvider.toSymbol(target)
            ) {
                rust("let mut out = std::vec::Vec::new();")
                parseLoop(Ctx(tag = "decoder", accum = null)) { ctx ->
                    case(member) {
                        withBlock("out.push(", ");") {
                            parseMember(member, ctx)
                        }
                    }
                }
                rust("Ok(out)")
            }
        }
        rust("#T(&mut ${ctx.tag})", listParser)
    }

    private fun RustWriter.parseFlatList(target: CollectionShape, ctx: Ctx) {
        val list = safeName("list")
        withBlock("Result::<#T, #T>::Ok({", "})", symbolProvider.toSymbol(target), xmlError) {
            val accum = ctx.accum ?: throw CodegenException("Need accum to parse flat list")
            rustTemplate("""let mut $list = $accum.unwrap_or_default();""", *codegenScope)
            withBlock("$list.push(", ");") {
                parseMember(target.member, ctx)
            }
            rust(list)
        }
    }

    private fun RustWriter.parseMap(target: MapShape, ctx: Ctx) {
        val fnName = "deserialize_${target.value.id.name.toSnakeCase()}"
        val mapParser = RuntimeType.forInlineFun(fnName, "xml_deser") {
            it.rustBlockTemplate(
                "pub fn $fnName(decoder: &mut #{ScopedDecoder}) -> Result<#{Map}, #{XmlError}>",
                *codegenScope,
                "Map" to symbolProvider.toSymbol(target)
            ) {
                rust("let mut out = #T::new();", RustType.HashMap.RuntimeType)
                parseLoop(Ctx(tag = "decoder", accum = null)) { ctx ->
                    rustBlock("s if ${XmlName(local = "entry").compareTo("s")} => ") {
                        rust("#T(&mut ${ctx.tag}, &mut out)?;", mapEntryParser(target, ctx))
                    }
                }
                rust("Ok(out)")
            }
        }
        rust("#T(&mut ${ctx.tag})", mapParser)
    }

    private fun RustWriter.parseFlatMap(target: MapShape, ctx: Ctx) {
        val map = safeName("map")
        val entryDecoder = mapEntryParser(target, ctx)
        withBlock("Result::<#T, #T>::Ok({", "})", symbolProvider.toSymbol(target), xmlError) {
            val accum = ctx.accum ?: throw CodegenException("need accum to parse flat map")
            rustTemplate(
                """
            let mut $map = $accum.unwrap_or_default();
            #{decoder}(&mut tag, &mut $map)?;
            $map
            """,
                *codegenScope,
                "decoder" to entryDecoder
            )
        }
    }

    private fun mapEntryParser(
        target: MapShape,
        ctx: Ctx
    ): RuntimeType {

        val fnName = target.value.id.name.toSnakeCase() + "_entry"
        return RuntimeType.forInlineFun(fnName, "xml_deser") {
            it.rustBlockTemplate(
                "pub fn $fnName(decoder: &mut #{ScopedDecoder}, out: &mut #{Map}) -> Result<(), #{XmlError}>",
                *codegenScope,
                "Map" to symbolProvider.toSymbol(target)
            ) {
                rust("let mut k: Option<String> = None;")
                rust(
                    "let mut v: Option<#T> = None;",
                    symbolProvider.toSymbol(model.expectShape(target.value.target))
                )
                parseLoop(Ctx("decoder", accum = null)) {
                    case(target.key) {
                        withBlock("k = Some(", ")") {
                            parseMember(target.key, ctx = ctx.copy(accum = null))
                        }
                    }
                    case(target.value) {
                        withBlock("v = Some(", ")") {
                            parseMember(target.value, ctx = ctx.copy(accum = "v"))
                        }
                    }
                }

                rustTemplate(
                    """
                let k = k.ok_or_else(||#{XmlError}::custom("missing key map entry"))?;
                let v = v.ok_or_else(||#{XmlError}::custom("missing value map entry"))?;
                out.insert(k, v);
                Ok(())
                        """,
                    *codegenScope
                )
            }
        }
    }

    /**
     * Parse a simple member from a data field
     * [provider] generates code for the inner data field
     */
    private fun RustWriter.parsePrimitiveInner(member: MemberShape, provider: RustWriter.() -> Unit) {
        when (val shape = model.expectShape(member.target)) {
            is StringShape -> parseStringInner(shape, provider)
            is NumberShape, is BooleanShape -> {
                rustBlock("") {
                    rust("use std::str::FromStr;")
                    withBlock("#T::from_str(", ")", symbolProvider.toSymbol(shape)) {
                        provider()
                    }
                    rustTemplate(
                        """.map_err(|_|#{XmlError}::custom("expected ${escape(shape.toString())}"))""",
                        *codegenScope
                    )
                }
            }
            is TimestampShape -> {
                val timestampFormat =
                    index.determineTimestampFormat(
                        member,
                        HttpBinding.Location.DOCUMENT,
                        TimestampFormatTrait.Format.DATE_TIME
                    )
                val timestampFormatType = RuntimeType.TimestampFormat(runtimeConfig, timestampFormat)
                withBlock("#T::from_str(", ")", RuntimeType.Instant(runtimeConfig)) {
                    provider()
                    rust(", #T", timestampFormatType)
                }
                rustTemplate(
                    """.map_err(|_|#{XmlError}::custom("expected ${escape(shape.toString())}"))""",
                    *codegenScope
                )
            }
            is BlobShape -> {
                withBlock("#T(", ")", RuntimeType.Base64Decode(runtimeConfig)) {
                    provider()
                }
                rustTemplate(
                    """.map_err(|err|#{XmlError}::custom(format!("invalid base64: {:?}", err))).map(#{Blob}::new)""",
                    *codegenScope
                )
            }
            else -> TODO(shape.toString())
        }
    }

    private fun RustWriter.parseStringInner(shape: StringShape, provider: RustWriter.() -> Unit) {
        withBlock("Result::<#T, #T>::Ok(", ")", symbolProvider.toSymbol(shape), xmlError) {
            val enumTrait = shape.getTrait(EnumTrait::class.java).orElse(null)
            if (enumTrait == null) {
                provider()
                // if it's already `Cow::Owned` then `.into()` is free (vs. to_string())
                rust(".into()")
            } else {
                val enumSymbol = symbolProvider.toSymbol(shape)
                withBlock("#T::from(", ")", enumSymbol) {
                    provider()
                }
            }
        }
    }

    private fun MemberShape.xmlName(): XmlName {
        val override = this.getTrait(XmlNameTrait::class.java).orNull()
        return override?.let { XmlName.parse(it.value) } ?: XmlName(local = this.memberName)
    }

    private fun MemberShape.isFlattened(): Boolean {
        return getMemberTrait(model, XmlFlattenedTrait::class.java).isPresent
    }

    fun XmlName.compareTo(start_el: String) =
        "$start_el.matches(${this.toString().dq()})"

    data class XmlMemberIndex(val dataMembers: List<MemberShape>, val attributeMembers: List<MemberShape>) {
        companion object {
            fun fromMembers(members: List<MemberShape>): XmlMemberIndex {
                val (attribute, data) = members.partition { it.hasTrait(XmlAttributeTrait::class.java) }
                return XmlMemberIndex(data, attribute)
            }
        }

        fun isNotEmpty() = dataMembers.isNotEmpty() || attributeMembers.isNotEmpty()
    }

    private fun OperationShape.operationXmlMembers(): XmlMemberIndex {
        val outputShape = this.outputShape(model)
        val documentMembers =
            index.getResponseBindings(this).filter { it.value.location == HttpBinding.Location.DOCUMENT }
                .keys.map { outputShape.expectMember(it) }
        return XmlMemberIndex.fromMembers(documentMembers)
    }

    private fun StructureShape.errorXmlMembers(): XmlMemberIndex {
        val documentMembers =
            index.getResponseBindings(this).filter { it.value.location == HttpBinding.Location.DOCUMENT }
                .keys.map { this.expectMember(it) }
        return XmlMemberIndex.fromMembers(documentMembers)
    }

    private fun StructureShape.xmlMembers(): XmlMemberIndex {
        return XmlMemberIndex.fromMembers(this.members().toList())
    }
}
