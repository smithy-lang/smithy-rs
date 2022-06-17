/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.protocols

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.HttpPayloadTrait
import software.amazon.smithy.model.traits.JsonNameTrait
import software.amazon.smithy.model.traits.MediaTypeTrait
import software.amazon.smithy.model.traits.StreamingTrait
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.smithy.CoreCodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolSupport
import software.amazon.smithy.rust.codegen.smithy.protocols.parse.JsonParserGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.parse.StructuredDataParserGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.serialize.JsonSerializerGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.serialize.StructuredDataSerializerGenerator
import software.amazon.smithy.rust.codegen.util.getTrait
import software.amazon.smithy.rust.codegen.util.hasTrait
import software.amazon.smithy.rust.codegen.util.outputShape

class RestJsonFactory : ProtocolGeneratorFactory<HttpBoundProtocolGenerator, ClientCodegenContext> {
    override fun protocol(codegenContext: ClientCodegenContext): Protocol = RestJson(codegenContext)

    override fun buildProtocolGenerator(codegenContext: ClientCodegenContext): HttpBoundProtocolGenerator =
        HttpBoundProtocolGenerator(codegenContext, RestJson(codegenContext))

    override fun transformModel(model: Model): Model = model

    override fun support(): ProtocolSupport {
        return ProtocolSupport(
            /* Client support */
            requestSerialization = true,
            requestBodySerialization = true,
            responseDeserialization = true,
            errorDeserialization = true,
            /* Server support */
            requestDeserialization = false,
            requestBodyDeserialization = false,
            responseSerialization = false,
            errorSerialization = false
        )
    }
}

/**
 * This [HttpBindingResolver] implementation mostly delegates to the [HttpTraitHttpBindingResolver] class, since the
 * RestJson1 protocol can be almost entirely described by Smithy's HTTP binding traits
 * (https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html).
 * The only protocol-specific behavior that is truly custom is the response `Content-Type` header, which defaults to
 * `application/json` if not overridden.
 */
class RestJsonHttpBindingResolver(
    private val model: Model,
    contentTypes: ProtocolContentTypes,
) : HttpTraitHttpBindingResolver(model, contentTypes) {
    /**
     * In the RestJson1 protocol, HTTP responses have a default `Content-Type: application/json` header if it is not
     * overridden by a specific mechanism e.g. an output shape member is targeted with `httpPayload` or `mediaType` traits.
     */
    override fun responseContentType(operationShape: OperationShape): String? {
        val members = operationShape
            .outputShape(model)
            .members()
        // TODO(https://github.com/awslabs/smithy/issues/1259)
        //  Temporary fix for https://github.com/awslabs/smithy/blob/df456a514f72f4e35f0fb07c7e26006ff03b2071/smithy-model/src/main/java/software/amazon/smithy/model/knowledge/HttpBindingIndex.java#L352
        for (member in members) {
            if (member.hasTrait<HttpPayloadTrait>()) {
                val target = model.expectShape(member.target)
                if (!target.hasTrait<StreamingTrait>() && !target.hasTrait<MediaTypeTrait>() && target.isBlobShape) {
                    return null
                }
            }
        }
        return super.responseContentType(operationShape) ?: "application/json"
    }
}

class RestJson(private val coreCodegenContext: CoreCodegenContext) : Protocol {
    private val runtimeConfig = coreCodegenContext.runtimeConfig
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
        RestJsonHttpBindingResolver(coreCodegenContext.model, ProtocolContentTypes.consistent("application/json"))

    override val defaultTimestampFormat: TimestampFormatTrait.Format = TimestampFormatTrait.Format.EPOCH_SECONDS

    /**
     * RestJson1 implementations can denote errors in responses in several ways.
     * New server-side protocol implementations MUST use a header field named `X-Amzn-Errortype`.
     */
    override fun additionalErrorResponseHeaders(errorShape: StructureShape): List<Pair<String, String>> =
        listOf("x-amzn-errortype" to errorShape.id.name)

    override fun structuredDataParser(operationShape: OperationShape): StructuredDataParserGenerator =
        JsonParserGenerator(coreCodegenContext, httpBindingResolver, ::restJsonFieldName)

    override fun structuredDataSerializer(operationShape: OperationShape): StructuredDataSerializerGenerator =
        JsonSerializerGenerator(coreCodegenContext, httpBindingResolver, ::restJsonFieldName)

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

fun restJsonFieldName(member: MemberShape): String {
    return member.getTrait<JsonNameTrait>()?.value ?: member.memberName
}
