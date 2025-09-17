package software.amazon.smithy.rust.codegen.server.traits

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.SourceLocation
import software.amazon.smithy.model.shapes.ShapeId

class ValidationFieldListTraitTest {
    @Test
    fun testValidationFieldListTrait() {
        val trait = ValidationFieldListTrait(SourceLocation.NONE)
        assertEquals(ShapeId.from("smithy.rust.codegen.server.traits#validationFieldList"), trait.toShapeId())

        // Test the Provider
        val provider = ValidationFieldListTrait.Provider()
        assertEquals(ShapeId.from("smithy.rust.codegen.server.traits#validationFieldList"), provider.shapeId)
    }
}
