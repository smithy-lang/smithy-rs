/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ToShapeId
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.CborParserGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.StructuredDataParserGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.CborSerializerGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.StructuredDataSerializerGenerator
import software.amazon.smithy.rust.codegen.core.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.core.util.PANIC
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.isStreaming
import software.amazon.smithy.rust.codegen.core.util.outputShape

class RpcV2CborHttpBindingResolver(
    private val model: Model,
    private val contentTypes: ProtocolContentTypes,
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

    // TODO(https://github.com/smithy-lang/smithy-rs/issues/3573)
    //   In the server, this is only used when the protocol actually supports the `@http` trait.
    //   However, we will have to do this for client support. Perhaps this method deserves a rename.
    override fun httpTrait(operationShape: OperationShape) = PANIC("RPC v2 does not support the `@http` trait")

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

open class RpcV2Cbor(val codegenContext: CodegenContext) : Protocol {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val errorScope =
        arrayOf(
            "Bytes" to RuntimeType.Bytes,
            "ErrorMetadataBuilder" to RuntimeType.errorMetadataBuilder(runtimeConfig),
            "HeaderMap" to RuntimeType.Http.resolve("HeaderMap"),
            "JsonError" to
                CargoDependency.smithyJson(runtimeConfig).toType()
                    .resolve("deserialize::error::DeserializeError"),
            "Response" to RuntimeType.Http.resolve("Response"),
            "json_errors" to RuntimeType.jsonErrors(runtimeConfig),
        )
    private val jsonDeserModule = RustModule.private("json_deser")

    override val httpBindingResolver: HttpBindingResolver =
        RpcV2CborHttpBindingResolver(
            codegenContext.model,
            ProtocolContentTypes(
                requestDocument = "application/cbor",
                responseDocument = "application/cbor",
                eventStreamContentType = "application/vnd.amazon.eventstream",
                eventStreamMessageContentType = "application/cbor",
            ),
        )

    // Note that [CborParserGenerator] and [CborSerializerGenerator] automatically (de)serialize timestamps
    // using floating point seconds from the epoch.
    override val defaultTimestampFormat: TimestampFormatTrait.Format = TimestampFormatTrait.Format.EPOCH_SECONDS

    override fun additionalResponseHeaders(operationShape: OperationShape): List<Pair<String, String>> =
        listOf("smithy-protocol" to "rpc-v2-cbor")

    override fun structuredDataParser(): StructuredDataParserGenerator =
        CborParserGenerator(codegenContext, httpBindingResolver)

    override fun structuredDataSerializer(): StructuredDataSerializerGenerator =
        CborSerializerGenerator(codegenContext, httpBindingResolver)

    // TODO(https://github.com/smithy-lang/smithy-rs/issues/3573)
    override fun parseHttpErrorMetadata(operationShape: OperationShape): RuntimeType =
        TODO("rpcv2Cbor client support has not yet been implemented")

    // TODO(https://github.com/smithy-lang/smithy-rs/issues/3573)
    override fun parseEventStreamErrorMetadata(operationShape: OperationShape): RuntimeType =
        TODO("rpcv2Cbor event streams have not yet been implemented")
}
