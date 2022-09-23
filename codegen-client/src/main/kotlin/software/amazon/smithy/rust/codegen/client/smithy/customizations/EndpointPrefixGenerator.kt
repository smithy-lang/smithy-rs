/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.EndpointTrait
import software.amazon.smithy.rust.codegen.client.smithy.generators.EndpointTraitBindings
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationSection

class EndpointPrefixGenerator(private val codegenContext: CodegenContext, private val shape: OperationShape) :
    OperationCustomization() {
    override fun section(section: OperationSection): Writable = when (section) {
        is OperationSection.MutateRequest -> writable {
            shape.getTrait(EndpointTrait::class.java).map { epTrait ->
                val endpointTraitBindings = EndpointTraitBindings(
                    codegenContext.model,
                    codegenContext.symbolProvider,
                    codegenContext.runtimeConfig,
                    shape,
                    epTrait,
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
