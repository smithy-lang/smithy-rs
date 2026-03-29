/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.protocol

import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.BooleanShape
import software.amazon.smithy.model.shapes.DoubleShape
import software.amazon.smithy.model.shapes.FloatShape
import software.amazon.smithy.model.shapes.IntegerShape
import software.amazon.smithy.model.shapes.LongShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShortShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.TimestampShape
import software.amazon.smithy.model.traits.HttpHeaderTrait
import software.amazon.smithy.model.traits.HttpLabelTrait
import software.amazon.smithy.model.traits.HttpPayloadTrait
import software.amazon.smithy.model.traits.HttpPrefixHeadersTrait
import software.amazon.smithy.model.traits.HttpQueryParamsTrait
import software.amazon.smithy.model.traits.HttpQueryTrait
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.customizations.SchemaSerdeAllowlist
import software.amazon.smithy.rust.codegen.client.smithy.generators.http.RequestBindingGenerator
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
            val streamingMember = inputShape.findStreamingMember(codegenContext.model)
            val isBlobStreaming =
                streamingMember != null && codegenContext.model.expectShape(streamingMember.target) is BlobShape
            val isEventStream = streamingMember != null && !isBlobStreaming
            if (isBlobStreaming) {
                val memberName = symbolProvider.toMemberName(streamingMember!!)
                rustTemplate(
                    """
                    let mut request = protocol.serialize_request(
                        &input, $schemaRef, "", _cfg,
                    ).map_err(#{BoxError}::from)?;
                    // Streaming blob payload: replace the body with the raw ByteStream.
                    *request.body_mut() = input.$memberName.into_inner();
                    if let #{Some}(content_length) = request.body().content_length() {
                        request.headers_mut().insert("Content-Length", content_length.to_string());
                    }
                    return #{Ok}(request);
                    """,
                    *codegenScope,
                )
            } else if (isEventStream) {
                // Event stream: use schema-based protocol for headers/URI/method,
                // then replace the body with the event stream and set the correct Content-Type.
                val eventStreamBody =
                    writable {
                        bodyGenerator?.generatePayload(this, "input", operationShape)
                    }
                // requestContentType returns the event stream content type for input streams
                // (e.g., application/vnd.amazon.eventstream) or the normal protocol content type
                // for output-only streams.
                val contentType = httpBindingResolver.requestContentType(operationShape)
                rustTemplate(
                    """
                    let mut request = protocol.serialize_request(
                        &input, $schemaRef, "", _cfg,
                    ).map_err(#{BoxError}::from)?;
                    *request.body_mut() = #{SdkBody}::from(#{event_stream_body});
                    """,
                    *codegenScope,
                    "event_stream_body" to eventStreamBody,
                )
                if (contentType != null) {
                    rust("request.headers_mut().insert(\"Content-Type\", ${contentType.dq()});")
                }
                rustTemplate("return #{Ok}(request);", *codegenScope)
            } else {
                val httpPayloadMember = inputShape.members().firstOrNull { it.hasTrait(HttpPayloadTrait::class.java) }
                val payloadTarget = httpPayloadMember?.let { codegenContext.model.expectShape(it.target) }
                val isBlobPayload = payloadTarget is BlobShape && httpPayloadMember != streamingMember
                val isStringPayload = payloadTarget is StringShape

                // Check if any members have HTTP request bindings
                val hasHttpBindings =
                    inputShape.members().any {
                        it.hasTrait(HttpHeaderTrait::class.java) ||
                            it.hasTrait(HttpQueryTrait::class.java) ||
                            it.hasTrait(HttpLabelTrait::class.java) ||
                            it.hasTrait(HttpPrefixHeadersTrait::class.java) ||
                            it.hasTrait(HttpQueryParamsTrait::class.java)
                    }

                if (hasHttpBindings) {
                    // Use serialize_body() for body-only serialization, then write
                    // HTTP-bound members directly — no runtime trait checks.
                    if (isBlobPayload || isStringPayload) {
                        val memberName = symbolProvider.toMemberName(httpPayloadMember!!)
                        val bodyExpr = if (isBlobPayload) "payload.into_inner()" else "payload.into_bytes()"
                        rustTemplate(
                            """
                            let mut input = input;
                            let payload = input.$memberName.take();
                            let mut request = protocol.serialize_body(
                                &input, $schemaRef, "", _cfg,
                            ).map_err(#{BoxError}::from)?;
                            if let #{Some}(payload) = payload {
                                *request.body_mut() = #{SdkBody}::from($bodyExpr);
                            }
                            """,
                            *codegenScope,
                        )
                    } else {
                        rustTemplate(
                            """
                            let mut request = protocol.serialize_body(
                                &input, $schemaRef, "", _cfg,
                            ).map_err(#{BoxError}::from)?;
                            """,
                            *codegenScope,
                        )
                    }
                    // Generate direct header/query/label writes
                    renderDirectHttpBindingWrites(inputShape, operationShape)
                    rustTemplate("return #{Ok}(request);", *codegenScope)
                } else if (isBlobPayload || isStringPayload) {
                    val memberName = symbolProvider.toMemberName(httpPayloadMember!!)
                    val bodyExpr = if (isBlobPayload) "payload.into_inner()" else "payload.into_bytes()"
                    rustTemplate(
                        """
                        let mut input = input;
                        let payload = input.$memberName.take();
                        let mut request = protocol.serialize_request(
                            &input, $schemaRef, "", _cfg,
                        ).map_err(#{BoxError}::from)?;
                        if let #{Some}(payload) = payload {
                            *request.body_mut() = #{SdkBody}::from($bodyExpr);
                        }
                        return #{Ok}(request);
                        """,
                        *codegenScope,
                    )
                } else {
                    rustTemplate(
                        """
                        return protocol.serialize_request(
                            &input, $schemaRef, "", _cfg,
                        ).map_err(#{BoxError}::from);
                        """,
                        *codegenScope,
                    )
                }
            }
        }

    /** Generate direct header/query/label writes for HTTP-bound members. */
    private fun RustWriter.renderDirectHttpBindingWrites(
        inputShape: StructureShape,
        operationShape: OperationShape,
    ) {
        val model = codegenContext.model
        val percentEncode = RuntimeType.smithySchema(codegenContext.runtimeConfig).resolve("http_protocol::percent_encode")

        // Get the @http trait URI pattern for label substitution
        val httpTrait = operationShape.getTrait(software.amazon.smithy.model.traits.HttpTrait::class.java)
        val uriPattern = httpTrait.map { it.uri.toString() }.orElse("/")

        // Build URI string with label substitution, matching serialize_request behavior
        rustTemplate(
            """
            {
                let mut uri = ${uriPattern.dq()}.to_string();
            """,
        )

        // Collect query params in a vec, then append at the end
        rustTemplate("let mut query_params: Vec<(String, String)> = Vec::new();")

        for (member in inputShape.members()) {
            val memberName = symbolProvider.toMemberName(member)
            val target = model.expectShape(member.target)

            val httpHeader = member.getTrait(HttpHeaderTrait::class.java)
            val httpQuery = member.getTrait(HttpQueryTrait::class.java)
            val httpLabel = member.getTrait(HttpLabelTrait::class.java)
            val httpPrefixHeaders = member.getTrait(HttpPrefixHeadersTrait::class.java)
            val httpQueryParams = member.getTrait(HttpQueryParamsTrait::class.java)

            if (httpHeader.isPresent) {
                val headerName = httpHeader.get().value
                val valueExpr = httpValueExpr("val", target, member)
                rustTemplate(
                    """
                    if let Some(ref val) = input.$memberName {
                        request.headers_mut().insert(${headerName.dq()}, $valueExpr);
                    }
                    """,
                )
            } else if (httpQuery.isPresent) {
                val queryName = httpQuery.get().value
                val valueExpr = httpValueExpr("val", target, member)
                rustTemplate(
                    """
                    if let Some(ref val) = input.$memberName {
                        query_params.push((${queryName.dq()}.to_string(), $valueExpr));
                    }
                    """,
                )
            } else if (httpLabel.isPresent) {
                val valueExpr = httpValueExpr("val", target, member)
                // Label name in the URI pattern matches the member name from the model
                val labelName = member.memberName
                rustTemplate(
                    """
                    if let Some(ref val) = input.$memberName {
                        uri = uri.replace("{$labelName}", &#{percent_encode}(&$valueExpr));
                    }
                    """,
                    "percent_encode" to percentEncode,
                )
            } else if (httpPrefixHeaders.isPresent) {
                val prefix = httpPrefixHeaders.get().value
                rustTemplate(
                    """
                    if let Some(ref map) = input.$memberName {
                        for (k, v) in map {
                            request.headers_mut().insert(format!("$prefix{k}"), v.to_string());
                        }
                    }
                    """,
                )
            } else if (httpQueryParams.isPresent) {
                rustTemplate(
                    """
                    if let Some(ref map) = input.$memberName {
                        for (k, v) in map {
                            query_params.push((k.clone(), v.clone()));
                        }
                    }
                    """,
                )
            }
        }

        // Append query params and set URI, matching serialize_request behavior
        rustTemplate(
            """
            if !query_params.is_empty() {
                uri.push(if uri.contains('?') { '&' } else { '?' });
                let pairs: Vec<String> = query_params.iter()
                    .map(|(k, v)| format!("{}={}", #{percent_encode}(k), #{percent_encode}(v)))
                    .collect();
                uri.push_str(&pairs.join("&"));
            }
            request.set_uri(uri.as_str()).expect("valid URI");
            }
            """,
            "percent_encode" to percentEncode,
        )
    }

    /** Generate a string expression for an HTTP binding value based on the target shape type. */
    private fun httpValueExpr(
        varName: String,
        target: software.amazon.smithy.model.shapes.Shape,
        member: software.amazon.smithy.model.shapes.MemberShape,
    ): String {
        return when (target) {
            is BooleanShape -> "$varName.to_string()"
            is IntegerShape, is ShortShape -> "$varName.to_string()"
            is LongShape -> "$varName.to_string()"
            is FloatShape, is DoubleShape -> "$varName.to_string()"
            is TimestampShape -> {
                val format = member.getTrait(TimestampFormatTrait::class.java)
                if (format.isPresent) {
                    when (format.get().format.toString()) {
                        "epoch-seconds" -> "$varName.fmt(::aws_smithy_types::date_time::Format::EpochSeconds).expect(\"valid timestamp\")"
                        "date-time" -> "$varName.fmt(::aws_smithy_types::date_time::Format::DateTime).expect(\"valid timestamp\")"
                        else -> "$varName.fmt(::aws_smithy_types::date_time::Format::HttpDate).expect(\"valid timestamp\")"
                    }
                } else {
                    // Default for headers is http-date
                    "$varName.fmt(::aws_smithy_types::date_time::Format::HttpDate).expect(\"valid timestamp\")"
                }
            }
            is BlobShape -> "::aws_smithy_types::base64::encode($varName.as_ref())"
            else -> "$varName.to_string()"
        }
    }
}
