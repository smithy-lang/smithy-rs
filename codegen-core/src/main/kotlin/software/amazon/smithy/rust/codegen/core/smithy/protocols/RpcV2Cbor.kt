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
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.CborParserCustomization
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.CborParserGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.StructuredDataParserGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.CborSerializerCustomization
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.CborSerializerGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.StructuredDataSerializerGenerator
import software.amazon.smithy.rust.codegen.core.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.core.util.isStreaming

class RpcV2CborHttpBindingResolver(
    private val model: Model,
    private val contentTypes: ProtocolContentTypes,
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

    override fun httpTrait(operationShape: OperationShape): HttpTrait =
        HttpTrait.builder()
            .code(200)
            .method("POST")
            .uri(UriPattern.parse("/service/${serviceShape.id.name}/operation/${operationShape.id.name}"))
            .build()

    override fun requestBindings(operationShape: OperationShape) = bindings(operationShape.inputShape)

    override fun responseBindings(operationShape: OperationShape) = bindings(operationShape.outputShape)

    override fun errorResponseBindings(errorShape: ToShapeId) = bindings(errorShape)

    /**
     * https://smithy.io/2.0/additional-specs/protocols/smithy-rpc-v2.html#requests
     * > Requests for operations with no defined input type MUST NOT contain bodies in their HTTP requests.
     * > The `Content-Type` for the serialization format MUST NOT be set.
     */
    override fun requestContentType(operationShape: OperationShape): String? =
        if (OperationNormalizer.hadUserModeledOperationInput(operationShape, model)) {
            contentTypes.requestDocument
        } else {
            null
        }

    /**
     * https://smithy.io/2.0/additional-specs/protocols/smithy-rpc-v2.html#responses
     * > Responses for operations with no defined output type MUST NOT contain bodies in their HTTP responses.
     * > The `Content-Type` for the serialization format MUST NOT be set.
     */
    override fun responseContentType(operationShape: OperationShape): String? =
        if (OperationNormalizer.hadUserModeledOperationOutput(operationShape, model)) {
            contentTypes.responseDocument
        } else {
            null
        }

    override fun eventStreamMessageContentType(memberShape: MemberShape): String? =
        ProtocolContentTypes.eventStreamMemberContentType(model, memberShape, "application/cbor")
}

open class RpcV2Cbor(
    val codegenContext: CodegenContext,
    private val serializeCustomization: List<CborSerializerCustomization> = listOf(),
    private val parserCustomization: List<CborParserCustomization> = listOf(),
) : Protocol {
    private val runtimeConfig = codegenContext.runtimeConfig

    override val httpBindingResolver: HttpBindingResolver =
        RpcV2CborHttpBindingResolver(
            codegenContext.model,
            ProtocolContentTypes(
                requestDocument = "application/cbor",
                responseDocument = "application/cbor",
                eventStreamContentType = "application/vnd.amazon.eventstream",
                eventStreamMessageContentType = "application/cbor",
            ),
            codegenContext.serviceShape,
        )

    // Note that [CborParserGenerator] and [CborSerializerGenerator] automatically (de)serialize timestamps
    // using floating point seconds from the epoch.
    override val defaultTimestampFormat: TimestampFormatTrait.Format = TimestampFormatTrait.Format.EPOCH_SECONDS

    // The accept header is required by the spec (and by all of the protocol tests)
    override fun additionalRequestHeaders(operationShape: OperationShape): List<Pair<String, String>> =
        listOf("smithy-protocol" to "rpc-v2-cbor", "accept" to "application/cbor")

    override fun additionalResponseHeaders(operationShape: OperationShape): List<Pair<String, String>> =
        listOf("smithy-protocol" to "rpc-v2-cbor")

    override fun structuredDataParser(): StructuredDataParserGenerator =
        CborParserGenerator(
            codegenContext, httpBindingResolver,
            handleNullForNonSparseCollection = { collectionName: String ->
                writable {
                    // The client should drop a null value in a dense collection, see
                    // https://github.com/smithy-lang/smithy/blob/6466fe77c65b8a17b219f0b0a60c767915205f95/smithy-protocol-tests/model/rpcv2Cbor/cbor-maps.smithy#L158
                    rustTemplate(
                        """
                        decoder.null()?;
                        return #{Ok}($collectionName)
                        """,
                        *RuntimeType.preludeScope,
                    )
                }
            },
            customizations = parserCustomization,
        )

    override fun structuredDataSerializer(): StructuredDataSerializerGenerator =
        CborSerializerGenerator(codegenContext, httpBindingResolver, customizations = serializeCustomization)

    override fun parseHttpErrorMetadata(operationShape: OperationShape): RuntimeType =
        RuntimeType.cborErrors(runtimeConfig).resolve("parse_error_metadata")

    override fun parseEventStreamErrorMetadata(operationShape: OperationShape): RuntimeType =
        ProtocolFunctions.crossOperationFn("parse_event_stream_error_metadata") { fnName ->
            rustTemplate(
                """
                pub fn $fnName(payload: &#{Bytes}) -> #{Result}<#{ErrorMetadataBuilder}, #{DeserializeError}> {
                    #{cbor_errors}::parse_error_metadata(0, &#{Headers}::new(), payload)
                }
                """,
                "cbor_errors" to RuntimeType.cborErrors(runtimeConfig),
                "Bytes" to RuntimeType.Bytes,
                "ErrorMetadataBuilder" to RuntimeType.errorMetadataBuilder(runtimeConfig),
                "DeserializeError" to
                    CargoDependency.smithyCbor(runtimeConfig).toType()
                        .resolve("decode::DeserializeError"),
                "Headers" to RuntimeType.headers(runtimeConfig),
                *RuntimeType.preludeScope,
            )
        }

    // Unlike other protocols, the `rpcv2Cbor` protocol requires that `Content-Length` is always set
    // unless there is no input or if the operation is an event stream, see
    // https://github.com/smithy-lang/smithy/blob/6466fe77c65b8a17b219f0b0a60c767915205f95/smithy-protocol-tests/model/rpcv2Cbor/empty-input-output.smithy#L106
    // TODO(https://github.com/smithy-lang/smithy-rs/issues/3772): Do not set `Content-Length` for event stream operations
    override fun needsRequestContentLength(operationShape: OperationShape) = operationShape.input.isPresent
}
