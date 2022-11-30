/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.Instantiator
import software.amazon.smithy.rust.codegen.core.smithy.generators.setterName

private fun enumFromStringFn(enumSymbol: Symbol, data: String): Writable = writable {
    rust("#T::from($data)", enumSymbol)
}

class ClientBuilderKindBehavior(val codegenContext: CodegenContext) : Instantiator.BuilderKindBehavior {
    override fun hasFallibleBuilder(shape: StructureShape): Boolean =
        BuilderGenerator.hasFallibleBuilder(shape, codegenContext.symbolProvider)

    override fun setterName(memberShape: MemberShape): String = memberShape.setterName()

    override fun doesSetterTakeInOption(memberShape: MemberShape): Boolean = true
}

fun clientInstantiator(codegenContext: CodegenContext) =
    Instantiator(
        codegenContext.symbolProvider,
        codegenContext.model,
        codegenContext.runtimeConfig,
        ClientBuilderKindBehavior(codegenContext),
        ::enumFromStringFn,
    )
