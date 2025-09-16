package software.amazon.smithy.rust.codegen.core.smithy.transformers

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.AbstractShapeBuilder
import software.amazon.smithy.model.shapes.EnumShape
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.core.smithy.DirectedWalker
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticImplDisplayTrait
import software.amazon.smithy.rust.codegen.core.util.UNREACHABLE
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.utils.ToSmithyBuilder

/**
 * Adds a synthetic trait to shapes that are reachable from error shapes to ensure they
 * implement the `Display` trait in generated code.
 *
 * When a shape is annotated with `@error`, it needs to implement Rust's `Display` trait.
 * If the error shape contains references to other structures, those structures also
 * need to implement `Display` for proper error formatting.
 */
object AddSyntheticTraitForImplDisplay {
    /**
     * Transforms the model by adding [SyntheticImplDisplayTrait] to all shapes that are:
     * 1. Reachable from an error shape
     * 2. Not already marked with `@error`
     * 3. Of a type that can implement `Display` (structure, list, union, or map)
     *
     * @param model The input model to transform
     * @return The transformed model with synthetic traits added
     */
    fun transform(model: Model): Model {
        val walker = DirectedWalker(model)

        // Find all error shapes from operations.
        val errorShapes =
            model.operationShapes
                .flatMap { it.errors }
                .mapNotNull { model.expectShape(it).asStructureShape().orElse(null) }

        // Get shapes reachable from error shapes that need Display impl.
        val shapesNeedingDisplay =
            errorShapes
                .flatMap { walker.walkShapes(it) }
                .filter {
                    (it is StructureShape || it is ListShape || it is UnionShape || it is MapShape || it is EnumShape) &&
                        it.getTrait<ErrorTrait>() == null
                }

        // Add synthetic trait to identified shapes.
        val transformedShapes =
            shapesNeedingDisplay.mapNotNull { shape ->
                if (shape !is ToSmithyBuilder<*>) {
                    UNREACHABLE("Shapes reachable from error shapes should be buildable")
                    return@mapNotNull null
                }

                val builder = shape.toBuilder()
                if (builder is AbstractShapeBuilder<*, *>) {
                    builder.addTrait(SyntheticImplDisplayTrait()).build()
                } else {
                    UNREACHABLE("`impl Display` cannot be generated for ${shape.id}")
                    null
                }
            }

        return ModelTransformer.create().replaceShapes(model, transformedShapes)
    }
}
