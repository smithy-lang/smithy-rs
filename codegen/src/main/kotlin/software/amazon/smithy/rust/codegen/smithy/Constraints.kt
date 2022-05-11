package software.amazon.smithy.rust.codegen.smithy

import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.neighbor.Walker
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.SetShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.LengthTrait
import software.amazon.smithy.model.traits.PatternTrait
import software.amazon.smithy.model.traits.RangeTrait
import software.amazon.smithy.model.traits.RequiredTrait
import software.amazon.smithy.rust.codegen.util.hasTrait

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

// TODO Maybe we should rename this to `isDirectlyConstrained`.
// TODO Perhaps it's best that we specialize and have `StringShape.isConstrained()`, `MapShape.isConstrained()` etc,
//   and we check only for the supported constraint traits and also only check for the ones compatible according to the
//   Smithy spec selectors.
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
    is MapShape -> this.hasConstraintTrait()
    is StringShape -> this.hasTrait<LengthTrait>() // TODO For the moment only `length` on string shapes is supported.
    else -> {
        // this.hasConstraintTrait()
        false
    }
}

fun StructureShape.canReachConstrainedShape(model: Model, symbolProvider: SymbolProvider) =
    Walker(model).walkShapes(this).toSet().any { it.isConstrained(symbolProvider) }

fun CollectionShape.canReachConstrainedShape(model: Model, symbolProvider: SymbolProvider) =
    Walker(model).walkShapes(this).toSet().any { it.isConstrained(symbolProvider) }

fun ListShape.canReachConstrainedShape(model: Model, symbolProvider: SymbolProvider) = (this as CollectionShape).canReachConstrainedShape(model, symbolProvider)
fun SetShape.canReachConstrainedShape(model: Model, symbolProvider: SymbolProvider) = (this as CollectionShape).canReachConstrainedShape(model, symbolProvider)

fun MapShape.canReachConstrainedShape(model: Model, symbolProvider: SymbolProvider) =
    Walker(model).walkShapes(this).toSet().any { it.isConstrained(symbolProvider) }

fun MemberShape.canReachConstrainedShape(model: Model, symbolProvider: SymbolProvider): Boolean =
    this.isConstrained(symbolProvider) || this.targetCanReachConstrainedShape(model, symbolProvider)

// TODO Callers should use `MemberShape.canReachConstrainedShape`, and this function should be inlined.
fun MemberShape.targetCanReachConstrainedShape(model: Model, symbolProvider: SymbolProvider): Boolean =
    when (val targetShape = model.expectShape(this.target)) {
        is CollectionShape -> targetShape.canReachConstrainedShape(model, symbolProvider)
        is MapShape -> targetShape.asMapShape().get().canReachConstrainedShape(model, symbolProvider)
        is StructureShape -> targetShape.asStructureShape().get().canReachConstrainedShape(model, symbolProvider)
        else -> false
    }

fun MemberShape.requiresNewtype() =
    // Note that member shapes whose only constraint trait is `required` do not require a newtype.
    this.hasTrait<LengthTrait>() ||
            this.hasTrait<RangeTrait>() ||
            // `uniqueItems` is deprecated, so we ignore it.
            // this.hasTrait<UniqueItemsTrait>() ||
            this.hasTrait<PatternTrait>()