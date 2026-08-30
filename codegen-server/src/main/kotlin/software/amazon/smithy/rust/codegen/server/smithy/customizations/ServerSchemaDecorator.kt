/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.customizations

import software.amazon.smithy.model.neighbor.Walker
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.generators.SchemaGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.shapeModuleName
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator
import software.amazon.smithy.rust.codegen.server.smithy.generators.serverSchemaShapeConstName
import software.amazon.smithy.rust.codegen.server.smithy.generators.serverSchemaShapeModule

/**
 * Emits schema statics for server shapes when `codegen.schemaSerde` is enabled.
 */
class ServerSchemaDecorator : ServerCodegenDecorator {
    override val name: String = "ServerSchemaDecorator"
    override val order: Byte = 0

    override fun extras(
        codegenContext: ServerCodegenContext,
        rustCrate: RustCrate,
    ) {
        if (!codegenContext.settings.codegenConfig.schemaSerde) {
            return
        }

        val schemaModule = RustModule.pubCrate("schema")
        // Walk every structure and union that needs a generated schema: shapes reached from the service's
        // operations, including input, output, error, and nested member shapes.
        for (shapeId in schemaClosure(codegenContext).sorted()) {
            val shape = codegenContext.model.expectShape(shapeId)
            // Operation inputs, operation outputs, modeled errors, and shared model shapes live under
            // `crate::schema::{input, output, error, model}`. Within each role namespace, reuse the
            // same per-shape module naming convention used for modeled Rust shapes.
            val schemaShapeModule = serverSchemaShapeModule(codegenContext, shape)
            val schemaRoleModule =
                RustModule.pubCrate(
                    schemaShapeModule.moduleName,
                    parent = schemaModule,
                )
            val shapeModule =
                RustModule.pubCrate(
                    codegenContext.symbolProvider.shapeModuleName(codegenContext.serviceShape, shape),
                    parent = schemaRoleModule,
            )
            rustCrate.withModule(shapeModule) {
                rust("##![allow(dead_code)]")
                val schemaConstName = serverSchemaShapeConstName(codegenContext, shape)
                // Emit the schema constant for this server shape into its role-specific module.
                // For example, `GetPokemonInput` becomes `crate::schema::input::get_pokemon_input::GET_POKEMON_INPUT`.
                SchemaGenerator(
                    codegenContext,
                    this,
                    shape,
                    schemaPrefix = schemaConstName,
                ).renderSchemaOnly()
            }
        }
    }

    private fun schemaClosure(codegenContext: ServerCodegenContext): Set<ShapeId> {
        val walker = Walker(codegenContext.model)
        val closure = mutableSetOf<ShapeId>()
        // Start from every operation input, output, and modeled error, then include modeled structures and unions
        // reachable from those roots. This gives protocols stable schema entry points while avoiding standalone
        // schema constants for primitives, collections, and other non-structure/union shapes.
        walker.walkShapes(codegenContext.serviceShape)
            .filterIsInstance<OperationShape>()
            .forEach { operation ->
                val roots =
                    listOf(operation.inputShape(codegenContext.model).id, operation.outputShape(codegenContext.model).id) +
                        operation.errors
                roots.forEach { rootId ->
                    walker.walkShapes(codegenContext.model.expectShape(rootId)).forEach { reachable: Shape ->
                        // `smithy.api#Unit` is the synthetic shape used when an operation has no input or output.
                        if (reachable.id != ShapeId.from("smithy.api#Unit") &&
                            (reachable is StructureShape || reachable is UnionShape)
                        ) {
                            closure.add(reachable.id)
                        }
                    }
                }
            }
        return closure
    }
}
