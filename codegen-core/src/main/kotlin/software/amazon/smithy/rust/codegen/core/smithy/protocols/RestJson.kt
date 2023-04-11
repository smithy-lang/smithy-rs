/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.HttpPayloadTrait
import software.amazon.smithy.model.traits.JsonNameTrait
import software.amazon.smithy.model.traits.MediaTypeTrait
import software.amazon.smithy.model.traits.StreamingTrait
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.generators.builderSymbol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.JsonParserGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.StructuredDataParserGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.JsonSerializerGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.StructuredDataSerializerGenerator
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.outputShape

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

open class RestJson(val codegenContext: CodegenContext) : Protocol {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val errorScope = arrayOf(
        "Bytes" to RuntimeType.Bytes,
        "Error" to RuntimeType.genericError(runtimeConfig),
        "HeaderMap" to RuntimeType.Http.resolve("HeaderMap"),
        "JsonError" to CargoDependency.smithyJson(runtimeConfig).toType()
            .resolve("deserialize::error::DeserializeError"),
        "Response" to RuntimeType.Http.resolve("Response"),
        "json_errors" to RuntimeType.jsonErrors(runtimeConfig),
    )
    private val jsonDeserModule = RustModule.private("json_deser")

    override val httpBindingResolver: HttpBindingResolver =
        RestJsonHttpBindingResolver(codegenContext.model, ProtocolContentTypes("application/json", "application/json", "application/vnd.amazon.eventstream"))

    override val defaultTimestampFormat: TimestampFormatTrait.Format = TimestampFormatTrait.Format.EPOCH_SECONDS

    /**
     * RestJson1 implementations can denote errors in responses in several ways.
     * New server-side protocol implementations MUST use a header field named `X-Amzn-Errortype`.
     *
     * Note that the spec says that implementations SHOULD strip the error shape ID's namespace.
     * However, our server implementation renders the full shape ID (including namespace), since some
     * existing clients rely on it to deserialize the error shape and fail if only the shape name is present.
     * This is compliant with the spec, see https://github.com/awslabs/smithy/pull/1493.
     * See https://github.com/awslabs/smithy/issues/1494 too.
     */
    override fun additionalErrorResponseHeaders(errorShape: StructureShape): List<Pair<String, String>> =
        listOf("x-amzn-errortype" to errorShape.id.toString())

    override fun structuredDataParser(operationShape: OperationShape): StructuredDataParserGenerator {
        fun builderSymbol(shape: StructureShape): Symbol =
            shape.builderSymbol(codegenContext.symbolProvider)
        return JsonParserGenerator(codegenContext, httpBindingResolver, ::restJsonFieldName, ::builderSymbol)
    }

    override fun structuredDataSerializer(operationShape: OperationShape): StructuredDataSerializerGenerator =
        JsonSerializerGenerator(codegenContext, httpBindingResolver, ::restJsonFieldName)

    override fun parseHttpGenericError(operationShape: OperationShape): RuntimeType =
        RuntimeType.forInlineFun("parse_http_generic_error", jsonDeserModule) {
            rustTemplate(
                """
                pub fn parse_http_generic_error(response: &#{Response}<#{Bytes}>) -> Result<#{Error}, #{JsonError}> {
                    #{json_errors}::parse_generic_error(response.body(), response.headers())
                }
                """,
                *errorScope,
            )
        }

    override fun parseEventStreamGenericError(operationShape: OperationShape): RuntimeType =
        RuntimeType.forInlineFun("parse_event_stream_generic_error", jsonDeserModule) {
            rustTemplate(
                """
                pub fn parse_event_stream_generic_error(payload: &#{Bytes}) -> Result<#{Error}, #{JsonError}> {
                    // Note: HeaderMap::new() doesn't allocate
                    #{json_errors}::parse_generic_error(payload, &#{HeaderMap}::new())
                }
                """,
                *errorScope,
            )
        }
}

fun restJsonFieldName(member: MemberShape): String {
    return member.getTrait<JsonNameTrait>()?.value ?: member.memberName
}
