/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.knowledge.ServiceIndex
import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext

/**
 * Generates service and operation descriptors that point at generated shape schema constants.
 */
class ServerServiceSchemaGenerator(
    private val codegenContext: ServerCodegenContext,
) {
    private val model = codegenContext.model
    private val service = codegenContext.serviceShape
    private val symbolProvider = codegenContext.symbolProvider
    private val smithySchema = RuntimeType.smithySchema(codegenContext.runtimeConfig)
    private val smithyHttpServer = ServerCargoDependency.smithyHttpServer(codegenContext.runtimeConfig).toType()
    private val operations =
        TopDownIndex.of(model).getContainedOperations(service)
            .sortedBy { it.id }

    fun renderSchemaModule(writer: RustWriter) {
        val serviceConstName = serviceConstName()
        writer.rust(
            """
            ##![allow(dead_code, unused_imports)]

            pub(crate) use service::$serviceConstName;
            """,
        )
    }

    fun renderOperationsModule(writer: RustWriter) {
        renderUnitSchema(writer)
        for (operation in operations) {
            renderOperationDescriptor(writer, operation)
        }
    }

    fun renderServiceModule(writer: RustWriter) {
        val protocolEntries =
            ServiceIndex.of(model).getProtocols(service).keys
                .sorted()
                .joinToString(",\n") { shapeIdExpr(it) }
        val operationRefs =
            operations.joinToString(",\n") { "&crate::schema::operations::${operationConstName(it)}" }
        val version = service.version?.dq() ?: "None"
        val versionExpr = service.version?.let { "Some($version)" } ?: "None"
        val prefix = serviceConstName()
        writer.rustTemplate(
            """
            static ${prefix}_SHAPE: #{Schema}<'static> = #{Schema}::new(
                ${shapeIdExpr(service.id)},
                #{ShapeType}::Service,
            );

            static ${prefix}_PROTOCOLS: &[#{ShapeId}<'static>] = &[
                $protocolEntries
            ];

            static ${prefix}_OPERATIONS: &[&#{OperationSchema}<'static>] = &[
                $operationRefs
            ];

            pub(crate) static $prefix: #{ServiceSchema}<'static> = #{ServiceSchema}::new(
                &${prefix}_SHAPE,
                $versionExpr,
                ${prefix}_PROTOCOLS,
                ${prefix}_OPERATIONS,
            );
            """,
            "Schema" to smithySchema.resolve("Schema"),
            "ShapeId" to smithySchema.resolve("ShapeId"),
            "ShapeType" to smithySchema.resolve("ShapeType"),
            "OperationSchema" to smithyHttpServer.resolve("schema::OperationSchema"),
            "ServiceSchema" to smithyHttpServer.resolve("schema::ServiceSchema"),
        )
    }

    private fun renderOperationDescriptor(
        writer: RustWriter,
        operation: OperationShape,
    ) {
        val prefix = operationConstName(operation)
        val input = operation.inputShape(model)
        val output = operation.outputShape(model)
        val errorRefs =
            operation.errorsSet
                .sorted()
                .joinToString(",\n") { errorId ->
                    val errorShape = model.expectShape(errorId)
                    schemaConstRef(errorShape)
                }
        writer.rustTemplate(
            """
            static ${prefix}_OPERATION_SHAPE: #{Schema}<'static> = #{Schema}::new(
                ${shapeIdExpr(operation.id)},
                #{ShapeType}::Operation,
            );

            static ${prefix}_ERRORS: &[&#{Schema}<'static>] = &[
                $errorRefs
            ];

            pub(crate) static $prefix: #{OperationSchema}<'static> = #{OperationSchema}::new(
                &${prefix}_OPERATION_SHAPE,
                ${schemaConstRef(input)},
                ${schemaConstRef(output)},
                ${prefix}_ERRORS,
            );
            """,
            "Schema" to smithySchema.resolve("Schema"),
            "ShapeId" to smithySchema.resolve("ShapeId"),
            "ShapeType" to smithySchema.resolve("ShapeType"),
            "OperationSchema" to smithyHttpServer.resolve("schema::OperationSchema"),
        )
    }

    private fun renderUnitSchema(writer: RustWriter) {
        writer.rustTemplate(
            """
            static UNIT_SCHEMA: #{Schema}<'static> = #{Schema}::new(
                #{ShapeId}::from_parts("smithy.api##Unit", "smithy.api", "Unit"),
                #{ShapeType}::Structure,
            );
            """,
            "Schema" to smithySchema.resolve("Schema"),
            "ShapeId" to smithySchema.resolve("ShapeId"),
            "ShapeType" to smithySchema.resolve("ShapeType"),
        )
    }

    private fun operationConstName(operation: OperationShape): String =
        symbolProvider.toSymbol(operation).name.toSnakeCase().uppercase()

    private fun serviceConstName(): String =
        service.id.name.toSnakeCase().uppercase()

    private fun schemaConstRef(shape: Shape): String =
        if (shape.id == ShapeId.from("smithy.api#Unit")) {
            // `smithy.api#Unit` represents an absent operation input/output and has no generated shape module.
            "&UNIT_SCHEMA"
        } else {
            serverSchemaShapeConstPath(codegenContext, shape)
        }

    private fun shapeIdExpr(id: ShapeId): String {
        val namespace = id.namespace
        val name = id.name
        val absolute = id.toString().replace("#", "##")
        return """#{ShapeId}::from_parts("$absolute", "$namespace", "$name")"""
    }
}
