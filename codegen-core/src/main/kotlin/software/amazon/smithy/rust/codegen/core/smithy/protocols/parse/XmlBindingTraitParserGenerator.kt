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
import software.amazon.smithy.model.shapes.BigDecimalShape
import software.amazon.smithy.model.shapes.BigIntegerShape
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
import software.amazon.smithy.model.traits.XmlFlattenedTrait
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.conditionalBlock
import software.amazon.smithy.rust.codegen.core.rustlang.escape
import software.amazon.smithy.rust.codegen.core.rustlang.render
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.rustlang.withBlockTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.SimpleShapes
import software.amazon.smithy.rust.codegen.core.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.renderUnknownVariant
import software.amazon.smithy.rust.codegen.core.smithy.generators.setterName
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.smithy.isRustBoxed
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolFunctions
import software.amazon.smithy.rust.codegen.core.smithy.protocols.XmlMemberIndex
import software.amazon.smithy.rust.codegen.core.smithy.protocols.XmlNameIndex
import software.amazon.smithy.rust.codegen.core.smithy.rustType
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
    /**
     * Maps a shape onto the symbol that the parser should produce for it. On the client this is the
     * shape's regular Rust type; on the server, for shapes that reach a constrained shape, this is
     * the unconstrained wrapper newtype so the parsed value can flow into the builder as
     * `MaybeConstrained::Unconstrained(...)` and have its constraint checks deferred to build time.
     * Mirrors the same hook in [JsonParserGenerator]. Defaults to the regular symbol so positional
     * (trailing-lambda) constructions keep working.
     */
    private val returnSymbolToParse: (Shape) -> ReturnSymbolToParse = { shape ->
        ReturnSymbolToParse(codegenContext.symbolProvider.toSymbol(shape), false)
    },
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

    // Maximum shape-tree recursion depth permitted by generated deserializers.
    // Guards against stack overflow from deeply-nested payloads targeting recursive shapes.
    // Matches serde_json's default of 128.
    private val maxDepth: Int = 128
    private val depthErrorMessage: String = "maximum nesting depth exceeded"

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
        // The inner `parseStructure` / `parseUnion` resolve their return type through
        // `returnSymbolToParse`, which on server codegen for constrained-reachable shapes is the
        // unconstrained builder rather than the constrained final shape. The wrapper that calls
        // this helper (`shape_<op>_input.rs`) is also generated through `returnSymbolToParse`, so
        // the two need to agree — otherwise the outer signature expects `Option<Builder>` while
        // this helper still claims to return the constrained final shape.
        val returnSymbol = returnSymbolToParse(shape).symbol
        return protocolFunctions.deserializeFn(member) { fnName ->
            rustBlock(
                "pub fn $fnName(inp: &[u8]) -> std::result::Result<#1T, #2T>",
                returnSymbol,
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
                    ##[allow(unused_variables)]
                    let depth = 0u32;
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
                    ##[allow(unused_variables)]
                    let depth = 0u32;
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
                        ##[allow(unused_variables)]
                        let depth = 0u32;
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
                    ##[allow(unused_variables)]
                    let depth = 0u32;
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
            // Route through the builder's setter, mirroring the data-member path. The setter knows
            // how to lift `Inner -> MaybeConstrained<T>` when the target reaches a constrained
            // shape (via the `From<Inner>` impls emitted by the constrained generators) and is a
            // no-op for unconstrained scalars. The setter's signature differs between optional and
            // required members though: optional takes `Option<impl Into<…>>` and accepts the
            // parsed `Option<Inner>` directly, while required takes `impl Into<…>` and rejects
            // `Option`. For the required case, only invoke the setter when the attribute is
            // present; absent attributes leave the builder's field at `None` so `build()` can
            // report the missing-required member like every other path.
            val isOptional = symbolProvider.toSymbol(member).isOptional()
            if (isOptional) {
                rust("$builder = $builder.${member.setterName()}($temp);")
            } else {
                rust(
                    """
                    if let Some(__attrib_value) = $temp {
                        $builder = $builder.${member.setterName()}(__attrib_value);
                    }
                    """,
                )
            }
        }
        // No need to generate a parse loop if there are no non-attribute members
        if (members.dataMembers.isEmpty()) {
            return
        }
        parseLoop(outerCtx) { ctx ->
            members.dataMembers.forEach { member ->
                case(member) {
                    val temp = safeName()
                    // The setter the generated builder emits for a *required* member takes the bare
                    // value (`impl Into<T>`), not `Option<T>`: required-ness is enforced when `build()`
                    // runs, not at the setter site. Wrapping the parsed value in `Some(...)`
                    // unconditionally produces `Option<T>` and rustc rejects the call. Respect the
                    // member's natural optionality so optional members still flow as `Some(value)`
                    // and required members flow as the bare value.
                    val memberIsOptional = symbolProvider.toSymbol(member).isOptional()
                    withBlock("let $temp =", ";") {
                        parseMember(
                            member,
                            ctx.copy(accum = "$builder.${symbolProvider.toMemberName(member)}.take()"),
                            forceOptional = memberIsOptional,
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
                    is BigIntegerShape, is BigDecimalShape -> {
                        parsePrimitiveInner(memberShape) {
                            rustTemplate("#{try_data}(&mut ${ctx.tag})?.as_ref()", *codegenScope)
                        }
                    }

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
        // For server codegen of constrained-reachable unions, `returnSymbolToParse` resolves to the
        // `<Name>Unconstrained` enum emitted by [UnconstrainedUnionGenerator]. Its variants share
        // their identifiers with the user-facing union's variants but each holds the unconstrained
        // inner type the per-member parsers actually produce — so by constructing variants on the
        // unconstrained enum here, the builder's `From<<Name>Unconstrained> for MaybeConstrained`
        // impl handles the lift. For client codegen and for unions that don't reach a constrained
        // shape, this stays at the regular union symbol.
        val returnSymbol = returnSymbolToParse(shape).symbol
        val nestedParser =
            protocolFunctions.deserializeFn(shape) { fnName ->
                rustBlockTemplate(
                    "pub fn $fnName(decoder: &mut #{ScopedDecoder}, depth: u32) -> #{Result}<#{Shape}, #{XmlDecodeError}>",
                    *codegenScope, "Shape" to returnSymbol,
                ) {
                    rustTemplate(
                        """
                        if depth >= ${maxDepth}u32 {
                            return Err(#{XmlDecodeError}::custom(${depthErrorMessage.dq()}));
                        }
                        """,
                        *codegenScope,
                    )
                    val members = shape.members()
                    rustTemplate("let mut base: Option<#{Shape}> = None;", *codegenScope, "Shape" to returnSymbol)
                    parseLoop(Ctx(tag = "decoder", accum = null), ignoreUnexpected = false) { ctx ->
                        members.forEach { member ->
                            val variantName = symbolProvider.toMemberName(member)
                            case(member) {
                                if (member.isTargetUnit()) {
                                    rust("base = Some(#T::$variantName);", returnSymbol)
                                } else {
                                    val current =
                                        """
                                        (match base.take() {
                                            None => None,
                                            Some(${format(returnSymbol)}::$variantName(inner)) => Some(inner),
                                            Some(_) => return Err(#{XmlDecodeError}::custom("mixed variants"))
                                        })
                                        """
                                    withBlock("let tmp =", ";") {
                                        parseMember(member, ctx.copy(accum = current.trim()))
                                    }
                                    rust("base = Some(#T::$variantName(tmp));", returnSymbol)
                                }
                            }
                        }
                        when (target.renderUnknownVariant()) {
                            true -> rust("_unknown => base = Some(#T::${UnionGenerator.UNKNOWN_VARIANT_NAME}),", returnSymbol)
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
        rust("#T(&mut ${ctx.tag}, depth + 1)", nestedParser)
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
        // The helper instantiates the constrained shape's builder either way — that's where the
        // member setters live — but its declared return type goes through `returnSymbolToParse`. For
        // server codegen with a constrained-reachable structure that resolves to the builder symbol
        // itself; the input builder's setter for the parent member then accepts the builder via
        // `From<Builder> for MaybeConstrained<Constrained>`. Client codegen keeps the structure
        // symbol unchanged.
        val symbol = symbolProvider.toSymbol(shape)
        val returnSymbol = returnSymbolToParse(shape).symbol
        val nestedParser =
            protocolFunctions.deserializeFn(shape) { fnName ->
                Attribute.AllowNeedlessQuestionMark.render(this)
                rustBlockTemplate(
                    "pub fn $fnName(decoder: &mut #{ScopedDecoder}, depth: u32) -> #{Result}<#{Shape}, #{XmlDecodeError}>",
                    *codegenScope, "Shape" to returnSymbol,
                ) {
                    rustTemplate(
                        """
                        if depth >= ${maxDepth}u32 {
                            return Err(#{XmlDecodeError}::custom(${depthErrorMessage.dq()}));
                        }
                        """,
                        *codegenScope,
                    )
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
        rust("#T(&mut ${ctx.tag}, depth + 1)", nestedParser)
    }

    private fun RustWriter.parseList(
        target: CollectionShape,
        ctx: Ctx,
    ) {
        val member = target.member
        val (returnSymbol, returnUnconstrained) = returnSymbolToParse(target)
        val listParser =
            protocolFunctions.deserializeFn(target) { fnName ->
                rustBlockTemplate(
                    "pub fn $fnName(decoder: &mut #{ScopedDecoder}, depth: u32) -> #{Result}<#{List}, #{XmlDecodeError}>",
                    *codegenScope,
                    "List" to returnSymbol,
                ) {
                    rustTemplate(
                        """
                        if depth >= ${maxDepth}u32 {
                            return Err(#{XmlDecodeError}::custom(${depthErrorMessage.dq()}));
                        }
                        """,
                        *codegenScope,
                    )
                    rust("let mut out = std::vec::Vec::new();")
                    parseLoop(Ctx(tag = "decoder", accum = null)) { ctx ->
                        case(member) {
                            withBlock("out.push(", ");") {
                                parseMember(member, ctx)
                            }
                        }
                    }
                    // For server codegen the helper's return type is the unconstrained wrapper newtype that
                    // accepts a `Vec` directly via its tuple constructor; for client codegen it is just the
                    // `Vec`, so emitting it unwrapped lines up with both.
                    if (returnUnconstrained) {
                        rust("Ok(#T(out))", returnSymbol)
                    } else {
                        rust("Ok(out)")
                    }
                }
            }
        rust("#T(&mut ${ctx.tag}, depth + 1)", listParser)
    }

    private fun RustWriter.parseFlatList(
        target: CollectionShape,
        ctx: Ctx,
    ) {
        val list = safeName("list")
        val (returnSymbol, returnUnconstrained) = returnSymbolToParse(target)
        withBlock("Result::<#T, #T>::Ok({", "})", returnSymbol, xmlDecodeError) {
            val accum = ctx.accum ?: throw CodegenException("Need accum to parse flat list")
            if (returnUnconstrained) {
                // The builder field for a constrained-reachable list is
                // `Option<MaybeConstrained<Wrapper>>`. `MaybeConstrained` is an enum (not a tuple
                // newtype, and it doesn't implement `Default`), so the unconstrained branch can't
                // use `.unwrap_or_default()` / `.0`. Pattern-match instead so that the in-progress
                // `Unconstrained` accumulator's inner `Vec` is reused and any other state — including
                // the first parsed element being absent — falls back to an empty `Vec`. We don't
                // expect to see a fully-`Constrained` value at parse time (the parser only ever
                // writes `Unconstrained`), so that arm panics if reached, mirroring how the existing
                // server-builder codegen handles the same invariant elsewhere.
                rustTemplate(
                    """
                    let mut $list = match $accum {
                        Some(#{MaybeConstrained}::Unconstrained(unconstrained)) => unconstrained.0,
                        Some(#{MaybeConstrained}::Constrained(_)) => unreachable!("server-side XML parser never writes a constrained value back to a flat-list accumulator; please file a bug report under https://github.com/smithy-lang/smithy-rs/issues"),
                        None => #{Vec}::new(),
                    };
                    """,
                    *codegenScope,
                    "MaybeConstrained" to RuntimeType.MaybeConstrained,
                    "Vec" to RuntimeType.Vec,
                )
            } else {
                rustTemplate("""let mut $list = $accum.unwrap_or_default();""", *codegenScope)
            }
            withBlock("$list.push(", ");") {
                parseMember(target.member, ctx)
            }
            if (returnUnconstrained) {
                rust("#T($list)", returnSymbol)
            } else {
                rust(list)
            }
        }
    }

    private fun RustWriter.parseMap(
        target: MapShape,
        ctx: Ctx,
    ) {
        val (returnSymbol, returnUnconstrained) = returnSymbolToParse(target)
        val mapParser =
            protocolFunctions.deserializeFn(target) { fnName ->
                rustBlockTemplate(
                    "pub fn $fnName(decoder: &mut #{ScopedDecoder}, depth: u32) -> #{Result}<#{Map}, #{XmlDecodeError}>",
                    *codegenScope,
                    "Map" to returnSymbol,
                ) {
                    rustTemplate(
                        """
                        if depth >= ${maxDepth}u32 {
                            return Err(#{XmlDecodeError}::custom(${depthErrorMessage.dq()}));
                        }
                        """,
                        *codegenScope,
                    )
                    rust("let mut out = #T::new();", RuntimeType.HashMap)
                    parseLoop(Ctx(tag = "decoder", accum = null)) { ctx ->
                        rustBlock("s if ${XmlName("entry").matchExpression("s")} => ") {
                            rust("#T(&mut ${ctx.tag}, &mut out, depth)?;", mapEntryParser(target, ctx))
                        }
                    }
                    if (returnUnconstrained) {
                        rust("Ok(#T(out))", returnSymbol)
                    } else {
                        rust("Ok(out)")
                    }
                }
            }
        rust("#T(&mut ${ctx.tag}, depth + 1)", mapParser)
    }

    private fun RustWriter.parseFlatMap(
        target: MapShape,
        ctx: Ctx,
    ) {
        val map = safeName("map")
        val entryDecoder = mapEntryParser(target, ctx)
        val (returnSymbol, returnUnconstrained) = returnSymbolToParse(target)
        withBlock("Result::<#T, #T>::Ok({", "})", returnSymbol, xmlDecodeError) {
            val accum = ctx.accum ?: throw CodegenException("need accum to parse flat map")
            if (returnUnconstrained) {
                // See `parseFlatList` for the matching analysis: the builder field is
                // `Option<MaybeConstrained<Wrapper>>` and `MaybeConstrained` is an enum that does
                // not implement `Default`, so the unconstrained-branch can't use
                // `.unwrap_or_default()` / `.0`.
                rustTemplate(
                    """
                    let mut $map = match $accum {
                        Some(#{MaybeConstrained}::Unconstrained(unconstrained)) => unconstrained.0,
                        Some(#{MaybeConstrained}::Constrained(_)) => unreachable!("server-side XML parser never writes a constrained value back to a flat-map accumulator; please file a bug report under https://github.com/smithy-lang/smithy-rs/issues"),
                        None => #{HashMap}::new(),
                    };
                    #{decoder}(&mut tag, &mut $map, depth)?;
                    """,
                    *codegenScope,
                    "MaybeConstrained" to RuntimeType.MaybeConstrained,
                    "HashMap" to RuntimeType.HashMap,
                    "decoder" to entryDecoder,
                )
                rust("#T($map)", returnSymbol)
            } else {
                rustTemplate(
                    """
                    let mut $map = $accum.unwrap_or_default();
                    #{decoder}(&mut tag, &mut $map, depth)?;
                    $map
                    """,
                    *codegenScope,
                    "decoder" to entryDecoder,
                )
            }
        }
    }

    private fun mapEntryParser(
        target: MapShape,
        ctx: Ctx,
    ): RuntimeType {
        // `parseMap` / `parseFlatMap` keep their accumulator as a raw `HashMap` (even when the map's
        // own return symbol is the constrained wrapper, the wrapper is only constructed at the end of
        // those functions), so this helper takes a `&mut HashMap<...>` rather than the wrapper —
        // otherwise `.insert(...)` would target a tuple newtype that has no such method. The key and
        // value types are resolved through `returnSymbolToParse` so that constrained-reachable
        // members come through as their unconstrained equivalents.
        return protocolFunctions.deserializeFn(target, "entry") { fnName ->
            val keyShape = model.expectShape(target.key.target)
            val valueShape = model.expectShape(target.value.target)
            val keyType = returnSymbolToParse(keyShape).symbol
            val valueType = returnSymbolToParse(valueShape).symbol
            rustBlockTemplate(
                "pub fn $fnName(decoder: &mut #{ScopedDecoder}, out: &mut #{HashMap}<#{Key}, #{Value}>, depth: u32) -> #{Result}<(), #{XmlDecodeError}>",
                *codegenScope,
                "HashMap" to RuntimeType.HashMap,
                "Key" to keyType,
                "Value" to valueType,
            ) {
                rustTemplate(
                    """
                    if depth >= ${maxDepth}u32 {
                        return Err(#{XmlDecodeError}::custom(${depthErrorMessage.dq()}));
                    }
                    """,
                    *codegenScope,
                )
                rust("let mut k: Option<#T> = None;", keyType)
                rust("let mut v: Option<#T> = None;", valueType)
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

            is BigIntegerShape -> {
                rustBlock("") {
                    rustTemplate(
                        "<#{BigInteger} as ::std::str::FromStr>::from_str(",
                        "BigInteger" to RuntimeType.bigInteger(runtimeConfig),
                        *codegenScope,
                    )
                    provider()
                    rustTemplate(
                        ").map_err(|e| #{XmlDecodeError}::custom(format!(\"invalid BigInteger: {}\", e)))",
                        *codegenScope,
                    )
                }
            }

            is BigDecimalShape -> {
                rustBlock("") {
                    rustTemplate(
                        "<#{BigDecimal} as ::std::str::FromStr>::from_str(",
                        "BigDecimal" to RuntimeType.bigDecimal(runtimeConfig),
                        *codegenScope,
                    )
                    provider()
                    rustTemplate(
                        ").map_err(|e| #{XmlDecodeError}::custom(format!(\"invalid BigDecimal: {}\", e)))",
                        *codegenScope,
                    )
                }
            }

            is NumberShape, is BooleanShape -> {
                // The symbol may resolve to a constrained wrapper newtype on the server side when
                // `@range` (numbers) is applied to the shape, but `aws_smithy_types::primitive::Parse`
                // is only implemented on the canonical primitive (`i32`, `i64`, `bool`, …). Parse via
                // the underlying primitive and let the builder run the constraint check — the input
                // builder accepts the unconstrained value through `MaybeConstrained::Unconstrained` and
                // unconstrained-target structures (e.g. output bodies) hold the primitive directly.
                val parseTargetType =
                    if (symbolProvider.toSymbol(shape).rustType() is RustType.Opaque) {
                        SimpleShapes[shape::class]?.render()
                            ?: throw CodegenException("unsupported NumberShape kind for primitive parsing: $shape")
                    } else {
                        null
                    }
                rustBlock("") {
                    withBlockTemplate(
                        if (parseTargetType != null) {
                            "<$parseTargetType as #{aws_smithy_types}::primitive::Parse>::parse_smithy_primitive("
                        } else {
                            "<#{shape} as #{aws_smithy_types}::primitive::Parse>::parse_smithy_primitive("
                        },
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
        val symbol = symbolProvider.toSymbol(shape)
        // Any opaque string shape on the server — `@pattern` / `@length` newtypes and closed enums alike —
        // wraps a `String` whose only `String -> T` conversion is `TryFrom`. Producing the wrapper here
        // would force the parser to enforce constraints at decode time and would not type-match the
        // builder's `MaybeConstrained::Unconstrained(String)` lift. Emit the raw `String` instead and
        // defer the constraint check to the builder. Client open enums (which implement `From<&str>`)
        // still resolve to the enum type directly.
        val isConstrainedOnServer = target == CodegenTarget.SERVER && symbol.rustType() is RustType.Opaque
        val resultType: Any = if (isConstrainedOnServer) RuntimeType.String else symbol
        withBlock("Result::<#T, #T>::Ok(", ")", resultType, xmlDecodeError) {
            if (shape.hasTrait<EnumTrait>() && !isConstrainedOnServer) {
                val enumSymbol = symbolProvider.toSymbol(shape)
                withBlock("#T::from(", ")", enumSymbol) {
                    provider()
                }
            } else {
                provider()
                // If it's already `Cow::Owned` then `.into()` is free (as opposed to using `to_string()`).
                rust(".into()")
            }
        }
    }

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
