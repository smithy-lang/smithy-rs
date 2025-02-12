/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols

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
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.std
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
 * (https://smithy.io/2.0/spec/http-bindings.html).
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
        val members =
            operationShape
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

        // The spec does not mention whether we should set the `Content-Type` header when there is no modeled output.
        // The protocol tests indicate it's optional:
        // <https://github.com/smithy-lang/smithy/blob/ad8aac3c9ce18ce2170443c0296ef38be46a7320/smithy-aws-protocol-tests/model/restJson1/empty-input-output.smithy#L52-L54>
        //
        // In our implementation, we opt to always set it to `application/json`.
        return super.responseContentType(operationShape) ?: "application/json"
    }
}

open class RestJson(val codegenContext: CodegenContext) : Protocol {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val errorScope =
        arrayOf(
            "Bytes" to RuntimeType.Bytes,
            "ErrorMetadataBuilder" to RuntimeType.errorMetadataBuilder(runtimeConfig),
            "Headers" to RuntimeType.headers(runtimeConfig),
            "JsonError" to
                CargoDependency.smithyJson(runtimeConfig).toType()
                    .resolve("deserialize::error::DeserializeError"),
            "json_errors" to RuntimeType.jsonErrors(runtimeConfig),
            "Result" to std.resolve("result::Result"),
        )

    override val httpBindingResolver: HttpBindingResolver =
        RestJsonHttpBindingResolver(
            codegenContext.model,
            ProtocolContentTypes(
                requestDocument = "application/json",
                responseDocument = "application/json",
                eventStreamContentType = "application/vnd.amazon.eventstream",
                eventStreamMessageContentType = "application/json",
            ),
        )

    override val defaultTimestampFormat: TimestampFormatTrait.Format = TimestampFormatTrait.Format.EPOCH_SECONDS

    /**
     * RestJson1 implementations can denote errors in responses in several ways.
     * New server-side protocol implementations MUST use a header field named `X-Amzn-Errortype`.
     *
     * Note that the spec says that implementations SHOULD strip the error shape ID's namespace
     * (see https://smithy.io/2.0/aws/protocols/aws-restjson1-protocol.html#operation-error-serialization):
     *
     * > The value of this component SHOULD contain only the shape name of the error's Shape ID.
     *
     * But it's a SHOULD; we could strip the namespace if we wanted to. In fact, we did so in smithy-rs versions
     * 0.52.0 to 0.55.4; see:
     * - https://github.com/smithy-lang/smithy-rs/pull/1982
     * - https://github.com/awslabs/smithy/pull/1493
     * - https://github.com/awslabs/smithy/issues/1494
     */
    override fun additionalErrorResponseHeaders(errorShape: StructureShape): List<Pair<String, String>> =
        listOf("x-amzn-errortype" to errorShape.id.name)

    override fun structuredDataParser(): StructuredDataParserGenerator =
        JsonParserGenerator(codegenContext, httpBindingResolver, ::restJsonFieldName)

    override fun structuredDataSerializer(): StructuredDataSerializerGenerator =
        JsonSerializerGenerator(codegenContext, httpBindingResolver, ::restJsonFieldName)

    override fun parseHttpErrorMetadata(operationShape: OperationShape): RuntimeType =
        ProtocolFunctions.crossOperationFn("parse_http_error_metadata") { fnName ->
            rustTemplate(
                """
                pub fn $fnName(_response_status: u16, response_headers: &#{Headers}, response_body: &[u8]) -> #{Result}<#{ErrorMetadataBuilder}, #{JsonError}> {
                    #{json_errors}::parse_error_metadata(response_body, response_headers)
                }
                """,
                *errorScope,
            )
        }

    override fun parseEventStreamErrorMetadata(operationShape: OperationShape): RuntimeType =
        ProtocolFunctions.crossOperationFn("parse_event_stream_error_metadata") { fnName ->
            // `HeaderMap::new()` doesn't allocate.
            rustTemplate(
                """
                pub fn $fnName(payload: &#{Bytes}) -> #{Result}<#{ErrorMetadataBuilder}, #{JsonError}> {
                    #{json_errors}::parse_error_metadata(payload, &#{Headers}::new())
                }
                """,
                *errorScope,
            )
        }
}

fun restJsonFieldName(member: MemberShape): String {
    return member.getTrait<JsonNameTrait>()?.value ?: member.memberName
}
