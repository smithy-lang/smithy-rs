/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.pattern.UriPattern
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.ToShapeId
import software.amazon.smithy.model.traits.HttpTrait
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.generators.serializationError
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.JsonParserGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.StructuredDataParserGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.JsonSerializerGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.StructuredDataSerializerGenerator
import software.amazon.smithy.rust.codegen.core.util.hasEventStreamMember
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.isInputEventStream
import software.amazon.smithy.rust.codegen.core.util.isOutputEventStream
import software.amazon.smithy.rust.codegen.core.util.isStreaming

sealed class AwsJsonVersion {
    abstract val value: String

    object Json10 : AwsJsonVersion() {
        override val value = "1.0"
    }

    object Json11 : AwsJsonVersion() {
        override val value = "1.1"
    }
}

class AwsJsonHttpBindingResolver(
    private val model: Model,
    private val awsJsonVersion: AwsJsonVersion,
    // TODO(https://github.com/smithy-lang/smithy-rs/issues/2237): Remove todoHandlingInitialMessages once the server supports non-streaming members
    private val todoHandlingInitialMessages: Boolean,
) : HttpBindingResolver {
    private val httpTrait =
        HttpTrait.builder()
            .code(200)
            .method("POST")
            .uri(UriPattern.parse("/"))
            .build()

    private fun bindings(shape: ToShapeId): List<HttpBindingDescriptor> {
        val members = shape.let { model.expectShape(it.toShapeId()) }.members()
        // TODO(https://github.com/smithy-lang/smithy-rs/issues/2237): Remove the exception once the server supports non-streaming members
        if (todoHandlingInitialMessages && members.size > 1 && members.any { it.isStreaming(model) }) {
            throw CodegenException(
                "We only support one payload member if that payload contains a streaming member." +
                    "Tracking issue to relax this constraint: https://github.com/smithy-lang/smithy-rs/issues/2237",
            )
        }

        return members.map {
            if (it.isStreaming(model)) {
                HttpBindingDescriptor(it, HttpLocation.PAYLOAD, "document")
            } else {
                HttpBindingDescriptor(it, HttpLocation.DOCUMENT, "document")
            }
        }
            .toList()
    }

    override fun httpTrait(operationShape: OperationShape): HttpTrait = httpTrait

    override fun requestBindings(operationShape: OperationShape): List<HttpBindingDescriptor> =
        bindings(operationShape.inputShape)

    override fun responseBindings(operationShape: OperationShape): List<HttpBindingDescriptor> =
        bindings(operationShape.outputShape)

    override fun errorResponseBindings(errorShape: ToShapeId): List<HttpBindingDescriptor> = bindings(errorShape)

    override fun requestContentType(operationShape: OperationShape): String =
        "application/x-amz-json-${awsJsonVersion.value}"

    override fun responseContentType(operationShape: OperationShape): String = requestContentType(operationShape)

    override fun eventStreamMessageContentType(memberShape: MemberShape): String? =
        ProtocolContentTypes.eventStreamMemberContentType(model, memberShape, "application/json")

    override fun handlesEventStreamInitialRequest(shape: Shape): Boolean {
        // True if the operation input contains an event stream member as well as non-event stream member.
        return when (shape) {
            is OperationShape -> {
                shape.isInputEventStream(model) && requestBindings(shape).any { it.location == HttpLocation.DOCUMENT }
            }

            is StructureShape -> {
                shape.hasEventStreamMember(model) && bindings(shape).any { it.location == HttpLocation.DOCUMENT }
            }

            else -> false
        }
    }

    override fun handlesEventStreamInitialResponse(shape: Shape): Boolean {
        // True if the operation output contains an event stream member.
        // Note that this check is asymmetrical compared to `handlesEventStreamInitialRequest`, as it does not verify
        // the presence of a non-event stream member to determine if the shape should handle the initial response.
        // This is because the server may still send the initial response even when the operation output includes
        // only the event stream member, so we need to defensively handle the initial response in all cases.
        return when (shape) {
            is OperationShape -> shape.isOutputEventStream(model)
            is StructureShape -> shape.hasEventStreamMember(model)
            else -> false
        }
    }
}

/**
 * AwsJson requires all empty inputs to be sent across as `{}`. This class
 * customizes wraps [JsonSerializerGenerator] to add this functionality.
 */
class AwsJsonSerializerGenerator(
    private val codegenContext: CodegenContext,
    httpBindingResolver: HttpBindingResolver,
    private val jsonSerializerGenerator: JsonSerializerGenerator =
        JsonSerializerGenerator(codegenContext, httpBindingResolver, ::awsJsonFieldName),
) : StructuredDataSerializerGenerator by jsonSerializerGenerator {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenScope =
        arrayOf(
            "Error" to runtimeConfig.serializationError(),
            "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
        )
    private val protocolFunctions = ProtocolFunctions(codegenContext)

    override fun operationInputSerializer(operationShape: OperationShape): RuntimeType {
        var serializer = jsonSerializerGenerator.operationInputSerializer(operationShape)
        if (serializer == null) {
            val inputShape = operationShape.inputShape(codegenContext.model)
            serializer =
                protocolFunctions.serializeFn(operationShape, fnNameSuffix = "input") { fnName ->
                    rustBlockTemplate(
                        "pub fn $fnName(_input: &#{target}) -> Result<#{SdkBody}, #{Error}>",
                        *codegenScope, "target" to codegenContext.symbolProvider.toSymbol(inputShape),
                    ) {
                        rustTemplate("""Ok(#{SdkBody}::from("{}"))""", *codegenScope)
                    }
                }
        }
        return serializer
    }
}

open class AwsJson(
    val codegenContext: CodegenContext,
    val awsJsonVersion: AwsJsonVersion,
) : Protocol {
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
        )

    val version: AwsJsonVersion get() = awsJsonVersion

    override val httpBindingResolver: HttpBindingResolver =
        AwsJsonHttpBindingResolver(codegenContext.model, awsJsonVersion, codegenContext.target == CodegenTarget.SERVER)

    override val defaultTimestampFormat: TimestampFormatTrait.Format = TimestampFormatTrait.Format.EPOCH_SECONDS

    override fun additionalRequestHeaders(operationShape: OperationShape): List<Pair<String, String>> =
        listOf("x-amz-target" to "${codegenContext.serviceShape.id.name}.${operationShape.id.name}")

    override fun structuredDataParser(): StructuredDataParserGenerator =
        JsonParserGenerator(
            codegenContext,
            httpBindingResolver,
            ::awsJsonFieldName,
        )

    override fun structuredDataSerializer(): StructuredDataSerializerGenerator =
        AwsJsonSerializerGenerator(codegenContext, httpBindingResolver)

    override fun parseHttpErrorMetadata(operationShape: OperationShape): RuntimeType =
        ProtocolFunctions.crossOperationFn("parse_http_error_metadata") { fnName ->
            rustTemplate(
                """
                pub fn $fnName(_response_status: u16, response_headers: &#{Headers}, response_body: &[u8]) -> Result<#{ErrorMetadataBuilder}, #{JsonError}> {
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
                pub fn $fnName(payload: &#{Bytes}) -> Result<#{ErrorMetadataBuilder}, #{JsonError}> {
                    #{json_errors}::parse_error_metadata(payload, &#{Headers}::new())
                }
                """,
                *errorScope,
            )
        }
}

fun awsJsonFieldName(member: MemberShape): String = member.memberName
