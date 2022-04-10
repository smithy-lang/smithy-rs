/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.rust.codegen.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.smithy.canReachConstrainedShape
import software.amazon.smithy.rust.codegen.smithy.isConstrained
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.Validation
import software.amazon.smithy.rust.codegen.smithy.WrappingSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.generators.builderSymbol
import software.amazon.smithy.rust.codegen.smithy.rustType
import software.amazon.smithy.rust.codegen.util.toPascalCase
import software.amazon.smithy.rust.codegen.util.toSnakeCase

// TODO Unit tests.
class ConstraintViolationSymbolProvider(
    private val base: RustSymbolProvider,
    private val model: Model,
    private val serviceShape: ServiceShape,
) : WrappingSymbolProvider(base) {
    override fun toSymbol(shape: Shape): Symbol =
        when {
            shape.isListShape -> {
                val listShape = shape.asListShape().get()
                check(listShape.canReachConstrainedShape(model, base))

                if (listShape.isConstrained(base)) {
                    TODO("Constraint traits on list shapes are currently not implemented")
                } else {
                    val name = "ValidationFailure"
                    // TODO This name is common in `UnconstrainedShapeSymbolProvider`, extract somewhere.
                    val unconstrainedListName = "${listShape.id.getName(serviceShape).toPascalCase()}Unconstrained"
                    val namespace = "crate::${Validation.namespace}::${RustReservedWords.escapeIfNeeded(unconstrainedListName.toSnakeCase())}"
                    val rustType = RustType.Opaque(name, namespace)
                    Symbol.builder()
                        .rustType(rustType)
                        .name(rustType.name)
                        .namespace(rustType.namespace, "::")
                        .definitionFile(Validation.filename)
                        .build()
                }
            }
            shape.isSetShape -> {
                TODO()
            }
            shape.isMapShape -> {
                TODO()
            }
            shape.isStructureShape -> {
                val structureShape = shape.asStructureShape().get()
                check(structureShape.canReachConstrainedShape(model, base))

                val builderSymbol = structureShape.builderSymbol(base)

                val name = "ValidationFailure"
                val namespace = builderSymbol.namespace
                val rustType = RustType.Opaque(name, namespace)
                Symbol.builder()
                    .rustType(rustType)
                    .name(rustType.name)
                    .namespace(rustType.namespace, "::")
                    .definitionFile(Validation.filename)
                    .build()
            }
            // TODO Simple shapes can have constraint traits.
            else -> base.toSymbol(shape)
        }
}
