/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.BooleanShape
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.DocumentShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.NumberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.SimpleShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.TimestampShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.DefaultTrait
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.model.traits.HttpHeaderTrait
import software.amazon.smithy.model.traits.HttpPayloadTrait
import software.amazon.smithy.model.traits.StreamingTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.rustlang.withBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderInstantiator
import software.amazon.smithy.rust.codegen.core.smithy.generators.Instantiator
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.ReturnSymbolToParse
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticOutputTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.isDirectlyConstrained
import software.amazon.smithy.rust.codegen.server.smithy.traits.isReachableFromOperationInput

class ServerBuilderKindBehavior(val codegenContext: CodegenContext) : Instantiator.BuilderKindBehavior {
    override fun hasFallibleBuilder(shape: StructureShape): Boolean {
        // Only operation input builders take in unconstrained types.
        val takesInUnconstrainedTypes = shape.isReachableFromOperationInput()

        val publicConstrainedTypes =
            if (codegenContext is ServerCodegenContext) {
                codegenContext.settings.codegenConfig.publicConstrainedTypes
            } else {
                true
            }

        return if (publicConstrainedTypes) {
            ServerBuilderGenerator.hasFallibleBuilder(
                shape,
                codegenContext.model,
                codegenContext.symbolProvider,
                takesInUnconstrainedTypes,
            )
        } else {
            ServerBuilderGeneratorWithoutPublicConstrainedTypes.hasFallibleBuilder(
                shape,
                codegenContext.symbolProvider,
            )
        }
    }

    override fun setterName(memberShape: MemberShape): String = codegenContext.symbolProvider.toMemberName(memberShape)

    override fun doesSetterTakeInOption(memberShape: MemberShape): Boolean =
        codegenContext.symbolProvider.toSymbol(memberShape).isOptional()
}

class ServerInstantiator(
    private val codegenContext: CodegenContext,
    private val customWritable: CustomWritable = NoCustomWritable(),
    private val ignoreMissingMembers: Boolean = false,
    withinTest: Boolean = false,
) :
    Instantiator(
            codegenContext.symbolProvider,
            codegenContext.model,
            codegenContext.runtimeConfig,
            ServerBuilderKindBehavior(codegenContext),
            defaultsForRequiredFields = true,
            // Construct with direct pattern to more closely replicate actual server customer usage
            constructPattern = InstantiatorConstructPattern.DIRECT,
            customWritable = customWritable,
            ignoreMissingMembers = ignoreMissingMembers,
            withinTest = withinTest,
        ) {
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val builderKindBehavior = ServerBuilderKindBehavior(codegenContext)

    override fun render(
        writer: RustWriter,
        shape: Shape,
        data: Node,
        headers: Map<String, String>,
        ctx: Ctx,
    ) {
        if (shape is StructureShape) {
            val customized = customWritable.generate(shape)
            if (customized != null) {
                customized(writer)
            } else {
                renderStructure(writer, shape, data as ObjectNode, headers, ctx)
            }
            return
        }

        val convert =
            (shape is SimpleShape || shape is MapShape || shape is CollectionShape) &&
                needsConversion(shape)
        if (convert) {
            val customized = customWritable.generate(shape)
            if (customized != null) {
                customized(writer)
                return
            }
        }
        // Parenthesize negative literals so conversion applies to the signed value.
        if (convert && shape is NumberShape) writer.rust("(")
        super.render(writer, shape, data, headers, ctx)
        if (convert) {
            if (shape is NumberShape) writer.rust(")")
            writer.rust(""".try_into().expect("this is only used in tests")""")
        }
    }

    private fun needsConversion(shape: Shape): Boolean {
        val publicConstrainedTypes =
            (codegenContext as? ServerCodegenContext)?.settings?.codegenConfig?.publicConstrainedTypes ?: true
        return publicConstrainedTypes &&
            !shape.hasTrait<EnumTrait>() &&
            !shape.hasTrait<StreamingTrait>() &&
            shape.isDirectlyConstrained(symbolProvider)
    }

    private fun constructPatternForStructure(
        shape: StructureShape,
        data: ObjectNode,
    ): InstantiatorConstructPattern =
        if (
            shape.hasTrait<SyntheticOutputTrait>() &&
            shape.allMembers.any { (name, member) ->
                member.hasTrait<DefaultTrait>() &&
                    !codegenContext.symbolProvider.toSymbol(member).isOptional() &&
                    !data.getMember(name).isPresent
            }
        ) {
            // Exercise the generated output defaults instead of inventing values
            // for omitted response-test parameters. Expected inputs stay direct.
            InstantiatorConstructPattern.BUILDER
        } else {
            InstantiatorConstructPattern.DIRECT
        }

    private fun renderStructure(
        writer: RustWriter,
        shape: StructureShape,
        data: ObjectNode,
        headers: Map<String, String>,
        ctx: Ctx,
    ) {
        val pattern = constructPatternForStructure(shape, data)
        val values = linkedMapOf<MemberShape, Node>()
        shape.allMembers.forEach { (name, member) ->
            if (
                !symbolProvider.toSymbol(member).isOptional() &&
                !data.getMember(name).isPresent &&
                !(pattern == InstantiatorConstructPattern.BUILDER && member.hasTrait<DefaultTrait>())
            ) {
                values[member] = defaultValue(model.expectShape(member.target))
            }
        }
        if (data.isEmpty) {
            shape.allMembers.values.forEach { member ->
                member.getTrait(HttpHeaderTrait::class.java).orElse(null)?.value?.let { header ->
                    headers[header]?.let { values[member] = Node.from(it) }
                }
            }
        }
        data.members.forEach { (name, value) ->
            val member = shape.getMember(name.value).orElse(null)
            if (member != null) {
                values[member] = value
            } else if (!ignoreMissingMembers) {
                throw CodegenException("Protocol test defines data for member shape `${name.value}`, but member shape was not found on structure shape ${shape.id}")
            }
        }
        shape.allMembers.values.firstOrNull {
            it.hasTrait<HttpPayloadTrait>() && it !in values &&
                model.expectShape(it.target) is StructureShape
        }?.let { values[it] = Node.objectNode() }

        val members: RustWriter.() -> Unit = {
            values.forEach { (member, value) ->
                val field = symbolProvider.toMemberName(member)
                if (pattern == InstantiatorConstructPattern.DIRECT) {
                    withBlock("$field:", ",") { render(this, member, value, mapOf(), ctx) }
                } else {
                    withBlock(".${builderKindBehavior.setterName(member)}(", ")") {
                        render(this, member, value, mapOf(), ctx)
                    }
                }
            }
            if (pattern == InstantiatorConstructPattern.DIRECT) {
                shape.allMembers.values.filter { it !in values }.forEach { member ->
                    check(symbolProvider.toSymbol(member).isOptional())
                    // Absence is None, including documents; an explicit null goes through renderMember.
                    withBlock("${symbolProvider.toMemberName(member)}:", ",") {
                        if (model.expectShape(member.target) is DocumentShape) {
                            val customized = customWritable.generate(member)
                            if (customized != null) customized(this) else rustTemplate("#{None}", *RuntimeType.preludeScope)
                        } else {
                            render(this, member, Node.nullNode(), mapOf(), ctx)
                        }
                    }
                }
            }
        }
        if (pattern == InstantiatorConstructPattern.DIRECT) {
            writer.withBlockTemplate("#{T} {", "}", "T" to symbolProvider.toSymbol(shape), block = members)
        } else {
            writer.rust("#T::builder()", symbolProvider.toSymbol(shape))
            members(writer)
            writer.rust(".build()")
            if (builderKindBehavior.hasFallibleBuilder(shape)) writer.rust(".unwrap()")
        }
    }

    /** Placeholder values for omitted required members, matching the shared instantiator. */
    private fun defaultValue(shape: Shape): Node =
        when (shape) {
            is StructureShape, is UnionShape, is MapShape, is DocumentShape -> Node.objectNode()
            is CollectionShape -> Node.arrayNode()
            is TimestampShape, is NumberShape -> Node.from(0)
            is BlobShape, is StringShape -> Node.from("")
            is BooleanShape -> Node.from(false)
            else -> throw CodegenException("Unrecognized shape `$shape`")
        }
}

class ServerBuilderInstantiator(
    private val symbolProvider: RustSymbolProvider,
    private val symbolParseFn: (Shape) -> ReturnSymbolToParse,
) : BuilderInstantiator {
    override fun setField(
        builder: String,
        value: Writable,
        field: MemberShape,
    ): Writable {
        // Server builders have the ability to have non-optional fields. When one of these fields is used,
        // we need to use `if let(...)` to only set the field when it is present.
        return if (!symbolProvider.toSymbol(field).isOptional()) {
            writable {
                val n = safeName()
                rustTemplate(
                    """
                    if let Some($n) = #{value} {
                        #{setter}
                    }
                    """,
                    "value" to value, "setter" to setFieldWithSetter(builder, writable(n), field),
                )
            }
        } else {
            setFieldWithSetter(builder, value, field)
        }
    }

    override fun finalizeBuilder(
        builder: String,
        shape: StructureShape,
        mapErr: Writable?,
    ): Writable =
        writable {
            val returnSymbolToParse = symbolParseFn(shape)
            if (returnSymbolToParse.isUnconstrained) {
                rust(builder)
            } else {
                rust("$builder.build()")
            }
        }
}
