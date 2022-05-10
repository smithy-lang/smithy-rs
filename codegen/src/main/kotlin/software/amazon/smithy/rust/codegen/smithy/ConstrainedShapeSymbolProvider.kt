/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.NullableIndex
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.SimpleShape
import software.amazon.smithy.model.traits.RequiredTrait
import software.amazon.smithy.rust.codegen.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.util.hasTrait
import software.amazon.smithy.rust.codegen.util.toPascalCase
import software.amazon.smithy.rust.codegen.util.toSnakeCase

fun constrainedTypeNameForListOrMapShape(shape: Shape, serviceShape: ServiceShape): String {
    check(shape.isListShape || shape.isMapShape)
    return "${shape.id.getName(serviceShape).toPascalCase()}Constrained"
}

class ConstrainedShapeSymbolProvider(
    private val base: RustSymbolProvider,
    private val model: Model,
    private val serviceShape: ServiceShape,
) : WrappingSymbolProvider(base) {
    private val nullableIndex = NullableIndex.of(model)

    private fun constrainedSymbolForCollectionOrMapShape(shape: Shape): Symbol {
        check(shape is ListShape || shape is MapShape)

        val name = constrainedTypeNameForListOrMapShape(shape, serviceShape)
        val namespace = "crate::${Constrained.namespace}::${RustReservedWords.escapeIfNeeded(name.toSnakeCase())}"
        val rustType = RustType.Opaque(name, namespace)
        return Symbol.builder()
            .rustType(rustType)
            .name(rustType.name)
            .namespace(rustType.namespace, "::")
            .definitionFile(Constrained.filename)
            .build()
    }

    // TODO The following two methods have been copied from `SymbolVisitor.kt`.
    private fun handleOptionality(symbol: Symbol, member: MemberShape): Symbol =
        if (member.isRequired) {
            symbol
        } else if (nullableIndex.isNullable(member)) {
            symbol.makeOptional()
        } else {
            symbol
        }

    /**
     * Boxes and returns [symbol], the symbol for the target of the member shape [shape], if [shape] is annotated with
     * [RustBoxTrait]; otherwise returns [symbol] unchanged.
     *
     * See `RecursiveShapeBoxer.kt` for the model transformation pass that annotates model shapes with [RustBoxTrait].
     */
    private fun handleRustBoxing(symbol: Symbol, shape: MemberShape): Symbol =
        if (shape.hasTrait<RustBoxTrait>()) {
            symbol.makeRustBoxed()
        } else symbol

    override fun toSymbol(shape: Shape): Symbol =
        when (shape) {
            is ListShape -> {
                check(shape.canReachConstrainedShape(model, base))

                if (shape.isConstrained(base)) {
//                    TODO("The `length` constraint trait on list shapes is currently not implemented")
                    constrainedSymbolForCollectionOrMapShape(shape)
                } else {
                    constrainedSymbolForCollectionOrMapShape(shape)
                }
            }
            is MapShape -> {
                check(shape.canReachConstrainedShape(model, base))

                if (shape.isConstrained(base)) {
//                    TODO("The `length` constraint trait on map shapes is currently not implemented")
                    constrainedSymbolForCollectionOrMapShape(shape)
                } else {
                    constrainedSymbolForCollectionOrMapShape(shape)
                }
            }
            is MemberShape -> {
                check(shape.canReachConstrainedShape(model, base)) {
                    shape.id
                }
                check(model.expectShape(shape.container).isStructureShape)

                if (shape.requiresNewtype()) {
                    //TODO()

                    // TODO What follows is wrong; here we should refer to an opaque type for the member shape.
                    //     But for now we add this to not make the validation model crash.

                    val targetShape = model.expectShape(shape.target)
                    if (targetShape is SimpleShape) {
                        base.toSymbol(shape)
                    } else {
                        val targetSymbol = this.toSymbol(targetShape)
                        // Handle boxing first so we end up with `Option<Box<_>>`, not `Box<Option<_>>`.
                        handleOptionality(handleRustBoxing(targetSymbol, shape), shape)
                    }
                } else {
                    val targetShape = model.expectShape(shape.target)

                    if (targetShape is SimpleShape) {
                        check(shape.hasTrait<RequiredTrait>()) {
                            "Targeting a simple shape that can reach a constrained shape and does not need a newtype; the member shape must be `required`"
                        }

                        base.toSymbol(shape)
                    } else {
                        val targetSymbol = this.toSymbol(targetShape)
                        // Handle boxing first so we end up with `Option<Box<_>>`, not `Box<Option<_>>`.
                        handleOptionality(handleRustBoxing(targetSymbol, shape), shape)
                    }
                }
            }
            else -> {
                // TODO Other shape types.
                base.toSymbol(shape)
            }
        }
}
