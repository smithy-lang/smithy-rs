/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.NullableIndex
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.traits.LengthTrait
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.smithy.Models
import software.amazon.smithy.rust.codegen.smithy.RustBoxTrait
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.WrappingSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.contextName
import software.amazon.smithy.rust.codegen.smithy.isConstrained
import software.amazon.smithy.rust.codegen.smithy.locatedIn
import software.amazon.smithy.rust.codegen.smithy.makeOptional
import software.amazon.smithy.rust.codegen.smithy.makeRustBoxed
import software.amazon.smithy.rust.codegen.smithy.symbolBuilder
import software.amazon.smithy.rust.codegen.util.hasTrait
import software.amazon.smithy.rust.codegen.util.toPascalCase

// TODO Docs. This symbol provider is wrapped by the other ones.
// TODO Unit tests.
class PublicConstrainedShapeSymbolProvider(
    private val base: RustSymbolProvider,
    private val model: Model,
    private val serviceShape: ServiceShape,
) : WrappingSymbolProvider(base) {
    private val nullableIndex = NullableIndex.of(model)

    private fun publicConstrainedSymbolForCollectionOrMapShape(shape: Shape): Symbol {
        check(shape is CollectionShape || shape is MapShape)

        val rustType = RustType.Opaque(shape.contextName(serviceShape).toPascalCase())
        return symbolBuilder(shape, rustType).locatedIn(Models).build()
    }

    override fun toSymbol(shape: Shape): Symbol {
        if (!shape.isMemberShape && !shape.isConstrained(base)) {
            return base.toSymbol(shape)
        }

        return when (shape) {
            is MapShape -> {
                check(shape.hasTrait<LengthTrait>()) { "Only the `length` constraint trait can be applied to maps" }
                publicConstrainedSymbolForCollectionOrMapShape(shape)
            }
            is MemberShape -> {
                // TODO member shapes can have constraint traits (constraint trait precedence).
                val target = model.expectShape(shape.target)
                val targetSymbol = this.toSymbol(target)
                // Handle boxing first so we end up with `Option<Box<_>>`, not `Box<Option<_>>`.
                handleOptionality(handleRustBoxing(targetSymbol, shape), shape)
            }
            is StringShape -> {
                val rustType = RustType.Opaque(shape.contextName(serviceShape).toPascalCase())
                symbolBuilder(shape, rustType).locatedIn(Models).build()
            }
            else -> base.toSymbol(shape)
        }
    }

    // TODO The following two methods have been copied from `SymbolVisitor.kt`
    private fun handleRustBoxing(symbol: Symbol, shape: MemberShape): Symbol =
        if (shape.hasTrait<RustBoxTrait>()) {
            symbol.makeRustBoxed()
        } else symbol

    private fun handleOptionality(symbol: Symbol, member: MemberShape): Symbol =
        if (member.isRequired) {
            symbol
        } else if (nullableIndex.isNullable(member)) {
            symbol.makeOptional()
        } else {
            symbol
        }
}
