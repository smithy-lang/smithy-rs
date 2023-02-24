/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.knowledge.NullableIndex
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.SimpleShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.WrappingSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.handleOptionality
import software.amazon.smithy.rust.codegen.core.smithy.handleRustBoxing
import software.amazon.smithy.rust.codegen.core.smithy.locatedIn
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.util.PANIC
import software.amazon.smithy.rust.codegen.core.util.toPascalCase
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase

/**
 * The [PubCrateConstrainedShapeSymbolProvider] returns, for a given
 * _transitively but not directly_ constrained shape, a symbol whose Rust type
 * can hold the constrained values.
 *
 * For collection and map shapes, this type is a [RustType.Opaque] wrapper
 * tuple newtype holding a container over the inner constrained type. For
 * member shapes, it's whatever their target shape resolves to.
 *
 * The class name is prefixed with `PubCrate` because the symbols it returns
 * have associated types that are generated as `pub(crate)`. See the
 * `PubCrate*Generator` classes to see how these types are generated.
 *
 * It is important that this symbol provider does _not_ wrap
 * [ConstrainedShapeSymbolProvider], since otherwise it will eventually
 * delegate to it and generate a symbol with a `pub` type.
 *
 * Note simple shapes cannot be transitively and not directly constrained at
 * the same time, so this symbol provider is only implemented for aggregate shapes.
 * The symbol provider will intentionally crash in such a case to avoid the caller
 * incorrectly using it.
 *
 * Note also that for the purposes of this symbol provider, a member shape is
 * transitively but not directly constrained only in the case where it itself
 * is not directly constrained and its target also is not directly constrained.
 *
 * If the shape is _directly_ constrained, use [ConstrainedShapeSymbolProvider]
 * instead.
 */
class PubCrateConstrainedShapeSymbolProvider(
    private val base: RustSymbolProvider,
    private val serviceShape: ServiceShape,
) : WrappingSymbolProvider(base) {
    private val nullableIndex = NullableIndex.of(model)

    private fun constrainedSymbolForCollectionOrMapShape(shape: Shape): Symbol {
        check(shape is CollectionShape || shape is MapShape)

        val name = constrainedTypeNameForCollectionOrMapShape(shape, serviceShape)
        val module = RustModule.new(
            RustReservedWords.escapeIfNeeded(name.toSnakeCase()),
            visibility = Visibility.PUBCRATE,
            parent = ServerRustModule.ConstrainedModule,
            inline = true,
        )
        val rustType = RustType.Opaque(name, module.fullyQualifiedPath())
        return Symbol.builder()
            .rustType(rustType)
            .name(rustType.name)
            .locatedIn(module)
            .build()
    }

    private fun errorMessage(shape: Shape) =
        "This symbol provider was called with $shape. However, it can only be called with a shape that is transitively constrained."

    override fun toSymbol(shape: Shape): Symbol {
        require(shape.isTransitivelyButNotDirectlyConstrained(model, base)) { errorMessage(shape) }

        return when (shape) {
            is CollectionShape, is MapShape -> {
                constrainedSymbolForCollectionOrMapShape(shape)
            }

            is MemberShape -> {
                require(!shape.hasConstraintTraitOrTargetHasConstraintTrait(model, base)) { errorMessage(shape) }

                val targetShape = model.expectShape(shape.target)

                if (targetShape is SimpleShape) {
                    base.toSymbol(shape)
                } else {
                    val targetSymbol = this.toSymbol(targetShape)
                    // Handle boxing first, so we end up with `Option<Box<_>>`, not `Box<Option<_>>`.
                    handleOptionality(
                        handleRustBoxing(targetSymbol, shape),
                        shape,
                        nullableIndex,
                        base.config.nullabilityCheckMode,
                    )
                }
            }

            is StructureShape, is UnionShape -> {
                // Structure shapes and union shapes always generate a [RustType.Opaque] constrained type.
                base.toSymbol(shape)
            }

            else -> {
                check(shape is SimpleShape)
                // The rest of the shape types are simple shapes, which are impossible to be transitively but not
                // directly constrained; directly constrained shapes generate public constrained types.
                PANIC(errorMessage(shape))
            }
        }
    }
}

fun constrainedTypeNameForCollectionOrMapShape(shape: Shape, serviceShape: ServiceShape): String {
    check(shape is CollectionShape || shape is MapShape)
    return "${shape.id.getName(serviceShape).toPascalCase()}Constrained"
}
