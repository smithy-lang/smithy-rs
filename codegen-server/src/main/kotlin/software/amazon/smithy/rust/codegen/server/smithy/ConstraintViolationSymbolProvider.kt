/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.smithy.Models
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.Unconstrained
import software.amazon.smithy.rust.codegen.smithy.WrappingSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.canReachConstrainedShape
import software.amazon.smithy.rust.codegen.smithy.contextName
import software.amazon.smithy.rust.codegen.smithy.generators.builderSymbol
import software.amazon.smithy.rust.codegen.smithy.isDirectlyConstrained
import software.amazon.smithy.rust.codegen.smithy.rustType
import software.amazon.smithy.rust.codegen.util.toSnakeCase

// TODO Unit tests.
// TODO Docs.
class ConstraintViolationSymbolProvider(
    private val base: RustSymbolProvider,
    private val model: Model,
    private val serviceShape: ServiceShape,
) : WrappingSymbolProvider(base) {
    private val constraintViolationName = "ConstraintViolation"

    private fun constraintViolationSymbolForCollectionOrMapShape(shape: Shape): Symbol {
        check(shape is CollectionShape || shape is MapShape)

        val symbol = base.toSymbol(shape)
        val constraintViolationNamespace =
            "${symbol.namespace.let { it.ifEmpty { "crate::${Models.namespace}" } }}::${
                RustReservedWords.escapeIfNeeded(
                    shape.contextName(serviceShape).toSnakeCase()
                )
            }"
        val rustType = RustType.Opaque(constraintViolationName, constraintViolationNamespace)
        return Symbol.builder()
            .rustType(rustType)
            .name(rustType.name)
            .namespace(rustType.namespace, "::")
            .definitionFile(symbol.definitionFile)
            .build()
    }

    override fun toSymbol(shape: Shape): Symbol =
        when (shape) {
            is CollectionShape -> {
                // TODO Move these checks out.
                check(shape.canReachConstrainedShape(model, base))

                constraintViolationSymbolForCollectionOrMapShape(shape)
            }
            is MapShape -> {
                check(shape.canReachConstrainedShape(model, base))

                constraintViolationSymbolForCollectionOrMapShape(shape)
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
            is StringShape -> {
                check(shape.isDirectlyConstrained(base))

                val namespace = "crate::${Models.namespace}::${RustReservedWords.escapeIfNeeded(shape.contextName(serviceShape).toSnakeCase())}"
                val rustType = RustType.Opaque(constraintViolationName, namespace)
                Symbol.builder()
                    .rustType(rustType)
                    .name(rustType.name)
                    .namespace(rustType.namespace, "::")
                    .definitionFile(Models.filename)
                    .build()
            }
            // TODO(https://github.com/awslabs/smithy-rs/pull/1199) Other simple shapes can have constraint traits.
            else -> base.toSymbol(shape)
        }
}
