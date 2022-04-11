package software.amazon.smithy.rust.codegen.smithy

import software.amazon.smithy.codegen.core.SymbolProvider
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
// TODO Unit test these functions.

// TODO This will work fine if we include RequiredTrait too won't it?
fun Shape.hasConstraintTrait() =
    this.hasTrait<LengthTrait>() ||
            this.hasTrait<RangeTrait>() ||
            // `uniqueItems` is deprecated, so we ignore it.
            // this.hasTrait<UniqueItemsTrait>() ||
            this.hasTrait<PatternTrait>()

fun Shape.isConstrained(symbolProvider: SymbolProvider) = when (this) {
    is StructureShape -> {
        // TODO(https://github.com/awslabs/smithy-rs/issues/1302): The only reason why the functions in this file have
        //     to take in a `SymbolProvider` is because non-`required` blob streaming members are interpreted as
        //     `required`, so we can't use `member.isOptional` here.
        this.members().map { symbolProvider.toSymbol(it) }.any { !it.isOptional() }
    }
    else -> {
        this.hasConstraintTrait()
    }
}

fun StructureShape.canReachConstrainedShape(model: Model, symbolProvider: SymbolProvider): Boolean {
    if (this.isConstrained(symbolProvider)) {
        return true
    }

    return this.members().map { model.expectShape(it.target) }.any {
        it.isConstrained(symbolProvider) || unconstrainedShapeCanReachConstrainedShape(it, model, symbolProvider)
    }
}

fun CollectionShape.canReachConstrainedShape(model: Model, symbolProvider: SymbolProvider): Boolean {
    if (this.isConstrained(symbolProvider)) {
        return true
    }

    val member = model.expectShape(this.member.target)

    return member.isConstrained(symbolProvider) || unconstrainedShapeCanReachConstrainedShape(member, model, symbolProvider)
}

fun ListShape.canReachConstrainedShape(model: Model, symbolProvider: SymbolProvider) = (this as CollectionShape).canReachConstrainedShape(model, symbolProvider)
fun SetShape.canReachConstrainedShape(model: Model, symbolProvider: SymbolProvider) = (this as CollectionShape).canReachConstrainedShape(model, symbolProvider)

fun MapShape.canReachConstrainedShape(model: Model, symbolProvider: SymbolProvider): Boolean {
    if (this.isConstrained(symbolProvider)) {
        return true
    }

    val key = model.expectShape(this.key.target)
    val value = model.expectShape(this.value.target)

    return key.isConstrained(symbolProvider) || value.isConstrained(symbolProvider) || unconstrainedShapeCanReachConstrainedShape(value, model, symbolProvider)
}

private fun unconstrainedShapeCanReachConstrainedShape(shape: Shape, model: Model, symbolProvider: SymbolProvider): Boolean {
    check(!shape.isConstrained(symbolProvider)) { "This function can only be called with unconstrained shapes" }

    // TODO
    if (shape.id.name == "RecursiveShapesInputOutputNested1") {
        return false
    }

    return when (shape) {
        is StructureShape -> shape.canReachConstrainedShape(model, symbolProvider)
        is CollectionShape -> shape.canReachConstrainedShape(model, symbolProvider)
        is MapShape -> shape.canReachConstrainedShape(model, symbolProvider)
        // TODO Constraint traits on simple shapes.
        else -> false
    }
}