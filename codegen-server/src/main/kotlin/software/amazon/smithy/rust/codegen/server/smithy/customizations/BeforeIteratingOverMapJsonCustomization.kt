/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.customizations

import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.JsonSerializerCustomization
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.JsonSerializerSection
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.ValueExpression
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.workingWithPublicConstrainedWrapperTupleType

/**
 * A customization to, just before we iterate over a _constrained_ map shape in a JSON serializer, unwrap the wrapper
 * newtype and take a shared reference to the actual `std::collections::HashMap` within it.
 */
class BeforeIteratingOverMapJsonCustomization(private val codegenContext: ServerCodegenContext) : JsonSerializerCustomization() {
    override fun section(section: JsonSerializerSection): Writable = when (section) {
        is JsonSerializerSection.BeforeIteratingOverMap -> writable {
            if (workingWithPublicConstrainedWrapperTupleType(
                    section.shape,
                    codegenContext.model,
                    codegenContext.settings.codegenConfig.publicConstrainedTypes,
                )
            ) {
                section.context.valueExpression =
                    ValueExpression.Reference("&${section.context.valueExpression.name}.0")
            }
        }
        else -> emptySection
    }
}
