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
import software.amazon.smithy.rust.codegen.rustlang.Attribute
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
import software.amazon.smithy.rust.codegen.smithy.generators.operationBuildError
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolBodyGenerator
import software.amazon.smithy.rust.codegen.smithy.isOptional
import software.amazon.smithy.rust.codegen.smithy.protocols.serialize.EventStreamMarshallerGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.serialize.StructuredDataSerializerGenerator
import software.amazon.smithy.rust.codegen.util.PANIC
import software.amazon.smithy.rust.codegen.util.UNREACHABLE
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.expectMember
import software.amazon.smithy.rust.codegen.util.hasTrait
import software.amazon.smithy.rust.codegen.util.inputShape
import software.amazon.smithy.rust.codegen.util.isEventStream
import software.amazon.smithy.rust.codegen.util.isInputEventStream
import software.amazon.smithy.rust.codegen.util.outputShape
import software.amazon.smithy.rust.codegen.util.toSnakeCase

class HttpBoundProtocolBodyGenerator(
    codegenContext: CodegenContext,
    private val protocol: Protocol,
) : ProtocolBodyGenerator {
    private val symbolProvider = codegenContext.symbolProvider
    private val model = codegenContext.model
    private val runtimeConfig = codegenContext.runtimeConfig
    private val mode = codegenContext.mode
    private val httpBindingResolver = protocol.httpBindingResolver

    private val operationSerModule = RustModule.private("operation_ser")

    private val codegenScope = arrayOf(
        // TODO prune
        "ParseStrict" to RuntimeType.parseStrictResponse(runtimeConfig),
        "ParseResponse" to RuntimeType.parseResponse(runtimeConfig),
        "http" to RuntimeType.http,
        "hyper" to CargoDependency.HyperWithStream.asType(),
        "operation" to RuntimeType.operationModule(runtimeConfig),
        "Bytes" to RuntimeType.Bytes,
        "ByteSlab" to RuntimeType.ByteSlab,
        "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
        "BuildError" to runtimeConfig.operationBuildError(),
        "SmithyHttp" to CargoDependency.SmithyHttp(runtimeConfig).asType()
    )

    override fun bodyMetadata(operationShape: OperationShape): ProtocolBodyGenerator.BodyMetadata {
        val inputShape = operationShape.inputShape(model)
        val payloadMemberName =
            httpBindingResolver.requestMembers(operationShape, HttpLocation.PAYLOAD).firstOrNull()?.memberName

        // Only streaming operations take ownership, so that's event streams and blobs.
        return if (payloadMemberName == null) {
            ProtocolBodyGenerator.BodyMetadata(takesOwnership = false)
        } else if (operationShape.isInputEventStream(model)) {
            ProtocolBodyGenerator.BodyMetadata(takesOwnership = true)
        } else {
            val member = inputShape.expectMember(payloadMemberName)
            when (val type = model.expectShape(member.target)) {
                is StringShape, is DocumentShape, is StructureShape, is UnionShape -> ProtocolBodyGenerator.BodyMetadata(
                    takesOwnership = false
                )
                is BlobShape -> ProtocolBodyGenerator.BodyMetadata(takesOwnership = true)
                else -> UNREACHABLE("Unexpected payload target type: $type")
            }
        }
    }

    override fun generateBody(writer: RustWriter, self: String, operationShape: OperationShape) {
        val server = true

        val bodyMetadata = bodyMetadata(operationShape)
        val serializerGenerator = protocol.structuredDataSerializer(operationShape)
        val payloadMemberName = if (server) {
            httpBindingResolver.responseMembers(operationShape, HttpLocation.PAYLOAD).firstOrNull()?.memberName
        } else {
            httpBindingResolver.requestMembers(operationShape, HttpLocation.PAYLOAD).firstOrNull()?.memberName
        }
        if (payloadMemberName == null) {
            if (server) {
                serializerGenerator.serverOutputSerializer(operationShape)?.let { serializer ->
                    writer.rust(
                        "#T(&$self)?",
                        serializer,
                    )
                } ?: writer.rustTemplate("\"\"", *codegenScope)
            } else {
                serializerGenerator.operationSerializer(operationShape)?.let { serializer ->
                    writer.rust(
                        "#T(&$self)?",
                        serializer,
                    )
                } ?: writer.rustTemplate("#{SdkBody}::from(\"\")", *codegenScope)
            }
        } else {
            val member = if (server) {
                operationShape.outputShape(model).expectMember(payloadMemberName)
            } else {
                operationShape.inputShape(model).expectMember(payloadMemberName)
            }

            // TODO Server event streams
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

    // TODO Document self
    private fun RustWriter.serializeViaPayload(
        bodyMetadata: ProtocolBodyGenerator.BodyMetadata,
        self: String,
        member: MemberShape,
        serializerGenerator: StructuredDataSerializerGenerator
    ) {
        val fnName = "serialize_payload_${member.container.name.toSnakeCase()}"
        val ref = if (bodyMetadata.takesOwnership) "" else "&"
        val serializer = RuntimeType.forInlineFun(fnName, operationSerModule) {
            // TODO Make this return the inner type better.
            it.rustBlockTemplate(
                "pub fn $fnName(payload: $ref#{Member}) -> Result<#{ByteSlab}, #{BuildError}>",
                "Member" to symbolProvider.toSymbol(member),
                *codegenScope
            ) {
                val asRef = if (bodyMetadata.takesOwnership) "" else ".as_ref()"

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
                                b"\"\"".to_vec()
                                """
                            )
                            // If this targets a member and the member is `None`, return an "empty" `Vec<u8>`.
                            is StructureShape -> rust("#T()", serializerGenerator.unsetStructure(targetShape))
                        }
                    }
                }

                // TODO Comment does not apply
                // When the body is a streaming blob it _literally_ is an `SdkBody` already.
                // Mute this Clippy warning to make the codegen a little simpler in that case.
                Attribute.Custom("allow(clippy::useless_conversion)").render(this)
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
                    rust("$payloadName.as_bytes().to_vec()")
                } else {
                    // TODO Cannot use `into_bytes()` because payload is behind shared reference.
                    rust("$payloadName.as_bytes().to_vec()")
                }
            }

            is BlobShape -> {
                // This works for streaming and non-streaming blobs because they both have `into_inner()` which
                // can be converted into an `SdkBody`!
                // Write the raw blob to the payload.
                rust("$payloadName.into_inner()")
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