/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.ByteShape
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.EnumShape
import software.amazon.smithy.model.shapes.IntEnumShape
import software.amazon.smithy.model.shapes.IntegerShape
import software.amazon.smithy.model.shapes.LongShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShortShape
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
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.DirectedWalker
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.smithy.module
import software.amazon.smithy.rust.codegen.core.util.UNREACHABLE
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.generators.serverBuilderModule
import software.amazon.smithy.rust.codegen.server.smithy.traits.SyntheticStructureFromConstrainedMemberTrait

/*
 * This file contains utilities to work with constrained shapes.
 */

/**
 * Whether the shape has any trait that could cause a request to be rejected with a constraint violation, _whether
 * we support it or not_.
 */
fun Shape.hasConstraintTrait() = allConstraintTraits.any(this::hasTrait)

val allConstraintTraits =
    setOf(
        LengthTrait::class.java,
        PatternTrait::class.java,
        RangeTrait::class.java,
        UniqueItemsTrait::class.java,
        EnumTrait::class.java,
        RequiredTrait::class.java,
    )

val supportedStringConstraintTraits = setOf(LengthTrait::class.java, PatternTrait::class.java)

/**
 * Supported constraint traits for the `list` and `set` shapes.
 */
val supportedCollectionConstraintTraits =
    setOf(
        LengthTrait::class.java,
        UniqueItemsTrait::class.java,
    )

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
fun Shape.isDirectlyConstrained(symbolProvider: SymbolProvider): Boolean =
    when (this) {
        is StructureShape -> {
            // TODO(https://github.com/smithy-lang/smithy-rs/issues/1302, https://github.com/awslabs/smithy/issues/1179):
            //  The only reason why the functions in this file have
            //  to take in a `SymbolProvider` is because non-`required` blob streaming members are interpreted as
            //  `required`, so we can't use `member.isOptional` here.
            this.members().any { !symbolProvider.toSymbol(it).isOptional() && !it.hasNonNullDefault() }
        }

        else -> this.isDirectlyConstrainedHelper()
    }

/**
 * See [Shape.isDirectlyConstrained]
 *
 * We use this to check for constrained shapes in validation phase because the [SymbolProvider] has not yet been created
 */
fun Shape.isDirectlyConstrainedForValidation(): Boolean =
    when (this) {
        is StructureShape -> {
            // we use `member.isOptional` here because the issue outlined in (https://github.com/smithy-lang/smithy-rs/issues/1302)
            // should not be relevant in validation phase
            this.members().any { !it.isOptional && !it.hasNonNullDefault() }
        }

        // For alignment with
        // (https://github.com/smithy-lang/smithy-rs/blob/custom-validation-rfc/design/src/rfcs/rfc0047_custom_validation.md#terminology)
        // TODO: move to [isDirectlyConstrainerHelper] if they can be safely applied to [isDirectlyConstrained] without breaking implications
        is EnumShape -> true
        is IntEnumShape -> true
        // constraint traits on members shapes is completed: https://github.com/smithy-lang/smithy-rs/issues/1969
        is MemberShape -> !this.isOptional && !this.hasNonNullDefault()

        else -> this.isDirectlyConstrainedHelper()
    }

private fun Shape.isDirectlyConstrainedHelper(): Boolean =
    when (this) {
        is MapShape -> this.hasTrait<LengthTrait>()
        is StringShape -> this.hasTrait<EnumTrait>() || supportedStringConstraintTraits.any { this.hasTrait(it) }
        is CollectionShape -> supportedCollectionConstraintTraits.any { this.hasTrait(it) }
        is IntegerShape, is ShortShape, is LongShape, is ByteShape -> this.hasTrait<RangeTrait>()
        is BlobShape -> this.hasTrait<LengthTrait>()
        else -> false
    }

fun MemberShape.hasConstraintTraitOrTargetHasConstraintTrait(
    model: Model,
    symbolProvider: SymbolProvider,
): Boolean =
    this.isDirectlyConstrained(symbolProvider) || model.expectShape(this.target).isDirectlyConstrained(symbolProvider)

fun Shape.isTransitivelyButNotDirectlyConstrained(
    model: Model,
    symbolProvider: SymbolProvider,
): Boolean = !this.isDirectlyConstrained(symbolProvider) && this.canReachConstrainedShape(model, symbolProvider)

fun Shape.canReachConstrainedShape(
    model: Model,
    symbolProvider: SymbolProvider,
): Boolean =
    if (this is MemberShape) {
        // TODO(https://github.com/smithy-lang/smithy-rs/issues/1401) Constraint traits on member shapes are not implemented
        //  yet. Also, note that a walker over a member shape can, perhaps counterintuitively, reach the _containing_ shape,
        //  so we can't simply delegate to the `else` branch when we implement them.
        this.targetCanReachConstrainedShape(model, symbolProvider)
    } else {
        DirectedWalker(model).walkShapes(this).toSet().any { it.isDirectlyConstrained(symbolProvider) }
    }

/**
 * See [Shape.canReachConstrainedShape]
 *
 * We use this to check for constrained shapes in validation phase because the [SymbolProvider] has not yet been created
 */
fun Shape.canReachConstrainedShapeForValidation(model: Model): Boolean =
    if (this is MemberShape) {
        this.targetCanReachConstrainedShapeForValidation(model)
    } else {
        DirectedWalker(model).walkShapes(this).toSet().any { it.isDirectlyConstrainedForValidation() }
    }

fun MemberShape.targetCanReachConstrainedShape(
    model: Model,
    symbolProvider: SymbolProvider,
): Boolean = model.expectShape(this.target).canReachConstrainedShape(model, symbolProvider)

fun MemberShape.targetCanReachConstrainedShapeForValidation(model: Model): Boolean =
    model.expectShape(this.target).canReachConstrainedShapeForValidation(model)

fun Shape.hasPublicConstrainedWrapperTupleType(
    model: Model,
    publicConstrainedTypes: Boolean,
): Boolean =
    when (this) {
        is CollectionShape -> publicConstrainedTypes && supportedCollectionConstraintTraits.any(this::hasTrait)
        is MapShape -> publicConstrainedTypes && this.hasTrait<LengthTrait>()
        is StringShape -> !this.hasTrait<EnumTrait>() && (publicConstrainedTypes && supportedStringConstraintTraits.any(this::hasTrait))
        is IntegerShape, is ShortShape, is LongShape, is ByteShape -> publicConstrainedTypes && this.hasTrait<RangeTrait>()
        is MemberShape -> model.expectShape(this.target).hasPublicConstrainedWrapperTupleType(model, publicConstrainedTypes)
        is BlobShape -> publicConstrainedTypes && this.hasTrait<LengthTrait>()
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
fun workingWithPublicConstrainedWrapperTupleType(
    shape: Shape,
    model: Model,
    publicConstrainedTypes: Boolean,
): Boolean = shape.hasPublicConstrainedWrapperTupleType(model, publicConstrainedTypes)

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
): Boolean =
    !publicConstrainedTypes &&
        when (this) {
            is SimpleShape -> wouldHaveConstrainedWrapperTupleTypeWerePublicConstrainedTypesEnabled(model)
            is MemberShape ->
                model.expectShape(this.target)
                    .typeNameContainsNonPublicType(model, symbolProvider, publicConstrainedTypes)

            is CollectionShape -> this.canReachConstrainedShape(model, symbolProvider)
            is MapShape -> this.canReachConstrainedShape(model, symbolProvider)
            is StructureShape, is UnionShape -> false
            else -> UNREACHABLE("the above arms should be exhaustive, but we received shape: $this")
        }

/**
 * For synthetic shapes that are added to the model because of member constrained shapes, it returns
 * the "container" and "the member shape" that originally had the constraint trait. For all other
 * shapes, it returns null.
 */
fun Shape.overriddenConstrainedMemberInfo(): Pair<Shape, MemberShape>? {
    val trait = getTrait<SyntheticStructureFromConstrainedMemberTrait>() ?: return null
    return Pair(trait.container, trait.member)
}

/**
 * Returns the parent and the inline module that this particular shape should go in.
 */
fun Shape.getParentAndInlineModuleForConstrainedMember(
    symbolProvider: RustSymbolProvider,
    publicConstrainedTypes: Boolean,
): Pair<RustModule.LeafModule, RustModule.LeafModule>? {
    val overriddenTrait = getTrait<SyntheticStructureFromConstrainedMemberTrait>() ?: return null
    return if (overriddenTrait.container is StructureShape) {
        val structureModule = symbolProvider.toSymbol(overriddenTrait.container).module()
        val builderModule = overriddenTrait.container.serverBuilderModule(symbolProvider, !publicConstrainedTypes)
        Pair(structureModule, builderModule)
    } else {
        // For constrained member shapes, the ConstraintViolation code needs to go in an inline rust module
        // that is a descendant of the module that contains the extracted shape itself.
        return if (publicConstrainedTypes) {
            // Non-structured shape types need to go into their own module.
            val shapeSymbol = symbolProvider.toSymbol(this)
            val shapeModule = shapeSymbol.module()
            check(!shapeModule.parent.isInline()) {
                "Parent module of $id should not be an inline module."
            }
            Pair(shapeModule.parent as RustModule.LeafModule, shapeModule)
        } else {
            val name = RustReservedWords.escapeIfNeeded(overriddenTrait.container.id.name).toSnakeCase() + "_internal"
            val innerModule =
                RustModule.new(
                    name = name,
                    visibility = Visibility.PUBCRATE,
                    parent = ServerRustModule.Model,
                    inline = true,
                )

            Pair(ServerRustModule.Model, innerModule)
        }
    }
}
