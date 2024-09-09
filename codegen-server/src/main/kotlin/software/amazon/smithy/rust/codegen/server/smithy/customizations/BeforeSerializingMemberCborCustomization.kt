/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.customizations

import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.ByteShape
import software.amazon.smithy.model.shapes.IntegerShape
import software.amazon.smithy.model.shapes.LongShape
import software.amazon.smithy.model.shapes.ShortShape
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.CborSerializerCustomization
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.CborSerializerSection
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.ValueExpression
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.workingWithPublicConstrainedWrapperTupleType

/**
 * Constrained shapes are wrapped in a Rust tuple struct that implements all necessary checks. However,
 * for serialization purposes, the inner type of the constrained shape is used for serialization.
 *
 * The `BeforeSerializingMemberCborCustomization` class generates a reference to the inner type when the shape being
 * code-generated is constrained and the `publicConstrainedTypes` codegen flag is set.
 */
class BeforeSerializingMemberCborCustomization(private val codegenContext: ServerCodegenContext) : CborSerializerCustomization() {
    override fun section(section: CborSerializerSection): Writable =
        when (section) {
            is CborSerializerSection.BeforeSerializingNonNullMember ->
                writable {
                    if (workingWithPublicConstrainedWrapperTupleType(
                            section.shape,
                            codegenContext.model,
                            codegenContext.settings.codegenConfig.publicConstrainedTypes,
                        )
                    ) {
                        if (section.shape is IntegerShape || section.shape is ShortShape || section.shape is LongShape || section.shape is ByteShape || section.shape is BlobShape) {
                            section.context.valueExpression =
                                ValueExpression.Reference("&${section.context.valueExpression.name}.0")
                        }
                    }
                }

            else -> emptySection
        }
}
