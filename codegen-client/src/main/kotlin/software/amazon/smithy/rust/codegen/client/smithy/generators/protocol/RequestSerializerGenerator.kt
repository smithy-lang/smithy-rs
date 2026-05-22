/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.protocol

import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.DocumentShape
import software.amazon.smithy.model.shapes.EnumShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.model.traits.HttpPayloadTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.customizations.SchemaSerdeAllowlist
import software.amazon.smithy.rust.codegen.client.smithy.generators.http.RequestBindingGenerator
import software.amazon.smithy.rust.codegen.client.smithy.protocols.renderClientEventStreamBody
import software.amazon.smithy.rust.codegen.core.rustlang.InlineDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolPayloadGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.findStreamingMember
import software.amazon.smithy.rust.codegen.core.util.inputShape

class RequestSerializerGenerator(
    private val codegenContext: ClientCodegenContext,
    private val protocol: Protocol,
    private val bodyGenerator: ProtocolPayloadGenerator?,
    private val nameOverride: String? = null,
) {
    private val httpBindingResolver = protocol.httpBindingResolver
    private val symbolProvider = codegenContext.symbolProvider
    private val codegenScope by lazy {
        val runtimeApi = RuntimeType.smithyRuntimeApiClient(codegenContext.runtimeConfig)
        val interceptorContext = runtimeApi.resolve("client::interceptors::context")
        arrayOf(
            *preludeScope,
            "BoxError" to RuntimeType.boxError(codegenContext.runtimeConfig),
            "config" to ClientRustModule.config,
            "ConfigBag" to RuntimeType.configBag(codegenContext.runtimeConfig),
            "header_util" to RuntimeType.smithyHttp(codegenContext.runtimeConfig).resolve("header"),
            "http" to RuntimeType.Http1x,
            "HttpRequest" to runtimeApi.resolve("client::orchestrator::HttpRequest"),
            "HttpRequestBuilder" to RuntimeType.HttpRequestBuilder1x,
            "Input" to interceptorContext.resolve("Input"),
            "SerializeRequest" to runtimeApi.resolve("client::ser_de::SerializeRequest"),
            "SharedClientProtocol" to RuntimeType.smithySchema(codegenContext.runtimeConfig).resolve("protocol::SharedClientProtocol"),
            "SdkBody" to RuntimeType.sdkBody(codegenContext.runtimeConfig),
            "HeaderSerializationSettings" to
                RuntimeType.forInlineDependency(
                    InlineDependency.serializationSettings(
                        codegenContext.runtimeConfig,
                    ),
                ).resolve("HeaderSerializationSettings"),
        )
    }

    private val schemaExclusive = SchemaSerdeAllowlist.usesSchemaSerdeExclusively(codegenContext)

    fun render(
        writer: RustWriter,
        operationShape: OperationShape,
    ) {
        val inputShape = operationShape.inputShape(codegenContext.model)
        val operationName = symbolProvider.toSymbol(operationShape).name
        val inputSymbol = symbolProvider.toSymbol(inputShape)
        val serializerName = nameOverride ?: "${operationName}RequestSerializer"
        val schemaRef =
            if (nameOverride != null) "$operationName::PRESIGNED_INPUT_SCHEMA" else "$operationName::INPUT_SCHEMA"

        if (schemaExclusive) {
            renderSchemaOnly(writer, operationShape, inputShape, operationName, inputSymbol, serializerName, schemaRef)
        } else {
            renderLegacy(writer, operationShape, inputShape, operationName, inputSymbol, serializerName)
        }
    }

    /** Schema-only path: no runtime check, no fallback to old codegen. */
    private fun renderSchemaOnly(
        writer: RustWriter,
        operationShape: OperationShape,
        inputShape: StructureShape,
        operationName: String,
        inputSymbol: software.amazon.smithy.codegen.core.Symbol,
        serializerName: String,
        schemaRef: String,
    ) {
        writer.rustTemplate(
            """
            ##[derive(Debug)]
            struct $serializerName;
            impl #{SerializeRequest} for $serializerName {
                ##[allow(unused_mut, clippy::let_and_return, clippy::needless_borrow, clippy::useless_conversion)]
                fn serialize_input(&self, input: #{Input}, _cfg: &mut #{ConfigBag}) -> #{Result}<#{HttpRequest}, #{BoxError}> {
                    let input = input.downcast::<#{ConcreteInput}>().expect("correct type");
                    let protocol = _cfg.load::<#{SharedClientProtocol}>()
                        .expect("a SharedClientProtocol is required");
                    #{schema_serialize}
                }
            }
            """,
            *codegenScope,
            "ConcreteInput" to inputSymbol,
            "schema_serialize" to schemaSerialize(operationShape, operationName, inputShape, schemaRef),
        )
    }

    /** Legacy path: old codegen only, no schema-based serialization. */
    private fun renderLegacy(
        writer: RustWriter,
        operationShape: OperationShape,
        inputShape: StructureShape,
        operationName: String,
        inputSymbol: software.amazon.smithy.codegen.core.Symbol,
        serializerName: String,
    ) {
        writer.rustTemplate(
            """
            ##[derive(Debug)]
            struct $serializerName;
            impl #{SerializeRequest} for $serializerName {
                ##[allow(unused_mut, clippy::let_and_return, clippy::needless_borrow, clippy::useless_conversion)]
                fn serialize_input(&self, input: #{Input}, _cfg: &mut #{ConfigBag}) -> #{Result}<#{HttpRequest}, #{BoxError}> {
                    let input = input.downcast::<#{ConcreteInput}>().expect("correct type");
                    let _header_serialization_settings = _cfg.load::<#{HeaderSerializationSettings}>().cloned().unwrap_or_default();
                    let mut request_builder = {
                        #{create_http_request}
                    };
                    let body = #{generate_body};
                    #{add_content_length}
                    #{Ok}(request_builder.body(body).expect("valid request").try_into().unwrap())
                }
            }
            """,
            *codegenScope,
            "ConcreteInput" to inputSymbol,
            "create_http_request" to createHttpRequest(operationShape),
            "generate_body" to
                writable {
                    if (bodyGenerator != null) {
                        val body =
                            writable {
                                bodyGenerator.generatePayload(this, "input", operationShape)
                            }
                        val streamingMember = inputShape.findStreamingMember(codegenContext.model)
                        val isBlobStreaming =
                            streamingMember != null && codegenContext.model.expectShape(streamingMember.target) is BlobShape
                        if (isBlobStreaming) {
                            rust("#T.into_inner()", body)
                        } else {
                            rustTemplate("#{SdkBody}::from(#{body})", *codegenScope, "body" to body)
                        }
                    } else {
                        rustTemplate("#{SdkBody}::empty()", *codegenScope)
                    }
                },
            "add_content_length" to
                if (needsContentLength(operationShape)) {
                    writable {
                        rustTemplate(
                            """
                            if let Some(content_length) = body.content_length() {
                                let content_length = content_length.to_string();
                                request_builder = _header_serialization_settings.set_default_header(request_builder, #{http}::header::CONTENT_LENGTH, &content_length);
                            }
                            """,
                            *codegenScope,
                        )
                    }
                } else {
                    writable { }
                },
        )
    }

    private fun needsContentLength(operationShape: OperationShape): Boolean =
        protocol.needsRequestContentLength(operationShape)

    private fun createHttpRequest(operationShape: OperationShape): Writable =
        writable {
            val httpBindingGenerator =
                RequestBindingGenerator(
                    codegenContext,
                    protocol,
                    operationShape,
                )
            httpBindingGenerator.renderUpdateHttpBuilder(this)
            val contentType = httpBindingResolver.requestContentType(operationShape)

            rustTemplate("let mut builder = update_http_builder(&input, #{HttpRequestBuilder}::new())?;", *codegenScope)
            if (contentType != null) {
                rustTemplate(
                    "builder = _header_serialization_settings.set_default_header(builder, #{http}::header::CONTENT_TYPE, ${contentType.dq()});",
                    *codegenScope,
                )
            }
            for (header in protocol.additionalRequestHeaders(operationShape)) {
                rustTemplate(
                    """
                    builder = _header_serialization_settings.set_default_header(
                        builder,
                        #{http}::header::HeaderName::from_static(${header.first.dq()}),
                        ${header.second.dq()}
                    );
                    """,
                    *codegenScope,
                )
            }
            rust("builder")
        }

    private fun schemaSerialize(
        operationShape: OperationShape,
        operationName: String,
        inputShape: StructureShape,
        schemaRef: String = "$operationName::INPUT_SCHEMA",
    ): Writable =
        writable {
            val additionalHeaders = protocol.additionalRequestHeaders(operationShape)
            // Helper: generates code to add protocol-level headers to a `request` variable.
            // x-amz-target is excluded because the runtime protocol (AwsJsonRpcProtocol)
            // sets it in serialize_request(). Emitting it here would prevent protocol swapping
            // from removing it when switching to a non-RPC protocol like restJson1.
            val addAdditionalHeaders =
                writable {
                    for (header in additionalHeaders) {
                        if (header.first == "x-amz-target") continue
                        rust("request.headers_mut().insert(${header.first.dq()}, ${header.second.dq()});")
                    }
                }
            // Operations with @http trait in the Smithy model have it encoded in the schema,
            // so the protocol resolves the URI at runtime — pass "". Operations without @http
            // (e.g., rpcv2Cbor) need the path passed from the httpBindingResolver.
            val uriPath =
                if (operationShape.hasTrait(software.amazon.smithy.model.traits.HttpTrait::class.java)) {
                    ""
                } else {
                    httpBindingResolver.httpTrait(operationShape).uri.toString()
                }
            val streamingMember = inputShape.findStreamingMember(codegenContext.model)
            val isBlobStreaming =
                streamingMember != null && codegenContext.model.expectShape(streamingMember.target) is BlobShape
            val isEventStream = streamingMember != null && !isBlobStreaming

            when {
                isBlobStreaming -> renderBlobStreamingRequest(streamingMember!!, schemaRef, uriPath, addAdditionalHeaders)
                isEventStream -> renderEventStreamRequest(operationShape, streamingMember!!, schemaRef, uriPath, addAdditionalHeaders)
                else -> renderStandardRequest(operationShape, inputShape, schemaRef, uriPath, addAdditionalHeaders)
            }
        }

    /** Streaming blob payload: serialize headers/URI via protocol, then replace body with raw ByteStream. */
    private fun RustWriter.renderBlobStreamingRequest(
        streamingMember: MemberShape,
        schemaRef: String,
        uriPath: String,
        addAdditionalHeaders: Writable,
    ) {
        val memberName = symbolProvider.toMemberName(streamingMember)
        val streamingTarget = codegenContext.model.expectShape(streamingMember.target)
        val mediaType = streamingTarget.getTrait(software.amazon.smithy.model.traits.MediaTypeTrait::class.java)
        val contentType = if (mediaType.isPresent) mediaType.get().value else "application/octet-stream"
        rustTemplate(
            """
            let mut request = protocol.serialize_request(
                &input, $schemaRef, ${uriPath.dq()}, _cfg,
            ).map_err(#{BoxError}::from)?;
            // Streaming blob payload: replace the body with the raw ByteStream.
            *request.body_mut() = input.$memberName.into_inner();
            // Default Content-Type for the streaming blob, unless an @httpHeader-bound
            // member already provided one (which the binding protocol set above).
            if !request.headers().contains_key("Content-Type") {
                request.headers_mut().insert("Content-Type", ${contentType.dq()});
            }
            if let #{Some}(content_length) = request.body().content_length() {
                request.headers_mut().insert("Content-Length", content_length.to_string());
            }
            #{add_headers}
            return #{Ok}(request);
            """,
            *codegenScope,
            "add_headers" to addAdditionalHeaders,
        )
    }

    /**
     * Event stream: use schema-based protocol for headers/URI/method,
     * then replace the body with the event stream and set the correct Content-Type.
     */
    private fun RustWriter.renderEventStreamRequest(
        operationShape: OperationShape,
        streamingMember: MemberShape,
        schemaRef: String,
        uriPath: String,
        addAdditionalHeaders: Writable,
    ) {
        val eventStreamBody =
            writable {
                renderClientEventStreamBody(
                    this,
                    codegenContext,
                    protocol,
                    operationShape,
                    streamingMember,
                    outerName = "input",
                )
            }
        // requestContentType returns the event stream content type for input streams
        // (e.g., application/vnd.amazon.eventstream) or the normal protocol content type
        // for output-only streams.
        val contentType = httpBindingResolver.requestContentType(operationShape)
        rustTemplate(
            """
            let mut request = protocol.serialize_request(
                &input, $schemaRef, ${uriPath.dq()}, _cfg,
            ).map_err(#{BoxError}::from)?;
            *request.body_mut() = #{event_stream_body};
            // The protocol may have set Content-Length based on the initial empty body.
            // Remove it since the event stream body has unknown length.
            request.headers_mut().remove("Content-Length");
            """,
            *codegenScope,
            "event_stream_body" to eventStreamBody,
        )
        if (contentType != null) {
            rust("request.headers_mut().insert(\"Content-Type\", ${contentType.dq()});")
        }
        rustTemplate(
            """
            #{add_headers}
            return #{Ok}(request);
            """,
            *codegenScope,
            "add_headers" to addAdditionalHeaders,
        )
    }

    /** Non-streaming request: handles @httpPayload (blob/string/document) and normal struct bodies. */
    private fun RustWriter.renderStandardRequest(
        operationShape: OperationShape,
        inputShape: StructureShape,
        schemaRef: String,
        uriPath: String,
        addAdditionalHeaders: Writable,
    ) {
        val httpPayloadMember = inputShape.members().firstOrNull { it.hasTrait(HttpPayloadTrait::class.java) }
        val payloadTarget = httpPayloadMember?.let { codegenContext.model.expectShape(it.target) }
        val streamingMember = inputShape.findStreamingMember(codegenContext.model)
        val isBlobPayload = payloadTarget is BlobShape && httpPayloadMember != streamingMember
        val isStringPayload = payloadTarget is StringShape
        val isEnumPayload = payloadTarget is StringShape && (payloadTarget.hasTrait(EnumTrait::class.java) || payloadTarget is EnumShape)
        val isDocumentPayload = payloadTarget is DocumentShape

        when {
            isBlobPayload || isStringPayload -> {
                val memberName = symbolProvider.toMemberName(httpPayloadMember!!)
                val bodyExpr =
                    when {
                        isBlobPayload -> "payload.into_inner()"
                        isEnumPayload -> "payload.as_str().as_bytes().to_vec()"
                        else -> "payload.into_bytes()"
                    }
                val mediaType = payloadTarget!!.getTrait(software.amazon.smithy.model.traits.MediaTypeTrait::class.java)
                val contentType =
                    when {
                        mediaType.isPresent -> mediaType.get().value
                        isStringPayload -> "text/plain"
                        else -> "application/octet-stream"
                    }
                rustTemplate(
                    """
                    let mut input = input;
                    let payload = input.$memberName.take();
                    let mut request = protocol.serialize_request(
                        &input, $schemaRef, ${uriPath.dq()}, _cfg,
                    ).map_err(#{BoxError}::from)?;
                    if let #{Some}(payload) = payload {
                        *request.body_mut() = #{SdkBody}::from($bodyExpr);
                        if !request.headers().contains_key("Content-Type") {
                            request.headers_mut().insert("Content-Type", ${contentType.dq()});
                        }
                        if let #{Some}(content_length) = request.body().content_length() {
                            request.headers_mut().insert("Content-Length", content_length.to_string());
                        }
                    }
                    #{add_headers}
                    return #{Ok}(request);
                    """,
                    *codegenScope,
                    "add_headers" to addAdditionalHeaders,
                )
            }
            isDocumentPayload -> {
                val memberName = symbolProvider.toMemberName(httpPayloadMember!!)
                rustTemplate(
                    """
                    let mut input = input;
                    let payload = input.$memberName.take();
                    let mut request = protocol.serialize_request(
                        &input, $schemaRef, ${uriPath.dq()}, _cfg,
                    ).map_err(#{BoxError}::from)?;
                    if let #{Some}(payload) = payload {
                        let mut json = String::new();
                        ::aws_smithy_json::serialize::JsonValueWriter::new(&mut json).document(&payload);
                        *request.body_mut() = #{SdkBody}::from(json.into_bytes());
                        if !request.headers().contains_key("Content-Type") {
                            request.headers_mut().insert("Content-Type", "application/json");
                        }
                        if let #{Some}(content_length) = request.body().content_length() {
                            request.headers_mut().insert("Content-Length", content_length.to_string());
                        }
                    }
                    #{add_headers}
                    return #{Ok}(request);
                    """,
                    *codegenScope,
                    "add_headers" to addAdditionalHeaders,
                )
            }
            httpBindingResolver.requestContentType(operationShape) != null -> {
                // Protocol wants a body (e.g., awsJson always sends {}; rpcv2Cbor sends body
                // when there's user-modeled input)
                rustTemplate(
                    """
                    let mut request = protocol.serialize_request(
                        &input, $schemaRef, ${uriPath.dq()}, _cfg,
                    ).map_err(#{BoxError}::from)?;
                    #{add_headers}
                    return #{Ok}(request);
                    """,
                    *codegenScope,
                    "add_headers" to addAdditionalHeaders,
                )
            }
            else -> {
                // No content type means no body (e.g., rpcv2Cbor: "Requests for operations
                // with no defined input type MUST NOT contain bodies")
                rustTemplate(
                    """
                    let mut request = protocol.serialize_request(
                        &input, $schemaRef, ${uriPath.dq()}, _cfg,
                    ).map_err(#{BoxError}::from)?;
                    *request.body_mut() = #{SdkBody}::empty();
                    request.headers_mut().remove("Content-Type");
                    request.headers_mut().remove("Content-Length");
                    #{add_headers}
                    return #{Ok}(request);
                    """,
                    *codegenScope,
                    "add_headers" to addAdditionalHeaders,
                )
            }
        }
    }
}
