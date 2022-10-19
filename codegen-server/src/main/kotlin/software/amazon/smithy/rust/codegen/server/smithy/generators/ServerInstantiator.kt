/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.generators.Instantiator
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.smithy.traits.isReachableFromOperationInput

private fun enumFromStringFn(enumSymbol: Symbol, data: String): Writable = writable {
    rust(
        """#T::try_from($data).expect("This is used in tests ONLY")""",
        enumSymbol,
    )
}

// TODO Move to a separate file.
class ServerBuilderKindBehavior(val codegenContext: CodegenContext): Instantiator.BuilderKindBehavior {
    override fun hasFallibleBuilder(shape: StructureShape): Boolean {
        // Only operation input builders take in unconstrained types.
        val takesInUnconstrainedTypes = shape.isReachableFromOperationInput()
        return StructureGenerator.serverHasFallibleBuilder(
            shape,
            codegenContext.model,
            codegenContext.symbolProvider,
            takesInUnconstrainedTypes,
        )
    }

    override fun setterName(memberShape: MemberShape) = codegenContext.symbolProvider.toMemberName(memberShape)

    override fun doesSetterTakeInOption(memberShape: MemberShape) =
        codegenContext.symbolProvider.toSymbol(memberShape).isOptional()
}

class ServerInstantiator(val codegenContext: CodegenContext) :
    Instantiator(
        codegenContext.symbolProvider,
        codegenContext.model,
        codegenContext.runtimeConfig,
        ServerBuilderKindBehavior(codegenContext),
        ::enumFromStringFn,
        defaultsForRequiredFields = true,
    )
