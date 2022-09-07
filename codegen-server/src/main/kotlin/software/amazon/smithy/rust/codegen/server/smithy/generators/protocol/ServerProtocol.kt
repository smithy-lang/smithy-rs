/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators.protocol

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ResourceShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.smithy.CoreCodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.protocols.AwsJson
import software.amazon.smithy.rust.codegen.smithy.protocols.AwsJsonVersion
import software.amazon.smithy.rust.codegen.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.smithy.protocols.RestJson
import software.amazon.smithy.rust.codegen.smithy.protocols.RestXml
import software.amazon.smithy.rust.codegen.util.PANIC
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.orNull

fun allOperationShapes(service: ServiceShape, model: Model): List<OperationShape> {
    val resourceOperationShapes = service
        .resources
        .mapNotNull { model.getShape(it).orNull() }
        .mapNotNull { it as? ResourceShape }
        .flatMap { it.allOperations }
        .mapNotNull { model.getShape(it).orNull() }
        .mapNotNull { it as? OperationShape }
    val operationShapes = service.operations.mapNotNull { model.getShape(it).orNull() }.mapNotNull { it as? OperationShape }
    return resourceOperationShapes + operationShapes
}

interface ServerProtocol {
    /** Returns the core `Protocol`. */
    fun coreProtocol(): Protocol

    /** Returns the Rust marker struct enjoying `OperationShape`. */
    fun markerStruct(): RuntimeType

    /** Returns the Rust router type. */
    fun routerType(): RuntimeType

    // TODO(Decouple): Perhaps this should lean on a Rust interface.
    /**
     * Returns the construction of the `routerType` given a `ServiceShape`, a collection of operation values
     * (`self.operation_name`, ...), and the `Model`.
     */
    fun routerConstruction(service: ServiceShape, operationValues: Iterable<Writable>, model: Model): Writable

    companion object {
        /** Upgrades the core protocol to a `ServerProtocol`. */
        fun fromCoreProtocol(coreCodegenContext: CoreCodegenContext, protocol: Protocol): ServerProtocol {
            val serverProtocol = when (protocol) {
                is AwsJson -> ServerAwsJsonProtocol(coreCodegenContext, protocol)
                is RestJson -> ServerRestJsonProtocol(coreCodegenContext, protocol)
                is RestXml -> ServerRestXmlProtocol(coreCodegenContext, protocol)
                else -> throw IllegalStateException("unsupported protocol")
            }
            return serverProtocol
        }
    }
}

class ServerAwsJsonProtocol(
    coreCodegenContext: CoreCodegenContext,
    private val coreProtocol: AwsJson,
) : ServerProtocol {
    private val runtimeConfig = coreCodegenContext.runtimeConfig
    private val codegenScope = arrayOf(
        "SmithyHttpServer" to ServerCargoDependency.SmithyHttpServer(runtimeConfig).asType(),
    )
    private val symbolProvider = coreCodegenContext.symbolProvider

    override fun coreProtocol() = coreProtocol

    override fun markerStruct(): RuntimeType {
        val name = when (coreProtocol.version) {
            is AwsJsonVersion.Json10 -> {
                "AwsJson10"
            }
            is AwsJsonVersion.Json11 -> {
                "AwsJson11"
            }
            else -> throw IllegalStateException()
        }
        return RuntimeType(name, ServerCargoDependency.SmithyHttpServer(runtimeConfig), "${runtimeConfig.crateSrcPrefix}_http_server::protocols")
    }

    override fun routerType() = RuntimeType("AwsJsonRouter", ServerCargoDependency.SmithyHttpServer(runtimeConfig), "${runtimeConfig.crateSrcPrefix}_http_server::routing::routers::aws_json")

    override fun routerConstruction(service: ServiceShape, operationValues: Iterable<Writable>, model: Model): Writable = writable {
        val allOperationShapes = allOperationShapes(service, model)

        // TODO(restore): This causes a panic: "symbol visitor should not be invoked in service shapes"
        // val serviceName = symbolProvider.toSymbol(service).name
        val serviceName = service.id.name
        val pairs = writable {
            for ((operation, operationValue) in allOperationShapes.zip(operationValues)) {
                val operationName = symbolProvider.toSymbol(operation).name
                val key = "$serviceName.$operationName".dq()
                rustTemplate(
                    """
                    (
                        String::from($key),
                        #{SmithyHttpServer}::routing::Route::new(#{OperationValue:W})
                    ),
                    """,
                    "OperationValue" to operationValue,
                    *codegenScope,
                )
            }
        }
        rustTemplate(
            """
            #{Router}::from_iter([#{Pairs:W}])
            """,
            "Router" to routerType(),
            "Pairs" to pairs,
        )
    }
}

open class RestProtocol(
    coreCodegenContext: CoreCodegenContext,
    private val coreProtocol: Protocol,
) : ServerProtocol {
    val runtimeConfig = coreCodegenContext.runtimeConfig
    private val codegenScope = arrayOf(
        "SmithyHttpServer" to ServerCargoDependency.SmithyHttpServer(runtimeConfig).asType(),
    )
    private val symbolProvider = coreCodegenContext.symbolProvider

    override fun coreProtocol(): Protocol {
        return coreProtocol
    }

    override fun markerStruct(): RuntimeType {
        PANIC("marker structure needs to specified")
    }

    override fun routerType() = RuntimeType("RestRouter", ServerCargoDependency.SmithyHttpServer(runtimeConfig), "${runtimeConfig.crateSrcPrefix}_http_server::routing::routers::rest")

    override fun routerConstruction(service: ServiceShape, operationValues: Iterable<Writable>, model: Model): Writable = writable {
        val allOperationShapes = allOperationShapes(service, model)

        // TODO(restore): This causes a panic: "symbol visitor should not be invoked in service shapes"
        // val serviceName = symbolProvider.toSymbol(service).name
        val serviceName = service.id.name
        val pairs = writable {
            for ((operationShape, operationValue) in allOperationShapes.zip(operationValues)) {
                val operationName = symbolProvider.toSymbol(operationShape).name
                val key = coreProtocol.serverRouterRequestSpec(
                    operationShape,
                    operationName,
                    serviceName,
                    ServerCargoDependency.SmithyHttpServer(runtimeConfig).asType().member("routing::request_spec"),
                )
                rustTemplate(
                    """
                    (
                        #{Key:W},
                        #{SmithyHttpServer}::routing::Route::new(#{OperationValue:W})
                    ),
                    """,
                    "Key" to key,
                    "OperationValue" to operationValue,
                    *codegenScope,
                )
            }
        }
        rustTemplate(
            """
            #{Router}::from_iter([#{Pairs:W}])
            """,
            "Router" to routerType(),
            "Pairs" to pairs,
        )
    }
}

class ServerRestJsonProtocol(
    coreCodegenContext: CoreCodegenContext,
    coreProtocol: RestJson,
) : RestProtocol(coreCodegenContext, coreProtocol) {
    override fun markerStruct() = RuntimeType("AwsRestJson1", ServerCargoDependency.SmithyHttpServer(runtimeConfig), "${runtimeConfig.crateSrcPrefix}_http_server::protocols")
}

class ServerRestXmlProtocol(
    coreCodegenContext: CoreCodegenContext,
    coreProtocol: RestXml,
) : RestProtocol(coreCodegenContext, coreProtocol) {
    override fun markerStruct() = RuntimeType("AwsRestXml", ServerCargoDependency.SmithyHttpServer(runtimeConfig), "${runtimeConfig.crateSrcPrefix}_http_server::protocols")
}
