/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy

import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.neighbor.Walker
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.SimpleShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.model.traits.LengthTrait
import software.amazon.smithy.rust.codegen.client.smithy.generators.CodegenTarget
import software.amazon.smithy.rust.codegen.core.util.UNREACHABLE
import software.amazon.smithy.rust.codegen.core.util.hasTrait

/**
 * This file contains utilities to work with constrained shapes.
 *
 * TODO Move this file to `core` or `server`.
 */

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
fun Shape.isDirectlyConstrained(symbolProvider: SymbolProvider) = when (this) {
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

fun Shape.hasPublicConstrainedWrapperTupleType(model: Model, publicConstrainedTypes: Boolean): Boolean = when (this) {
    is MapShape -> publicConstrainedTypes && this.hasTrait<LengthTrait>()
    is StringShape -> !this.hasTrait<EnumTrait>() && (publicConstrainedTypes && this.hasTrait<LengthTrait>())
    is MemberShape -> model.expectShape(this.target).hasPublicConstrainedWrapperTupleType(model, publicConstrainedTypes)
    else -> false
}

fun Shape.wouldHaveConstrainedWrapperTupleTypeWerePublicConstrainedTypesEnabled(model: Model) =
    hasPublicConstrainedWrapperTupleType(model, true)

/**
 * Helper function to determine whether a shape will map to a _public_ constrained wrapper tuple type.
 *
 * This function is used in core code generators, so it takes in a [CoreCodegenContext] that is downcast
 * to [ServerCodegenContext] when generating servers.
 */
fun workingWithPublicConstrainedWrapperTupleType(shape: Shape, coreCodegenContext: CoreCodegenContext) =
    coreCodegenContext.target == CodegenTarget.SERVER &&
        shape.hasPublicConstrainedWrapperTupleType(
            coreCodegenContext.model,
            (coreCodegenContext as ServerCodegenContext).settings.codegenConfig.publicConstrainedTypes,
        )

/**
 * Returns whether a shape's type _name_ contains a non-public type when `publicConstrainedTypes` is `false`.
 *
 * For example, a `Vec<crate::model::LengthString>` contains a non-public type, because `crate::model::LengthString`
 * is `pub(crate)` when `publicConstrainedTypes` is `false`
 *
 * Note that a structure shape's type _definition_ may contain non-public types, but its _name_ is always public.
 *
 * Note how we short-circuit on `publicConstrainedTypes = true`, but we still require it to be passed in instead of laying
 * the responsibility on the caller, for API safety usage.
 */
fun Shape.typeNameContainsNonPublicType(
    model: Model,
    symbolProvider: SymbolProvider,
    publicConstrainedTypes: Boolean,
): Boolean = !publicConstrainedTypes && when (this) {
    is SimpleShape -> wouldHaveConstrainedWrapperTupleTypeWerePublicConstrainedTypesEnabled(model)
    is MemberShape -> model.expectShape(this.target).typeNameContainsNonPublicType(model, symbolProvider, publicConstrainedTypes)
    is CollectionShape -> this.canReachConstrainedShape(model, symbolProvider)
    is MapShape -> this.canReachConstrainedShape(model, symbolProvider)
    is StructureShape, is UnionShape -> false
    else -> UNREACHABLE("the above arms should be exhaustive, but we received shape: $this")
}

fun MemberShape.targetCanReachConstrainedShape(model: Model, symbolProvider: SymbolProvider): Boolean =
    model.expectShape(this.target).canReachConstrainedShape(model, symbolProvider)

fun MemberShape.hasConstraintTraitOrTargetHasConstraintTrait(model: Model, symbolProvider: SymbolProvider) =
    this.isDirectlyConstrained(symbolProvider) || (model.expectShape(this.target).isDirectlyConstrained(symbolProvider))

fun Shape.isTransitivelyButNotDirectlyConstrained(model: Model, symbolProvider: SymbolProvider) =
    !this.isDirectlyConstrained(symbolProvider) && this.canReachConstrainedShape(model, symbolProvider)

fun Shape.canReachConstrainedShape(model: Model, symbolProvider: SymbolProvider) =
    if (this is MemberShape) {
        // TODO(https://github.com/awslabs/smithy-rs/issues/1401) Constraint traits on member shapes are not implemented
        //  yet. Also, note that a walker over a member shape can, perhaps counterintuitively, reach the _containing_ shape,
        //  so we can't simply delegate to the `else` branch when we implement them.
        this.targetCanReachConstrainedShape(model, symbolProvider)
    } else {
        Walker(model).walkShapes(this).toSet().any { it.isDirectlyConstrained(symbolProvider) }
    }
