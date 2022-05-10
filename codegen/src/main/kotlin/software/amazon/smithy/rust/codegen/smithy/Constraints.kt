package software.amazon.smithy.rust.codegen.smithy

import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.SetShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.LengthTrait
import software.amazon.smithy.model.traits.PatternTrait
import software.amazon.smithy.model.traits.RangeTrait
import software.amazon.smithy.model.traits.RequiredTrait
import software.amazon.smithy.rust.codegen.util.hasTrait

// TODO Unit test these functions and then refactor to use a `Walker` instead of hand-rolling our own DFS.

/**
 * A shape has a constraint trait if it has one of these traits attached.
 */
fun Shape.hasConstraintTrait() =
    this.hasTrait<RequiredTrait>() ||
        this.hasTrait<LengthTrait>() ||
        this.hasTrait<RangeTrait>() ||
        // `uniqueItems` is deprecated, so we ignore it.
        // this.hasTrait<UniqueItemsTrait>() ||
        this.hasTrait<PatternTrait>()

/**
 * A shape is constrained if:
 *
 *     - it has a constraint trait, or;
 *     - in the case of it being an aggregate shape, one of its member shapes has a constraint trait.
 *
 * Note that an aggregate shape whose member shapes do not have constraint traits but that has a member whose target is
 * a constrained shape is _not_ constrained.
 *
 * At the moment the only supported constraint trait is `required`, which can only be attached to structure member shapes.
 */
fun Shape.isConstrained(symbolProvider: SymbolProvider) = when (this) {
    is StructureShape -> {
        // TODO(https://github.com/awslabs/smithy-rs/issues/1302): The only reason why the functions in this file have
        //     to take in a `SymbolProvider` is because non-`required` blob streaming members are interpreted as
        //     `required`, so we can't use `member.isOptional` here.
        this.members().map { symbolProvider.toSymbol(it) }.any { !it.isOptional() }
    }
    else -> {
        // this.hasConstraintTrait()
        false
    }
}

fun StructureShape.canReachConstrainedShape(model: Model, symbolProvider: SymbolProvider, visited: Set<Shape> = emptySet()): Boolean {
    if (this.isConstrained(symbolProvider)) {
        return true
    }

    return this.members().map { model.expectShape(it.target) }.any {
        it.isConstrained(symbolProvider) || unconstrainedShapeCanReachConstrainedShape(it, model, symbolProvider, visited)
    }
}

fun CollectionShape.canReachConstrainedShape(model: Model, symbolProvider: SymbolProvider, visited: Set<Shape> = emptySet()): Boolean {
    if (this.isConstrained(symbolProvider)) {
        return true
    }

    val member = model.expectShape(this.member.target)

    return member.isConstrained(symbolProvider) || unconstrainedShapeCanReachConstrainedShape(member, model, symbolProvider, visited)
}

fun ListShape.canReachConstrainedShape(model: Model, symbolProvider: SymbolProvider) = (this as CollectionShape).canReachConstrainedShape(model, symbolProvider)
fun SetShape.canReachConstrainedShape(model: Model, symbolProvider: SymbolProvider) = (this as CollectionShape).canReachConstrainedShape(model, symbolProvider)

fun MapShape.canReachConstrainedShape(model: Model, symbolProvider: SymbolProvider, visited: Set<Shape> = emptySet()): Boolean {
    if (this.isConstrained(symbolProvider)) {
        return true
    }

    val key = model.expectShape(this.key.target)
    val value = model.expectShape(this.value.target)

    return key.isConstrained(symbolProvider) || value.isConstrained(symbolProvider) || unconstrainedShapeCanReachConstrainedShape(value, model, symbolProvider, visited)
}

fun MemberShape.requiresNewtype() =
    // Note that member shapes whose only constraint trait is `required` do not require a newtype.
    this.hasTrait<LengthTrait>() ||
            this.hasTrait<RangeTrait>() ||
            // `uniqueItems` is deprecated, so we ignore it.
            // this.hasTrait<UniqueItemsTrait>() ||
            this.hasTrait<PatternTrait>()

fun MemberShape.canReachConstrainedShape(model: Model, symbolProvider: SymbolProvider): Boolean =
    this.isConstrained(symbolProvider) || this.targetCanReachConstrainedShape(model, symbolProvider)

// TODO Callers should use `MemberShape.canReachConstrainedShape`, and this function should be inlined.
fun MemberShape.targetCanReachConstrainedShape(model: Model, symbolProvider: SymbolProvider): Boolean =
    when (val targetShape = model.expectShape(this.target)) {
        // TODO Use is CollectionShape
        is ListShape -> targetShape.asListShape().get().canReachConstrainedShape(model, symbolProvider)
        is SetShape -> targetShape.asSetShape().get().canReachConstrainedShape(model, symbolProvider)
        is MapShape -> targetShape.asMapShape().get().canReachConstrainedShape(model, symbolProvider)
        is StructureShape -> targetShape.asStructureShape().get().canReachConstrainedShape(model, symbolProvider)
        else -> false
    }

private fun unconstrainedShapeCanReachConstrainedShape(shape: Shape, model: Model, symbolProvider: SymbolProvider, visited: Set<Shape>): Boolean {
    check(!shape.isConstrained(symbolProvider)) { "This function can only be called with unconstrained shapes" }

    if (visited.contains(shape)) {
        return false
    }

    val newVisited = setOf(shape).plus(visited)

    return when (shape) {
        is StructureShape -> shape.canReachConstrainedShape(model, symbolProvider, newVisited)
        is CollectionShape -> shape.canReachConstrainedShape(model, symbolProvider, newVisited)
        is MapShape -> shape.canReachConstrainedShape(model, symbolProvider, newVisited)
        // TODO(https://github.com/awslabs/smithy-rs/pull/1199) Constraint traits on simple shapes.
        else -> false
    }
}
