/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

/**
 * Emits the `TypeRegistry::builder()` entries for [shapes] — one
 * `insert_shape(Type::SCHEMA, |d| ...)` call per shape, where the closure
 * deserializes the shape from a `ShapeDeserializer` into a `TypeErasedBox`.
 *
 * Shared by the primary type registry ([TypeRegistryDecorator]), the
 * service-wide error registry, and the per-operation error registries
 * ([ErrorRegistryDecorator]), which differ only in which shapes they include.
 */
internal fun registryEntries(
    codegenContext: ClientCodegenContext,
    shapes: List<StructureShape>,
): Writable =
    writable {
        val symbolProvider = codegenContext.symbolProvider
        val rc = codegenContext.runtimeConfig
        val smithySchema = RuntimeType.smithySchema(rc)
        val smithyTypes = RuntimeType.smithyTypes(rc)
        val typeErasedBox = smithyTypes.resolve("type_erasure::TypeErasedBox")
        val shapeDeserializer = smithySchema.resolve("serde::ShapeDeserializer")
        val serdeError = smithySchema.resolve("serde::SerdeError")

        shapes.forEach { shape ->
            val type = symbolProvider.toSymbol(shape)
            rustTemplate(
                """
                .insert_shape(
                    #{Type}::SCHEMA,
                    |d: &mut dyn #{ShapeDeserializer}| -> #{Result}<#{TypeErasedBox}, #{SerdeError}> {
                        #{Result}::Ok(#{TypeErasedBox}::new(#{Type}::deserialize(d)?))
                    },
                )
                """,
                "Type" to type,
                "ShapeDeserializer" to shapeDeserializer,
                "TypeErasedBox" to typeErasedBox,
                "SerdeError" to serdeError,
                "Result" to RuntimeType.std.resolve("result::Result"),
            )
        }
    }
