/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.customizations

import software.amazon.smithy.model.shapes.IntegerShape
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.JsonSerializerCustomization
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.JsonSerializerSection
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.workingWithPublicConstrainedWrapperTupleType

/**
 * A customization to, just before we serialize a _constrained_ shape in a JSON serializer, unwrap the wrapper
 * newtype and take a shared reference to the actual unconstrained value within it.
 */
class BeforeSerializingMemberJsonCustomization(private val codegenContext: ServerCodegenContext) : JsonSerializerCustomization() {
    override fun section(section: JsonSerializerSection): Writable = when (section) {
        is JsonSerializerSection.BeforeSerializingMember -> writable {
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
                if (section.shape is IntegerShape) {
                    rust("""let ${section.valueExpression.name} = &${section.valueExpression.name}.0;""")
                }
            }
        }
        else -> emptySection
    }
}
