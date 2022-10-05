/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators.protocol

import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.asType
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.generators.http.RestRequestSpecGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.AwsJson
import software.amazon.smithy.rust.codegen.core.smithy.protocols.AwsJsonVersion
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.RestJson
import software.amazon.smithy.rust.codegen.core.smithy.protocols.RestXml
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.StructuredDataSerializerGenerator
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerRuntimeType
import software.amazon.smithy.rust.codegen.server.smithy.protocols.ServerAwsJsonSerializerGenerator

private fun allOperations(codegenContext: CodegenContext): List<OperationShape> {
    val index = TopDownIndex.of(codegenContext.model)
    return index.getContainedOperations(codegenContext.serviceShape).sortedBy { it.id }
}

interface ServerProtocol : Protocol {
    /** Returns the Rust marker struct enjoying `OperationShape`. */
    fun markerStruct(): RuntimeType

    /** Returns the Rust router type. */
    fun routerType(): RuntimeType

    /**
     * Returns the construction of the `routerType` given a `ServiceShape`, a collection of operation values
     * (`self.operation_name`, ...), and the `Model`.
     */
    fun routerConstruction(operationValues: Iterable<Writable>): Writable

    /**
     * Returns the name of the constructor to be used on the `Router` type, to instantiate a `Router` using this
     * protocol.
     */
    fun serverRouterRuntimeConstructor(): String

    /**
     * Returns a writable for the `RequestSpec` for an operation.
     */
    fun serverRouterRequestSpec(
        operationShape: OperationShape,
        operationName: String,
        serviceName: String,
        requestSpecModule: RuntimeType,
    ): Writable

    /**
     * In some protocols, such as restJson1,
     * when there is no modeled body input, content type must not be set and the body must be empty.
     * Returns a boolean indicating whether to perform this check.
     */
    fun serverContentTypeCheckNoModeledInput(): Boolean = false

    companion object {
        /** Upgrades the core protocol to a `ServerProtocol`. */
        fun fromCoreProtocol(protocol: Protocol): ServerProtocol = when (protocol) {
            is AwsJson -> ServerAwsJsonProtocol.fromCoreProtocol(protocol)
            is RestJson -> ServerRestJsonProtocol.fromCoreProtocol(protocol)
            is RestXml -> ServerRestXmlProtocol.fromCoreProtocol(protocol)
            else -> throw IllegalStateException("unsupported protocol")
        }
    }
}

class ServerAwsJsonProtocol(
    codegenContext: CodegenContext,
    awsJsonVersion: AwsJsonVersion,
) : AwsJson(codegenContext, awsJsonVersion), ServerProtocol {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenScope = arrayOf(
        "SmithyHttpServer" to ServerCargoDependency.SmithyHttpServer(runtimeConfig).asType(),
    )
    private val symbolProvider = codegenContext.symbolProvider
    private val service = codegenContext.serviceShape

    override fun structuredDataSerializer(operationShape: OperationShape): StructuredDataSerializerGenerator =
        ServerAwsJsonSerializerGenerator(codegenContext, httpBindingResolver, awsJsonVersion)

    companion object {
        fun fromCoreProtocol(awsJson: AwsJson): ServerAwsJsonProtocol = ServerAwsJsonProtocol(awsJson.codegenContext, awsJson.version)
    }

    override fun markerStruct(): RuntimeType {
        return when (version) {
            is AwsJsonVersion.Json10 -> {
                ServerRuntimeType.Protocol("AwsJson10", "aws_json_10", runtimeConfig)
            }
            is AwsJsonVersion.Json11 -> {
                ServerRuntimeType.Protocol("AwsJson11", "aws_json_11", runtimeConfig)
            }
        }
    }

    override fun routerType() = RuntimeType("AwsJsonRouter", ServerCargoDependency.SmithyHttpServer(runtimeConfig), "${runtimeConfig.crateSrcPrefix}_http_server::proto::aws_json::router")

    override fun routerConstruction(operationValues: Iterable<Writable>): Writable = writable {
        val allOperationShapes = allOperations(codegenContext)

        // TODO(https://github.com/awslabs/smithy-rs/issues/1724#issue-1367509999): This causes a panic: "symbol
        // visitor should not be invoked in service shapes"
        // val serviceName = symbolProvider.toSymbol(service).name
        val serviceName = service.id.name
        val pairs = writable {
            for ((operation, operationValue) in allOperationShapes.zip(operationValues)) {
                val operationName = symbolProvider.toSymbol(operation).name
                rustTemplate(
                    """
                    (
                        String::from("$serviceName.$operationName"),
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

    /**
     * Returns the operation name as required by the awsJson1.x protocols.
     */
    override fun serverRouterRequestSpec(
        operationShape: OperationShape,
        operationName: String,
        serviceName: String,
        requestSpecModule: RuntimeType,
    ) = writable {
        rust("""String::from("$serviceName.$operationName")""")
    }

    override fun serverRouterRuntimeConstructor() = when (version) {
        AwsJsonVersion.Json10 -> "new_aws_json_10_router"
        AwsJsonVersion.Json11 -> "new_aws_json_11_router"
    }
}

private fun restRouterType(runtimeConfig: RuntimeConfig) = RuntimeType("RestRouter", ServerCargoDependency.SmithyHttpServer(runtimeConfig), "${runtimeConfig.crateSrcPrefix}_http_server::proto::rest::router")

private fun restRouterConstruction(
    protocol: ServerProtocol,
    operationValues: Iterable<Writable>,
    codegenContext: CodegenContext,
): Writable = writable {
    val operations = allOperations(codegenContext)

    // TODO(https://github.com/awslabs/smithy-rs/issues/1724#issue-1367509999): This causes a panic: "symbol visitor
    //  should not be invoked in service shapes"
    //  val serviceName = symbolProvider.toSymbol(service).name
    val serviceName = codegenContext.serviceShape.id.name
    val pairs = writable {
        for ((operationShape, operationValue) in operations.zip(operationValues)) {
            val operationName = codegenContext.symbolProvider.toSymbol(operationShape).name
            val key = protocol.serverRouterRequestSpec(
                operationShape,
                operationName,
                serviceName,
                ServerCargoDependency.SmithyHttpServer(codegenContext.runtimeConfig).asType().member("routing::request_spec"),
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
                "SmithyHttpServer" to ServerCargoDependency.SmithyHttpServer(codegenContext.runtimeConfig).asType(),
            )
        }
    }
    rustTemplate(
        """
        #{Router}::from_iter([#{Pairs:W}])
        """,
        "Router" to protocol.routerType(),
        "Pairs" to pairs,
    )
}

class ServerRestJsonProtocol(
    codegenContext: CodegenContext,
) : RestJson(codegenContext), ServerProtocol {
    val runtimeConfig = codegenContext.runtimeConfig

    companion object {
        fun fromCoreProtocol(restJson: RestJson): ServerRestJsonProtocol = ServerRestJsonProtocol(restJson.codegenContext)
    }

    override fun markerStruct() = ServerRuntimeType.Protocol("AwsRestJson1", "rest_json_1", runtimeConfig)

    override fun routerType() = restRouterType(runtimeConfig)

    override fun routerConstruction(operationValues: Iterable<Writable>): Writable = restRouterConstruction(this, operationValues, codegenContext)

    override fun serverRouterRequestSpec(
        operationShape: OperationShape,
        operationName: String,
        serviceName: String,
        requestSpecModule: RuntimeType,
    ): Writable = RestRequestSpecGenerator(httpBindingResolver, requestSpecModule).generate(operationShape)

    override fun serverRouterRuntimeConstructor() = "new_rest_json_router"

    override fun serverContentTypeCheckNoModeledInput() = true
}

class ServerRestXmlProtocol(
    codegenContext: CodegenContext,
) : RestXml(codegenContext), ServerProtocol {
    val runtimeConfig = codegenContext.runtimeConfig

    companion object {
        fun fromCoreProtocol(restXml: RestXml): ServerRestXmlProtocol {
            return ServerRestXmlProtocol(restXml.codegenContext)
        }
    }

    override fun markerStruct() = ServerRuntimeType.Protocol("AwsRestXml", "rest_xml", runtimeConfig)

    override fun routerType() = restRouterType(runtimeConfig)

    override fun routerConstruction(operationValues: Iterable<Writable>): Writable = restRouterConstruction(this, operationValues, codegenContext)

    override fun serverRouterRequestSpec(
        operationShape: OperationShape,
        operationName: String,
        serviceName: String,
        requestSpecModule: RuntimeType,
    ): Writable = RestRequestSpecGenerator(httpBindingResolver, requestSpecModule).generate(operationShape)

    override fun serverRouterRuntimeConstructor() = "new_rest_xml_router"

    override fun serverContentTypeCheckNoModeledInput() = true
}
