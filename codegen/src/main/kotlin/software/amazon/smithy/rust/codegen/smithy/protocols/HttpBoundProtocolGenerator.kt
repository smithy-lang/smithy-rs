/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.protocols

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.DocumentShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.assignment
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.withBlock
import software.amazon.smithy.rust.codegen.rustlang.withBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.builderSymbol
import software.amazon.smithy.rust.codegen.smithy.generators.error.errorSymbol
import software.amazon.smithy.rust.codegen.smithy.generators.http.ResponseBindingGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.operationBuildError
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.MakeOperationGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolBodyGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolBodyGenerator.BodyMetadata
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolTraitImplGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.setterName
import software.amazon.smithy.rust.codegen.smithy.isOptional
import software.amazon.smithy.rust.codegen.smithy.protocols.parse.StructuredDataParserGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.serialize.EventStreamMarshallerGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.serialize.StructuredDataSerializerGenerator
import software.amazon.smithy.rust.codegen.smithy.transformers.errorMessageMember
import software.amazon.smithy.rust.codegen.util.PANIC
import software.amazon.smithy.rust.codegen.util.UNREACHABLE
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.expectMember
import software.amazon.smithy.rust.codegen.util.hasStreamingMember
import software.amazon.smithy.rust.codegen.util.hasTrait
import software.amazon.smithy.rust.codegen.util.inputShape
import software.amazon.smithy.rust.codegen.util.isEventStream
import software.amazon.smithy.rust.codegen.util.isInputEventStream
import software.amazon.smithy.rust.codegen.util.isStreaming
import software.amazon.smithy.rust.codegen.util.outputShape
import software.amazon.smithy.rust.codegen.util.toSnakeCase

class HttpBoundProtocolGenerator(
    codegenContext: CodegenContext,
    protocol: Protocol,
) : ProtocolGenerator(
    codegenContext,
    protocol,
    MakeOperationGenerator(codegenContext, protocol, HttpBoundProtocolBodyGenerator(codegenContext, protocol)),
    HttpBoundProtocolTraitImplGenerator(codegenContext, protocol),
)

class HttpBoundProtocolTraitImplGenerator(
    private val codegenContext: CodegenContext,
    private val protocol: Protocol,
) : ProtocolTraitImplGenerator {
    private val symbolProvider = codegenContext.symbolProvider
    private val model = codegenContext.model
    private val runtimeConfig = codegenContext.runtimeConfig
    private val httpBindingResolver = protocol.httpBindingResolver
    private val operationDeserModule = RustModule.private("operation_deser")

    private val codegenScope = arrayOf(
        "ParseStrict" to RuntimeType.parseStrictResponse(runtimeConfig),
        "ParseResponse" to RuntimeType.parseResponse(runtimeConfig),
        "http" to RuntimeType.http,
        "operation" to RuntimeType.operationModule(runtimeConfig),
        "Bytes" to RuntimeType.Bytes,
    )

    override fun generateTraitImpls(operationWriter: RustWriter, operationShape: OperationShape) {
        val outputSymbol = symbolProvider.toSymbol(operationShape.outputShape(model))
        val operationName = symbolProvider.toSymbol(operationShape).name

        // For streaming response bodies, we need to generate a different implementation of the parse traits.
        // These will first offer the streaming input to the parser & potentially read the body into memory
        // if an error occurred or if the streaming parser indicates that it needs the full data to proceed.
        if (operationShape.outputShape(model).hasStreamingMember(model)) {
            with(operationWriter) {
                renderStreamingTraits(operationName, outputSymbol, operationShape)
            }
        } else {
            with(operationWriter) {
                renderNonStreamingTraits(operationName, outputSymbol, operationShape)
            }
        }
    }

    private fun RustWriter.renderNonStreamingTraits(
        operationName: String?,
        outputSymbol: Symbol,
        operationShape: OperationShape
    ) {
        val successCode = httpBindingResolver.httpTrait(operationShape).code
        rustTemplate(
            """
            impl #{ParseStrict} for $operationName {
                type Output = std::result::Result<#{O}, #{E}>;
                fn parse(&self, response: &#{http}::Response<#{Bytes}>) -> Self::Output {
                     if !response.status().is_success() && response.status().as_u16() != $successCode {
                        #{parse_error}(response)
                     } else {
                        #{parse_response}(response)
                     }
                }
            }""",
            *codegenScope,
            "O" to outputSymbol,
            "E" to operationShape.errorSymbol(symbolProvider),
            "parse_error" to parseError(operationShape),
            "parse_response" to parseResponse(operationShape)
        )
    }

    private fun RustWriter.renderStreamingTraits(
        operationName: String,
        outputSymbol: Symbol,
        operationShape: OperationShape
    ) {
        val successCode = httpBindingResolver.httpTrait(operationShape).code
        rustTemplate(
            """
            impl #{ParseResponse} for $operationName {
                type Output = std::result::Result<#{O}, #{E}>;
                fn parse_unloaded(&self, response: &mut #{operation}::Response) -> Option<Self::Output> {
                    // This is an error, defer to the non-streaming parser
                    if !response.http().status().is_success() && response.http().status().as_u16() != $successCode {
                        return None;
                    }
                    Some(#{parse_streaming_response}(response))
                }
                fn parse_loaded(&self, response: &#{http}::Response<#{Bytes}>) -> Self::Output {
                    // if streaming, we only hit this case if its an error
                    #{parse_error}(response)
                }
            }
            """,
            "O" to outputSymbol,
            "E" to operationShape.errorSymbol(symbolProvider),
            "parse_streaming_response" to parseStreamingResponse(operationShape),
            "parse_error" to parseError(operationShape),
            *codegenScope
        )
    }

    fun parseError(operationShape: OperationShape): RuntimeType {
        val fnName = "parse_${operationShape.id.name.toSnakeCase()}_error"
        val outputShape = operationShape.outputShape(model)
        val outputSymbol = symbolProvider.toSymbol(outputShape)
        val errorSymbol = operationShape.errorSymbol(symbolProvider)
        return RuntimeType.forInlineFun(fnName, operationDeserModule) {
            Attribute.Custom("allow(clippy::unnecessary_wraps)").render(it)
            it.rustBlockTemplate(
                "pub fn $fnName(response: &#{http}::Response<#{Bytes}>) -> std::result::Result<#{O}, #{E}>",
                *codegenScope,
                "O" to outputSymbol,
                "E" to errorSymbol
            ) {
                rust(
                    "let generic = #T(response).map_err(#T::unhandled)?;",
                    protocol.parseHttpGenericError(operationShape),
                    errorSymbol
                )
                if (operationShape.errors.isNotEmpty()) {
                    rustTemplate(
                        """
                        let error_code = match generic.code() {
                            Some(code) => code,
                            None => return Err(#{error_symbol}::unhandled(generic))
                        };

                        let _error_message = generic.message().map(|msg|msg.to_owned());
                        """,
                        "error_symbol" to errorSymbol,
                    )
                    withBlock("Err(match error_code {", "})") {
                        operationShape.errors.forEach { error ->
                            val errorShape = model.expectShape(error, StructureShape::class.java)
                            val variantName = symbolProvider.toSymbol(model.expectShape(error)).name
                            val errorCode = httpBindingResolver.errorCode(errorShape).dq()
                            withBlock(
                                "$errorCode => #1T { meta: generic, kind: #1TKind::$variantName({",
                                "})},",
                                errorSymbol
                            ) {
                                Attribute.AllowUnusedMut.render(this)
                                assignment("mut tmp") {
                                    rustBlock("") {
                                        renderShapeParser(
                                            operationShape,
                                            errorShape,
                                            httpBindingResolver.errorResponseBindings(errorShape),
                                            errorSymbol
                                        )
                                    }
                                }
                                if (errorShape.errorMessageMember() != null) {
                                    rust(
                                        """
                                        if (&tmp.message).is_none() {
                                            tmp.message = _error_message;
                                        }
                                        """
                                    )
                                }
                                rust("tmp")
                            }
                        }
                        rust("_ => #T::generic(generic)", errorSymbol)
                    }
                } else {
                    rust("Err(#T::generic(generic))", errorSymbol)
                }
            }
        }
    }

    private fun parseStreamingResponse(operationShape: OperationShape): RuntimeType {
        val fnName = "parse_${operationShape.id.name.toSnakeCase()}"
        val outputShape = operationShape.outputShape(model)
        val outputSymbol = symbolProvider.toSymbol(outputShape)
        val errorSymbol = operationShape.errorSymbol(symbolProvider)
        return RuntimeType.forInlineFun(fnName, operationDeserModule) {
            Attribute.Custom("allow(clippy::unnecessary_wraps)").render(it)
            it.rustBlockTemplate(
                "pub fn $fnName(op_response: &mut #{operation}::Response) -> std::result::Result<#{O}, #{E}>",
                *codegenScope,
                "O" to outputSymbol,
                "E" to errorSymbol
            ) {
                write("let response = op_response.http_mut();")
                withBlock("Ok({", "})") {
                    renderShapeParser(
                        operationShape,
                        outputShape,
                        httpBindingResolver.responseBindings(operationShape),
                        errorSymbol
                    )
                }
            }
        }
    }

    fun parseResponse(operationShape: OperationShape): RuntimeType {
        val fnName = "parse_${operationShape.id.name.toSnakeCase()}_response"
        val outputShape = operationShape.outputShape(model)
        val outputSymbol = symbolProvider.toSymbol(outputShape)
        val errorSymbol = operationShape.errorSymbol(symbolProvider)
        return RuntimeType.forInlineFun(fnName, operationDeserModule) {
            Attribute.Custom("allow(clippy::unnecessary_wraps)").render(it)
            it.rustBlockTemplate(
                "pub fn $fnName(response: &#{http}::Response<#{Bytes}>) -> std::result::Result<#{O}, #{E}>",
                *codegenScope,
                "O" to outputSymbol,
                "E" to errorSymbol
            ) {
                withBlock("Ok({", "})") {
                    renderShapeParser(
                        operationShape,
                        outputShape,
                        httpBindingResolver.responseBindings(operationShape),
                        errorSymbol
                    )
                }
            }
        }
    }

    private fun RustWriter.renderShapeParser(
        operationShape: OperationShape,
        outputShape: StructureShape,
        bindings: List<HttpBindingDescriptor>,
        errorSymbol: RuntimeType,
    ) {
        val httpBindingGenerator = ResponseBindingGenerator(protocol, codegenContext, operationShape)
        val structuredDataParser = protocol.structuredDataParser(operationShape)
        Attribute.AllowUnusedMut.render(this)
        rust("let mut output = #T::default();", outputShape.builderSymbol(symbolProvider))
        // avoid non-usage warnings for response
        rust("let _ = response;")
        if (outputShape.id == operationShape.output.get()) {
            structuredDataParser.operationParser(operationShape)?.also { parser ->
                rust(
                    "output = #T(response.body().as_ref(), output).map_err(#T::unhandled)?;",
                    parser,
                    errorSymbol
                )
            }
        } else {
            check(outputShape.hasTrait<ErrorTrait>()) { "should only be called on outputs or errors $outputShape" }
            structuredDataParser.errorParser(outputShape)?.also { parser ->
                rust(
                    "output = #T(response.body().as_ref(), output).map_err(#T::unhandled)?;",
                    parser, errorSymbol
                )
            }
        }
        for (binding in bindings) {
            val member = binding.member
            val parsedValue = renderBindingParser(binding, operationShape, httpBindingGenerator, structuredDataParser)
            if (parsedValue != null) {
                withBlock("output = output.${member.setterName()}(", ");") {
                    parsedValue(this)
                }
            }
        }

        val err = if (StructureGenerator.fallibleBuilder(outputShape, symbolProvider)) {
            ".map_err(${format(errorSymbol)}::unhandled)?"
        } else ""
        rust("output.build()$err")
    }

    /**
     * Generate a parser & a parsed value converter for each output member of `operationShape`
     *
     * Returns a map with key = memberName, value = parsedValue
     */
    private fun renderBindingParser(
        binding: HttpBindingDescriptor,
        operationShape: OperationShape,
        httpBindingGenerator: ResponseBindingGenerator,
        structuredDataParser: StructuredDataParserGenerator,
    ): Writable? {
        val errorSymbol = operationShape.errorSymbol(symbolProvider)
        val member = binding.member
        return when (binding.location) {
            HttpLocation.HEADER -> writable {
                val fnName = httpBindingGenerator.generateDeserializeHeaderFn(binding)
                rust(
                    """
                    #T(response.headers())
                        .map_err(|_|#T::unhandled("Failed to parse ${member.memberName} from header `${binding.locationName}"))?
                    """,
                    fnName, errorSymbol
                )
            }
            HttpLocation.DOCUMENT -> {
                // document is handled separately
                null
            }
            HttpLocation.PAYLOAD -> {
                val docShapeHandler: RustWriter.(String) -> Unit = { body ->
                    rust(
                        "#T($body).map_err(#T::unhandled)",
                        structuredDataParser.documentParser(operationShape),
                        errorSymbol
                    )
                }
                val structureShapeHandler: RustWriter.(String) -> Unit = { body ->
                    rust("#T($body).map_err(#T::unhandled)", structuredDataParser.payloadParser(member), errorSymbol)
                }
                val deserializer = httpBindingGenerator.generateDeserializePayloadFn(
                    operationShape,
                    binding,
                    errorSymbol,
                    docHandler = docShapeHandler,
                    structuredHandler = structureShapeHandler
                )
                return if (binding.member.isStreaming(model)) {
                    writable { rust("Some(#T(response.body_mut())?)", deserializer) }
                } else {
                    writable { rust("#T(response.body().as_ref())?", deserializer) }
                }
            }
            HttpLocation.RESPONSE_CODE -> writable {
                rust("Some(response.status().as_u16() as _)")
            }
            HttpLocation.PREFIX_HEADERS -> {
                val sym = httpBindingGenerator.generateDeserializePrefixHeaderFn(binding)
                writable {
                    rustTemplate(
                        """
                        #{deser}(response.headers())
                             .map_err(|_|
                                #{err}::unhandled("Failed to parse ${member.memberName} from prefix header `${binding.locationName}")
                             )?
                        """,
                        "deser" to sym, "err" to errorSymbol
                    )
                }
            }
            else -> {
                UNREACHABLE("Unexpected binding location: ${binding.location}")
            }
        }
    }
}

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
        "ParseStrict" to RuntimeType.parseStrictResponse(runtimeConfig),
        "ParseResponse" to RuntimeType.parseResponse(runtimeConfig),
        "http" to RuntimeType.http,
        "hyper" to CargoDependency.HyperWithStream.asType(),
        "operation" to RuntimeType.operationModule(runtimeConfig),
        "Bytes" to RuntimeType.Bytes,
        "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
        "BuildError" to runtimeConfig.operationBuildError(),
        "SmithyHttp" to CargoDependency.SmithyHttp(runtimeConfig).asType()
    )

    override fun bodyMetadata(operationShape: OperationShape): BodyMetadata {
        val inputShape = operationShape.inputShape(model)
        val payloadMemberName =
            httpBindingResolver.requestMembers(operationShape, HttpLocation.PAYLOAD).firstOrNull()?.memberName

        // Only streaming operations take ownership, so that's event streams and blobs
        return if (payloadMemberName == null) {
            BodyMetadata(takesOwnership = false)
        } else if (operationShape.isInputEventStream(model)) {
            BodyMetadata(takesOwnership = true)
        } else {
            val member = inputShape.expectMember(payloadMemberName)
            when (val type = model.expectShape(member.target)) {
                is StringShape, is DocumentShape, is StructureShape, is UnionShape -> BodyMetadata(takesOwnership = false)
                is BlobShape -> BodyMetadata(takesOwnership = true)
                else -> UNREACHABLE("Unexpected payload target type: $type")
            }
        }
    }

    override fun generateBody(writer: RustWriter, self: String, operationShape: OperationShape) {
        val bodyMetadata = bodyMetadata(operationShape)
        val serializerGenerator = protocol.structuredDataSerializer(operationShape)
        val inputShape = operationShape.inputShape(model)
        val payloadMemberName =
            httpBindingResolver.requestMembers(operationShape, HttpLocation.PAYLOAD).firstOrNull()?.memberName
        if (payloadMemberName == null) {
            serializerGenerator.operationSerializer(operationShape)?.let { serializer ->
                writer.rust(
                    "#T(&self)?",
                    serializer,
                )
            } ?: writer.rustTemplate("#{SdkBody}::from(\"\")", *codegenScope)
        } else {
            val member = inputShape.expectMember(payloadMemberName)
            if (operationShape.isInputEventStream(model)) {
                writer.serializeViaEventStream(operationShape, member, serializerGenerator)
            } else {
                writer.serializeViaPayload(bodyMetadata, member, serializerGenerator)
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
        bodyMetadata: BodyMetadata,
        member: MemberShape,
        serializerGenerator: StructuredDataSerializerGenerator
    ) {
        val fnName = "ser_payload_${member.container.name.toSnakeCase()}"
        val ref = when (bodyMetadata.takesOwnership) {
            true -> ""
            false -> "&"
        }
        val serializer = RuntimeType.forInlineFun(fnName, operationSerModule) {
            it.rustBlockTemplate(
                "pub fn $fnName(payload: $ref #{Member}) -> std::result::Result<#{SdkBody}, #{BuildError}>",
                "Member" to symbolProvider.toSymbol(member),
                *codegenScope
            ) {
                // If this targets a member & the member is None, return an empty vec
                val ref = when (bodyMetadata.takesOwnership) {
                    false -> ".as_ref()"
                    true -> ""
                }

                if (symbolProvider.toSymbol(member).isOptional()) {
                    withBlockTemplate(
                        """
                        let payload = match payload$ref {
                            Some(t) => t,
                            None => return Ok(#{SdkBody}::from(""",
                        "))};",
                        *codegenScope
                    ) {
                        when (val targetShape = model.expectShape(member.target)) {
                            is StringShape, is BlobShape, is DocumentShape -> rust("".dq())
                            is StructureShape -> rust("#T()", serializerGenerator.unsetStructure(targetShape))
                        }
                    }
                }
                // When the body is a streaming blob it _literally_ is a SdkBody already
                // mute this clippy warning to make the codegen a little simpler
                Attribute.Custom("allow(clippy::useless_conversion)").render(this)
                withBlock("Ok(#T::from(", "))", RuntimeType.sdkBody(runtimeConfig)) {
                    renderPayload(member, "payload", serializerGenerator)
                }
            }
        }
        rust("#T($ref self.${symbolProvider.toMemberName(member)})?", serializer)
    }

    private fun RustWriter.renderPayload(
        member: MemberShape,
        payloadName: String,
        serializer: StructuredDataSerializerGenerator
    ) {
        when (val targetShape = model.expectShape(member.target)) {
            // Write the raw string to the payload
            is StringShape -> {
                if (targetShape.hasTrait<EnumTrait>()) {
                    rust("$payloadName.as_str()")
                } else {
                    rust("""$payloadName.to_string()""")
                }
            }

            // This works for streaming & non streaming blobs because they both have `into_inner()` which
            // can be converted into an SDK body!
            is BlobShape -> {
                // Write the raw blob to the payload
                rust("$payloadName.into_inner()")
            }
            is StructureShape, is UnionShape -> {
                check(
                    !((targetShape as? UnionShape)?.isEventStream() ?: false)
                ) { "Event Streams should be handled further up" }

                // JSON serialize the structure or union targeted
                rust(
                    "#T($payloadName)?",
                    serializer.payloadSerializer(member)
                )
            }
            is DocumentShape -> {
                rust(
                    "#T($payloadName)?",
                    serializer.documentSerializer(),
                )
            }
            else -> PANIC("Unexpected payload target type: $targetShape")
        }
    }
}
