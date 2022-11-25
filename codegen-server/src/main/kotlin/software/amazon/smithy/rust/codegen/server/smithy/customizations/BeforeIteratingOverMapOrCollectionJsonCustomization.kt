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
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.workingWithPublicConstrainedWrapperTupleType

/**
 * A customization to, just before we iterate over a _constrained_ map or collection shape in a JSON serializer,
 * unwrap the wrapper newtype and take a shared reference to the actual value within it.
 * That value will be a `std::collections::HashMap` for map shapes, and a `std::vec::Vec` for collection shapes.
 */
class BeforeIteratingOverMapOrCollectionJsonCustomization(private val codegenContext: ServerCodegenContext) : JsonSerializerCustomization() {
    override fun section(section: JsonSerializerSection): Writable = when (section) {
        is JsonSerializerSection.BeforeIteratingOverMapOrCollection -> writable {
            if (workingWithPublicConstrainedWrapperTupleType(
                    section.shape,
                    codegenContext.model,
                    codegenContext.settings.codegenConfig.publicConstrainedTypes,
                )
            ) {
                // Note that this particular implementation just so happens to work because when the customization
                // is invoked in the JSON serializer, the value expression is guaranteed to be a variable binding name.
                // If the expression in the future were to be more complex, we wouldn't be able to write the left-hand
                // side of this assignment.
                rust("""let ${section.valueExpression.name} = &${section.valueExpression.name}.0;""")
            }
        }
        else -> emptySection
    }
}
