/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols.parse

import software.amazon.smithy.aws.traits.customizations.S3UnwrappedXmlOutputTrait
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.Model
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
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.conditionalBlock
import software.amazon.smithy.rust.codegen.core.rustlang.escape
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.rustlang.withBlockTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.renderUnknownVariant
import software.amazon.smithy.rust.codegen.core.smithy.generators.setterName
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.smithy.isRustBoxed
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolFunctions
import software.amazon.smithy.rust.codegen.core.smithy.protocols.XmlMemberIndex
import software.amazon.smithy.rust.codegen.core.smithy.protocols.XmlNameIndex
import software.amazon.smithy.rust.codegen.core.util.PANIC
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.expectMember
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.isTargetUnit
import software.amazon.smithy.rust.codegen.core.util.outputShape

// The string argument is the name of the XML ScopedDecoder to continue parsing from
typealias OperationInnerWriteable = RustWriter.(String) -> Unit

data class OperationWrapperContext(
    val shape: OperationShape,
    val outputShapeName: String,
    val xmlDecodeErrorType: RuntimeType,
    val model: Model,
)

class XmlBindingTraitParserGenerator(
    codegenContext: CodegenContext,
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

    private val symbolProvider = codegenContext.symbolProvider
    private val smithyXml = CargoDependency.smithyXml(codegenContext.runtimeConfig).toType()
    private val xmlDecodeError = smithyXml.resolve("decode::XmlDecodeError")

    private val scopedDecoder = smithyXml.resolve("decode::ScopedDecoder")
    private val runtimeConfig = codegenContext.runtimeConfig
    private val protocolFunctions = ProtocolFunctions(codegenContext)
    private val codegenTarget = codegenContext.target
    private val builderInstantiator = codegenContext.builderInstantiator()

    // The symbols we want all the time
    private val codegenScope =
        arrayOf(
            "Blob" to RuntimeType.blob(runtimeConfig),
            "Document" to smithyXml.resolve("decode::Document"),
            "XmlDecodeError" to xmlDecodeError,
            "next_start_element" to smithyXml.resolve("decode::next_start_element"),
            "try_data" to smithyXml.resolve("decode::try_data"),
            "ScopedDecoder" to scopedDecoder,
            "aws_smithy_types" to CargoDependency.smithyTypes(runtimeConfig).toType(),
            *RuntimeType.preludeScope,
        )
    private val model = codegenContext.model
    private val index = HttpBindingIndex.of(model)
    private val xmlIndex = XmlNameIndex.of(model)
    private val target = codegenContext.target

    /**
     * Generate a parse function for a given targeted as a payload.
     * Entry point for payload-based parsing.
     * Roughly:
     * ```rust
     * fn parse_my_struct(input: &[u8]) -> Result<MyStruct, XmlDecodeError> {
     *      ...
     * }
     * ```
     */
    override fun payloadParser(member: MemberShape): RuntimeType {
        val shape = model.expectShape(member.target)
        check(shape is UnionShape || shape is StructureShape) {
            "payload parser should only be used on structures & unions"
        }
        return protocolFunctions.deserializeFn(member) { fnName ->
            rustBlock(
                "pub fn $fnName(inp: &[u8]) -> std::result::Result<#1T, #2T>",
                symbolProvider.toSymbol(shape),
                xmlDecodeError,
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
                        return Err(#{XmlDecodeError}::custom(format!("invalid root, expected $shapeName got {start_el:?}")))
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
     * fn parse_some_operation(inp: &[u8], builder: my_operation::Builder) -> Result<my_operation::Builder, XmlDecodeError> {
     *   ...
     * }
     * ```
     */
    override fun operationParser(operationShape: OperationShape): RuntimeType? {
        val outputShape = operationShape.outputShape(model)
        val shapeName = xmlIndex.operationOutputShapeName(operationShape)
        val members = operationShape.operationXmlMembers()
        if (shapeName == null || !members.isNotEmpty()) {
            return null
        }
        return protocolFunctions.deserializeFn(operationShape) { fnName ->
            Attribute.AllowUnusedMut.render(this)
            rustBlock(
                "pub fn $fnName(inp: &[u8], mut builder: #1T) -> std::result::Result<#1T, #2T>",
                symbolProvider.symbolForBuilder(outputShape),
                xmlDecodeError,
            ) {
                rustTemplate(
                    """
                    let mut doc = #{Document}::try_from(inp)?;

                    ##[allow(unused_mut)]
                    let mut decoder = doc.root_element()?;
                    ##[allow(unused_variables)]
                    let start_el = decoder.start_el();
                    """,
                    *codegenScope,
                )
                val context = OperationWrapperContext(operationShape, shapeName, xmlDecodeError, model)
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
        return protocolFunctions.deserializeFn(errorShape, fnNameSuffix = "xml_err") { fnName ->
            Attribute.AllowUnusedMut.render(this)
            rustBlock(
                "pub fn $fnName(inp: &[u8], mut builder: #1T) -> std::result::Result<#1T, #2T>",
                symbolProvider.symbolForBuilder(errorShape),
                xmlDecodeError,
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
        val shapeName = xmlIndex.operationInputShapeName(operationShape)
        val members = operationShape.serverInputXmlMembers()
        if (shapeName == null || !members.isNotEmpty()) {
            return null
        }
        return protocolFunctions.deserializeFn(operationShape) { fnName ->
            Attribute.AllowUnusedMut.render(this)
            rustBlock(
                "pub fn $fnName(inp: &[u8], mut builder: #1T) -> std::result::Result<#1T, #2T>",
                symbolProvider.symbolForBuilder(inputShape),
                xmlDecodeError,
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
                val context = OperationWrapperContext(operationShape, shapeName, xmlDecodeError, model)
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
            rustTemplate(
                "_ => return Err(#{XmlDecodeError}::custom(\"expected ${member.xmlName()} tag\"))",
                *codegenScope,
            )
        }
    }

    /**
     * Update a structure builder based on the [members], specifying where to find each member (document vs. attributes)
     */
    private fun RustWriter.parseStructureInner(
        members: XmlMemberIndex,
        builder: String,
        outerCtx: Ctx,
    ) {
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
    private fun RustWriter.parseLoop(
        ctx: Ctx,
        ignoreUnexpected: Boolean = true,
        inner: RustWriter.(Ctx) -> Unit,
    ) {
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
    private fun RustWriter.parseMember(
        memberShape: MemberShape,
        ctx: Ctx,
        forceOptional: Boolean = false,
    ) {
        val target = model.expectShape(memberShape.target)
        val symbol = symbolProvider.toSymbol(memberShape)
        conditionalBlock("Some(", ")", forceOptional || symbol.isOptional()) {
            conditionalBlock("Box::new(", ")", symbol.isRustBoxed()) {
                when (target) {
                    is StringShape, is BooleanShape, is NumberShape, is TimestampShape, is BlobShape ->
                        parsePrimitiveInner(memberShape) {
                            rustTemplate("#{try_data}(&mut ${ctx.tag})?.as_ref()", *codegenScope)
                        }

                    is MapShape ->
                        if (memberShape.isFlattened()) {
                            parseFlatMap(target, ctx)
                        } else {
                            parseMap(target, ctx)
                        }

                    is CollectionShape ->
                        if (memberShape.isFlattened()) {
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

    private fun RustWriter.parseAttributeMember(
        memberShape: MemberShape,
        ctx: Ctx,
    ) {
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

    private fun RustWriter.parseUnion(
        shape: UnionShape,
        ctx: Ctx,
    ) {
        val symbol = symbolProvider.toSymbol(shape)
        val nestedParser =
            protocolFunctions.deserializeFn(shape) { fnName ->
                rustBlockTemplate(
                    "pub fn $fnName(decoder: &mut #{ScopedDecoder}) -> #{Result}<#{Shape}, #{XmlDecodeError}>",
                    *codegenScope, "Shape" to symbol,
                ) {
                    val members = shape.members()
                    rustTemplate("let mut base: Option<#{Shape}> = None;", *codegenScope, "Shape" to symbol)
                    parseLoop(Ctx(tag = "decoder", accum = null), ignoreUnexpected = false) { ctx ->
                        members.forEach { member ->
                            val variantName = symbolProvider.toMemberName(member)
                            case(member) {
                                if (member.isTargetUnit()) {
                                    rust("base = Some(#T::$variantName);", symbol)
                                } else {
                                    val current =
                                        """
                                        (match base.take() {
                                            None => None,
                                            Some(${format(symbol)}::$variantName(inner)) => Some(inner),
                                            Some(_) => return Err(#{XmlDecodeError}::custom("mixed variants"))
                                        })
                                        """
                                    withBlock("let tmp =", ";") {
                                        parseMember(member, ctx.copy(accum = current.trim()))
                                    }
                                    rust("base = Some(#T::$variantName(tmp));", symbol)
                                }
                            }
                        }
                        when (target.renderUnknownVariant()) {
                            true -> rust("_unknown => base = Some(#T::${UnionGenerator.UNKNOWN_VARIANT_NAME}),", symbol)
                            false ->
                                rustTemplate(
                                    """variant => return Err(#{XmlDecodeError}::custom(format!("unexpected union variant: {variant:?}")))""",
                                    *codegenScope,
                                )
                        }
                    }
                    rustTemplate(
                        """base.ok_or_else(||#{XmlDecodeError}::custom("expected union, got nothing"))""",
                        *codegenScope,
                    )
                }
            }
        rust("#T(&mut ${ctx.tag})", nestedParser)
    }

    /**
     * The match clause to check if the tag matches a given member
     */
    private fun RustWriter.case(
        member: MemberShape,
        inner: Writable,
    ) {
        rustBlock(
            "s if ${
                member.xmlName().matchExpression("s")
            } /* ${member.memberName} ${escape(member.id.toString())} */ => ",
        ) {
            inner()
        }
        rust(",")
    }

    private fun RustWriter.parseStructure(
        shape: StructureShape,
        ctx: Ctx,
    ) {
        val symbol = symbolProvider.toSymbol(shape)
        val nestedParser =
            protocolFunctions.deserializeFn(shape) { fnName ->
                Attribute.AllowNeedlessQuestionMark.render(this)
                rustBlockTemplate(
                    "pub fn $fnName(decoder: &mut #{ScopedDecoder}) -> #{Result}<#{Shape}, #{XmlDecodeError}>",
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
                    val builder =
                        builderInstantiator.finalizeBuilder(
                            "builder",
                            shape,
                            mapErr = {
                                rustTemplate(
                                    """|_|#{XmlDecodeError}::custom("missing field")""",
                                    *codegenScope,
                                )
                            },
                        )
                    rust("Ok(#T)", builder)
                }
            }
        rust("#T(&mut ${ctx.tag})", nestedParser)
    }

    private fun RustWriter.parseList(
        target: CollectionShape,
        ctx: Ctx,
    ) {
        val member = target.member
        val listParser =
            protocolFunctions.deserializeFn(target) { fnName ->
                rustBlockTemplate(
                    "pub fn $fnName(decoder: &mut #{ScopedDecoder}) -> #{Result}<#{List}, #{XmlDecodeError}>",
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

    private fun RustWriter.parseFlatList(
        target: CollectionShape,
        ctx: Ctx,
    ) {
        val list = safeName("list")
        withBlock("Result::<#T, #T>::Ok({", "})", symbolProvider.toSymbol(target), xmlDecodeError) {
            val accum = ctx.accum ?: throw CodegenException("Need accum to parse flat list")
            rustTemplate("""let mut $list = $accum.unwrap_or_default();""", *codegenScope)
            withBlock("$list.push(", ");") {
                parseMember(target.member, ctx)
            }
            rust(list)
        }
    }

    private fun RustWriter.parseMap(
        target: MapShape,
        ctx: Ctx,
    ) {
        val mapParser =
            protocolFunctions.deserializeFn(target) { fnName ->
                rustBlockTemplate(
                    "pub fn $fnName(decoder: &mut #{ScopedDecoder}) -> #{Result}<#{Map}, #{XmlDecodeError}>",
                    *codegenScope,
                    "Map" to symbolProvider.toSymbol(target),
                ) {
                    rust("let mut out = #T::new();", RuntimeType.HashMap)
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

    private fun RustWriter.parseFlatMap(
        target: MapShape,
        ctx: Ctx,
    ) {
        val map = safeName("map")
        val entryDecoder = mapEntryParser(target, ctx)
        withBlock("Result::<#T, #T>::Ok({", "})", symbolProvider.toSymbol(target), xmlDecodeError) {
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

    private fun mapEntryParser(
        target: MapShape,
        ctx: Ctx,
    ): RuntimeType {
        return protocolFunctions.deserializeFn(target, "entry") { fnName ->
            rustBlockTemplate(
                "pub fn $fnName(decoder: &mut #{ScopedDecoder}, out: &mut #{Map}) -> #{Result}<(), #{XmlDecodeError}>",
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
                    let k = k.ok_or_else(||#{XmlDecodeError}::custom("missing key map entry"))?;
                    let v = v.ok_or_else(||#{XmlDecodeError}::custom("missing value map entry"))?;
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
    private fun RustWriter.parsePrimitiveInner(
        member: MemberShape,
        provider: Writable,
    ) {
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
                        """.map_err(|_|#{XmlDecodeError}::custom("expected ${escape(shape.toString())}"))""",
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
                val timestampFormatType =
                    RuntimeType.parseTimestampFormat(codegenTarget, runtimeConfig, timestampFormat)
                withBlock("#T::from_str(", ")", RuntimeType.dateTime(runtimeConfig)) {
                    provider()
                    rust(", #T", timestampFormatType)
                }
                rustTemplate(
                    """.map_err(|_|#{XmlDecodeError}::custom("expected ${escape(shape.toString())}"))""",
                    *codegenScope,
                )
            }

            is BlobShape -> {
                withBlock("#T(", ")", RuntimeType.base64Decode(runtimeConfig)) {
                    provider()
                }
                rustTemplate(
                    """.map_err(|err|#{XmlDecodeError}::custom(format!("invalid base64: {err:?}"))).map(#{Blob}::new)""",
                    *codegenScope,
                )
            }

            else -> PANIC("unexpected shape: $shape")
        }
    }

    private fun RustWriter.parseStringInner(
        shape: StringShape,
        provider: Writable,
    ) {
        withBlock("Result::<#T, #T>::Ok(", ")", symbolProvider.toSymbol(shape), xmlDecodeError) {
            if (shape.hasTrait<EnumTrait>()) {
                val enumSymbol = symbolProvider.toSymbol(shape)
                if (convertsToEnumInServer(shape)) {
                    withBlock("#T::try_from(", ")", enumSymbol) {
                        provider()
                    }
                    rustTemplate(
                        """.map_err(|e| #{XmlDecodeError}::custom(format!("unknown variant {e}")))?""",
                        *codegenScope,
                    )
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

    private fun convertsToEnumInServer(shape: StringShape) =
        target == CodegenTarget.SERVER && shape.hasTrait<EnumTrait>()

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
