/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.codegen.core.SymbolProvider
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
import software.amazon.smithy.model.traits.HttpPrefixHeadersTrait
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.conditionalBlock
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.rustlang.withBlock
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.isOptional
import software.amazon.smithy.rust.codegen.smithy.letIf
import software.amazon.smithy.rust.codegen.smithy.rustType
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.expectMember
import software.amazon.smithy.rust.codegen.util.toPascalCase

/**
 * Instantiator generates code to instantiate a given Shape given a `Node` representing the value
 *
 * This is primarily used during Protocol test generation
 */
class Instantiator(
    private val symbolProvider: SymbolProvider,
    private val model: Model,
    private val runtimeConfig: RuntimeConfig
) {

    // The Rust HTTP library lower cases headers but Smithy protocol tests
    // contain httpPrefix headers with uppercase keys
    data class Ctx(val lowercaseMapKeys: Boolean)

    fun render(writer: RustWriter, shape: Shape, arg: Node, ctx: Ctx = Ctx(false)) {
        when (shape) {
            // Compound Shapes
            is StructureShape -> renderStructure(writer, shape, arg as ObjectNode, ctx)
            is UnionShape -> renderUnion(writer, shape, arg as ObjectNode, ctx)

            // Collections
            is ListShape -> renderList(writer, shape, arg as ArrayNode, ctx)
            is MapShape -> renderMap(writer, shape, arg as ObjectNode, ctx)
            is SetShape -> renderSet(writer, shape, arg as ArrayNode, ctx)

            // Members, supporting potentially optional members
            is MemberShape -> renderMember(writer, shape, arg, ctx)

            // Wrapped Shapes
            is TimestampShape -> writer.write(
                "#T::from_epoch_seconds(${(arg as NumberNode).value})",
                RuntimeType.Instant(runtimeConfig)
            )

            /**
             * ```rust
             * Blob::new("arg")
             * ```
             */
            is BlobShape -> writer.write(
                "#T::new(${(arg as StringNode).value.dq()})",
                RuntimeType.Blob(runtimeConfig)
            )

            // Simple Shapes
            is StringShape -> renderString(writer, shape, arg as StringNode)
            is NumberShape -> writer.write(arg.asNumberNode().get())
            is BooleanShape -> writer.write(arg.asBooleanNode().get().toString())
            is DocumentShape -> {
                writer.rust(
                    """{
                    let as_json = #T! { ${Node.prettyPrintJson(arg)} };
                    #T::json_to_doc(as_json)
                }""",
                    RuntimeType.SerdeJson("json"), RuntimeType.DocJson
                )
            }
            else -> writer.writeWithNoFormatting("todo!() /* $shape $arg */")
        }
    }

    /**
     * If the shape is optional: `Some(inner)` or `None`
     * otherwise: `inner`
     */
    private fun renderMember(
        writer: RustWriter,
        shape: MemberShape,
        arg: Node,
        ctx: Ctx
    ) {
        val target = model.expectShape(shape.target)
        val symbol = symbolProvider.toSymbol(shape)
        if (arg is NullNode) {
            check(
                symbol.isOptional()
            ) { "A null node was provided for $shape but the symbol was not optional. This is invalid input data." }
            writer.write("None")
        } else {
            writer.conditionalBlock("Some(", ")", conditional = symbol.isOptional()) {
                writer.conditionalBlock(
                    "Box::new(",
                    ")",
                    conditional = symbol.rustType().stripOuter<RustType.Option>() is RustType.Box
                ) {
                    render(
                        this,
                        target,
                        arg,
                        ctx.letIf(shape.getMemberTrait(model, HttpPrefixHeadersTrait::class.java).isPresent) {
                            ctx.copy(lowercaseMapKeys = true)
                        }
                    )
                }
            }
        }
    }

    private fun renderSet(writer: RustWriter, shape: SetShape, data: ArrayNode, ctx: Ctx) {
        if (symbolProvider.toSymbol(shape).rustType() is RustType.HashSet) {
            if (!data.isEmpty) {
                writer.rustBlock("") {
                    write("let mut ret = #T::new();", RustType.HashSet.RuntimeType)
                    data.forEach { v ->
                        withBlock("ret.insert(", ");") {
                            renderMember(this, shape.member, v, ctx)
                        }
                    }
                    write("ret")
                }
            } else {
                writer.write("#T::new()", RustType.HashSet.RuntimeType)
            }
        } else {
            renderList(writer, shape, data, ctx)
        }
    }

    /**
     * ```rust
     * {
     *      let mut ret = HashMap::new();
     *      ret.insert("k", ...);
     *      ret.insert("k2", ...);
     *      ret
     * }
     */
    private fun renderMap(
        writer: RustWriter,
        shape: MapShape,
        data: ObjectNode,
        ctx: Ctx,
    ) {
        val lowercase = when (ctx.lowercaseMapKeys) {
            true -> ".to_ascii_lowercase()"
            else -> ""
        }
        if (data.members.isNotEmpty()) {
            writer.rustBlock("") {
                write("let mut ret = #T::new();", RustType.HashMap.RuntimeType)
                data.members.forEach { (k, v) ->
                    withBlock("ret.insert(${k.value.dq()}.to_string()$lowercase,", ");") {
                        renderMember(this, shape.value, v, ctx)
                    }
                }
                write("ret")
            }
        } else {
            writer.write("#T::new()", RustType.HashMap.RuntimeType)
        }
    }

    /**
     * ```rust
     * MyUnion::Variant(...)
     * ```
     */
    private fun renderUnion(
        writer: RustWriter,
        shape: UnionShape,
        data: ObjectNode,
        ctx: Ctx
    ) {
        val unionSymbol = symbolProvider.toSymbol(shape)
        check(data.members.size == 1)
        val variant = data.members.iterator().next()
        val memberName = variant.key.value
        val member = shape.expectMember(memberName)
            .let { model.expectShape(it.target) }
        // TODO: refactor this detail into UnionGenerator
        writer.write("#T::${memberName.toPascalCase()}", unionSymbol)
        // unions should specify exactly one member
        writer.withBlock("(", ")") {
            render(this, member, variant.value, ctx)
        }
    }

    /**
     * ```rust
     * vec![..., ..., ...]
     * ```
     */
    private fun renderList(
        writer: RustWriter,
        shape: CollectionShape,
        data: ArrayNode,
        ctx: Ctx
    ) {
        writer.withBlock("vec![", "]") {
            data.elements.forEach { v ->
                renderMember(this, shape.member, v, ctx)
                write(",")
            }
        }
    }

    private fun renderString(
        writer: RustWriter,
        shape: StringShape,
        arg: StringNode
    ) {
        val enumTrait = shape.getTrait(EnumTrait::class.java).orElse(null)
        val data = arg.value.dq()
        if (enumTrait == null) {
            writer.write("$data.to_string()")
        } else {
            val enumSymbol = symbolProvider.toSymbol(shape)
            writer.write("#T::from($data)", enumSymbol)
        }
    }

    /**
     * ```rust
     * MyStruct::builder().field_1("hello").field_2(5).build()
     * ```
     */
    private fun renderStructure(
        writer: RustWriter,
        shape: StructureShape,
        data: ObjectNode,
        ctx: Ctx
    ) {
        writer.write("#T::builder()", symbolProvider.toSymbol(shape))
        data.members.forEach { (key, value) ->
            val memberShape = shape.expectMember(key.value)
            writer.withBlock(".${memberShape.setterName()}(", ")") {
                renderMember(
                    this,
                    memberShape,
                    value,
                    ctx
                )
            }
        }
        writer.write(".build()")
        if (StructureGenerator.fallibleBuilder(shape, symbolProvider)) {
            writer.write(".unwrap()")
        }
    }
}
