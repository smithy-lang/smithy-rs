/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.customizations

import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.CborSerializerCustomization
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.CborSerializerSection
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.ValueExpression
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.workingWithPublicConstrainedWrapperTupleType

/**
 * A customization to, just before we encode over a _constrained_ map or collection shape in a CBOR serializer,
 * unwrap the wrapper newtype and take a shared reference to the actual value within it.
 * That value will be a `std::collections::HashMap` for map shapes, and a `std::vec::Vec` for collection shapes.
 */
class BeforeEncodingMapOrCollectionCborCustomization(private val codegenContext: ServerCodegenContext) : CborSerializerCustomization() {
    override fun section(section: CborSerializerSection): Writable =
        when (section) {
            is CborSerializerSection.BeforeIteratingOverMapOrCollection ->
                writable {
                    check(section.shape is CollectionShape || section.shape is MapShape)
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
