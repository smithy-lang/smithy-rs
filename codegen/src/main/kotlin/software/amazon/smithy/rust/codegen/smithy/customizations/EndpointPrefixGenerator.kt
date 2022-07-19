/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.customizations

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.EndpointTrait
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.withBlock
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.CoreCodegenContext
import software.amazon.smithy.rust.codegen.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.smithy.generators.EndpointTraitBindings

class EndpointPrefixGenerator(private val coreCodegenContext: CoreCodegenContext, private val shape: OperationShape) :
    OperationCustomization() {
    override fun section(section: OperationSection): Writable = when (section) {
        is OperationSection.MutateRequest -> writable {
            shape.getTrait(EndpointTrait::class.java).map { epTrait ->
                val endpointTraitBindings = EndpointTraitBindings(
                    coreCodegenContext.model,
                    coreCodegenContext.symbolProvider,
                    coreCodegenContext.runtimeConfig,
                    shape,
                    epTrait
                )
                withBlock("let endpoint_prefix = ", "?;") {
                    endpointTraitBindings.render(this, "self")
                }
                rust("request.properties_mut().insert(endpoint_prefix);")
            }
        }
        else -> emptySection
    }
}
