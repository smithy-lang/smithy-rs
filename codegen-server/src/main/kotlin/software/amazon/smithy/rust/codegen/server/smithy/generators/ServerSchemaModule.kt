/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.rust.codegen.core.smithy.protocols.shapeModuleName
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext

/**
 * Top-level module under `crate::schema` where a shape's schema constant is emitted.
 */
enum class ServerSchemaShapeModule(val moduleName: String) {
    Input("input"),
    Output("output"),
    Error("error"),
    Model("model"),
}

fun serverSchemaShapeModule(
    codegenContext: ServerCodegenContext,
    shape: Shape,
): ServerSchemaShapeModule =
    serverSchemaShapeModule(codegenContext.model, codegenContext.serviceShape, shape)

fun serverSchemaShapeModule(
    model: Model,
    service: ServiceShape,
    shape: Shape,
): ServerSchemaShapeModule {
    // Operation inputs, outputs, and modeled errors are protocol entry points. Other reachable
    // shapes are shared model schemas referenced from those entry-point schemas.
    val operations = TopDownIndex.of(model).getContainedOperations(service)
    val inputs = operations.map { it.inputShape(model).id }.toSet()
    val outputs = operations.map { it.outputShape(model).id }.toSet()
    val errors = operations.flatMap { it.errors }.toSet()

    return when (shape.id) {
        in inputs -> ServerSchemaShapeModule.Input
        in outputs -> ServerSchemaShapeModule.Output
        in errors -> ServerSchemaShapeModule.Error
        else -> ServerSchemaShapeModule.Model
    }
}

fun serverSchemaShapeConstName(
    codegenContext: ServerCodegenContext,
    shape: Shape,
): String = codegenContext.symbolProvider.toSymbol(shape).name.toSnakeCase().uppercase()

/**
 * Returns the module path that matches the generated schema layout.
 *
 * Example: `GetPokemonInput` maps to `crate::schema::input::get_pokemon_input`.
 */
fun serverSchemaShapePath(
    codegenContext: ServerCodegenContext,
    shape: Shape,
): String {
    val schemaRole = serverSchemaShapeModule(codegenContext, shape)
    // `shapeModuleName` returns a Rust-module-safe segment with a `shape_` prefix, so Smithy shapes
    // named like Rust keywords do not produce paths such as `crate::schema::model::self`.
    val shapeModule = codegenContext.symbolProvider.shapeModuleName(codegenContext.serviceShape, shape)
    return "crate::schema::${schemaRole.moduleName}::$shapeModule"
}

fun serverSchemaShapeConstPath(
    codegenContext: ServerCodegenContext,
    shape: Shape,
): String = "${serverSchemaShapePath(codegenContext, shape)}::${serverSchemaShapeConstName(codegenContext, shape)}"
