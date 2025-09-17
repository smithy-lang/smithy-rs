package software.amazon.smithy.rust.codegen.server.traits

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.SourceLocation
import software.amazon.smithy.model.shapes.ShapeId

class ValidationMessageTraitTest {
    @Test
    fun testValidationMessageTrait() {
        val trait = ValidationMessageTrait(SourceLocation.NONE)
        assertEquals(ShapeId.from("smithy.rust.codegen.server.traits#validationMessage"), trait.toShapeId())

        // Test the Provider
        val provider = ValidationMessageTrait.Provider()
        assertEquals(ShapeId.from("smithy.rust.codegen.server.traits#validationMessage"), provider.shapeId)
    }
}
