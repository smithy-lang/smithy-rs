package software.amazon.smithy.rust.codegen.core.smithy

import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.neighbor.Walker
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.model.traits.LengthTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait

// TODO Move to `Constraints.kt` in `codegen-server`. I can't yet because we need to split `Instantiator.kt`.

fun Shape.canReachConstrainedShape(model: Model, symbolProvider: SymbolProvider): Boolean =
    if (this is MemberShape) {
        // TODO(https://github.com/awslabs/smithy-rs/issues/1401) Constraint traits on member shapes are not implemented
        //  yet. Also, note that a walker over a member shape can, perhaps counterintuitively, reach the _containing_ shape,
        //  so we can't simply delegate to the `else` branch when we implement them.
        this.targetCanReachConstrainedShape(model, symbolProvider)
    } else {
        Walker(model).walkShapes(this).toSet().any { it.isDirectlyConstrained(symbolProvider) }
    }

fun MemberShape.targetCanReachConstrainedShape(model: Model, symbolProvider: SymbolProvider): Boolean =
    model.expectShape(this.target).canReachConstrainedShape(model, symbolProvider)

/**
 * We say a shape is _directly_ constrained if:
 *
 *     - it has a constraint trait, or;
 *     - in the case of it being an aggregate shape, one of its member shapes has a constraint trait.
 *
 * Note that an aggregate shape whose member shapes do not have constraint traits but that has a member whose target is
 * a constrained shape is _not_ directly constrained.
 *
 * At the moment only a subset of constraint traits are implemented on a subset of shapes; that's why we match against
 * a subset of shapes in each arm, and check for a subset of constraint traits attached to the shape in the arm's
 * (with these subsets being smaller than what [the spec] accounts for).
 *
 * [the spec]: https://awslabs.github.io/smithy/2.0/spec/constraint-traits.html
 */
fun Shape.isDirectlyConstrained(symbolProvider: SymbolProvider): Boolean = when (this) {
    is StructureShape -> {
        // TODO(https://github.com/awslabs/smithy-rs/issues/1302, https://github.com/awslabs/smithy/issues/1179):
        //  The only reason why the functions in this file have
        //  to take in a `SymbolProvider` is because non-`required` blob streaming members are interpreted as
        //  `required`, so we can't use `member.isOptional` here.
        this.members().map { symbolProvider.toSymbol(it) }.any { !it.isOptional() }
    }
    is MapShape -> this.hasTrait<LengthTrait>()
    is StringShape -> this.hasTrait<EnumTrait>() || this.hasTrait<LengthTrait>()
    else -> false
}
