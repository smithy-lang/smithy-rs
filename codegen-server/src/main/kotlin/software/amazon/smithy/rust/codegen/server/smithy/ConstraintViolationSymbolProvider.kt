/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.SetShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.Unconstrained
import software.amazon.smithy.rust.codegen.smithy.WrappingSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.canReachConstrainedShape
import software.amazon.smithy.rust.codegen.smithy.generators.builderSymbol
import software.amazon.smithy.rust.codegen.smithy.isConstrained
import software.amazon.smithy.rust.codegen.smithy.rustType
import software.amazon.smithy.rust.codegen.smithy.unconstrainedTypeNameForListOrMapShape
import software.amazon.smithy.rust.codegen.util.toSnakeCase

// TODO Unit tests.
class ConstraintViolationSymbolProvider(
    private val base: RustSymbolProvider,
    private val model: Model,
    private val serviceShape: ServiceShape,
) : WrappingSymbolProvider(base) {
    private val constraintViolationName = "ConstraintViolation"

    private fun unconstrainedSymbolForListOrMapShape(shape: Shape): Symbol {
        check(shape is ListShape || shape is MapShape)

        // TODO Move ConstraintViolation type to the constrained namespace.
        val unconstrainedTypeName = unconstrainedTypeNameForListOrMapShape(shape, serviceShape)
        val namespace = "crate::${Unconstrained.namespace}::${RustReservedWords.escapeIfNeeded(unconstrainedTypeName.toSnakeCase())}"
        val rustType = RustType.Opaque(constraintViolationName, namespace)
        return Symbol.builder()
            .rustType(rustType)
            .name(rustType.name)
            .namespace(rustType.namespace, "::")
            .definitionFile(Unconstrained.filename)
            .build()
    }

    override fun toSymbol(shape: Shape): Symbol =
        when (shape) {
            is SetShape -> {
//                TODO("Set shapes can only contain some simple shapes, but constraint traits on simple shapes are not implemented")
                base.toSymbol(shape)
            }
            is ListShape -> {
                check(shape.canReachConstrainedShape(model, base))

                if (shape.isConstrained(base)) {
//                    TODO("The `length` constraint trait on list shapes is currently not implemented")
                    unconstrainedSymbolForListOrMapShape(shape)
                } else {
                    unconstrainedSymbolForListOrMapShape(shape)
                }
            }
            is MapShape -> {
                check(shape.canReachConstrainedShape(model, base))

                if (shape.isConstrained(base)) {
//                    TODO("The `length` constraint trait on map shapes is currently not implemented")
                    unconstrainedSymbolForListOrMapShape(shape)
                } else {
                    unconstrainedSymbolForListOrMapShape(shape)
                }
            }
            is StructureShape -> {
                check(shape.canReachConstrainedShape(model, base))

                val builderSymbol = shape.builderSymbol(base)

                val namespace = builderSymbol.namespace
                val rustType = RustType.Opaque(constraintViolationName, namespace)
                Symbol.builder()
                    .rustType(rustType)
                    .name(rustType.name)
                    .namespace(rustType.namespace, "::")
                    .definitionFile(Unconstrained.filename)
                    .build()
            }
            // TODO(https://github.com/awslabs/smithy-rs/pull/1199) Simple shapes can have constraint traits.
            else -> base.toSymbol(shape)
        }
}
