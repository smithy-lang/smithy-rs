package software.amazon.smithy.rust.codegen.smithy.transformers

import software.amazon.smithy.codegen.core.TopologicalIndex
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.SetShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.smithy.RustBoxTrait

object RecursiveShapeBoxer {
    /**
     * Transform a model which may contain recursive shapes into a model annotated with [RustBoxTrait]
     *
     * When recursive shapes do not go through a List, Map, or Set, they must be boxed in Rust. The function will
     * iteratively find loops & add the `RustBox` trait in a deterministic way until it reaches a fixed point.
     *
     * This function is MUST to be deterministic. If it is not, that is a bug.
     */
    fun transform(model: Model): Model {
        val next = transformInner(model)
        return if (next == null) {
            model
        } else {
            transform(next)
        }
    }

    /**
     * If [model] contains a recursive loop that must be boxed, apply one instance of [RustBoxTrait] return the new model.
     * If [model] contains no loops, return null.
     */
    private fun transformInner(model: Model): Model? {
        // Execute 1-step of the boxing algorithm in the path to reaching a fixed point
        // 1. Find all the shapes that are part of a cycle
        // 2. Find all the loops that those shapes are part of
        // 3. Filter out the loops that go through a layer of indirection
        // 3. Pick _just one_ of the remaining loops to fix
        // 4. Select the member shape in that loop with the earliest shape id
        // 5. Box it.
        // (External to this function) Go back to 1.
        val index = TopologicalIndex(model)
        val recursiveShapes = index.recursiveShapes
        val loops = recursiveShapes.map {
            // Get all the shapes in the closure (represented as Paths
            shapeId ->
            index.getRecursiveClosure(shapeId)
        }.flatMap {
            // flatten the connections into shapes
            loops ->
            loops.map { it.shapes }
        }
        val loopToFix = loops.firstOrNull { !containsIndirection(it) }

        return loopToFix?.let { loop: List<Shape> ->
            check(loop.isNotEmpty())
            // pick the shape to box in a deterministic way
            val shapeToBox = loop.filterIsInstance<MemberShape>().minBy { it.id }!!
            ModelTransformer.create().mapShapes(model) { shape ->
                if (shape == shapeToBox) {
                    shape.asMemberShape().get().toBuilder().addTrait(RustBoxTrait()).build()
                } else {
                    shape
                }
            }
        }
    }

    /**
     * Check if a List<Shape> contains a shape which will use a pointer when represented in Rust, avoiding the
     * need to add more Boxes
     */
    private fun containsIndirection(loop: List<Shape>): Boolean {
        return loop.find {
            when (it) {
                is ListShape,
                is MapShape,
                is SetShape -> true
                else -> it.hasTrait(RustBoxTrait::class.java)
            }
        } != null
    }
}
