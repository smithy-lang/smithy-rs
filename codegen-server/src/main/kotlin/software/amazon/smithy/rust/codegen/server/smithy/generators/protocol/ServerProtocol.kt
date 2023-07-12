/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators.protocol

import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.protocols.AwsJson
import software.amazon.smithy.rust.codegen.core.smithy.protocols.AwsJsonVersion
import software.amazon.smithy.rust.codegen.core.smithy.protocols.HttpBindingResolver
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.RestJson
import software.amazon.smithy.rust.codegen.core.smithy.protocols.RestXml
import software.amazon.smithy.rust.codegen.core.smithy.protocols.awsJsonFieldName
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.JsonParserCustomization
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.JsonParserGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.JsonParserSection
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.ReturnSymbolToParse
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.StructuredDataParserGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.restJsonFieldName
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.StructuredDataSerializerGenerator
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.ServerRuntimeType
import software.amazon.smithy.rust.codegen.server.smithy.canReachConstrainedShape
import software.amazon.smithy.rust.codegen.server.smithy.generators.http.RestRequestSpecGenerator
import software.amazon.smithy.rust.codegen.server.smithy.protocols.ServerAwsJsonSerializerGenerator
import software.amazon.smithy.rust.codegen.server.smithy.protocols.ServerRestJsonSerializerGenerator
import software.amazon.smithy.rust.codegen.server.smithy.targetCanReachConstrainedShape

interface ServerProtocol : Protocol {
    /** The path such that `aws_smithy_http_server::protocol::$path` points to the protocol's module. */
    val protocolModulePath: String

    /** Returns the Rust marker struct enjoying `OperationShape`. */
    fun markerStruct(): RuntimeType

    /** Returns the Rust router type. */
    fun routerType(): RuntimeType

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
     * Returns the Rust type of the `RequestSpec` for an operation.
     */
    fun serverRouterRequestSpecType(requestSpecModule: RuntimeType): RuntimeType

    /**
     * In some protocols, such as restJson1,
     * when there is no modeled body input, content type must not be set and the body must be empty.
     * Returns a boolean indicating whether to perform this check.
     */
    fun serverContentTypeCheckNoModeledInput(): Boolean = false

    /** The protocol-specific `RequestRejection` type. **/
    fun requestRejection(runtimeConfig: RuntimeConfig): RuntimeType =
        ServerCargoDependency.smithyHttpServer(runtimeConfig)
            .toType().resolve("protocol::$protocolModulePath::rejection::RequestRejection")

    /** The protocol-specific `ResponseRejection` type. **/
    fun responseRejection(runtimeConfig: RuntimeConfig): RuntimeType =
        ServerCargoDependency.smithyHttpServer(runtimeConfig)
            .toType().resolve("protocol::$protocolModulePath::rejection::ResponseRejection")

    /** The protocol-specific `RuntimeError` type. **/
    fun runtimeError(runtimeConfig: RuntimeConfig): RuntimeType =
        ServerCargoDependency.smithyHttpServer(runtimeConfig)
            .toType().resolve("protocol::$protocolModulePath::runtime_error::RuntimeError")
}

fun returnSymbolToParseFn(codegenContext: ServerCodegenContext): (Shape) -> ReturnSymbolToParse {
    fun returnSymbolToParse(shape: Shape): ReturnSymbolToParse =
        if (shape.canReachConstrainedShape(codegenContext.model, codegenContext.symbolProvider)) {
            ReturnSymbolToParse(codegenContext.unconstrainedShapeSymbolProvider.toSymbol(shape), true)
        } else {
            ReturnSymbolToParse(codegenContext.symbolProvider.toSymbol(shape), false)
        }
    return ::returnSymbolToParse
}

fun jsonParserGenerator(
    codegenContext: ServerCodegenContext,
    httpBindingResolver: HttpBindingResolver,
    jsonName: (MemberShape) -> String,
    additionalParserCustomizations: List<JsonParserCustomization> = listOf(),
): JsonParserGenerator =
    JsonParserGenerator(
        codegenContext,
        httpBindingResolver,
        jsonName,
        returnSymbolToParseFn(codegenContext),
        listOf(
            ServerRequestBeforeBoxingDeserializedMemberConvertToMaybeConstrainedJsonParserCustomization(codegenContext),
        ) + additionalParserCustomizations,
    )

class ServerAwsJsonProtocol(
    private val serverCodegenContext: ServerCodegenContext,
    awsJsonVersion: AwsJsonVersion,
    private val additionalParserCustomizations: List<JsonParserCustomization> = listOf(),
) : AwsJson(serverCodegenContext, awsJsonVersion), ServerProtocol {
    private val runtimeConfig = codegenContext.runtimeConfig

    override val protocolModulePath: String
        get() = when (version) {
            is AwsJsonVersion.Json10 -> "aws_json_10"
            is AwsJsonVersion.Json11 -> "aws_json_11"
        }

    override fun structuredDataParser(): StructuredDataParserGenerator =
        jsonParserGenerator(serverCodegenContext, httpBindingResolver, ::awsJsonFieldName, additionalParserCustomizations)

    override fun structuredDataSerializer(): StructuredDataSerializerGenerator =
        ServerAwsJsonSerializerGenerator(serverCodegenContext, httpBindingResolver, awsJsonVersion)

    override fun markerStruct(): RuntimeType {
        return when (version) {
            is AwsJsonVersion.Json10 -> ServerRuntimeType.protocol("AwsJson1_0", protocolModulePath, runtimeConfig)
            is AwsJsonVersion.Json11 -> ServerRuntimeType.protocol("AwsJson1_1", protocolModulePath, runtimeConfig)
        }
    }

    override fun routerType() = ServerCargoDependency.smithyHttpServer(runtimeConfig).toType()
        .resolve("protocol::aws_json::router::AwsJsonRouter")

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

    override fun serverRouterRequestSpecType(
        requestSpecModule: RuntimeType,
    ): RuntimeType = RuntimeType.String

    override fun serverRouterRuntimeConstructor() = when (version) {
        AwsJsonVersion.Json10 -> "new_aws_json_10_router"
        AwsJsonVersion.Json11 -> "new_aws_json_11_router"
    }

    override fun requestRejection(runtimeConfig: RuntimeConfig): RuntimeType =
        ServerCargoDependency.smithyHttpServer(runtimeConfig)
            .toType().resolve("protocol::aws_json::rejection::RequestRejection")
    override fun responseRejection(runtimeConfig: RuntimeConfig): RuntimeType =
        ServerCargoDependency.smithyHttpServer(runtimeConfig)
            .toType().resolve("protocol::aws_json::rejection::ResponseRejection")
    override fun runtimeError(runtimeConfig: RuntimeConfig): RuntimeType =
        ServerCargoDependency.smithyHttpServer(runtimeConfig)
            .toType().resolve("protocol::aws_json::runtime_error::RuntimeError")
}

private fun restRouterType(runtimeConfig: RuntimeConfig) =
    ServerCargoDependency.smithyHttpServer(runtimeConfig).toType()
        .resolve("protocol::rest::router::RestRouter")

class ServerRestJsonProtocol(
    private val serverCodegenContext: ServerCodegenContext,
    private val additionalParserCustomizations: List<JsonParserCustomization> = listOf(),
) : RestJson(serverCodegenContext), ServerProtocol {
    val runtimeConfig = codegenContext.runtimeConfig

    override val protocolModulePath: String = "rest_json_1"

    override fun structuredDataParser(): StructuredDataParserGenerator =
        jsonParserGenerator(serverCodegenContext, httpBindingResolver, ::restJsonFieldName, additionalParserCustomizations)

    override fun structuredDataSerializer(): StructuredDataSerializerGenerator =
        ServerRestJsonSerializerGenerator(serverCodegenContext, httpBindingResolver)

    override fun markerStruct() = ServerRuntimeType.protocol("RestJson1", protocolModulePath, runtimeConfig)

    override fun routerType() = restRouterType(runtimeConfig)

    override fun serverRouterRequestSpec(
        operationShape: OperationShape,
        operationName: String,
        serviceName: String,
        requestSpecModule: RuntimeType,
    ): Writable = RestRequestSpecGenerator(httpBindingResolver, requestSpecModule).generate(operationShape)

    override fun serverRouterRequestSpecType(requestSpecModule: RuntimeType): RuntimeType =
        requestSpecModule.resolve("RequestSpec")

    override fun serverRouterRuntimeConstructor() = "new_rest_json_router"

    override fun serverContentTypeCheckNoModeledInput() = true
}

class ServerRestXmlProtocol(
    codegenContext: CodegenContext,
) : RestXml(codegenContext), ServerProtocol {
    val runtimeConfig = codegenContext.runtimeConfig
    override val protocolModulePath = "rest_xml"

    override fun markerStruct() = ServerRuntimeType.protocol("RestXml", protocolModulePath, runtimeConfig)

    override fun routerType() = restRouterType(runtimeConfig)

    override fun serverRouterRequestSpec(
        operationShape: OperationShape,
        operationName: String,
        serviceName: String,
        requestSpecModule: RuntimeType,
    ): Writable = RestRequestSpecGenerator(httpBindingResolver, requestSpecModule).generate(operationShape)

    override fun serverRouterRequestSpecType(requestSpecModule: RuntimeType): RuntimeType =
        requestSpecModule.resolve("RequestSpec")

    override fun serverRouterRuntimeConstructor() = "new_rest_xml_router"

    override fun serverContentTypeCheckNoModeledInput() = true
}

/**
 * A customization to, just before we box a recursive member that we've deserialized into `Option<T>`, convert it into
 * `MaybeConstrained` if the target shape can reach a constrained shape.
 */
class ServerRequestBeforeBoxingDeserializedMemberConvertToMaybeConstrainedJsonParserCustomization(val codegenContext: ServerCodegenContext) :
    JsonParserCustomization() {
    override fun section(section: JsonParserSection): Writable = when (section) {
        is JsonParserSection.BeforeBoxingDeserializedMember -> writable {
            // We're only interested in _structure_ member shapes that can reach constrained shapes.
            if (
                codegenContext.model.expectShape(section.shape.container) is StructureShape &&
                section.shape.targetCanReachConstrainedShape(codegenContext.model, codegenContext.symbolProvider)
            ) {
                rust(".map(|x| x.into())")
            }
        }
        else -> emptySection
    }
}
