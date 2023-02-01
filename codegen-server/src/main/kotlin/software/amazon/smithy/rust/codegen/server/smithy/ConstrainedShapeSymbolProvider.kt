/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.NullableIndex
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.ByteShape
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.IntegerShape
import software.amazon.smithy.model.shapes.LongShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShortShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.traits.LengthTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.smithy.ModelsModule
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.WrappingSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.contextName
import software.amazon.smithy.rust.codegen.core.smithy.handleOptionality
import software.amazon.smithy.rust.codegen.core.smithy.handleRustBoxing
import software.amazon.smithy.rust.codegen.core.smithy.locatedIn
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.smithy.symbolBuilder
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.orNull
import software.amazon.smithy.rust.codegen.core.util.toPascalCase

/**
 * The [ConstrainedShapeSymbolProvider] returns, for a given _directly_
 * constrained shape, a symbol whose Rust type can hold the constrained values.
 *
 * For all shapes with supported traits directly attached to them, this type is
 * a [RustType.Opaque] wrapper tuple newtype holding the inner constrained
 * type.
 *
 * The symbols this symbol provider returns are always public and exposed to
 * the end user.
 *
 * This symbol provider is meant to be used "deep" within the wrapped symbol
 * providers chain, just above the core base symbol provider, `SymbolVisitor`.
 *
 * If the shape is _transitively but not directly_ constrained, use
 * [PubCrateConstrainedShapeSymbolProvider] instead, which returns symbols
 * whose associated types are `pub(crate)` and thus not exposed to the end
 * user.
 */
class ConstrainedShapeSymbolProvider(
    private val base: RustSymbolProvider,
    private val model: Model,
    private val serviceShape: ServiceShape,
) : WrappingSymbolProvider(base) {
    private val nullableIndex = NullableIndex.of(model)

    private fun publicConstrainedSymbolForMapOrCollectionShape(shape: Shape): Symbol {
        check(shape is MapShape || shape is CollectionShape)

        val rustType = RustType.Opaque(shape.contextName(serviceShape).toPascalCase())
        return symbolBuilder(shape, rustType).locatedIn(ModelsModule).build()
    }

    override fun toSymbol(shape: Shape): Symbol {
        return when (shape) {
            is MemberShape -> {
                // TODO(https://github.com/awslabs/smithy-rs/issues/1401) Member shapes can have constraint traits
                //  (constraint trait precedence).
                val target = model.expectShape(shape.target)
                val targetSymbol = this.toSymbol(target)
                // Handle boxing first, so we end up with `Option<Box<_>>`, not `Box<Option<_>>`.
                handleOptionality(handleRustBoxing(targetSymbol, shape), shape, nullableIndex, base.config().nullabilityCheckMode)
            }
            is MapShape -> {
                if (shape.isDirectlyConstrained(base)) {
                    check(shape.hasTrait<LengthTrait>()) {
                        "Only the `length` constraint trait can be applied to map shapes"
                    }
                    publicConstrainedSymbolForMapOrCollectionShape(shape)
                } else {
                    val keySymbol = this.toSymbol(shape.key)
                    val valueSymbol = this.toSymbol(shape.value)
                    symbolBuilder(shape, RustType.HashMap(keySymbol.rustType(), valueSymbol.rustType()))
                        .addReference(keySymbol)
                        .addReference(valueSymbol)
                        .build()
                }
            }
            is CollectionShape -> {
                if (shape.isDirectlyConstrained(base)) {
                    check(constrainedCollectionCheck(shape)) {
                        "Only the `length` and `uniqueItems` constraint traits can be applied to list shapes"
                    }
                    publicConstrainedSymbolForMapOrCollectionShape(shape)
                } else {
                    val inner = this.toSymbol(shape.member)
                    symbolBuilder(shape, RustType.Vec(inner.rustType())).addReference(inner).build()
                }
            }

            is StringShape, is IntegerShape, is ShortShape, is LongShape, is ByteShape, is BlobShape -> {
                if (shape.isDirectlyConstrained(base)) {
                    val rustType = RustType.Opaque(shape.contextName(serviceShape).toPascalCase())
                    symbolBuilder(shape, rustType).locatedIn(ModelsModule).build()
                } else {
                    base.toSymbol(shape)
                }
            }

            else -> base.toSymbol(shape)
        }
    }

    /**
     * Checks that the collection:
     *  - Has at least 1 supported constraint applied to it, and
     *  - That it has no unsupported constraints applied.
     */
    private fun constrainedCollectionCheck(shape: CollectionShape): Boolean {
        val supportedConstraintTraits = supportedCollectionConstraintTraits.mapNotNull { shape.getTrait(it).orNull() }.toSet()
        val allConstraintTraits = allConstraintTraits.mapNotNull { shape.getTrait(it).orNull() }.toSet()

        return supportedConstraintTraits.isNotEmpty() && allConstraintTraits.subtract(supportedConstraintTraits).isEmpty()
    }
}
