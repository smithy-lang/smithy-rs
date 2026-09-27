/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext

/**
 * Renders a shape's schema statics next to its generated Rust type and exposes them as `Type::SCHEMA`.
 *
 * This mirrors the client's convention, so protocol code reaches a shape's schema through the type itself
 * rather than through a parallel module tree.
 */
class ServerSchemaConstantGenerator(
    private val codegenContext: ServerCodegenContext,
    private val writer: RustWriter,
    private val shape: Shape,
) {
    fun render() {
        val symbol = codegenContext.symbolProvider.toSymbol(shape)
        ServerSchemaGenerator(codegenContext, writer, shape, schemaPrefix = symbol.name.uppercase()).renderSerializeOnly()
    }
}

/**
 * Generates the `crate::schema` descriptor modules:
 *
 * - `schema::operations` holds one `OperationSchema` static per operation bound to the service, carrying the
 *   operation shape ID, its input and output schemas, and its modeled errors. The operation's `@http` binding is
 *   read from its input schema, where [ServerSchemaGenerator] records it.
 * - `schema::service` holds the `ServiceSchema` static tying the service shape ID, version, protocols and
 *   operations together.
 *
 * Shape schemas are reached through the `SCHEMA` constants that [ServerSchemaConstantGenerator] renders next
 * to each generated type.
 */
class ServerServiceSchemaGenerator(
    private val codegenContext: ServerCodegenContext,
) {
    private val model = codegenContext.model
    private val service = codegenContext.serviceShape
    private val symbolProvider = codegenContext.symbolProvider
    private val smithySchema = RuntimeType.smithySchema(codegenContext.runtimeConfig)
    private val serverSchema =
        ServerCargoDependency.smithyHttpServer(codegenContext.runtimeConfig).toType().resolve("schema")
    private val codegenScope =
        arrayOf(
            "OperationSchema" to serverSchema.resolve("OperationSchema"),
            "Schema" to smithySchema.resolve("Schema"),
            "ServiceSchema" to serverSchema.resolve("ServiceSchema"),
            "ShapeId" to smithySchema.resolve("ShapeId"),
        )
    private val operations = TopDownIndex.of(model).getContainedOperations(service).sortedBy { it.id }
    private val serviceConstName = serviceSchemaConstName(service)

    fun render(rustCrate: RustCrate) {
        rustCrate.withModule(OperationsModule) {
            operations.forEach { renderOperation(this, it) }
        }
        rustCrate.withModule(ServiceModule) {
            renderService(this)
        }
    }

    private fun renderOperation(
        writer: RustWriter,
        operation: OperationShape,
    ) {
        val name = operationConstName(operation)
        val errorRefs = operation.errorsSet.sorted().joinToString(", ") { schemaRef(model.expectShape(it)) }
        writer.rustTemplate(
            """
            static ${name}_ERRORS: &[&#{Schema}<'static>] = &[$errorRefs];

            /// Descriptor for the `${escape(operation.id)}` operation.
            pub(crate) static $name: #{OperationSchema}<'static> = #{OperationSchema}::new(
                ${shapeIdExpr(operation.id)},
                ${schemaRef(operation.inputShape(model))},
                ${schemaRef(operation.outputShape(model))},
                ${name}_ERRORS,
            );
            """,
            *codegenScope,
        )
    }

    private fun renderService(writer: RustWriter) {
        val protocols = codegenContext.servedProtocols.joinToString(", ") { shapeIdExpr(it) }
        val operationRefs = operations.joinToString(", ") { "&super::operations::${operationConstName(it)}" }
        val version = service.version.takeIf { it.isNotEmpty() }?.let { "Some(${it.dq()})" } ?: "None"
        writer.rustTemplate(
            """
            static ${serviceConstName}_PROTOCOLS: &[#{ShapeId}<'static>] = &[$protocols];

            static ${serviceConstName}_OPERATIONS: &[&#{OperationSchema}<'static>] = &[$operationRefs];

            /// Descriptor for the `${escape(service.id)}` service and the operations bound to it.
            // Nothing in the crate reads the descriptors until the schema-driven request path lands, so keep
            // the dead-code lint quiet for this root and everything reachable from it.
            ##[allow(dead_code)]
            pub(crate) static $serviceConstName: #{ServiceSchema}<'static> = #{ServiceSchema}::new(
                ${shapeIdExpr(service.id)},
                $version,
                ${serviceConstName}_PROTOCOLS,
                ${serviceConstName}_OPERATIONS,
            );
            """,
            *codegenScope,
        )
    }

    private fun operationConstName(operation: OperationShape): String =
        symbolProvider.toSymbol(operation).name.toSnakeCase().uppercase()

    private fun schemaRef(shape: Shape): String = "${symbolProvider.toSymbol(shape).fullName}::SCHEMA"

    private fun shapeIdExpr(id: ShapeId): String =
        """#{ShapeId}::from_parts(${escape(id).dq()}, ${id.namespace.dq()}, ${id.name.dq()})"""

    /** Shape IDs contain `#`, which is the template escape character. */
    private fun escape(id: ShapeId): String = id.toString().replace("#", "##")

    companion object {
        private val SchemaModule = RustModule.pubCrate("schema")

        /** `crate::schema::operations`: one `OperationSchema` per operation bound to the service. */
        val OperationsModule = RustModule.pubCrate("operations", parent = SchemaModule)

        /** `crate::schema::service`: the `ServiceSchema` for the generated service. */
        val ServiceModule = RustModule.pubCrate("service", parent = SchemaModule)

        /** The name of the `ServiceSchema` static in `crate::schema::service`. */
        fun serviceSchemaConstName(service: ServiceShape): String = service.id.name.toSnakeCase().uppercase()
    }
}
