/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.EndpointTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.generators.EndpointTraitBindings
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.util.orNull

// TODO(enableNewSmithyRuntimeCleanup): Delete this file

class EndpointPrefixGenerator(private val codegenContext: ClientCodegenContext, private val shape: OperationShape) :
    OperationCustomization() {
    companion object {
        fun endpointTraitBindings(codegenContext: ClientCodegenContext, shape: OperationShape): EndpointTraitBindings? =
            shape.getTrait(EndpointTrait::class.java).map { epTrait ->
                EndpointTraitBindings(
                    codegenContext.model,
                    codegenContext.symbolProvider,
                    codegenContext.runtimeConfig,
                    shape,
                    epTrait,
                )
            }.orNull()
    }

    override fun section(section: OperationSection): Writable = when (section) {
        is OperationSection.MutateRequest -> writable {
            endpointTraitBindings(codegenContext, shape)?.also { endpointTraitBindings ->
                withBlock("let endpoint_prefix = ", "?;") {
                    endpointTraitBindings.render(this, "self", codegenContext.smithyRuntimeMode)
                }
                rust("request.properties_mut().insert(endpoint_prefix);")
            }
        }

        else -> emptySection
    }
}
