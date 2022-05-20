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
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.model.traits.LengthTrait
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.smithy.Models
import software.amazon.smithy.rust.codegen.smithy.RustBoxTrait
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.WrappingSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.contextName
import software.amazon.smithy.rust.codegen.smithy.isDirectlyConstrained
import software.amazon.smithy.rust.codegen.smithy.locatedIn
import software.amazon.smithy.rust.codegen.smithy.makeOptional
import software.amazon.smithy.rust.codegen.smithy.makeRustBoxed
import software.amazon.smithy.rust.codegen.smithy.rustType
import software.amazon.smithy.rust.codegen.smithy.symbolBuilder
import software.amazon.smithy.rust.codegen.util.hasTrait
import software.amazon.smithy.rust.codegen.util.toPascalCase

// TODO Docs. This symbol provider is wrapped by the other ones.
// TODO Unit tests.
class ConstrainedShapeSymbolProvider(
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
        return when (shape) {
            is MemberShape -> {
                // TODO member shapes can have constraint traits (constraint trait precedence).
                val target = model.expectShape(shape.target)
                val targetSymbol = this.toSymbol(target)
                // Handle boxing first so we end up with `Option<Box<_>>`, not `Box<Option<_>>`.
                handleOptionality(handleRustBoxing(targetSymbol, shape), shape)
            }
            is MapShape -> {
                if (shape.isDirectlyConstrained(base)) {
                    check(shape.hasTrait<LengthTrait>()) { "Only the `length` constraint trait can be applied to maps" }
                    publicConstrainedSymbolForCollectionOrMapShape(shape)
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
                // TODO Both arms return the same because we haven't implemented any constraint trait on collection shapes yet.
                if (shape.isDirectlyConstrained(base)) {
                    val inner = this.toSymbol(shape.member)
                    symbolBuilder(shape, RustType.Vec(inner.rustType())).addReference(inner).build()
                } else {
                    val inner = this.toSymbol(shape.member)
                    symbolBuilder(shape, RustType.Vec(inner.rustType())).addReference(inner).build()
                }
            }
            is StringShape -> {
                if (!shape.isDirectlyConstrained(base)) {
                    return base.toSymbol(shape)
                }

                if (shape.hasTrait<EnumTrait>()) {
                    // String shape has a constraint trait in addition to the `enum` trait.
                    // The `enum` trait takes precedence, so the base symbol provider will generate a Rust `enum`.
                    // We warn about this case in [ServerCodegenVisitor].
                    // See https://github.com/awslabs/smithy/issues/1121f for more information.
                    return base.toSymbol(shape)
                }

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
