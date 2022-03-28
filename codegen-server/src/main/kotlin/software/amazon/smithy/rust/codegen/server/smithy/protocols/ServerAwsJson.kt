/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolSupport
import software.amazon.smithy.rust.codegen.smithy.protocols.AwsJsonHttpBindingResolver
import software.amazon.smithy.rust.codegen.smithy.protocols.AwsJsonVersion
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpBindingResolver
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpLocation
import software.amazon.smithy.rust.codegen.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.smithy.protocols.ProtocolGeneratorFactory
import software.amazon.smithy.rust.codegen.smithy.protocols.awsJsonFieldName
import software.amazon.smithy.rust.codegen.smithy.protocols.parse.JsonParserGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.parse.StructuredDataParserGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.serialize.JsonSerializerGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.serialize.StructuredDataSerializerGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.serializeFunctionName

/*
 * RestJson1 server-side protocol factory. This factory creates the [ServerHttpBoundProtocolGeneror]
 * with RestJson1 specific configurations.
 */
class ServerAwsJsonFactory(private val version: AwsJsonVersion) : ProtocolGeneratorFactory<ServerHttpBoundProtocolGenerator> {
    override fun protocol(codegenContext: CodegenContext): Protocol = ServerAwsJson(codegenContext, version)

    override fun buildProtocolGenerator(codegenContext: CodegenContext): ServerHttpBoundProtocolGenerator =
        ServerHttpBoundProtocolGenerator(codegenContext, protocol(codegenContext))

    override fun transformModel(model: Model): Model = model

    override fun support(): ProtocolSupport {
        return ProtocolSupport(
            /* Client support */
            requestSerialization = false,
            requestBodySerialization = false,
            responseDeserialization = false,
            errorDeserialization = false,
            /* Server support */
            requestDeserialization = true,
            requestBodyDeserialization = true,
            responseSerialization = true,
            errorSerialization = true
        )
    }
}

/**
 * AwsJson requires all empty inputs to be sent across as `{}`. This class
 * customizes wraps [JsonSerializerGenerator] to add this functionality.
 */
class ServerAwsJsonSerializerGenerator(
    private val codegenContext: CodegenContext,
    private val httpBindingResolver: HttpBindingResolver,
    private val awsJsonVersion: AwsJsonVersion,
    private val jsonSerializerGenerator: JsonSerializerGenerator =
        JsonSerializerGenerator(codegenContext, httpBindingResolver, ::awsJsonFieldName)
) : StructuredDataSerializerGenerator by jsonSerializerGenerator {
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider

    override fun serverErrorSerializer(shape: ShapeId): RuntimeType {
        val errorShape = model.expectShape(shape, StructureShape::class.java)
        val includedMembers =
            httpBindingResolver.errorResponseBindings(shape).filter { it.location == HttpLocation.DOCUMENT }
                .map { it.member }
        val fnName = symbolProvider.serializeFunctionName(errorShape)
        return jsonSerializerGenerator.serverStructureSerializer(fnName, errorShape, includedMembers, true, awsJsonVersion == AwsJsonVersion.Json10)
    }
}

class ServerAwsJson(
    private val codegenContext: CodegenContext,
    private val awsJsonVersion: AwsJsonVersion
) : Protocol {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val errorScope = arrayOf(
        "Bytes" to RuntimeType.Bytes,
        "Error" to RuntimeType.GenericError(runtimeConfig),
        "JsonError" to CargoDependency.smithyJson(runtimeConfig).asType().member("deserialize::Error"),
        "Response" to RuntimeType.http.member("Response"),
    )
    private val jsonDeserModule = RustModule.private("json_deser")

    override val httpBindingResolver: HttpBindingResolver =
        AwsJsonHttpBindingResolver(codegenContext.model, awsJsonVersion)

    override val defaultTimestampFormat: TimestampFormatTrait.Format = TimestampFormatTrait.Format.EPOCH_SECONDS

    override fun additionalRequestHeaders(operationShape: OperationShape): List<Pair<String, String>> =
        listOf("x-amz-target" to "${codegenContext.serviceShape.id.name}.${operationShape.id.name}")

    override fun structuredDataParser(operationShape: OperationShape): StructuredDataParserGenerator =
        JsonParserGenerator(codegenContext, httpBindingResolver, ::awsJsonFieldName)

    override fun structuredDataSerializer(operationShape: OperationShape): StructuredDataSerializerGenerator =
        ServerAwsJsonSerializerGenerator(codegenContext, httpBindingResolver, awsJsonVersion)

    override fun parseHttpGenericError(operationShape: OperationShape): RuntimeType =
        RuntimeType.forInlineFun("parse_http_generic_error", jsonDeserModule) { writer ->
            writer.rustTemplate(
                """
                pub fn parse_http_generic_error(response: &#{Response}<#{Bytes}>) -> Result<#{Error}, #{JsonError}> {
                    panic!("Parsing of http generic error is not supported in server implementation")
                }
                """,
                *errorScope
            )
        }

    override fun parseEventStreamGenericError(operationShape: OperationShape): RuntimeType =
        RuntimeType.forInlineFun("parse_event_stream_generic_error", jsonDeserModule) { writer ->
            writer.rustTemplate(
                """
                pub fn parse_event_stream_generic_error(payload: &#{Bytes}) -> Result<#{Error}, #{JsonError}> {
                    panic!("Parsing of http stream generic error is not supported in server implementation")
                }
                """,
                *errorScope
            )
        }
}
