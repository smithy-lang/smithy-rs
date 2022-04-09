package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.SetShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.LengthTrait
import software.amazon.smithy.model.traits.PatternTrait
import software.amazon.smithy.model.traits.RangeTrait
import software.amazon.smithy.rust.codegen.util.hasTrait

// TODO Find a better place for these primitives. Probably rust.codegen.util

fun Shape.hasConstraintTrait() =
    this.hasTrait<LengthTrait>() ||
            this.hasTrait<RangeTrait>() ||
            // `uniqueItems` is deprecated, so we ignore it.
            // this.hasTrait<UniqueItemsTrait>() ||
            this.hasTrait<PatternTrait>()

fun Shape.isConstrained() = when (this) {
    is StructureShape -> {
        this.members().any { it.isRequired }
    }
    else -> {
        this.hasConstraintTrait()
    }
}

fun StructureShape.canReachConstrainedShape(model: Model): Boolean {
    if (this.isConstrained()) {
        return true
    }

    return this.members().map { model.expectShape(it.target) }.any {
        it.isConstrained() || unconstrainedShapeCanReachConstrainedShape(it, model)
    }
}

fun CollectionShape.canReachConstrainedShape(model: Model): Boolean {
    if (this.isConstrained()) {
        return true
    }

    val member = model.expectShape(this.member.target)

    return member.isConstrained() || unconstrainedShapeCanReachConstrainedShape(member, model)
}

fun ListShape.canReachConstrainedShape(model: Model) = (this as CollectionShape).canReachConstrainedShape(model)
fun SetShape.canReachConstrainedShape(model: Model) = (this as CollectionShape).canReachConstrainedShape(model)

fun MapShape.canReachConstrainedShape(model: Model): Boolean {
    if (this.isConstrained()) {
        return true
    }

    val key = model.expectShape(this.key.target)
    val value = model.expectShape(this.key.target)

    return key.isConstrained() || value.isConstrained() || unconstrainedShapeCanReachConstrainedShape(value, model)
}

private fun unconstrainedShapeCanReachConstrainedShape(shape: Shape, model: Model): Boolean {
    check(!shape.isConstrained()) { "This function can only be called with unconstrained shapes" }

    return when (shape) {
        is StructureShape -> shape.canReachConstrainedShape(model)
        is CollectionShape -> shape.canReachConstrainedShape(model)
        is MapShape -> shape.canReachConstrainedShape(model)
        else -> false
    }
}