/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.node.ArrayNode
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.NullNode
import software.amazon.smithy.model.node.NumberNode
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.node.StringNode
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.BooleanShape
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.DocumentShape
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.NumberShape
import software.amazon.smithy.model.shapes.SetShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.TimestampShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.model.traits.HttpHeaderTrait
import software.amazon.smithy.model.traits.HttpPayloadTrait
import software.amazon.smithy.model.traits.HttpPrefixHeadersTrait
import software.amazon.smithy.model.traits.StreamingTrait
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.conditionalBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.escape
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.customize.NamedCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.Section
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.expectMember
import software.amazon.smithy.rust.codegen.core.util.expectTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.isTargetUnit
import software.amazon.smithy.rust.codegen.core.util.letIf
import java.math.BigDecimal

/**
 * Class describing an instantiator section that can be used in a customization.
 */
sealed class InstantiatorSection(name: String) : Section(name) {
    data class AfterInstantiatingValue(val shape: Shape) : InstantiatorSection("AfterInstantiatingValue")
}

/**
 * Customization for the instantiator.
 */
typealias InstantiatorCustomization = NamedCustomization<InstantiatorSection>

/**
 * Instantiator generates code to instantiate a given shape given a `Node` representing the value.
 *
 * This is only used during protocol test generation.
 */
open class Instantiator(
    private val symbolProvider: RustSymbolProvider,
    private val model: Model,
    private val runtimeConfig: RuntimeConfig,
    /** Behavior of the builder type used for structure shapes. */
    private val builderKindBehavior: BuilderKindBehavior,
    /**
     * A function that given a symbol for an enum shape and a string, returns a writable to instantiate the enum with
     * the string value.
     **/
    private val enumFromStringFn: (Symbol, String) -> Writable,
    /** Fill out required fields with a default value. **/
    private val defaultsForRequiredFields: Boolean = false,
    private val customizations: List<InstantiatorCustomization> = listOf(),
) {
    data class Ctx(
        // The `http` crate requires that headers be lowercase, but Smithy protocol tests
        // contain headers with uppercase keys.
        val lowercaseMapKeys: Boolean = false,
    )

    /**
     * Client and server structures have different builder types. `Instantiator` needs to know how the builder
     * type behaves to generate code for it.
     */
    interface BuilderKindBehavior {
        fun hasFallibleBuilder(shape: StructureShape): Boolean

        // Client structure builders have two kinds of setters: one that always takes in `Option<T>`, and one that takes
        // in the structure field's type. The latter's method name is the field's name, whereas the former is prefixed
        // with `set_`. Client instantiators call the `set_*` builder setters.
        fun setterName(memberShape: MemberShape): String
        fun doesSetterTakeInOption(memberShape: MemberShape): Boolean
    }

    fun generate(shape: Shape, data: Node, headers: Map<String, String> = mapOf(), ctx: Ctx = Ctx()) = writable {
        render(this, shape, data, headers, ctx)
    }

    fun render(writer: RustWriter, shape: Shape, data: Node, headers: Map<String, String> = mapOf(), ctx: Ctx = Ctx()) {
        when (shape) {
            // Compound Shapes
            is StructureShape -> renderStructure(writer, shape, data as ObjectNode, headers, ctx)
            is UnionShape -> renderUnion(writer, shape, data as ObjectNode, ctx)

            // Collections
            is ListShape -> renderList(writer, shape, data as ArrayNode, ctx)
            is MapShape -> renderMap(writer, shape, data as ObjectNode, ctx)
            is SetShape -> renderSet(writer, shape, data as ArrayNode, ctx)

            // Members, supporting potentially optional members
            is MemberShape -> renderMember(writer, shape, data, ctx)

            // Wrapped Shapes
            is TimestampShape -> {
                val node = (data as NumberNode)
                val num = BigDecimal(node.toString())
                val wholePart = num.toInt()
                val fractionalPart = num.remainder(BigDecimal.ONE)
                writer.rust(
                    "#T::from_fractional_secs($wholePart, ${fractionalPart}_f64)",
                    RuntimeType.dateTime(runtimeConfig),
                )
            }

            /**
             * ```rust
             * Blob::new("arg")
             * ```
             */
            is BlobShape -> if (shape.hasTrait<StreamingTrait>()) {
                writer.rust(
                    "#T::from_static(b${(data as StringNode).value.dq()})",
                    RuntimeType.byteStream(runtimeConfig),
                )
            } else {
                writer.rust(
                    "#T::new(${(data as StringNode).value.dq()})",
                    RuntimeType.blob(runtimeConfig),
                )
            }

            // Simple Shapes
            is StringShape -> renderString(writer, shape, data as StringNode)
            is NumberShape -> when (data) {
                is StringNode -> {
                    val numberSymbol = symbolProvider.toSymbol(shape)
                    // support Smithy custom values, such as Infinity
                    writer.rust(
                        """<#T as #T>::parse_smithy_primitive(${data.value.dq()}).expect("invalid string for number")""",
                        numberSymbol,
                        RuntimeType.smithyTypes(runtimeConfig).resolve("primitive::Parse"),
                    )
                }

                is NumberNode -> writer.write(data.value)
            }

            is BooleanShape -> writer.rust(data.asBooleanNode().get().toString())
            is DocumentShape -> writer.rustBlock("") {
                val smithyJson = CargoDependency.smithyJson(runtimeConfig).toType()
                rustTemplate(
                    """
                    let json_bytes = br##"${Node.prettyPrintJson(data)}"##;
                    let mut tokens = #{json_token_iter}(json_bytes).peekable();
                    #{expect_document}(&mut tokens).expect("well formed json")
                    """,
                    "expect_document" to smithyJson.resolve("deserialize::token::expect_document"),
                    "json_token_iter" to smithyJson.resolve("deserialize::json_token_iter"),
                )
            }

            else -> writer.writeWithNoFormatting("todo!() /* $shape $data */")
        }
    }

    /**
     * If the shape is optional: `Some(inner)` or `None`.
     * Otherwise: `inner`.
     */
    private fun renderMember(writer: RustWriter, memberShape: MemberShape, data: Node, ctx: Ctx) {
        val targetShape = model.expectShape(memberShape.target)
        val symbol = symbolProvider.toSymbol(memberShape)
        if (data is NullNode) {
            check(symbol.isOptional()) {
                "A null node was provided for $memberShape but the symbol was not optional. This is invalid input data."
            }
            writer.rustTemplate("#{None}", *preludeScope)
        } else {
            // Structure builder setters for structure shape members _always_ take in `Option<T>`.
            // Other aggregate shapes' members are optional only when their symbol is.
            writer.conditionalBlockTemplate(
                "#{Some}(",
                ")",
                // The conditions are not commutative: note client builders always take in `Option<T>`.
                conditional = symbol.isOptional() ||
                    (model.expectShape(memberShape.container) is StructureShape && builderKindBehavior.doesSetterTakeInOption(memberShape)),
                *preludeScope,
            ) {
                writer.conditionalBlockTemplate(
                    "#{Box}::new(",
                    ")",
                    conditional = symbol.rustType().stripOuter<RustType.Option>() is RustType.Box,
                    *preludeScope,
                ) {
                    render(
                        this,
                        targetShape,
                        data,
                        mapOf(),
                        ctx.copy()
                            .letIf(memberShape.hasTrait<HttpPrefixHeadersTrait>()) {
                                it.copy(lowercaseMapKeys = true)
                            },
                    )
                }
            }
        }
    }

    private fun renderSet(writer: RustWriter, shape: SetShape, data: ArrayNode, ctx: Ctx) = renderList(writer, shape, data, ctx)

    /**
     * ```rust
     * {
     *      let mut ret = HashMap::new();
     *      ret.insert("k", ...);
     *      ret.insert("k2", ...);
     *      ret
     * }
     * ```
     */
    private fun renderMap(writer: RustWriter, shape: MapShape, data: ObjectNode, ctx: Ctx) {
        if (data.members.isEmpty()) {
            writer.rust("#T::new()", RuntimeType.HashMap)
        } else {
            writer.rustBlock("") {
                rust("let mut ret = #T::new();", RuntimeType.HashMap)
                for ((key, value) in data.members) {
                    withBlock("ret.insert(", ");") {
                        renderMember(this, shape.key, key, ctx)
                        when (ctx.lowercaseMapKeys) {
                            true -> rust(".to_ascii_lowercase(), ")
                            else -> rust(", ")
                        }
                        renderMember(this, shape.value, value, ctx)
                    }
                }
                rust("ret")
            }
        }
    }

    /**
     * ```rust
     * MyUnion::Variant(...)
     * ```
     */
    private fun renderUnion(writer: RustWriter, shape: UnionShape, data: ObjectNode, ctx: Ctx) {
        val unionSymbol = symbolProvider.toSymbol(shape)

        val variant = if (defaultsForRequiredFields && data.members.isEmpty()) {
            val (name, memberShape) = shape.allMembers.entries.first()
            val targetShape = model.expectShape(memberShape.target)
            Node.from(name) to fillDefaultValue(targetShape)
        } else {
            check(data.members.size == 1)
            val entry = data.members.iterator().next()
            entry.key to entry.value
        }

        val memberName = variant.first.value
        val member = shape.expectMember(memberName)
        writer.rust("#T::${symbolProvider.toMemberName(member)}", unionSymbol)
        // Unions should specify exactly one member.
        if (!member.isTargetUnit()) {
            writer.withBlock("(", ")") {
                renderMember(this, member, variant.second, ctx)
            }
        }
    }

    /**
     * ```rust
     * vec![..., ..., ...]
     * ```
     */
    private fun renderList(writer: RustWriter, shape: CollectionShape, data: ArrayNode, ctx: Ctx) {
        writer.withBlock("vec![", "]") {
            data.elements.forEach { v ->
                renderMember(this, shape.member, v, ctx)
                rust(",")
            }
        }
        for (customization in customizations) {
            customization.section(InstantiatorSection.AfterInstantiatingValue(shape))(writer)
        }
    }

    private fun renderString(writer: RustWriter, shape: StringShape, arg: StringNode) {
        val data = writer.escape(arg.value).dq()
        if (!shape.hasTrait<EnumTrait>()) {
            writer.rust("$data.to_owned()")
        } else {
            val enumSymbol = symbolProvider.toSymbol(shape)
            writer.rustTemplate("#{EnumFromStringFn:W}", "EnumFromStringFn" to enumFromStringFn(enumSymbol, data))
        }
    }

    /**
     * ```rust
     * MyStruct::builder().field_1("hello").field_2(5).build()
     * ```
     */
    private fun renderStructure(writer: RustWriter, shape: StructureShape, data: ObjectNode, headers: Map<String, String>, ctx: Ctx) {
        writer.rust("#T::builder()", symbolProvider.toSymbol(shape))

        renderStructureMembers(writer, shape, data, headers, ctx)

        writer.rust(".build()")
        if (builderKindBehavior.hasFallibleBuilder(shape)) {
            writer.rust(".unwrap()")
        }
    }

    protected fun renderStructureMembers(
        writer: RustWriter,
        shape: StructureShape,
        data: ObjectNode,
        headers: Map<String, String>,
        ctx: Ctx,
    ) {
        fun renderMemberHelper(memberShape: MemberShape, value: Node) {
            val setterName = builderKindBehavior.setterName(memberShape)
            writer.withBlock(".$setterName(", ")") {
                renderMember(this, memberShape, value, ctx)
            }
        }

        if (defaultsForRequiredFields) {
            shape.allMembers.entries
                .filter { (name, memberShape) ->
                    !symbolProvider.toSymbol(memberShape).isOptional() && !data.members.containsKey(Node.from(name))
                }
                .forEach { (_, memberShape) ->
                    renderMemberHelper(memberShape, fillDefaultValue(memberShape))
                }
        }

        if (data.isEmpty) {
            shape.allMembers.entries
                .filter { it.value.hasTrait<HttpHeaderTrait>() }
                .forEach { (_, value) ->
                    val trait = value.expectTrait<HttpHeaderTrait>().value
                    headers.get(trait)?.let { renderMemberHelper(value, Node.from(it)) }
                }
        }

        data.members.forEach { (key, value) ->
            val memberShape = shape.expectMember(key.value)
            renderMemberHelper(memberShape, value)
        }

        shape.allMembers.entries
            .firstOrNull {
                it.value.hasTrait<HttpPayloadTrait>() &&
                    !data.members.containsKey(Node.from(it.key)) &&
                    model.expectShape(it.value.target) is StructureShape
            }
            ?.let {
                renderMemberHelper(it.value, fillDefaultValue(model.expectShape(it.value.target)))
            }
    }

    /**
     * Returns a default value for a shape.
     *
     * Warning: this method does not take into account any constraint traits attached to the shape.
     */
    private fun fillDefaultValue(shape: Shape): Node = when (shape) {
        is MemberShape -> fillDefaultValue(model.expectShape(shape.target))

        // Aggregate shapes.
        is StructureShape -> Node.objectNode()
        is UnionShape -> Node.objectNode()
        is CollectionShape -> Node.arrayNode()
        is MapShape -> Node.objectNode()

        // Simple Shapes
        is TimestampShape -> Node.from(0) // Number node for timestamp
        is BlobShape -> Node.from("") // String node for bytes
        is StringShape -> Node.from("")
        is NumberShape -> Node.from(0)
        is BooleanShape -> Node.from(false)
        is DocumentShape -> Node.objectNode()
        else -> throw CodegenException("Unrecognized shape `$shape`")
    }
}
