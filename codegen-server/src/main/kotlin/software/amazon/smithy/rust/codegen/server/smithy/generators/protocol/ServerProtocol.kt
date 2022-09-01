/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators.protocol

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
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
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.orNull

interface ServerProtocol {
    fun coreProtocol(): Protocol

    fun markerStruct(): RuntimeType

    fun routerType(): RuntimeType

    // TODO(Decouple): Perhaps this should lean on a Rust interface.
    fun routerConstruction(service: ServiceShape, operationValues: Iterable<String>, model: Model): Writable

    companion object {
        fun fromCoreProtocol(coreCodegenContext: CoreCodegenContext, protocol: Protocol): ServerProtocol {
            val serverProtocol = when (protocol) {
                is AwsJson -> AwsJsonServerProtocol(coreCodegenContext, protocol)
                is RestJson -> RestJsonServerProtocol(coreCodegenContext, protocol)
                else -> throw IllegalStateException("unsupported protocol")
            }
            return serverProtocol
        }
    }
}

class AwsJsonServerProtocol(
    coreCodegenContext: CoreCodegenContext,
    private val coreProtocol: AwsJson,
) : ServerProtocol {
    private val runtimeConfig = coreCodegenContext.runtimeConfig
    private val codegenScope = arrayOf(
        "SmithyHttpServer" to ServerCargoDependency.SmithyHttpServer(runtimeConfig).asType(),
    )
    private val symbolProvider = coreCodegenContext.symbolProvider

    override fun coreProtocol(): Protocol {
        return coreProtocol
    }

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

    override fun routerType(): RuntimeType {
        return RuntimeType("AwsJsonRouter", ServerCargoDependency.SmithyHttpServer(runtimeConfig), "${runtimeConfig.crateSrcPrefix}_http_server::routing::routers::aws_json")
    }

    override fun routerConstruction(service: ServiceShape, operationValues: Iterable<String>, model: Model): Writable = writable {
        val operationShapes = service.operations.mapNotNull { model.getShape(it).orNull() }.mapNotNull { it as? OperationShape }
        // TODO(restore): This causes a panic: "symbol visitor should not be invoked in service shapes"
        // val serviceName = symbolProvider.toSymbol(service).name
        val serviceName = service.id.name
        val pairs = writable {
            for ((operation, operationValue) in operationShapes.zip(operationValues)) {
                val operationName = symbolProvider.toSymbol(operation).name
                val key = "String::from($serviceName.$operationName)".dq()
                rustTemplate(
                    """
                    (
                        String::from($key),
                        #{SmithyHttpServer}::routing::Route::from_box_clone_service($operationValue)
                    )
                    """,
                    *codegenScope,
                )
            }
        }
        rustTemplate(
            """
            #{Router}::from_iter(#{Pairs:W})
            """,
            "Router" to routerType(), "Pairs" to pairs,
        )
    }
}

class RestJsonServerProtocol(
    coreCodegenContext: CoreCodegenContext,
    private val coreProtocol: RestJson,
) : ServerProtocol {
    private val runtimeConfig = coreCodegenContext.runtimeConfig
    private val codegenScope = arrayOf(
        "SmithyHttpServer" to ServerCargoDependency.SmithyHttpServer(runtimeConfig).asType(),
    )
    private val symbolProvider = coreCodegenContext.symbolProvider

    override fun coreProtocol(): Protocol {
        return coreProtocol
    }

    override fun markerStruct(): RuntimeType {
        return RuntimeType("AwsRestJson1", ServerCargoDependency.SmithyHttpServer(runtimeConfig), "${runtimeConfig.crateSrcPrefix}_http_server::protocols")
    }

    override fun routerType(): RuntimeType {
        return RuntimeType("RestRouter", ServerCargoDependency.SmithyHttpServer(runtimeConfig), "${runtimeConfig.crateSrcPrefix}_http_server::routing::routers::rest")
    }

    override fun routerConstruction(service: ServiceShape, operationValues: Iterable<String>, model: Model): Writable = writable {
        val operationShapes = service.operations.mapNotNull { model.getShape(it).orNull() }.mapNotNull { it as? OperationShape }
        // TODO(restore): This causes a panic: "symbol visitor should not be invoked in service shapes"
        // val serviceName = symbolProvider.toSymbol(service).name
        val serviceName = service.id.name
        val pairs = writable {
            for ((operationShape, operationValue) in operationShapes.zip(operationValues)) {
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
                        #{SmithyHttpServer}::routing::Route::new($operationValue)
                    ),
                    """,
                    "Key" to key,
                    *codegenScope,
                )
            }
        }
        rustTemplate(
            """
            #{Router}::from_iter([#{Pairs:W}])
            """,
            "Router" to routerType(), "Pairs" to pairs,
        )
    }
}
