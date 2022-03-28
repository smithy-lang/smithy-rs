/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolSupport
import software.amazon.smithy.rust.codegen.smithy.generators.serializationError
import software.amazon.smithy.rust.codegen.smithy.protocols.AwsJsonHttpBindingResolver
import software.amazon.smithy.rust.codegen.smithy.protocols.AwsJsonVersion
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpBindingResolver
import software.amazon.smithy.rust.codegen.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.smithy.protocols.ProtocolGeneratorFactory
import software.amazon.smithy.rust.codegen.smithy.protocols.parse.JsonParserGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.parse.StructuredDataParserGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.serialize.JsonSerializerGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.serialize.StructuredDataSerializerGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.serializeFunctionName
import software.amazon.smithy.rust.codegen.util.inputShape

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
    httpBindingResolver: HttpBindingResolver,
    private val jsonSerializerGenerator: JsonSerializerGenerator =
        JsonSerializerGenerator(codegenContext, httpBindingResolver, ::awsJsonFieldName)
) : StructuredDataSerializerGenerator by jsonSerializerGenerator {
    private val symbolProvider = codegenContext.symbolProvider
    private val jsonSerModule = RustModule.private("json_ser")
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenScope = arrayOf(
        "Error" to runtimeConfig.serializationError(),
        "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
    )

    override fun operationInputSerializer(operationShape: OperationShape): RuntimeType {
        var serializer = jsonSerializerGenerator.operationInputSerializer(operationShape)
        if (serializer == null) {
            val inputShape = operationShape.inputShape(codegenContext.model)
            val fnName = codegenContext.symbolProvider.serializeFunctionName(operationShape)
            serializer = RuntimeType.forInlineFun(fnName, RustModule.private("operation_ser")) {
                it.rustBlockTemplate(
                    "pub fn $fnName(_input: &#{target}) -> Result<#{SdkBody}, #{Error}>",
                    *codegenScope, "target" to codegenContext.symbolProvider.toSymbol(inputShape)
                ) {
                    rustTemplate("""Ok(#{SdkBody}::from("{}"))""", *codegenScope)
                }
            }
        }
        return serializer
    }
}

class ServerAwsJson(
    private val codegenContext: CodegenContext,
    awsJsonVersion: AwsJsonVersion
) : Protocol {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val errorScope = arrayOf(
        "Bytes" to RuntimeType.Bytes,
        "Error" to RuntimeType.GenericError(runtimeConfig),
        "HeaderMap" to RuntimeType.http.member("HeaderMap"),
        "JsonError" to CargoDependency.smithyJson(runtimeConfig).asType().member("deserialize::Error"),
        "Response" to RuntimeType.http.member("Response"),
        "json_errors" to RuntimeType.jsonErrors(runtimeConfig),
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
        ServerAwsJsonSerializerGenerator(codegenContext, httpBindingResolver)

    override fun parseHttpGenericError(operationShape: OperationShape): RuntimeType =
        RuntimeType.forInlineFun("parse_http_generic_error", jsonDeserModule) { writer ->
            writer.rustTemplate(
                """
                pub fn parse_http_generic_error(response: &#{Response}<#{Bytes}>) -> Result<#{Error}, #{JsonError}> {
                    #{json_errors}::parse_generic_error(response.body(), response.headers())
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
                    // Note: HeaderMap::new() doesn't allocate
                    #{json_errors}::parse_generic_error(payload, &#{HeaderMap}::new())
                }
                """,
                *errorScope
            )
        }
}

private fun awsJsonFieldName(member: MemberShape): String {
    return member.memberName
}
