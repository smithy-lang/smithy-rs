/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.BooleanShape
import software.amazon.smithy.model.shapes.DocumentShape
import software.amazon.smithy.model.shapes.EnumShape
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.NumberShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.TimestampShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.isEmpty
import software.amazon.smithy.rust.codegen.core.rustlang.map
import software.amazon.smithy.rust.codegen.core.rustlang.plus
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.some
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.PrimitiveInstantiator
import software.amazon.smithy.rust.codegen.core.smithy.isRustBoxed
import software.amazon.smithy.rust.codegen.core.smithy.protocols.shapeFunctionName
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.isEventStream
import software.amazon.smithy.rust.codegen.core.util.isStreaming
import software.amazon.smithy.rust.codegen.core.util.letIf

/**
 * For AWS-services, the spec defines error correction semantics to recover from missing default values for required members:
 * https://smithy.io/2.0/spec/aggregate-types.html?highlight=error%20correction#client-error-correction
 */

private fun ClientCodegenContext.errorCorrectedDefault(member: MemberShape): Writable? {
    if (!member.isRequired) {
        return null
    }
    val target = model.expectShape(member.target)
    val memberSymbol = symbolProvider.toSymbol(member)
    val targetSymbol = symbolProvider.toSymbol(target)
    if (member.isEventStream(model) || member.isStreaming(model)) {
        return null
    }
    val instantiator = PrimitiveInstantiator(runtimeConfig, symbolProvider)
    return writable {
        when {
            target is EnumShape || target.hasTrait<EnumTrait>() ->
                rustTemplate(
                    """"no value was set".parse::<#{Shape}>().ok()""",
                    "Shape" to targetSymbol,
                )

            target is BooleanShape || target is NumberShape || target is StringShape || target is DocumentShape || target is ListShape || target is MapShape ->
                rust(
                    "Some(Default::default())",
                )

            target is StructureShape ->
                rustTemplate(
                    "{ let builder = #{Builder}::default(); #{instantiate} }",
                    "Builder" to symbolProvider.symbolForBuilder(target),
                    "instantiate" to
                        builderInstantiator().finalizeBuilder("builder", target).map {
                            if (BuilderGenerator.hasFallibleBuilder(target, symbolProvider)) {
                                rust("#T.ok()", it)
                            } else {
                                it.some()(this)
                            }
                        }.letIf(memberSymbol.isRustBoxed()) {
                            it.plus { rustTemplate(".map(#{Box}::new)", *preludeScope) }
                        },
                )

            target is TimestampShape -> instantiator.instantiate(target, Node.from(0)).some()(this)
            target is BlobShape -> instantiator.instantiate(target, Node.from("")).some()(this)
            target is UnionShape ->
                rustTemplate(
                    "Some(#{unknown})", *preludeScope,
                    "unknown" to
                        writable {
                            if (memberSymbol.isRustBoxed()) {
                                rust("Box::new(#T::Unknown)", targetSymbol)
                            } else {
                                rust("#T::Unknown", targetSymbol)
                            }
                        },
                )
        }
    }
}

fun ClientCodegenContext.correctErrors(shape: StructureShape): RuntimeType? {
    val name = symbolProvider.shapeFunctionName(serviceShape, shape) + "_correct_errors"
    val corrections =
        writable {
            shape.members().forEach { member ->
                val memberName = symbolProvider.toMemberName(member)
                errorCorrectedDefault(member)?.also { default ->
                    rustTemplate(
                        """if builder.$memberName.is_none() { builder.$memberName = #{default} }""",
                        "default" to default,
                    )
                }
            }
        }

    if (corrections.isEmpty()) {
        return null
    }

    return RuntimeType.forInlineFun(name, RustModule.private("serde_util")) {
        rustTemplate(
            """
            pub(crate) fn $name(mut builder: #{Builder}) -> #{Builder} {
                #{corrections}
                builder
            }

            """,
            "Builder" to symbolProvider.symbolForBuilder(shape),
            "corrections" to corrections,
        )
    }
}
