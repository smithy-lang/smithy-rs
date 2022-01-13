/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolSupport
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpBindingResolver
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpTraitHttpBindingResolver
import software.amazon.smithy.rust.codegen.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.smithy.protocols.ProtocolContentTypes
import software.amazon.smithy.rust.codegen.smithy.protocols.ProtocolGeneratorFactory
import software.amazon.smithy.rust.codegen.smithy.protocols.parse.JsonParserGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.parse.StructuredDataParserGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.restJsonFieldName
import software.amazon.smithy.rust.codegen.smithy.protocols.serialize.JsonSerializerGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.serialize.StructuredDataSerializerGenerator

/*
 * RestJson1 server-side protocol implementation and details. This factory generates the [ServerHttpProtocolGenerator]
 * with RestJson1 specific configurations.
 */
class ServerRestJsonFactory : ProtocolGeneratorFactory<ServerHttpProtocolGenerator> {
    override fun protocol(codegenContext: CodegenContext): Protocol = ServerRestJson(codegenContext)

    override fun buildProtocolGenerator(codegenContext: CodegenContext): ServerHttpProtocolGenerator =
        ServerHttpProtocolGenerator(codegenContext, ServerRestJson(codegenContext))

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

/*
 * RestJson1 implementation. This class is fed to the [ServerHttpProtocolGenerator] to configure things like
 * wire-protocol serializers, content-type, etc.
 */
class ServerRestJson(private val codegenContext: CodegenContext) : Protocol {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val errorScope = arrayOf(
        "Bytes" to RuntimeType.Bytes,
        "Error" to RuntimeType.GenericError(runtimeConfig),
        "HeaderMap" to RuntimeType.http.member("HeaderMap"),
        "JsonError" to CargoDependency.smithyJson(runtimeConfig).asType().member("deserialize::Error"),
        "Response" to RuntimeType.http.member("Response"),
        "JsonErrors" to RuntimeType.jsonErrors(runtimeConfig),
    )
    private val jsonDeserModule = RustModule.private("json_deser")

    override val httpBindingResolver: HttpBindingResolver =
        HttpTraitHttpBindingResolver(codegenContext.model, ProtocolContentTypes.consistent("application/json"))

    override val defaultTimestampFormat: TimestampFormatTrait.Format = TimestampFormatTrait.Format.EPOCH_SECONDS

    override fun structuredDataParser(operationShape: OperationShape): StructuredDataParserGenerator {
        return JsonParserGenerator(codegenContext, httpBindingResolver, ::restJsonFieldName)
    }

    override fun structuredDataSerializer(operationShape: OperationShape): StructuredDataSerializerGenerator {
        return JsonSerializerGenerator(codegenContext, httpBindingResolver, ::restJsonFieldName)
    }

    // NOTE: this method is only needed for the little part of client-codegen we use in tests.
    override fun parseHttpGenericError(operationShape: OperationShape): RuntimeType {
        return RuntimeType.forInlineFun("parse_http_generic_error", jsonDeserModule) { writer ->
            writer.rustTemplate(
                """
                pub fn parse_http_generic_error(response: &#{Response}<#{Bytes}>) -> Result<#{Error}, #{JsonError}> {
                    #{JsonErrors}::parse_generic_error(response.body(), response.headers())
                }
                """,
                *errorScope
            )
        }
    }

    // NOTE: this method is only needed for the little part of client-codegen we use in tests.
    override fun parseEventStreamGenericError(operationShape: OperationShape): RuntimeType {
        return RuntimeType.forInlineFun("parse_event_stream_generic_error", jsonDeserModule) { writer ->
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
}
