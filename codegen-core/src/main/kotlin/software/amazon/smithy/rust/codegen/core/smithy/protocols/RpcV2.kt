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
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ToShapeId
import software.amazon.smithy.model.traits.HttpTrait
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.CborParserGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.StructuredDataParserGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.CborSerializerGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.StructuredDataSerializerGenerator
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticOutputTrait
import software.amazon.smithy.rust.codegen.core.util.expectTrait
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.isStreaming
import software.amazon.smithy.rust.codegen.core.util.outputShape

// TODO Rename these to RpcV2Cbor
class RpcV2HttpBindingResolver(
    private val model: Model,
    private val serviceShape: ServiceShape,
) : HttpBindingResolver {
    private fun bindings(shape: ToShapeId): List<HttpBindingDescriptor> {
        val members = shape.let { model.expectShape(it.toShapeId()) }.members()
        // TODO(https://github.com/awslabs/smithy-rs/issues/2237): support non-streaming members too
        if (members.size > 1 && members.any { it.isStreaming(model) }) {
            throw CodegenException(
                "We only support one payload member if that payload contains a streaming member." +
                    "Tracking issue to relax this constraint: https://github.com/awslabs/smithy-rs/issues/2237",
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

    override fun httpTrait(operationShape: OperationShape) =
        HttpTrait.builder()
            .code(200)
            .method("POST")
            .uri(UriPattern.parse("/service/${serviceShape.id.name}/operation/${operationShape.id.name}"))
            .build()!!

    override fun requestBindings(operationShape: OperationShape) = bindings(operationShape.inputShape)

    override fun responseBindings(operationShape: OperationShape) = bindings(operationShape.outputShape)

    override fun errorResponseBindings(errorShape: ToShapeId) = bindings(errorShape)

    override fun requestContentType(operationShape: OperationShape): String? {
        // When `syntheticInputTrait.originalId == null` it implies that the operation had no input defined
        // in the Smithy model.
        if (operationShape.inputShape(model).expectTrait<SyntheticInputTrait>().originalId == null) {
            return null
        }
        return "application/cbor"
    }

    /**
     * > Responses for operations with no defined output type MUST NOT contain bodies in their HTTP responses.
     * > The `Content-Type` for the serialization format MUST NOT be set.
     */
    override fun responseContentType(operationShape: OperationShape): String? {
        // When `syntheticOutputTrait.originalId == null` it implies that the operation had no output defined
        // in the Smithy model.
        val syntheticOutputTrait = operationShape.outputShape(model).expectTrait<SyntheticOutputTrait>()
        if (syntheticOutputTrait.originalId == null) {
            return null
        }
        return "application/cbor"
    }

    override fun eventStreamMessageContentType(memberShape: MemberShape): String? =
        ProtocolContentTypes.eventStreamMemberContentType(model, memberShape, "application/cbor")
}

/**
 * TODO: Docs.
 */
open class RpcV2(val codegenContext: CodegenContext) : Protocol {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val errorScope =
        arrayOf(
            "Bytes" to RuntimeType.Bytes,
            "ErrorMetadataBuilder" to RuntimeType.errorMetadataBuilder(runtimeConfig),
            "Headers" to RuntimeType.headers(runtimeConfig),
            "HeaderMap" to RuntimeType.Http.resolve("HeaderMap"),
            "JsonError" to
                CargoDependency.smithyJson(runtimeConfig).toType()
                    .resolve("deserialize::error::DeserializeError"),
            "Response" to RuntimeType.Http.resolve("Response"),
            "json_errors" to RuntimeType.jsonErrors(runtimeConfig),
        )
    private val jsonDeserModule = RustModule.private("json_deser")

    override val httpBindingResolver: HttpBindingResolver =
        RpcV2HttpBindingResolver(codegenContext.model, codegenContext.serviceShape)

    // Note that [CborParserGenerator] and [CborSerializerGenerator] automatically (de)serialize timestamps
    // using floating point seconds from the epoch.
    override val defaultTimestampFormat: TimestampFormatTrait.Format = TimestampFormatTrait.Format.EPOCH_SECONDS

    override fun additionalRequestHeaders(operationShape: OperationShape): List<Pair<String, String>> =
        listOf("smithy-protocol" to "rpc-v2-cbor")

    override fun additionalResponseHeaders(operationShape: OperationShape): List<Pair<String, String>> =
        listOf("smithy-protocol" to "rpc-v2-cbor")

    override fun structuredDataParser(): StructuredDataParserGenerator =
        CborParserGenerator(codegenContext, httpBindingResolver)

    override fun structuredDataSerializer(): StructuredDataSerializerGenerator =
        CborSerializerGenerator(codegenContext, httpBindingResolver)

    override fun parseHttpErrorMetadata(operationShape: OperationShape): RuntimeType =
        RuntimeType.cborErrors(runtimeConfig).resolve("parse_error_metadata")

    // TODO: Implement `RpcV2.parseEventStreamErrorMetadata`
    override fun parseEventStreamErrorMetadata(operationShape: OperationShape): RuntimeType =
        RuntimeType.forInlineFun("parse_event_stream_error_metadata", jsonDeserModule) {
            // `HeaderMap::new()` doesn't allocate.
            rustTemplate(
                """
                pub fn parse_event_stream_error_metadata(payload: &#{Bytes}) -> Result<#{ErrorMetadataBuilder}, #{JsonError}> {
                    #{json_errors}::parse_error_metadata(payload, &#{HeaderMap}::new())
                }
                """,
                *errorScope,
            )
        }
}
