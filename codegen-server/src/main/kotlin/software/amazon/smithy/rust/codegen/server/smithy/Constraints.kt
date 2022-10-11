/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
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
import software.amazon.smithy.model.traits.PatternTrait
import software.amazon.smithy.model.traits.RangeTrait
import software.amazon.smithy.model.traits.RequiredTrait
import software.amazon.smithy.model.traits.UniqueItemsTrait
import software.amazon.smithy.rust.codegen.core.smithy.canReachConstrainedShape
import software.amazon.smithy.rust.codegen.core.smithy.isDirectlyConstrained
import software.amazon.smithy.rust.codegen.core.util.UNREACHABLE
import software.amazon.smithy.rust.codegen.core.util.hasTrait

/**
 * This file contains utilities to work with constrained shapes.
 */

/**
 * Whether the shape has any trait that could cause a request to be rejected with a constraint violation, _whether
 * we support it or not_.
 */
fun Shape.hasConstraintTrait() =
    hasTrait<LengthTrait>() ||
        hasTrait<EnumTrait>() ||
        hasTrait<UniqueItemsTrait>() ||
        hasTrait<PatternTrait>() ||
        hasTrait<RangeTrait>() ||
        hasTrait<RequiredTrait>()

fun Shape.hasPublicConstrainedWrapperTupleType(model: Model, publicConstrainedTypes: Boolean): Boolean = when (this) {
    is MapShape -> publicConstrainedTypes && this.hasTrait<LengthTrait>()
    is StringShape -> !this.hasTrait<EnumTrait>() && (publicConstrainedTypes && this.hasTrait<LengthTrait>())
    is MemberShape -> model.expectShape(this.target).hasPublicConstrainedWrapperTupleType(model, publicConstrainedTypes)
    else -> false
}

fun Shape.wouldHaveConstrainedWrapperTupleTypeWerePublicConstrainedTypesEnabled(model: Model): Boolean =
    hasPublicConstrainedWrapperTupleType(model, true)

/**
 * Helper function to determine whether a shape will map to a _public_ constrained wrapper tuple type.
 *
 * This function is used in core code generators, so it takes in a [CodegenContext] that is downcast
 * to [ServerCodegenContext] when generating servers.
 */
fun workingWithPublicConstrainedWrapperTupleType(shape: Shape, model: Model, publicConstrainedTypes: Boolean): Boolean =
    shape.hasPublicConstrainedWrapperTupleType(model, publicConstrainedTypes)

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

fun MemberShape.hasConstraintTraitOrTargetHasConstraintTrait(model: Model, symbolProvider: SymbolProvider): Boolean =
    this.isDirectlyConstrained(symbolProvider) || (model.expectShape(this.target).isDirectlyConstrained(symbolProvider))

fun Shape.isTransitivelyButNotDirectlyConstrained(model: Model, symbolProvider: SymbolProvider): Boolean =
    !this.isDirectlyConstrained(symbolProvider) && this.canReachConstrainedShape(model, symbolProvider)
