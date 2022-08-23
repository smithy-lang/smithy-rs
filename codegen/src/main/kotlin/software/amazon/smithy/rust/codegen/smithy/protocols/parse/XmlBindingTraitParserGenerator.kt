/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.protocols.parse

import software.amazon.smithy.aws.traits.customizations.S3UnwrappedXmlOutputTrait
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
import software.amazon.smithy.model.traits.XmlFlattenedTrait
import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustModule
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
import software.amazon.smithy.rust.codegen.rustlang.withBlockTemplate
import software.amazon.smithy.rust.codegen.smithy.CoreCodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.CodegenTarget
import software.amazon.smithy.rust.codegen.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.builderSymbol
import software.amazon.smithy.rust.codegen.smithy.generators.renderUnknownVariant
import software.amazon.smithy.rust.codegen.smithy.generators.setterName
import software.amazon.smithy.rust.codegen.smithy.isOptional
import software.amazon.smithy.rust.codegen.smithy.isRustBoxed
import software.amazon.smithy.rust.codegen.smithy.protocols.XmlMemberIndex
import software.amazon.smithy.rust.codegen.smithy.protocols.XmlNameIndex
import software.amazon.smithy.rust.codegen.smithy.protocols.deserializeFunctionName
import software.amazon.smithy.rust.codegen.util.PANIC
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.expectMember
import software.amazon.smithy.rust.codegen.util.hasTrait
import software.amazon.smithy.rust.codegen.util.inputShape
import software.amazon.smithy.rust.codegen.util.outputShape

// The string argument is the name of the XML ScopedDecoder to continue parsing from
typealias OperationInnerWriteable = RustWriter.(String) -> Unit

data class OperationWrapperContext(
    val shape: OperationShape,
    val outputShapeName: String,
    val xmlErrorType: RuntimeType,
)

class XmlBindingTraitParserGenerator(
    coreCodegenContext: CoreCodegenContext,
    private val xmlErrors: RuntimeType,
    private val writeOperationWrapper: RustWriter.(OperationWrapperContext, OperationInnerWriteable) -> Unit,
) : StructuredDataParserGenerator {

    /** Abstraction to represent an XML element name */
    data class XmlName(val name: String) {
        /** Generates an expression to match a given element against this XML tag name */
        fun matchExpression(start_el: String) = "$start_el.matches(${this.toString().dq()})"

        override fun toString(): String {
            return name
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

    private val symbolProvider = coreCodegenContext.symbolProvider
    private val smithyXml = CargoDependency.smithyXml(coreCodegenContext.runtimeConfig).asType()
    private val xmlError = smithyXml.member("decode::XmlError")

    private val scopedDecoder = smithyXml.member("decode::ScopedDecoder")
    private val runtimeConfig = coreCodegenContext.runtimeConfig

    // The symbols we want all the time
    private val codegenScope = arrayOf(
        "Blob" to RuntimeType.Blob(runtimeConfig),
        "Document" to smithyXml.member("decode::Document"),
        "XmlError" to xmlError,
        "next_start_element" to smithyXml.member("decode::next_start_element"),
        "try_data" to smithyXml.member("decode::try_data"),
        "ScopedDecoder" to scopedDecoder,
        "aws_smithy_types" to CargoDependency.SmithyTypes(runtimeConfig).asType(),
    )
    private val model = coreCodegenContext.model
    private val index = HttpBindingIndex.of(model)
    private val xmlIndex = XmlNameIndex.of(model)
    private val target = coreCodegenContext.target
    private val xmlDeserModule = RustModule.private("xml_deser")

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
        val fnName = symbolProvider.deserializeFunctionName(member)
        return RuntimeType.forInlineFun(fnName, xmlDeserModule) {
            it.rustBlock(
                "pub fn $fnName(inp: &[u8]) -> Result<#1T, #2T>",
                symbolProvider.toSymbol(shape),
                xmlError,
            ) {
                // for payloads, first look at the member trait
                // next, look to see if this structure was renamed

                val shapeName = XmlName(xmlIndex.payloadShapeName(member))
                rustTemplate(
                    """
                    let mut doc = #{Document}::try_from(inp)?;
                    ##[allow(unused_mut)]
                    let mut decoder = doc.root_element()?;
                    let start_el = decoder.start_el();
                    if !(${shapeName.matchExpression("start_el")}) {
                        return Err(#{XmlError}::custom(format!("invalid root, expected $shapeName got {:?}", start_el)))
                    }
                    """,
                    *codegenScope,
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
        val fnName = symbolProvider.deserializeFunctionName(operationShape)
        val shapeName = xmlIndex.operationOutputShapeName(operationShape)
        val members = operationShape.operationXmlMembers()
        if (shapeName == null || !members.isNotEmpty()) {
            return null
        }
        return RuntimeType.forInlineFun(fnName, xmlDeserModule) {
            Attribute.AllowUnusedMut.render(it)
            it.rustBlock(
                "pub fn $fnName(inp: &[u8], mut builder: #1T) -> Result<#1T, #2T>",
                outputShape.builderSymbol(symbolProvider),
                xmlError,
            ) {
                rustTemplate(
                    """
                    let mut doc = #{Document}::try_from(inp)?;

                    ##[allow(unused_mut)]
                    let mut decoder = doc.root_element()?;
                    let start_el = decoder.start_el();
                    """,
                    *codegenScope,
                )
                val context = OperationWrapperContext(operationShape, shapeName, xmlError)
                if (operationShape.hasTrait<S3UnwrappedXmlOutputTrait>()) {
                    unwrappedResponseParser("builder", "decoder", "start_el", outputShape.members())
                } else {
                    writeOperationWrapper(context) { tagName ->
                        parseStructureInner(members, builder = "builder", Ctx(tag = tagName, accum = null))
                    }
                }
                rust("Ok(builder)")
            }
        }
    }

    override fun errorParser(errorShape: StructureShape): RuntimeType {
        val fnName = symbolProvider.deserializeFunctionName(errorShape) + "_xml_err"
        return RuntimeType.forInlineFun(fnName, xmlDeserModule) {
            Attribute.AllowUnusedMut.render(it)
            it.rustBlock(
                "pub fn $fnName(inp: &[u8], mut builder: #1T) -> Result<#1T, #2T>",
                errorShape.builderSymbol(symbolProvider),
                xmlError,
            ) {
                val members = errorShape.errorXmlMembers()
                rust("if inp.is_empty() { return Ok(builder) }")
                if (members.isNotEmpty()) {
                    rustTemplate(
                        """
                        let mut document = #{Document}::try_from(inp)?;
                        ##[allow(unused_mut)]
                        let mut error_decoder = #{xml_errors}::error_scope(&mut document)?;
                        """,
                        *codegenScope,
                        "xml_errors" to xmlErrors,
                    )
                    parseStructureInner(members, builder = "builder", Ctx(tag = "error_decoder", accum = null))
                }
                rust("Ok(builder)")
            }
        }
    }

    override fun serverInputParser(operationShape: OperationShape): RuntimeType? {
        val inputShape = operationShape.inputShape(model)
        val fnName = symbolProvider.deserializeFunctionName(operationShape)
        val shapeName = xmlIndex.operationInputShapeName(operationShape)
        val members = operationShape.serverInputXmlMembers()
        if (shapeName == null || !members.isNotEmpty()) {
            return null
        }
        return RuntimeType.forInlineFun(fnName, xmlDeserModule) {
            Attribute.AllowUnusedMut.render(it)
            it.rustBlock(
                "pub fn $fnName(inp: &[u8], mut builder: #1T) -> Result<#1T, #2T>",
                inputShape.builderSymbol(symbolProvider),
                xmlError,
            ) {
                rustTemplate(
                    """
                    let mut doc = #{Document}::try_from(inp)?;

                    ##[allow(unused_mut)]
                    let mut decoder = doc.root_element()?;
                    let start_el = decoder.start_el();
                    """,
                    *codegenScope,
                )
                val context = OperationWrapperContext(operationShape, shapeName, xmlError)
                writeOperationWrapper(context) { tagName ->
                    parseStructureInner(members, builder = "builder", Ctx(tag = tagName, accum = null))
                }
                rust("Ok(builder)")
            }
        }
    }

    private fun RustWriter.unwrappedResponseParser(
        builder: String,
        decoder: String,
        element: String,
        members: Collection<MemberShape>,
    ) {
        check(members.size == 1) {
            "The S3UnwrappedXmlOutputTrait is only allowed on structs with exactly one member"
        }
        val member = members.first()
        rustBlock("match $element") {
            case(member) {
                val temp = safeName()
                withBlock("let $temp =", ";") {
                    parseMember(
                        member,
                        Ctx(tag = decoder, accum = "$builder.${symbolProvider.toMemberName(member)}.take()"),
                    )
                }
                rust("$builder = $builder.${member.setterName()}($temp);")
            }
            rustTemplate("_ => return Err(#{XmlError}::custom(\"expected ${member.xmlName()} tag\"))", *codegenScope)
        }
    }

    /**
     * Update a structure builder based on the [members], specifying where to find each member (document vs. attributes)
     */
    private fun RustWriter.parseStructureInner(members: XmlMemberIndex, builder: String, outerCtx: Ctx) {
        members.attributeMembers.forEach { member ->
            val temp = safeName("attrib")
            withBlock("let $temp =", ";") {
                parseAttributeMember(member, outerCtx)
            }
            rust("$builder.${symbolProvider.toMemberName(member)} = $temp;")
        }
        // No need to generate a parse loop if there are no non-attribute members
        if (members.dataMembers.isEmpty()) {
            return
        }
        parseLoop(outerCtx) { ctx ->
            members.dataMembers.forEach { member ->
                case(member) {
                    val temp = safeName()
                    withBlock("let $temp =", ";") {
                        parseMember(
                            member,
                            ctx.copy(accum = "$builder.${symbolProvider.toMemberName(member)}.take()"),
                            forceOptional = true,
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
    private fun RustWriter.parseMember(memberShape: MemberShape, ctx: Ctx, forceOptional: Boolean = false) {
        val target = model.expectShape(memberShape.target)
        val symbol = symbolProvider.toSymbol(memberShape)
        conditionalBlock("Some(", ")", forceOptional || symbol.isOptional()) {
            conditionalBlock("Box::new(", ")", symbol.isRustBoxed()) {
                when (target) {
                    is StringShape, is BooleanShape, is NumberShape, is TimestampShape, is BlobShape ->
                        parsePrimitiveInner(memberShape) {
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
                    else -> PANIC("Unhandled: $target")
                }
                // each internal `parseT` function writes an `Result<T, E>` expression, unwrap those:
                rust("?")
            }
        }
    }

    private fun RustWriter.parseAttributeMember(memberShape: MemberShape, ctx: Ctx) {
        rustBlock("") {
            rustTemplate(
                """
                let s = ${ctx.tag}
                    .start_el()
                    .attr(${memberShape.xmlName().toString().dq()});
                """,
                *codegenScope,
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
        val fnName = symbolProvider.deserializeFunctionName(shape)
        val symbol = symbolProvider.toSymbol(shape)
        val nestedParser = RuntimeType.forInlineFun(fnName, xmlDeserModule) {
            it.rustBlockTemplate(
                "pub fn $fnName(decoder: &mut #{ScopedDecoder}) -> Result<#{Shape}, #{XmlError}>",
                *codegenScope, "Shape" to symbol,
            ) {
                val members = shape.members()
                rustTemplate("let mut base: Option<#{Shape}> = None;", *codegenScope, "Shape" to symbol)
                parseLoop(Ctx(tag = "decoder", accum = null), ignoreUnexpected = false) { ctx ->
                    members.forEach { member ->
                        val variantName = symbolProvider.toMemberName(member)
                        case(member) {
                            val current =
                                """
                                (match base.take() {
                                    None => None,
                                    Some(${format(symbol)}::$variantName(inner)) => Some(inner),
                                    Some(_) => return Err(#{XmlError}::custom("mixed variants"))
                                })
                                """
                            withBlock("let tmp =", ";") {
                                parseMember(member, ctx.copy(accum = current.trim()))
                            }
                            rust("base = Some(#T::$variantName(tmp));", symbol)
                        }
                    }
                    when (target.renderUnknownVariant()) {
                        true -> rust("_unknown => base = Some(#T::${UnionGenerator.UnknownVariantName}),", symbol)
                        false -> rustTemplate("""variant => return Err(#{XmlError}::custom(format!("unexpected union variant: {:?}", variant)))""", *codegenScope)
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
            member.xmlName().matchExpression("s")
            } /* ${member.memberName} ${escape(member.id.toString())} */ => ",
        ) {
            inner()
        }
        rust(",")
    }

    private fun RustWriter.parseStructure(shape: StructureShape, ctx: Ctx) {
        val fnName = symbolProvider.deserializeFunctionName(shape)
        val symbol = symbolProvider.toSymbol(shape)
        val nestedParser = RuntimeType.forInlineFun(fnName, xmlDeserModule) {
            it.rustBlockTemplate(
                "pub fn $fnName(decoder: &mut #{ScopedDecoder}) -> Result<#{Shape}, #{XmlError}>",
                *codegenScope, "Shape" to symbol,
            ) {
                Attribute.AllowUnusedMut.render(this)
                rustTemplate("let mut builder = #{Shape}::builder();", *codegenScope, "Shape" to symbol)
                val members = shape.xmlMembers()
                if (members.isNotEmpty()) {
                    parseStructureInner(members, "builder", Ctx(tag = "decoder", accum = null))
                } else {
                    rust("let _ = decoder;")
                }
                withBlock("Ok(builder.build()", ")") {
                    if (StructureGenerator.fallibleBuilder(shape, symbolProvider)) {
                        // NOTE:(rcoh) This branch is unreachable given the current nullability rules.
                        // Only synthetic inputs can have fallible builders, but synthetic inputs can never be parsed
                        // (because they're inputs, only outputs will be parsed!)

                        // I'm leaving this branch here so that the binding trait parser generator would work for a server
                        // side implementation in the future.
                        rustTemplate(""".map_err(|_|#{XmlError}::custom("missing field"))?""", *codegenScope)
                    }
                }
            }
        }
        rust("#T(&mut ${ctx.tag})", nestedParser)
    }

    private fun RustWriter.parseList(target: CollectionShape, ctx: Ctx) {
        val fnName = symbolProvider.deserializeFunctionName(target)
        val member = target.member
        val listParser = RuntimeType.forInlineFun(fnName, xmlDeserModule) {
            it.rustBlockTemplate(
                "pub fn $fnName(decoder: &mut #{ScopedDecoder}) -> Result<#{List}, #{XmlError}>",
                *codegenScope,
                "List" to symbolProvider.toSymbol(target),
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
        val fnName = symbolProvider.deserializeFunctionName(target)
        val mapParser = RuntimeType.forInlineFun(fnName, xmlDeserModule) {
            it.rustBlockTemplate(
                "pub fn $fnName(decoder: &mut #{ScopedDecoder}) -> Result<#{Map}, #{XmlError}>",
                *codegenScope,
                "Map" to symbolProvider.toSymbol(target),
            ) {
                rust("let mut out = #T::new();", RustType.HashMap.RuntimeType)
                parseLoop(Ctx(tag = "decoder", accum = null)) { ctx ->
                    rustBlock("s if ${XmlName("entry").matchExpression("s")} => ") {
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
                "decoder" to entryDecoder,
            )
        }
    }

    private fun mapEntryParser(target: MapShape, ctx: Ctx): RuntimeType {
        val fnName = symbolProvider.deserializeFunctionName(target) + "_entry"
        return RuntimeType.forInlineFun(fnName, xmlDeserModule) {
            it.rustBlockTemplate(
                "pub fn $fnName(decoder: &mut #{ScopedDecoder}, out: &mut #{Map}) -> Result<(), #{XmlError}>",
                *codegenScope,
                "Map" to symbolProvider.toSymbol(target),
            ) {
                val keySymbol = symbolProvider.toSymbol(target.key)
                rust("let mut k: Option<#T> = None;", keySymbol)
                rust(
                    "let mut v: Option<#T> = None;",
                    symbolProvider.toSymbol(model.expectShape(target.value.target)),
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
                    *codegenScope,
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
                    withBlockTemplate(
                        "<#{shape} as #{aws_smithy_types}::primitive::Parse>::parse_smithy_primitive(",
                        ")",
                        *codegenScope,
                        "shape" to symbolProvider.toSymbol(shape),
                    ) {
                        provider()
                    }
                    rustTemplate(
                        """.map_err(|_|#{XmlError}::custom("expected ${escape(shape.toString())}"))""",
                        *codegenScope,
                    )
                }
            }
            is TimestampShape -> {
                val timestampFormat =
                    index.determineTimestampFormat(
                        member,
                        HttpBinding.Location.DOCUMENT,
                        TimestampFormatTrait.Format.DATE_TIME,
                    )
                val timestampFormatType = RuntimeType.TimestampFormat(runtimeConfig, timestampFormat)
                withBlock("#T::from_str(", ")", RuntimeType.DateTime(runtimeConfig)) {
                    provider()
                    rust(", #T", timestampFormatType)
                }
                rustTemplate(
                    """.map_err(|_|#{XmlError}::custom("expected ${escape(shape.toString())}"))""",
                    *codegenScope,
                )
            }
            is BlobShape -> {
                withBlock("#T(", ")", RuntimeType.Base64Decode(runtimeConfig)) {
                    provider()
                }
                rustTemplate(
                    """.map_err(|err|#{XmlError}::custom(format!("invalid base64: {:?}", err))).map(#{Blob}::new)""",
                    *codegenScope,
                )
            }
            else -> PANIC("unexpected shape: $shape")
        }
    }

    private fun RustWriter.parseStringInner(shape: StringShape, provider: RustWriter.() -> Unit) {
        withBlock("Result::<#T, #T>::Ok(", ")", symbolProvider.toSymbol(shape), xmlError) {
            if (shape.hasTrait<EnumTrait>()) {
                val enumSymbol = symbolProvider.toSymbol(shape)
                if (convertsToEnumInServer(shape)) {
                    withBlock("#T::try_from(", ")", enumSymbol) {
                        provider()
                    }
                    rustTemplate(""".map_err(|e| #{XmlError}::custom(format!("unknown variant {}", e)))?""", *codegenScope)
                } else {
                    withBlock("#T::from(", ")", enumSymbol) {
                        provider()
                    }
                }
            } else {
                provider()
                // If it's already `Cow::Owned` then `.into()` is free (as opposed to using `to_string()`).
                rust(".into()")
            }
        }
    }

    private fun convertsToEnumInServer(shape: StringShape) = target == CodegenTarget.SERVER && shape.hasTrait<EnumTrait>()

    private fun MemberShape.xmlName(): XmlName {
        return XmlName(xmlIndex.memberName(this))
    }

    private fun MemberShape.isFlattened(): Boolean {
        return getMemberTrait(model, XmlFlattenedTrait::class.java).isPresent
    }

    private fun OperationShape.operationXmlMembers(): XmlMemberIndex {
        val outputShape = this.outputShape(model)

        // HTTP trait bound protocols, such as RestXml, need to restrict the members to DOCUMENT members,
        // while protocols like AwsQuery do not support HTTP traits, and thus, all members are used.
        val responseBindings = index.getResponseBindings(this)
        return when (responseBindings.isEmpty()) {
            true -> XmlMemberIndex.fromMembers(outputShape.members().toList())
            else -> {
                val documentMembers =
                    responseBindings.filter { it.value.location == HttpBinding.Location.DOCUMENT }
                        .keys.map { outputShape.expectMember(it) }
                return XmlMemberIndex.fromMembers(documentMembers)
            }
        }
    }

    private fun OperationShape.serverInputXmlMembers(): XmlMemberIndex {
        val inputShape = this.inputShape(model)

        // HTTP trait bound protocols, such as RestXml, need to restrict the members to DOCUMENT members,
        // while protocols like AwsQuery do not support HTTP traits, and thus, all members are used.
        val requestBindings = index.getRequestBindings(this)
        return when (requestBindings.isEmpty()) {
            true -> XmlMemberIndex.fromMembers(inputShape.members().toList())
            else -> {
                val documentMembers =
                    requestBindings.filter { it.value.location == HttpBinding.Location.DOCUMENT }
                        .keys.map { inputShape.expectMember(it) }
                return XmlMemberIndex.fromMembers(documentMembers)
            }
        }
    }

    private fun StructureShape.errorXmlMembers(): XmlMemberIndex {
        val responseBindings = index.getResponseBindings(this)

        // HTTP trait bound protocols, such as RestXml, need to restrict the members to DOCUMENT members,
        // while protocols like AwsQuery do not support HTTP traits, and thus, all members are used.
        return when (responseBindings.isEmpty()) {
            true -> XmlMemberIndex.fromMembers(members().toList())
            else -> {
                val documentMembers =
                    responseBindings.filter { it.value.location == HttpBinding.Location.DOCUMENT }
                        .keys.map { this.expectMember(it) }
                return XmlMemberIndex.fromMembers(documentMembers)
            }
        }
    }

    private fun StructureShape.xmlMembers(): XmlMemberIndex {
        return XmlMemberIndex.fromMembers(this.members().toList())
    }
}
