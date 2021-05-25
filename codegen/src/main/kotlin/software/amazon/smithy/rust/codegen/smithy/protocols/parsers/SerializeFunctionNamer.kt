/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.protocols.parsers

import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.util.toSnakeCase

object SerializeFunctionNamer {
    fun name(model: Model, symbolProvider: SymbolProvider, shape: Shape): String {
        val symbolNameSnakeCase = symbolProvider.toSymbol(shape).name.toSnakeCase()
        return when (shape) {
            is OperationShape -> "serialize_operation_$symbolNameSnakeCase"
            is StructureShape -> "serialize_structure_$symbolNameSnakeCase"
            is UnionShape -> "serialize_union_$symbolNameSnakeCase"
            is MemberShape -> {
                val target = model.expectShape(shape.target, StructureShape::class.java)
                return "serialize_payload_${target.id.name.toSnakeCase()}_${shape.container.name.toSnakeCase()}"
            }
            else -> TODO("SerializerFunctionNamer.name: $shape")
        }
    }
}
