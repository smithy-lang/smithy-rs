/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.traits

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.SourceLocation
import software.amazon.smithy.model.shapes.ShapeId

class ValidationExceptionTraitTest {
    @Test
    fun testValidationExceptionTrait() {
        val trait = ValidationExceptionTrait(SourceLocation.NONE)
        assertEquals(ShapeId.from("smithy.rust.codegen.traits#validationException"), trait.toShapeId())

        // Test the Provider
        val provider = ValidationExceptionTrait.Provider()
        assertEquals(ShapeId.from("smithy.rust.codegen.traits#validationException"), provider.shapeId)
    }
}
