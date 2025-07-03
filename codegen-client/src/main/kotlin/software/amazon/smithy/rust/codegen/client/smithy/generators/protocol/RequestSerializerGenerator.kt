/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.protocol

import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
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
            "SdkBody" to RuntimeType.sdkBody(codegenContext.runtimeConfig),
            "HeaderSerializationSettings" to
                RuntimeType.forInlineDependency(
                    InlineDependency.serializationSettings(
                        codegenContext.runtimeConfig,
                    ),
                ).resolve("HeaderSerializationSettings"),
        )
    }

    fun render(
        writer: RustWriter,
        operationShape: OperationShape,
    ) {
        val inputShape = operationShape.inputShape(codegenContext.model)
        val operationName = symbolProvider.toSymbol(operationShape).name
        val inputSymbol = symbolProvider.toSymbol(inputShape)
        val serializerName = nameOverride ?: "${operationName}RequestSerializer"
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
                            // Consume the `ByteStream` into its inner `SdkBody`.
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
}
