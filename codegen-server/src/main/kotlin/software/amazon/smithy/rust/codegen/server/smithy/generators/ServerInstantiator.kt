/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.generators.Instantiator
import software.amazon.smithy.rust.codegen.core.smithy.generators.InstantiatorCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.InstantiatorSection
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.isDirectlyConstrained
import software.amazon.smithy.rust.codegen.server.smithy.traits.isReachableFromOperationInput

class ServerAfterInstantiatingValueConstrainItIfNecessary(val codegenContext: CodegenContext) :
    InstantiatorCustomization() {

    override fun section(section: InstantiatorSection): Writable = when (section) {
        is InstantiatorSection.AfterInstantiatingValue -> writable {
            if (section.shape.isDirectlyConstrained(codegenContext.symbolProvider)) {
                rust(""".try_into().expect("this is only used in tests")""")
            }
        }
    }
}

class ServerBuilderKindBehavior(val codegenContext: CodegenContext) : Instantiator.BuilderKindBehavior {
    override fun hasFallibleBuilder(shape: StructureShape): Boolean {
        // Only operation input builders take in unconstrained types.
        val takesInUnconstrainedTypes = shape.isReachableFromOperationInput()

        val publicConstrainedTypes = if (codegenContext is ServerCodegenContext) {
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

fun serverInstantiator(codegenContext: CodegenContext) =
    Instantiator(
        codegenContext.symbolProvider,
        codegenContext.model,
        codegenContext.runtimeConfig,
        ServerBuilderKindBehavior(codegenContext),
        defaultsForRequiredFields = true,
        customizations = listOf(ServerAfterInstantiatingValueConstrainItIfNecessary(codegenContext)),
    )
