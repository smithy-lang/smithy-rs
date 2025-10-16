/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.framework.rust

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.SourceLocation
import software.amazon.smithy.model.shapes.ShapeId

class ValidationMessageTraitTest {
    @Test
    fun testValidationMessageTrait() {
        val trait = ValidationMessageTrait(SourceLocation.NONE)
        assertEquals(ShapeId.from("smithy.framework.rust#validationMessage"), trait.toShapeId())

        // Test the Provider
        val provider = ValidationMessageTrait.Provider()
        assertEquals(ShapeId.from("smithy.framework.rust#validationMessage"), provider.shapeId)
    }
}
