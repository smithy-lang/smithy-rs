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
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope

/**
 * Emits the `TypeRegistry::builder()` entries for [shapes] — one
 * `insert_shape(Type::SCHEMA, |d| ...)` call per shape, where the closure
 * deserializes the shape from a `ShapeDeserializer` into a `TypeErasedBox`.
 *
 * When [emitErrorConstructor] is set (for the service-wide and per-operation
 * error registries), each entry is emitted via `insert_error_shape` with an
 * additional closure that boxes the reified value as a `dyn std::error::Error`,
 * letting the schema-serde error path attach it as the `source` of an unhandled
 * error. The shapes must be `@error` structures (their generated types implement
 * `std::error::Error`).
 *
 * Shared by the primary type registry ([TypeRegistryDecorator]), the
 * service-wide error registry, and the per-operation error registries
 * ([ErrorRegistryDecorator]), which differ only in which shapes they include and
 * whether they carry the error constructor.
 */
internal fun registryEntries(
    codegenContext: ClientCodegenContext,
    shapes: List<StructureShape>,
    emitErrorConstructor: Boolean = false,
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
            if (emitErrorConstructor) {
                rustTemplate(
                    """
                    .insert_error_shape(
                        #{Type}::SCHEMA,
                        |d: &mut dyn #{ShapeDeserializer}| -> #{Result}<#{TypeErasedBox}, #{SerdeError}> {
                            #{Result}::Ok(#{TypeErasedBox}::new(#{Type}::deserialize(d)?))
                        },
                        |d: &mut dyn #{ShapeDeserializer}| -> #{Result}<#{Box}<dyn #{StdError} + #{Send} + #{Sync}>, #{SerdeError}> {
                            #{Result}::Ok(#{Box}::new(#{Type}::deserialize(d)?))
                        },
                    )
                    """,
                    *preludeScope,
                    "Type" to type,
                    "ShapeDeserializer" to shapeDeserializer,
                    "TypeErasedBox" to typeErasedBox,
                    "SerdeError" to serdeError,
                    "StdError" to RuntimeType.StdError,
                )
            } else {
                rustTemplate(
                    """
                    .insert_shape(
                        #{Type}::SCHEMA,
                        |d: &mut dyn #{ShapeDeserializer}| -> #{Result}<#{TypeErasedBox}, #{SerdeError}> {
                            #{Result}::Ok(#{TypeErasedBox}::new(#{Type}::deserialize(d)?))
                        },
                    )
                    """,
                    *preludeScope,
                    "Type" to type,
                    "ShapeDeserializer" to shapeDeserializer,
                    "TypeErasedBox" to typeErasedBox,
                    "SerdeError" to serdeError,
                )
            }
        }
    }
