package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.node.ArrayNode
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.NumberNode
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.node.StringNode
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.BooleanShape
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.NumberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.TimestampShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.rust.codegen.lang.RustWriter
import software.amazon.smithy.rust.codegen.lang.rustBlock
import software.amazon.smithy.rust.codegen.lang.withBlock
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.isOptional
import software.amazon.smithy.rust.codegen.util.dq

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

    fun render(arg: Node, shape: Shape, writer: RustWriter) {
        when (shape) {
            // Compound Shapes
            is StructureShape -> renderStructure(writer, shape, arg as ObjectNode)
            is UnionShape -> renderUnion(writer, shape, arg as ObjectNode)

            // Collections
            is ListShape -> renderList(writer, shape, arg as ArrayNode)
            is MapShape -> renderMap(writer, shape, arg as ObjectNode)

            // Wrapped Shapes
            is TimestampShape -> writer.write(
                "\$T::from_epoch_seconds(${(arg as NumberNode).value})",
                RuntimeType.Instant(runtimeConfig)
            )

            /**
             * ```rust
             * Blob::new(\"arg\")
             * ```
             */
            is BlobShape -> writer.write(
                "\$T::new(${(arg as StringNode).value.dq()})",
                RuntimeType.Blob(runtimeConfig)
            )

            // Simple Shapes
            is StringShape -> renderString(writer, shape, arg as StringNode)
            is NumberShape -> writer.write(arg.asNumberNode().get())
            is BooleanShape -> writer.write(arg.asBooleanNode().get().toString())
            else -> writer.write("todo!() /* $shape $arg */")
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
        data: ObjectNode
    ) {
        writer.rustBlock("") {
            if (data.members.isNotEmpty()) {
                write("let mut ret = \$T::new();", RuntimeType.HashMap)
                val valueShape = shape.value.let { model.expectShape(it.target) }
                data.members.forEach { (k, v) ->
                    withBlock("ret.insert(${k.value.dq()}.to_string(),", ");") {
                        render(v, valueShape, this)
                    }
                }
                write("ret")
            } else {
                writer.write("\$T::new()", RuntimeType.HashMap)
            }
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
        data: ObjectNode
    ) {
        val unionSymbol = symbolProvider.toSymbol(shape)
        check(data.members.size == 1)
        val variant = data.members.iterator().next()
        val memberName = variant.key.value
        val member = shape.getMember(memberName).get()
            .let { model.expectShape(it.target) }
        // TODO: refactor this detail into UnionGenerator
        writer.write("\$T::${memberName.toPascalCase()}", unionSymbol)
        // unions should specify exactly one member
        writer.withBlock("(", ")") {
            render(variant.value, member, this)
        }
    }

    /**
     * ```rust
     * vec![..., ..., ...]
     * ```
     */
    private fun renderList(
        writer: RustWriter,
        shape: ListShape,
        data: ArrayNode
    ) {
        val member = model.expectShape(shape.member.target)
        val memberSymbol = symbolProvider.toSymbol(shape.member)
        writer.withBlock("vec![", "]") {
            data.elements.forEach {
                if (it.isNullNode) {
                    write("None")
                } else {
                    withBlock("Some(", ")", conditional = memberSymbol.isOptional()) {
                        render(it, member, this)
                    }
                }
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
            writer.write("\$T::from($data)", enumSymbol)
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
        data: ObjectNode
    ) {
        writer.write("\$T::builder()", symbolProvider.toSymbol(shape))
        data.members.forEach { (key, value) ->
            val (memberShape, targetShape) = getMember(shape, key)
            val func = symbolProvider.toMemberName(memberShape)
            if (!value.isNullNode) {
                writer.withBlock(".$func(", ")") {
                    render(value, targetShape, this)
                }
            }
        }
        writer.write(".build()")
        if (StructureGenerator.fallibleBuilder(shape, symbolProvider)) {
            writer.write(".unwrap()")
        }
    }

    private fun getMember(shape: StructureShape, key: StringNode): Pair<MemberShape, Shape> {
        val memberShape = shape.getMember(key.value)
            .orElseThrow { IllegalArgumentException("$shape did not have member ${key.value}") }
        return memberShape to model.expectShape(memberShape.target)
    }
}
