package software.amazon.smithy.rust.codegen.util

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId

inline fun <reified  T: Shape>Model.lookup(shapeId: String): T {
    return this.expectShape(ShapeId.from(shapeId), T::class.java)
}