/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.protocols

import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.util.toSnakeCase

/**
 * Creates a unique name for a serialization function.
 *
 * The prefixes will look like the following (for grep):
 * - serialize_operation
 * - serialize_structure
 * - serialize_union
 * - serialize_payload
 */
fun RustSymbolProvider.serializeFunctionName(shape: Shape): String = shapeFunctionName("serialize", shape)

private fun RustSymbolProvider.shapeFunctionName(prefix: String, shape: Shape): String {
    val symbolNameSnakeCase = toSymbol(shape).name.toSnakeCase()
    return prefix + "_" + when (shape) {
        is OperationShape -> "operation_$symbolNameSnakeCase"
        is StructureShape -> "structure_$symbolNameSnakeCase"
        is UnionShape -> "union_$symbolNameSnakeCase"
        is MemberShape -> "payload_${shape.target.name.toSnakeCase()}_${shape.container.name.toSnakeCase()}"
        else -> TODO("SerializerFunctionNamer.name: $shape")
    }
}
