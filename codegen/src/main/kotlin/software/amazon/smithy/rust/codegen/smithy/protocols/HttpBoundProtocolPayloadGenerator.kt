package software.amazon.smithy.rust.codegen.smithy.protocols

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.DocumentShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.withBlock
import software.amazon.smithy.rust.codegen.rustlang.withBlockTemplate
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.error.errorSymbol
import software.amazon.smithy.rust.codegen.smithy.generators.http.HttpMessageType
import software.amazon.smithy.rust.codegen.smithy.generators.operationBuildError
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolPayloadGenerator
import software.amazon.smithy.rust.codegen.smithy.isOptional
import software.amazon.smithy.rust.codegen.smithy.protocols.serialize.EventStreamMarshallerGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.serialize.StructuredDataSerializerGenerator
import software.amazon.smithy.rust.codegen.util.PANIC
import software.amazon.smithy.rust.codegen.util.UNREACHABLE
import software.amazon.smithy.rust.codegen.util.expectMember
import software.amazon.smithy.rust.codegen.util.hasTrait
import software.amazon.smithy.rust.codegen.util.inputShape
import software.amazon.smithy.rust.codegen.util.isEventStream
import software.amazon.smithy.rust.codegen.util.isInputEventStream
import software.amazon.smithy.rust.codegen.util.isStreaming
import software.amazon.smithy.rust.codegen.util.outputShape
import software.amazon.smithy.rust.codegen.util.toSnakeCase

class HttpBoundProtocolPayloadGenerator(
    codegenContext: CodegenContext,
    private val protocol: Protocol,
    private val httpMessageType: HttpMessageType = HttpMessageType.REQUEST
) : ProtocolPayloadGenerator {
    private val symbolProvider = codegenContext.symbolProvider
    private val model = codegenContext.model
    private val runtimeConfig = codegenContext.runtimeConfig
    private val mode = codegenContext.mode
    private val httpBindingResolver = protocol.httpBindingResolver

    private val operationSerModule = RustModule.private("operation_ser")

    private val codegenScope = arrayOf(
        "hyper" to CargoDependency.HyperWithStream.asType(),
        "ByteStream" to RuntimeType.byteStream(runtimeConfig),
        "ByteSlab" to RuntimeType.ByteSlab,
        "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
        "BuildError" to runtimeConfig.operationBuildError(),
        "SmithyHttp" to CargoDependency.SmithyHttp(runtimeConfig).asType()
    )

    override fun payloadMetadata(operationShape: OperationShape): ProtocolPayloadGenerator.PayloadMetadata {
        val (shape, payloadMemberName) = when (httpMessageType) {
            HttpMessageType.RESPONSE -> operationShape.outputShape(model) to
                    httpBindingResolver.responseMembers(operationShape, HttpLocation.PAYLOAD).firstOrNull()?.memberName
            HttpMessageType.REQUEST -> operationShape.inputShape(model) to
                    httpBindingResolver.requestMembers(operationShape, HttpLocation.PAYLOAD).firstOrNull()?.memberName
        }

        // Only streaming operations (blob streaming and event streams) and *blob non-streaming* operations need to
        // take ownership.
        return if (payloadMemberName == null) {
            ProtocolPayloadGenerator.PayloadMetadata(takesOwnership = false)
        } else if (operationShape.isInputEventStream(model)) {
            ProtocolPayloadGenerator.PayloadMetadata(takesOwnership = true)
        } else {
            val member = shape.expectMember(payloadMemberName)
            when (val type = model.expectShape(member.target)) {
                is StringShape, is DocumentShape, is StructureShape, is UnionShape -> ProtocolPayloadGenerator.PayloadMetadata(
                    takesOwnership = false
                )
                is BlobShape -> ProtocolPayloadGenerator.PayloadMetadata(takesOwnership = true)
                else -> UNREACHABLE("Unexpected payload target type: $type")
            }
        }
    }

    override fun generatePayload(writer: RustWriter, self: String, operationShape: OperationShape) {
        val bodyMetadata = payloadMetadata(operationShape)
        val serializerGenerator = protocol.structuredDataSerializer(operationShape)
        val payloadMemberName = when (httpMessageType) {
            HttpMessageType.RESPONSE ->
                httpBindingResolver.responseMembers(operationShape, HttpLocation.PAYLOAD)
            HttpMessageType.REQUEST ->
                httpBindingResolver.requestMembers(operationShape, HttpLocation.PAYLOAD)
        }.firstOrNull()?.memberName
        if (payloadMemberName == null) {
            val serializer = when (httpMessageType) {
                HttpMessageType.RESPONSE ->
                    serializerGenerator.serverOutputSerializer(operationShape)
                HttpMessageType.REQUEST ->
                    serializerGenerator.operationSerializer(operationShape)
            }
            if (serializer == null) {
                writer.rust("\"\"")
            } else {
                writer.rust(
                    "#T(&$self)?",
                    serializer,
                )
            }
        } else {
            val member = when (httpMessageType) {
                HttpMessageType.RESPONSE ->
                    operationShape.outputShape(model).expectMember(payloadMemberName)
                HttpMessageType.REQUEST ->
                    operationShape.inputShape(model).expectMember(payloadMemberName)
            }

            // TODO(https://github.com/awslabs/smithy-rs/issues/1157) Add support for server event streams.
            if (operationShape.isInputEventStream(model)) {
                writer.serializeViaEventStream(operationShape, member, serializerGenerator)
            } else {
                writer.serializeViaPayload(bodyMetadata, self, member, serializerGenerator)
            }
        }
    }

    private fun RustWriter.serializeViaEventStream(
        operationShape: OperationShape,
        memberShape: MemberShape,
        serializerGenerator: StructuredDataSerializerGenerator
    ) {
        val memberName = symbolProvider.toMemberName(memberShape)
        val unionShape = model.expectShape(memberShape.target, UnionShape::class.java)

        val marshallerConstructorFn = EventStreamMarshallerGenerator(
            model,
            mode,
            runtimeConfig,
            symbolProvider,
            unionShape,
            serializerGenerator,
            httpBindingResolver.requestContentType(operationShape)
                ?: throw CodegenException("event streams must set a content type"),
        ).render()

        // TODO(EventStream): [RPC] RPC protocols need to send an initial message with the
        // parameters that are not `@eventHeader` or `@eventPayload`.
        rustTemplate(
            """
            {
                let marshaller = #{marshallerConstructorFn}();
                let signer = _config.new_event_stream_signer(properties.clone());
                let adapter: #{SmithyHttp}::event_stream::MessageStreamAdapter<_, #{OperationError}> =
                    self.$memberName.into_body_stream(marshaller, signer);
                let body: #{SdkBody} = #{hyper}::Body::wrap_stream(adapter).into();
                body
            }
            """,
            *codegenScope,
            "marshallerConstructorFn" to marshallerConstructorFn,
            "OperationError" to operationShape.errorSymbol(symbolProvider)
        )
    }

    private fun RustWriter.serializeViaPayload(
        payloadMetadata: ProtocolPayloadGenerator.PayloadMetadata,
        self: String,
        member: MemberShape,
        serializerGenerator: StructuredDataSerializerGenerator
    ) {
        val fnName = "serialize_payload_${member.container.name.toSnakeCase()}"
        val ref = if (payloadMetadata.takesOwnership) "" else "&"
        val serializer = RuntimeType.forInlineFun(fnName, operationSerModule) {
            val outputT = if (member.isStreaming(model)) "ByteStream" else "ByteSlab"
            it.rustBlockTemplate(
                "pub fn $fnName(payload: $ref#{Member}) -> Result<#{$outputT}, #{BuildError}>",
                "Member" to symbolProvider.toSymbol(member),
                *codegenScope
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
                        *codegenScope
                    ) {
                        when (val targetShape = model.expectShape(member.target)) {
                            is StringShape, is BlobShape, is DocumentShape -> rust(
                                // TODO Can we get rid of this allocation and make the function return a slice of bytes?
                                """
                                b"".to_vec()
                                """
                            )
                            // If this targets a member and the member is `None`, return an "empty" `Vec<u8>`.
                            is StructureShape -> rust("#T()", serializerGenerator.unsetStructure(targetShape))
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
        serializer: StructuredDataSerializerGenerator
    ) {
        when (val targetShape = model.expectShape(member.target)) {
            is StringShape -> {
                // Write the raw string to the payload.
                // TODO Can we get rid of this allocation and make the function return a slice of bytes?
                if (targetShape.hasTrait<EnumTrait>()) {
                    rust("$payloadName.as_str().as_bytes().to_vec()")
                } else {
                    // TODO Cannot use `into_bytes()` because payload is behind shared reference.
                    rust("$payloadName.as_bytes().to_vec()")
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
                    !((targetShape as? UnionShape)?.isEventStream() ?: false)
                ) { "Event Streams should be handled further up" }

                rust(
                    "#T($payloadName)?",
                    serializer.payloadSerializer(member)
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