package software.amazon.smithy.rust.codegen.server.smithy.transformers

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.neighbor.Relationship
import software.amazon.smithy.model.neighbor.RelationshipDirection
import software.amazon.smithy.model.neighbor.Walker
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.RequiredTrait
import software.amazon.smithy.model.traits.Trait
import software.amazon.smithy.model.transform.ModelTransformer
import java.util.*
import software.amazon.smithy.rust.codegen.core.util.orNull
import java.util.function.Predicate

class DirectedWalker(model: Model) {
    private val inner = Walker(model)

    fun walkShapes(shape: Shape): Set<Shape> {
        return walkShapes(shape) { _ -> true }
    }

    fun walkShapes(shape: Shape, predicate: Predicate<Relationship>): Set<Shape> {
        return inner.walkShapes(shape) { rel -> predicate.test(rel) && rel.direction == RelationshipDirection.DIRECTED }
    }
}

/**
 * Transforms `@required` member shapes into non-required member shapes
 */
object RequiredMemberTransform {
    private data class MemberShapeTransformation(
        val memberToChange: MemberShape,
        val traitsToKeep: List<Trait>,
    )

    private val constraintsToRemove = setOf(RequiredTrait::class.java)

    private fun Shape.hasRequiredMemberTrait() =
        constraintsToRemove.any(this::hasTrait)

    fun transform(model: Model): Model {
        val additionalNames = HashSet<ShapeId>()
        val walker = DirectedWalker(model)

        val transformations = model.operationShapes
            .flatMap { operation ->
                listOfNotNull(operation.input.orNull(), operation.output.orNull())
            }
            .map { model.expectShape(it) }
            .flatMap {
                walker.walkShapes(it)
            }
            .filter { it is StructureShape || it is ListShape || it is UnionShape || it is MapShape }
            .flatMap {
                it.requiredMembers()
            }
            .mapNotNull {
                it.makeNonRequired(model, additionalNames)
            }

        return applyTransformations(model, transformations)
    }

    /***
     * Returns a Model that has all the transformations applied on the original model.
     */
    private fun applyTransformations(
        model: Model,
        transformations: List<MemberShapeTransformation>,
    ): Model {
        if (transformations.isEmpty())
            return model

        val modelBuilder = model.toBuilder()
        val memberShapesToChange: MutableList<MemberShape> = mutableListOf()

        transformations.forEach {
            val changedMember = it.memberToChange.toBuilder()
                .traits(it.traitsToKeep)
                .build()
            memberShapesToChange.add(changedMember)
        }

        return ModelTransformer.create()
            .replaceShapes(modelBuilder.build(), memberShapesToChange)
    }

    /**
     * Returns a list of members that have constraint traits applied to them
     */
    private fun Shape.requiredMembers(): List<MemberShape> =
        this.allMembers.values.filter {
            it.hasRequiredMemberTrait()
        }

    /**
     * Returns the transformation that would be required to turn the given member shape
     * into a non-constrained member shape.
     */
    private fun MemberShape.makeNonRequired(
        model: Model,
        additionalNames: MutableSet<ShapeId>,
    ): MemberShapeTransformation? {
        val (requiredTrait, otherTraits) = this.allTraits.values
            .partition {
                constraintsToRemove.any { traitToRemove -> traitToRemove == it.javaClass }
            }

        // No transformation required in case the member shape has no required trait.
        if (requiredTrait.isEmpty())
            return null

        return MemberShapeTransformation(this, otherTraits)
    }
}
