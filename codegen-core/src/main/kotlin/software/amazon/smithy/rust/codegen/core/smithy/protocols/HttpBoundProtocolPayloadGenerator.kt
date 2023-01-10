/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.DocumentShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.rustlang.withBlockTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.generators.http.HttpMessageType
import software.amazon.smithy.rust.codegen.core.smithy.generators.operationBuildError
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolPayloadGenerator
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.EventStreamErrorMarshallerGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.EventStreamMarshallerGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.StructuredDataSerializerGenerator
import software.amazon.smithy.rust.codegen.core.util.PANIC
import software.amazon.smithy.rust.codegen.core.util.UNREACHABLE
import software.amazon.smithy.rust.codegen.core.util.expectMember
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.isEventStream
import software.amazon.smithy.rust.codegen.core.util.isInputEventStream
import software.amazon.smithy.rust.codegen.core.util.isOutputEventStream
import software.amazon.smithy.rust.codegen.core.util.isStreaming
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase

class HttpBoundProtocolPayloadGenerator(
    codegenContext: CodegenContext,
    private val protocol: Protocol,
    private val httpMessageType: HttpMessageType = HttpMessageType.REQUEST,
) : ProtocolPayloadGenerator {
    private val symbolProvider = codegenContext.symbolProvider
    private val model = codegenContext.model
    private val runtimeConfig = codegenContext.runtimeConfig
    private val target = codegenContext.target
    private val httpBindingResolver = protocol.httpBindingResolver
    private val operationSerModule = RustModule.private("operation_ser")
    private val smithyEventStream = RuntimeType.smithyEventStream(runtimeConfig)
    private val codegenScope = arrayOf(
        "hyper" to CargoDependency.HyperWithStream.toType(),
        "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
        "BuildError" to runtimeConfig.operationBuildError(),
        "SmithyHttp" to RuntimeType.smithyHttp(runtimeConfig),
        "NoOpSigner" to smithyEventStream.resolve("frame::NoOpSigner"),
    )

    override fun payloadMetadata(operationShape: OperationShape): ProtocolPayloadGenerator.PayloadMetadata {
        val (shape, payloadMemberName) = when (httpMessageType) {
            HttpMessageType.RESPONSE -> operationShape.outputShape(model) to
                httpBindingResolver.responseMembers(operationShape, HttpLocation.PAYLOAD).firstOrNull()?.memberName
            HttpMessageType.REQUEST -> operationShape.inputShape(model) to
                httpBindingResolver.requestMembers(operationShape, HttpLocation.PAYLOAD).firstOrNull()?.memberName
        }

        // Only:
        //     - streaming operations (blob streaming and event streams),
        //     - *blob non-streaming* operations; and
        //     - string payloads
        // need to take ownership.
        return if (payloadMemberName == null) {
            ProtocolPayloadGenerator.PayloadMetadata(takesOwnership = false)
        } else if (operationShape.isInputEventStream(model)) {
            ProtocolPayloadGenerator.PayloadMetadata(takesOwnership = true)
        } else {
            val member = shape.expectMember(payloadMemberName)
            when (val type = model.expectShape(member.target)) {
                is DocumentShape, is StructureShape, is UnionShape -> ProtocolPayloadGenerator.PayloadMetadata(
                    takesOwnership = false,
                )
                is StringShape, is BlobShape -> ProtocolPayloadGenerator.PayloadMetadata(takesOwnership = true)
                else -> UNREACHABLE("Unexpected payload target type: $type")
            }
        }
    }

    override fun generatePayload(writer: RustWriter, self: String, operationShape: OperationShape) {
        when (httpMessageType) {
            HttpMessageType.RESPONSE -> generateResponsePayload(writer, self, operationShape)
            HttpMessageType.REQUEST -> generateRequestPayload(writer, self, operationShape)
        }
    }

    private fun generateRequestPayload(writer: RustWriter, self: String, operationShape: OperationShape) {
        val payloadMemberName = httpBindingResolver.requestMembers(operationShape, HttpLocation.PAYLOAD).firstOrNull()?.memberName

        if (payloadMemberName == null) {
            val serializerGenerator = protocol.structuredDataSerializer(operationShape)
            generateStructureSerializer(writer, self, serializerGenerator.operationInputSerializer(operationShape))
        } else {
            generatePayloadMemberSerializer(writer, self, operationShape, payloadMemberName)
        }
    }

    private fun generateResponsePayload(writer: RustWriter, self: String, operationShape: OperationShape) {
        val payloadMemberName = httpBindingResolver.responseMembers(operationShape, HttpLocation.PAYLOAD).firstOrNull()?.memberName

        if (payloadMemberName == null) {
            val serializerGenerator = protocol.structuredDataSerializer(operationShape)
            generateStructureSerializer(writer, self, serializerGenerator.operationOutputSerializer(operationShape))
        } else {
            generatePayloadMemberSerializer(writer, self, operationShape, payloadMemberName)
        }
    }

    private fun generatePayloadMemberSerializer(
        writer: RustWriter,
        self: String,
        operationShape: OperationShape,
        payloadMemberName: String,
    ) {
        val serializerGenerator = protocol.structuredDataSerializer(operationShape)

        if (operationShape.isEventStream(model)) {
            if (operationShape.isInputEventStream(model) && target == CodegenTarget.CLIENT) {
                val payloadMember = operationShape.inputShape(model).expectMember(payloadMemberName)
                writer.serializeViaEventStream(operationShape, payloadMember, serializerGenerator, "self")
            } else if (operationShape.isOutputEventStream(model) && target == CodegenTarget.SERVER) {
                val payloadMember = operationShape.outputShape(model).expectMember(payloadMemberName)
                writer.serializeViaEventStream(operationShape, payloadMember, serializerGenerator, "output")
            } else {
                throw CodegenException("Payload serializer for event streams with an invalid configuration")
            }
        } else {
            val bodyMetadata = payloadMetadata(operationShape)
            val payloadMember = when (httpMessageType) {
                HttpMessageType.RESPONSE -> operationShape.outputShape(model).expectMember(payloadMemberName)
                HttpMessageType.REQUEST -> operationShape.inputShape(model).expectMember(payloadMemberName)
            }
            writer.serializeViaPayload(bodyMetadata, self, payloadMember, serializerGenerator)
        }
    }

    private fun generateStructureSerializer(writer: RustWriter, self: String, serializer: RuntimeType?) {
        if (serializer == null) {
            writer.rust("\"\"")
        } else {
            writer.rust(
                "#T(&$self)?",
                serializer,
            )
        }
    }

    private fun RustWriter.serializeViaEventStream(
        operationShape: OperationShape,
        memberShape: MemberShape,
        serializerGenerator: StructuredDataSerializerGenerator,
        outerName: String,
    ) {
        val memberName = symbolProvider.toMemberName(memberShape)
        val unionShape = model.expectShape(memberShape.target, UnionShape::class.java)

        val contentType = when (target) {
            CodegenTarget.CLIENT -> httpBindingResolver.requestContentType(operationShape)
            CodegenTarget.SERVER -> httpBindingResolver.responseContentType(operationShape)
        }
        val errorMarshallerConstructorFn = EventStreamErrorMarshallerGenerator(
            model,
            target,
            runtimeConfig,
            symbolProvider,
            unionShape,
            serializerGenerator,
            contentType ?: throw CodegenException("event streams must set a content type"),
        ).render()
        val marshallerConstructorFn = EventStreamMarshallerGenerator(
            model,
            target,
            runtimeConfig,
            symbolProvider,
            unionShape,
            serializerGenerator,
            contentType,
        ).render()

        // TODO(EventStream): [RPC] RPC protocols need to send an initial message with the
        //  parameters that are not `@eventHeader` or `@eventPayload`.
        when (target) {
            CodegenTarget.CLIENT ->
                rustTemplate(
                    """
                    {
                        let error_marshaller = #{errorMarshallerConstructorFn}();
                        let marshaller = #{marshallerConstructorFn}();
                        let signer = _config.new_event_stream_signer(properties.clone());
                        let adapter: #{SmithyHttp}::event_stream::MessageStreamAdapter<_, _> =
                            $outerName.$memberName.into_body_stream(marshaller, error_marshaller, signer);
                        let body: #{SdkBody} = #{hyper}::Body::wrap_stream(adapter).into();
                        body
                    }
                    """,
                    *codegenScope,
                    "marshallerConstructorFn" to marshallerConstructorFn,
                    "errorMarshallerConstructorFn" to errorMarshallerConstructorFn,
                )
            CodegenTarget.SERVER -> {
                rustTemplate(
                    """
                    {
                        let error_marshaller = #{errorMarshallerConstructorFn}();
                        let marshaller = #{marshallerConstructorFn}();
                        let signer = #{NoOpSigner}{};
                        let adapter: #{SmithyHttp}::event_stream::MessageStreamAdapter<_, _> =
                            $outerName.$memberName.into_body_stream(marshaller, error_marshaller, signer);
                        adapter
                    }
                    """,
                    *codegenScope,
                    "marshallerConstructorFn" to marshallerConstructorFn,
                    "errorMarshallerConstructorFn" to errorMarshallerConstructorFn,
                )
            }
        }
    }

    private fun RustWriter.serializeViaPayload(
        payloadMetadata: ProtocolPayloadGenerator.PayloadMetadata,
        self: String,
        member: MemberShape,
        serializerGenerator: StructuredDataSerializerGenerator,
    ) {
        val fnName = "serialize_payload_${member.container.name.toSnakeCase()}"
        val ref = if (payloadMetadata.takesOwnership) "" else "&"
        val serializer = RuntimeType.forInlineFun(fnName, operationSerModule) {
            val outputT = if (member.isStreaming(model)) symbolProvider.toSymbol(member) else RuntimeType.ByteSlab.toSymbol()
            rustBlockTemplate(
                "pub fn $fnName(payload: $ref#{Member}) -> Result<#{outputT}, #{BuildError}>",
                "Member" to symbolProvider.toSymbol(member),
                "outputT" to outputT,
                *codegenScope,
            ) {
                val asRef = if (payloadMetadata.takesOwnership) "" else ".as_ref()"

                if (symbolProvider.toSymbol(member).isOptional()) {
                    withBlockTemplate(
                        """
                        let payload = match payload$asRef {
                            Some(t) => t,
                            None => return Ok(
                        """,
                        ")};",
                        *codegenScope,
                    ) {
                        when (val targetShape = model.expectShape(member.target)) {
                            // Return an empty `Vec<u8>`.
                            is StringShape, is BlobShape, is DocumentShape -> rust(
                                """
                                Vec::new()
                                """,
                            )
                            is StructureShape -> rust("#T()", serializerGenerator.unsetStructure(targetShape))
                            is UnionShape -> throw CodegenException("Currently unsupported. Tracking issue: https://github.com/awslabs/smithy-rs/issues/1896")
                            else -> throw CodegenException("`httpPayload` on member shapes targeting shapes of type ${targetShape.type} is unsupported")
                        }
                    }
                }

                withBlock("Ok(", ")") {
                    renderPayload(member, "payload", serializerGenerator)
                }
            }
        }
        rust("#T($ref $self.${symbolProvider.toMemberName(member)})?", serializer)
    }

    private fun RustWriter.renderPayload(
        member: MemberShape,
        payloadName: String,
        serializer: StructuredDataSerializerGenerator,
    ) {
        when (val targetShape = model.expectShape(member.target)) {
            is StringShape -> {
                // Write the raw string to the payload.
                if (targetShape.hasTrait<EnumTrait>()) {
                    // Convert an enum to `&str` then to `&[u8]` then to `Vec<u8>`.
                    rust("$payloadName.as_str().as_bytes().to_vec()")
                } else {
                    // Convert a `String` to `Vec<u8>`.
                    rust("$payloadName.into_bytes()")
                }
            }

            is BlobShape -> {
                // Write the raw blob to the payload.
                if (member.isStreaming(model)) {
                    // Return the `ByteStream`.
                    rust(payloadName)
                } else {
                    // Convert the `Blob` into a `Vec<u8>` and return it.
                    rust("$payloadName.into_inner()")
                }
            }
            is StructureShape, is UnionShape -> {
                check(
                    !((targetShape as? UnionShape)?.isEventStream() ?: false),
                ) { "Event Streams should be handled further up" }

                rust(
                    "#T($payloadName)?",
                    serializer.payloadSerializer(member),
                )
            }
            is DocumentShape -> {
                rust(
                    "#T($payloadName)",
                    serializer.documentSerializer(),
                )
            }
            else -> PANIC("Unexpected payload target type: $targetShape")
        }
    }
}
