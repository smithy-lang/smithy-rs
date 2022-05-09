/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.rust.codegen.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.rustlang.RustType
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
                // TODO This only applies really if the member shape is a structure member. We could check in
                //   the beginning of this case that our containing shape is a structure shape maybe?
                if (shape.isConstrained(base)) {
                    // TODO Constraint traits precedence has a role here.
//                    base.toSymbol(shape)
                    val targetShape = model.expectShape(shape.target)
                    this.toSymbol(targetShape)
                } else {
                    val targetShape = model.expectShape(shape.target)
                    this.toSymbol(targetShape)
                        // The member shape is not constrained, so it doesn't have `required`.
                        .makeOptional()
                }
            }
            else -> {
                // TODO Other shape types.
                base.toSymbol(shape)
            }
        }
}
