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
import software.amazon.smithy.rust.codegen.core.smithy.generators.Instantiator
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.setterName

private fun enumFromStringFn(enumSymbol: Symbol, data: String): Writable = writable {
    rust("#T::from($data)", enumSymbol)
}

// TODO Move to a separate file.
class ClientBuilderKindBehavior(val codegenContext: CodegenContext) : Instantiator.BuilderKindBehavior {
    override fun hasFallibleBuilder(shape: StructureShape) =
        StructureGenerator.hasFallibleBuilder(shape, codegenContext.symbolProvider)

    override fun setterName(memberShape: MemberShape) = memberShape.setterName()

    override fun doesSetterTakeInOption(memberShape: MemberShape) = true
}

class ClientInstantiator(val codegenContext: CodegenContext) :
    Instantiator(
        codegenContext.symbolProvider,
        codegenContext.model,
        codegenContext.runtimeConfig,
        ClientBuilderKindBehavior(codegenContext),
        ::enumFromStringFn,
    )
