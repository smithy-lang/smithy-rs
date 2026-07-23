/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.generators.client.FluentClientGenerator
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.Instantiator
import software.amazon.smithy.rust.codegen.core.smithy.generators.setterName

class ClientBuilderKindBehavior(val codegenContext: CodegenContext) : Instantiator.BuilderKindBehavior {
    override fun hasFallibleBuilder(shape: StructureShape): Boolean =
        BuilderGenerator.hasFallibleBuilder(shape, codegenContext.symbolProvider)

    override fun setterName(memberShape: MemberShape): String = memberShape.setterName(codegenContext.symbolProvider)

    override fun doesSetterTakeInOption(memberShape: MemberShape): Boolean = true
}

class ClientInstantiator(private val codegenContext: ClientCodegenContext, withinTest: Boolean = false) : Instantiator(
    codegenContext.symbolProvider,
    codegenContext.model,
    codegenContext.runtimeConfig,
    ClientBuilderKindBehavior(codegenContext),
    withinTest = false,
) {
    fun renderFluentCall(
        writer: RustWriter,
        clientName: String,
        operationShape: OperationShape,
        inputShape: StructureShape,
        data: Node,
        headers: Map<String, String> = mapOf(),
        ctx: Ctx = Ctx(),
    ) {
        val operationBuilderName =
            FluentClientGenerator.clientOperationFnName(operationShape, codegenContext.symbolProvider)

        writer.rust("$clientName.$operationBuilderName()")

        renderStructureMembers(writer, inputShape, data as ObjectNode, headers, ctx)
    }
}
