package software.amazon.smithy.rust.codegen.util

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape

inline fun <reified T : Shape> Model.lookup(shapeId: String): T {
    return this.expectShape(ShapeId.from(shapeId), T::class.java)
}

fun OperationShape.inputShape(model: Model): StructureShape {
    // The Rust Smithy generator adds an input to all shapes automatically
    return model.expectShape(this.input.get(), StructureShape::class.java)
}
