/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.protocols

import software.amazon.smithy.model.shapes.DocumentShape
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.SetShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.util.PANIC
import software.amazon.smithy.rust.codegen.util.toSnakeCase

fun RustSymbolProvider.lensName(prefix: String, root: Shape, path: List<MemberShape>): String {
    val base = shapeFunctionName("${prefix}lens", root)
    val rest = path.joinToString("_") { toMemberName(it) }
    return "${base}_$rest"
}

/**
 * Creates a unique name for a serialization function.
 *
 * The prefixes will look like the following (for grep):
 * - serialize_list
 * - serialize_map
 * - serialize_member
 * - serialize_operation
 * - serialize_set
 * - serialize_structure
 * - serialize_union
 */
fun RustSymbolProvider.serializeFunctionName(shape: Shape): String = shapeFunctionName("serialize", shape)

/**
 * Creates a unique name for a serialization function.
 *
 * The prefixes will look like the following (for grep):
 * - deser_list
 * - deser_map
 * - deser_member
 * - deser_operation
 * - deser_set
 * - deser_structure
 * - deser_union
 */
fun RustSymbolProvider.deserializeFunctionName(shape: Shape): String = shapeFunctionName("deser", shape)

fun ShapeId.toRustIdentifier(): String {
    return "${namespace.replace(".", "_").toSnakeCase()}_${name.toSnakeCase()}"
}

private fun RustSymbolProvider.shapeFunctionName(prefix: String, shape: Shape): String {
    val symbolNameSnakeCase = toSymbol(shape).fullName.replace("::", "_").toSnakeCase()
    return prefix + "_" + when (shape) {
        is ListShape -> "list_${shape.id.toRustIdentifier()}"
        is MapShape -> "map_${shape.id.toRustIdentifier()}"
        is MemberShape -> "member_${shape.container.toRustIdentifier()}_${shape.memberName.toSnakeCase()}"
        is OperationShape -> "operation_$symbolNameSnakeCase"
        is SetShape -> "set_${shape.id.toRustIdentifier()}"
        is StructureShape -> "structure_$symbolNameSnakeCase"
        is UnionShape -> "union_$symbolNameSnakeCase"
        is DocumentShape -> "document"
        else -> PANIC("SerializerFunctionNamer.name: $shape")
    }
}
