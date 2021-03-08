/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.customizations

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.EndpointTrait
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.withBlock
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.EndpointTraitBindings
import software.amazon.smithy.rust.codegen.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig

class EndpointPrefixGenerator(private val shape: OperationShape, private val protocolConfig: ProtocolConfig) : OperationCustomization() {
    override fun section(section: OperationSection): Writable = when (section) {
        is OperationSection.MutateRequest -> writable {
            shape.getTrait(EndpointTrait::class.java).map { epTrait ->
                val endpointTraitBindings = EndpointTraitBindings(protocolConfig.model, protocolConfig.symbolProvider, shape, epTrait)
                withBlock("let endpoint_prefix = ", ".map_err(|_|\"Invalid endpoint prefix\")?;") {
                    endpointTraitBindings.render(this, "&op.input")
                }
                rust("request.config_mut().insert(endpoint_prefix);")
            }
        }
        else -> emptySection
    }
}
