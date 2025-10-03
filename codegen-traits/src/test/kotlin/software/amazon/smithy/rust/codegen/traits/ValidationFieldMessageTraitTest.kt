/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.traits

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.SourceLocation
import software.amazon.smithy.model.shapes.ShapeId

class ValidationFieldMessageTraitTest {
    @Test
    fun testValidationFieldMessageTrait() {
        val trait = ValidationFieldMessageTrait(SourceLocation.NONE)
        assertEquals(ShapeId.from("smithy.rust.codegen.traits#validationFieldMessage"), trait.toShapeId())

        // Test the Provider
        val provider = ValidationFieldMessageTrait.Provider()
        assertEquals(ShapeId.from("smithy.rust.codegen.traits#validationFieldMessage"), provider.shapeId)
    }
}
