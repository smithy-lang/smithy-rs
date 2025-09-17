package software.amazon.smithy.rust.codegen.server.traits

import org.junit.jupiter.api.Test
import org.junit.jupiter.api.Assertions.assertEquals
import software.amazon.smithy.model.SourceLocation
import software.amazon.smithy.model.shapes.ShapeId

class ValidationExceptionTraitTest {
    @Test
    fun testValidationExceptionTrait() {
        val trait = ValidationExceptionTrait(SourceLocation.NONE)
        assertEquals(ShapeId.from("smithy.rust.codegen.server.traits#validationException"), trait.toShapeId())

        // Test the Provider
        val provider = ValidationExceptionTrait.Provider()
        assertEquals(ShapeId.from("smithy.rust.codegen.server.traits#validationException"), provider.shapeId)
    }
}
