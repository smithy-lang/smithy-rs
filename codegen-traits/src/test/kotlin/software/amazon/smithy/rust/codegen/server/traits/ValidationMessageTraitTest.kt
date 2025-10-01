package software.amazon.smithy.rust.codegen.traits

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.SourceLocation
import software.amazon.smithy.model.shapes.ShapeId

class ValidationMessageTraitTest {
    @Test
    fun testValidationMessageTrait() {
        val trait = ValidationMessageTrait(SourceLocation.NONE)
        assertEquals(ShapeId.from("smithy.rust.codegen.traits#validationMessage"), trait.toShapeId())

        // Test the Provider
        val provider = ValidationMessageTrait.Provider()
        assertEquals(ShapeId.from("smithy.rust.codegen.traits#validationMessage"), provider.shapeId)
    }
}
