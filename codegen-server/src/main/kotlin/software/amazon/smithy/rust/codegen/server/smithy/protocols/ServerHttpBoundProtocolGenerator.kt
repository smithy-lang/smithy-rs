/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.knowledge.HttpBindingIndex
import software.amazon.smithy.model.node.ExpectationNotMetException
import software.amazon.smithy.model.pattern.UriPattern
import software.amazon.smithy.model.shapes.BooleanShape
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.NumberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.model.traits.HttpErrorTrait
import software.amazon.smithy.model.traits.HttpPayloadTrait
import software.amazon.smithy.model.traits.HttpTrait
import software.amazon.smithy.model.traits.MediaTypeTrait
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.conditionalBlock
import software.amazon.smithy.rust.codegen.core.rustlang.escape
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.rustlang.withBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.customize.NamedCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.Section
import software.amazon.smithy.rust.codegen.core.smithy.generators.http.HttpBindingCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.http.HttpMessageType
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolPayloadGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.setterName
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.smithy.mapRustType
import software.amazon.smithy.rust.codegen.core.smithy.protocols.EventStreamBodyParams
import software.amazon.smithy.rust.codegen.core.smithy.protocols.HttpBindingDescriptor
import software.amazon.smithy.rust.codegen.core.smithy.protocols.HttpBoundProtocolPayloadGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.HttpLocation
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolFunctions
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.StructuredDataParserGenerator
import software.amazon.smithy.rust.codegen.core.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.core.smithy.transformers.operationErrors
import software.amazon.smithy.rust.codegen.core.smithy.wrapOptional
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.expectTrait
import software.amazon.smithy.rust.codegen.core.util.findStreamingMember
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.hasStreamingMember
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.isEventStream
import software.amazon.smithy.rust.codegen.core.util.isStreaming
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.canReachConstrainedShape
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerBuilderGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.http.ServerRequestBindingGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.http.ServerResponseBindingGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocol
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocolGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.serverBuilderSymbol
import java.util.logging.Logger

data class StreamPayloadSerializerParams(
    val codegenContext: ServerCodegenContext,
    val payloadGenerator: ServerHttpBoundProtocolPayloadGenerator,
    val shapeName: String,
    val shape: OperationShape,
)

/**
 * Class describing a ServerHttpBoundProtocol section that can be used in a customization.
 */
sealed class ServerHttpBoundProtocolSection(name: String) : Section(name) {
    data class AfterTimestampDeserializedMember(val shape: MemberShape) :
        ServerHttpBoundProtocolSection("AfterTimestampDeserializedMember")

    /**
     * Represent a section for rendering the serialized stream payload.
     *
     * If the payload does not implement the `futures_core::stream::Stream`, which is the case for
     * `aws_smithy_types::byte_stream::ByteStream`, the section needs to be overridden and renders a new-type wrapper
     * around the payload to enable the `Stream` trait.
     */
    data class WrapStreamPayload(val params: StreamPayloadSerializerParams) :
        ServerHttpBoundProtocolSection("WrapStreamPayload")
}

/**
 * Customization for the ServerHttpBoundProtocol generator.
 */
typealias ServerHttpBoundProtocolCustomization = NamedCustomization<ServerHttpBoundProtocolSection>

/**
 * Implement operations' input parsing and output serialization. Protocols can plug their own implementations
 * and overrides by creating a protocol factory inheriting from this class and feeding it to the [ServerProtocolLoader].
 * See [ServerRestJsonFactory] for more info.
 */
class ServerHttpBoundProtocolGenerator(
    codegenContext: ServerCodegenContext,
    protocol: ServerProtocol,
    customizations: List<ServerHttpBoundProtocolCustomization> = listOf(),
    additionalHttpBindingCustomizations: List<HttpBindingCustomization> = listOf(),
) :
    ServerProtocolGenerator(
            protocol,
            ServerHttpBoundProtocolTraitImplGenerator(
                codegenContext,
                protocol,
                customizations,
                additionalHttpBindingCustomizations,
            ),
        )

class ServerHttpBoundProtocolPayloadGenerator(
    private val codegenContext: ServerCodegenContext,
    private val protocol: ServerProtocol,
) :
    ProtocolPayloadGenerator by HttpBoundProtocolPayloadGenerator(
            codegenContext,
            protocol,
            HttpMessageType.RESPONSE,
            renderEventStreamBody = { writer, params ->
                writer.rustTemplate(
                    """
                    {
                        let error_marshaller = #{errorMarshallerConstructorFn}();
                        let marshaller = #{marshallerConstructorFn}();
                        let signer = #{NoOpSigner}{};
                        #{event_stream}
                    }
                    """,
                    "aws_smithy_http" to
                        RuntimeType.smithyHttp(codegenContext.runtimeConfig),
                    "NoOpSigner" to
                        RuntimeType.smithyEventStream(codegenContext.runtimeConfig)
                            .resolve("frame::NoOpSigner"),
                    "marshallerConstructorFn" to
                        params.eventStreamMarshallerGenerator.render(),
                    "errorMarshallerConstructorFn" to params.errorMarshallerConstructorFn,
                    "event_stream" to eventStreamWithInitialResponse(codegenContext, protocol, params),
                )
            },
        )

/*
 * Generate all operation input parsers and output serializers for streaming and
 * non-streaming types.
 */
class ServerHttpBoundProtocolTraitImplGenerator(
    private val codegenContext: ServerCodegenContext,
    private val protocol: ServerProtocol,
    private val customizations: List<ServerHttpBoundProtocolCustomization>,
    private val additionalHttpBindingCustomizations: List<HttpBindingCustomization>,
) {
    private val logger = Logger.getLogger(javaClass.name)
    private val symbolProvider = codegenContext.symbolProvider
    private val unconstrainedShapeSymbolProvider = codegenContext.unconstrainedShapeSymbolProvider
    private val model = codegenContext.model
    private val runtimeConfig = codegenContext.runtimeConfig
    private val httpBindingResolver = protocol.httpBindingResolver
    private val protocolFunctions = ProtocolFunctions(codegenContext)

    private val codegenScope =
        arrayOf(
            "AsyncTrait" to ServerCargoDependency.AsyncTrait.toType(),
            "Cow" to RuntimeType.Cow,
            "DateTime" to RuntimeType.dateTime(runtimeConfig),
            "FormUrlEncoded" to ServerCargoDependency.FormUrlEncoded.toType(),
            "FuturesUtil" to ServerCargoDependency.FuturesUtil.toType(),
            "HttpBody" to RuntimeType.HttpBody,
            "header_util" to RuntimeType.smithyHttp(runtimeConfig).resolve("header"),
            "Hyper" to RuntimeType.Hyper,
            "LazyStatic" to RuntimeType.LazyStatic,
            "Mime" to ServerCargoDependency.Mime.toType(),
            "Nom" to ServerCargoDependency.Nom.toType(),
            "PercentEncoding" to RuntimeType.PercentEncoding,
            "Regex" to RuntimeType.Regex,
            "SmithyHttpServer" to
                ServerCargoDependency.smithyHttpServer(runtimeConfig).toType(),
            "SmithyTypes" to RuntimeType.smithyTypes(runtimeConfig),
            "RuntimeError" to protocol.runtimeError(runtimeConfig),
            "RequestRejection" to protocol.requestRejection(runtimeConfig),
            "ResponseRejection" to protocol.responseRejection(runtimeConfig),
            "PinProjectLite" to ServerCargoDependency.PinProjectLite.toType(),
            "http" to RuntimeType.Http,
            "Tracing" to RuntimeType.Tracing,
        )

    fun generateTraitImpls(
        operationWriter: RustWriter,
        operationShape: OperationShape,
    ) {
        val inputSymbol = symbolProvider.toSymbol(operationShape.inputShape(model))
        val outputSymbol = symbolProvider.toSymbol(operationShape.outputShape(model))

        operationWriter.renderTraits(inputSymbol, outputSymbol, operationShape)
    }

    /*
     * Generation of `from_request` and `into_response`.
     * For non-streaming request bodies, that is, models without streaming traits
     * (https://smithy.io/2.0/spec/streaming.html#streaming-trait)
     * we require the HTTP body to be fully read in memory before parsing or deserialization.
     * From a server perspective we need a way to parse an HTTP request from `Bytes` and serialize
     * an HTTP response to `Bytes`.
     * These traits are the public entrypoint of the ser/de logic of the generated server.
     */
    private fun RustWriter.renderTraits(
        inputSymbol: Symbol,
        outputSymbol: Symbol,
        operationShape: OperationShape,
    ) {
        val verifyAcceptHeader =
            writable {
                httpBindingResolver.responseContentType(operationShape)?.also { contentType ->
                    val legacyContentType = httpBindingResolver.legacyBackwardsCompatContentType(operationShape)
                    if (legacyContentType != null) {
                        // For operations with legacy backwards compatibility, accept both content types
                        rustTemplate(
                            """
                            if !#{SmithyHttpServer}::protocol::accept_header_classifier(request.headers(), &#{ContentType}) &&
                               !#{SmithyHttpServer}::protocol::accept_header_classifier(request.headers(), &#{FallbackContentType}) {
                                return Err(#{RequestRejection}::NotAcceptable);
                            }
                            """,
                            "ContentType" to mimeType(contentType),
                            "FallbackContentType" to mimeType(legacyContentType),
                            *codegenScope,
                        )
                    } else {
                        rustTemplate(
                            """
                            if !#{SmithyHttpServer}::protocol::accept_header_classifier(request.headers(), &#{ContentType}) {
                                return Err(#{RequestRejection}::NotAcceptable);
                            }
                            """,
                            "ContentType" to mimeType(contentType),
                            *codegenScope,
                        )
                    }
                }
            }

        // Implement `from_request` trait for input types.
        val inputFuture = "${inputSymbol.name}Future"
        // TODO(https://github.com/smithy-lang/smithy-rs/issues/2238): Remove the `Pin<Box<dyn Future>>` and replace with thin wrapper around `Collect`.
        rustTemplate(
            """
            #{PinProjectLite}::pin_project! {
                /// A [`Future`](std::future::Future) aggregating the body bytes of a [`Request`] and constructing the
                /// [`${inputSymbol.name}`](#{I}) using modelled bindings.
                pub struct $inputFuture {
                    inner: std::pin::Pin<Box<dyn std::future::Future<Output = Result<#{I}, #{RuntimeError}>> + Send>>
                }
            }

            impl std::future::Future for $inputFuture {
                type Output = Result<#{I}, #{RuntimeError}>;

                fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
                    let this = self.project();
                    this.inner.as_mut().poll(cx)
                }
            }

            impl<B> #{SmithyHttpServer}::request::FromRequest<#{Marker}, B> for #{I}
            where
                B: #{SmithyHttpServer}::body::HttpBody + Send,
                B: 'static,
                ${streamingBodyTraitBounds(operationShape)}
                B::Data: Send,
                #{RequestRejection} : From<<B as #{SmithyHttpServer}::body::HttpBody>::Error>
            {
                type Rejection = #{RuntimeError};
                type Future = $inputFuture;

                fn from_request(request: #{http}::Request<B>) -> Self::Future {
                    let fut = async move {
                        #{verifyAcceptHeader:W}
                        #{parse_request}(request)
                            .await
                    };
                    use #{FuturesUtil}::future::TryFutureExt;
                    let fut = fut.map_err(|e: #{RequestRejection}| {
                        #{Tracing}::debug!(error = %e, "failed to deserialize request");
                        #{RuntimeError}::from(e)
                    });
                    $inputFuture {
                        inner: Box::pin(fut)
                    }
                }
            }
            """,
            *codegenScope,
            "I" to inputSymbol,
            "Marker" to protocol.markerStruct(),
            "parse_request" to serverParseRequest(operationShape),
            "verifyAcceptHeader" to verifyAcceptHeader,
        )

        // Implement `into_response` for output types.
        val errorSymbol = symbolProvider.symbolForOperationError(operationShape)

        // All `ResponseRejection`s are errors; the service owners are to blame. So we centrally log them here
        // to let them know.
        rustTemplate(
            """
            impl #{SmithyHttpServer}::response::IntoResponse<#{Marker}> for #{O} {
                fn into_response(self) -> #{SmithyHttpServer}::response::Response {
                    match #{serialize_response}(self) {
                        Ok(response) => response,
                        Err(e) => {
                            #{Tracing}::error!(error = %e, "failed to serialize response");
                            #{SmithyHttpServer}::response::IntoResponse::<#{Marker}>::into_response(#{RuntimeError}::from(e))
                        }
                    }
                }
            }
            """,
            *codegenScope,
            "O" to outputSymbol,
            "Marker" to protocol.markerStruct(),
            "serialize_response" to serverSerializeResponse(operationShape),
        )

        if (operationShape.operationErrors(model).isNotEmpty()) {
            rustTemplate(
                """
                impl #{SmithyHttpServer}::response::IntoResponse<#{Marker}> for #{E} {
                    fn into_response(self) -> #{SmithyHttpServer}::response::Response {
                        match #{serialize_error}(&self) {
                            Ok(mut response) => {
                                response.extensions_mut().insert(#{SmithyHttpServer}::extension::ModeledErrorExtension::new(self.name()));
                                response
                            },
                            Err(e) => {
                                #{Tracing}::error!(error = %e, "failed to serialize response");
                                #{SmithyHttpServer}::response::IntoResponse::<#{Marker}>::into_response(#{RuntimeError}::from(e))
                            }
                        }
                    }
                }
                """.trimIndent(),
                *codegenScope,
                "E" to errorSymbol,
                "Marker" to protocol.markerStruct(),
                "serialize_error" to serverSerializeError(operationShape),
            )
        }
    }

    /**
     * Generates `pub(crate) static CONTENT_TYPE_<MIME_TYPE> = ....
     *
     * Usage: In templates, #{MimeType}, "MimeType" to mimeType("yourDesiredType")
     */
    private fun mimeType(type: String): RuntimeType {
        val variableName = type.toSnakeCase().uppercase()
        val typeName = "CONTENT_TYPE_$variableName"
        return RuntimeType.forInlineFun(typeName, RustModule.private("mimes")) {
            rustTemplate(
                """
                pub(crate) static $typeName: std::sync::LazyLock<#{Mime}::Mime> = std::sync::LazyLock::new(|| {
                    ${type.dq()}.parse::<#{Mime}::Mime>().expect("BUG: MIME parsing failed, content_type is not valid")
                });
                """,
                *codegenScope,
            )
        }
    }

    private fun serverParseRequest(operationShape: OperationShape): RuntimeType {
        val inputShape = operationShape.inputShape(model)
        val inputSymbol = symbolProvider.toSymbol(inputShape)

        return protocolFunctions.deserializeFn(operationShape, fnNameSuffix = "http_request") { fnName ->
            Attribute.AllowClippyUnnecessaryWraps.render(this)
            // The last conversion trait bound is needed by the `hyper::body::to_bytes(body).await?` call.
            rustBlockTemplate(
                """
                pub async fn $fnName<B>(
                    ##[allow(unused_variables)] request: #{http}::Request<B>
                ) -> std::result::Result<
                    #{I},
                    #{RequestRejection}
                >
                where
                    B: #{SmithyHttpServer}::body::HttpBody + Send, ${streamingBodyTraitBounds(operationShape)}
                    B::Data: Send,
                    #{RequestRejection}: From<<B as #{SmithyHttpServer}::body::HttpBody>::Error>
                """.trimIndent(),
                *codegenScope,
                "I" to inputSymbol,
            ) {
                withBlock("Ok({", "})") {
                    serverRenderShapeParser(
                        operationShape,
                        inputShape,
                        httpBindingResolver.requestBindings(operationShape),
                    )
                }
            }
        }
    }

    private fun serverSerializeResponse(operationShape: OperationShape): RuntimeType {
        val outputShape = operationShape.outputShape(model)
        val outputSymbol = symbolProvider.toSymbol(outputShape)

        return protocolFunctions.serializeFn(operationShape, fnNameSuffix = "http_response") { fnName ->
            Attribute.AllowClippyUnnecessaryWraps.render(this)

            // Note we only need to take ownership of the output in the case that it contains streaming members.
            // However, we currently always take ownership here, but worth noting in case in the future we want
            // to generate different signatures for streaming vs non-streaming for some reason.
            rustBlockTemplate(
                """
                pub fn $fnName(
                    ##[allow(unused_variables)] output: #{O}
                ) -> std::result::Result<
                    #{SmithyHttpServer}::response::Response,
                    #{ResponseRejection}
                >
                """.trimIndent(),
                *codegenScope,
                "O" to outputSymbol,
            ) {
                withBlock("Ok({", "})") {
                    serverRenderOutputShapeResponseSerializer(
                        operationShape,
                        httpBindingResolver.responseBindings(operationShape),
                    )
                }
            }
        }
    }

    private fun serverSerializeError(operationShape: OperationShape): RuntimeType {
        val errorSymbol = symbolProvider.symbolForOperationError(operationShape)
        return protocolFunctions.serializeFn(operationShape, fnNameSuffix = "http_error") { fnName ->
            Attribute.AllowClippyUnnecessaryWraps.render(this)
            rustBlockTemplate(
                "pub fn $fnName(error: &#{E}) -> std::result::Result<#{SmithyHttpServer}::response::Response, #{ResponseRejection}>",
                *codegenScope,
                "E" to errorSymbol,
            ) {
                withBlock("Ok({", "})") {
                    serverRenderErrorShapeResponseSerializer(
                        operationShape,
                        errorSymbol,
                    )
                }
            }
        }
    }

    private fun RustWriter.serverRenderErrorShapeResponseSerializer(
        operationShape: OperationShape,
        errorSymbol: Symbol,
    ) {
        val operationName = symbolProvider.toSymbol(operationShape).name
        val structuredDataSerializer = protocol.structuredDataSerializer()
        withBlock("match error {", "}") {
            val errors = operationShape.operationErrors(model)
            errors.forEach {
                val variantShape = model.expectShape(it.id, StructureShape::class.java)
                val errorTrait = variantShape.expectTrait<ErrorTrait>()
                val variantSymbol = symbolProvider.toSymbol(variantShape)
                val serializerSymbol = structuredDataSerializer.serverErrorSerializer(it.id)

                rustBlock("#T::${variantSymbol.name}(output) =>", errorSymbol) {
                    rust(
                        """
                        let payload = #T(output)?;
                        """,
                        serializerSymbol,
                    )

                    val bindings = httpBindingResolver.errorResponseBindings(it)

                    Attribute.AllowUnusedMut.render(this)
                    rustTemplate("let mut builder = #{http}::Response::builder();", *codegenScope)
                    serverRenderResponseHeaders(operationShape, variantShape)

                    bindings.forEach { binding ->
                        when (val location = binding.location) {
                            HttpLocation.RESPONSE_CODE, HttpLocation.DOCUMENT -> {}
                            else -> {
                                logger.warning("[rust-server-codegen] $operationName: error serialization does not currently support $location bindings")
                            }
                        }
                    }
                    val status =
                        variantShape.getTrait<HttpErrorTrait>()?.code
                            ?: errorTrait.defaultHttpStatusCode

                    serverRenderContentLengthHeader()

                    rustTemplate(
                        """
                        builder.status($status).body(#{SmithyHttpServer}::body::to_boxed(payload))?
                        """,
                        *codegenScope,
                    )
                }
            }
        }
    }

    /**
     * Render an HTTP response (headers, response code, body) for an operation's output and the given [bindings].
     */
    private fun RustWriter.serverRenderOutputShapeResponseSerializer(
        operationShape: OperationShape,
        bindings: List<HttpBindingDescriptor>,
    ) {
        Attribute.AllowUnusedMut.render(this)
        rustTemplate("let mut builder = #{http}::Response::builder();", *codegenScope)
        serverRenderResponseHeaders(operationShape)
        // Fallback to the default code of `@http`, which should be 200.
        val httpTraitDefaultStatusCode =
            HttpTrait.builder()
                .method("GET")
                .uri(UriPattern.parse("/")) // Required to build
                .build()
                .code
        check(httpTraitDefaultStatusCode == 200)
        val httpTraitStatusCode =
            operationShape.getTrait<HttpTrait>()?.code ?: httpTraitDefaultStatusCode
        bindings.find { it.location == HttpLocation.RESPONSE_CODE }?.let {
            serverRenderResponseCodeBinding(it, httpTraitStatusCode)(this)
        }
            // No binding, use `@http`.
            ?: serverRenderHttpResponseCode(httpTraitStatusCode)(this)

        operationShape.outputShape(model).findStreamingMember(model)?.let {
            val payloadGenerator = ServerHttpBoundProtocolPayloadGenerator(codegenContext, protocol)
            withBlockTemplate(
                "let body = #{SmithyHttpServer}::body::boxed(#{SmithyHttpServer}::body::Body::wrap_stream(",
                "));",
                *codegenScope,
            ) {
                for (customization in customizations) {
                    customization.section(
                        ServerHttpBoundProtocolSection.WrapStreamPayload(
                            StreamPayloadSerializerParams(
                                codegenContext,
                                payloadGenerator,
                                "output",
                                operationShape,
                            ),
                        ),
                    )(this)
                }
            }
        }
            ?: run {
                val payloadGenerator =
                    ServerHttpBoundProtocolPayloadGenerator(codegenContext, protocol)
                withBlockTemplate("let payload = ", ";") {
                    payloadGenerator.generatePayload(this, "output", operationShape)
                }

                serverRenderContentLengthHeader()

                rustTemplate(
                    """
                    let body = #{SmithyHttpServer}::body::to_boxed(payload);
                    """,
                    *codegenScope,
                )
            }

        rustTemplate(
            """
            builder.body(body)?
            """,
            *codegenScope,
        )
    }

    private fun setResponseHeaderIfAbsent(
        writer: RustWriter,
        headerName: String,
        headerValue: String,
    ) {
        // We can be a tad more efficient if there's a `const` `HeaderName` in the `http` crate that matches.
        // https://docs.rs/http/latest/http/header/index.html#constants
        val headerNameExpr =
            if (headerName == "content-type") {
                "#{http}::header::CONTENT_TYPE"
            } else {
                "#{http}::header::HeaderName::from_static(\"$headerName\")"
            }

        writer.rustTemplate(
            """
            builder = #{header_util}::set_response_header_if_absent(
                builder,
                $headerNameExpr,
                "${writer.escape(headerValue)}",
            );
            """,
            *codegenScope,
        )
    }

    /**
     * Sets HTTP response headers for the operation's output shape or the operation's error shape.
     * It will generate response headers for the operation's output shape, unless [errorShape] is non-null, in which
     * case it will generate response headers for the given error shape.
     *
     * It sets three groups of headers in order. Headers from one group take precedence over headers in a later group.
     *     1. Headers bound by the `httpHeader` and `httpPrefixHeader` traits.
     *     2. The protocol-specific `Content-Type` header for the operation.
     *     3. Additional protocol-specific headers for errors, if [errorShape] is non-null.
     */
    private fun RustWriter.serverRenderResponseHeaders(
        operationShape: OperationShape,
        errorShape: StructureShape? = null,
    ) {
        val bindingGenerator =
            ServerResponseBindingGenerator(protocol, codegenContext, operationShape)
        val addHeadersFn = bindingGenerator.generateAddHeadersFn(errorShape ?: operationShape)
        if (addHeadersFn != null) {
            // Notice that we need to borrow the output only for output shapes but not for error shapes.
            val outputOwnedOrBorrowed = if (errorShape == null) "&output" else "output"
            rust(
                """
                builder = #{T}($outputOwnedOrBorrowed, builder)?;
                """,
                addHeadersFn,
            )
        }

        // Set the `Content-Type` header *after* the response bindings headers have been set,
        // to allow operations that bind a member to `Content-Type` (which we set earlier) to take precedence (this is
        // because we always use `set_response_header_if_absent`, so the _first_ header value we set for a given
        // header name is the one that takes precedence).
        httpBindingResolver.responseContentType(operationShape)?.let { contentTypeValue ->
            setResponseHeaderIfAbsent(this, "content-type", contentTypeValue)
        }

        for ((headerName, headerValue) in protocol.additionalResponseHeaders(operationShape)) {
            setResponseHeaderIfAbsent(this, headerName, headerValue)
        }

        if (errorShape != null) {
            for ((headerName, headerValue) in protocol.additionalErrorResponseHeaders(errorShape)) {
                setResponseHeaderIfAbsent(this, headerName, headerValue)
            }
        }
    }

    /**
     * Adds the `Content-Length` header.
     *
     * Unlike the headers added in `serverRenderResponseHeaders` the `Content-Length` depends on
     * the payload post-serialization.
     */
    private fun RustWriter.serverRenderContentLengthHeader() {
        rustTemplate(
            """
            let content_length = payload.len();
            builder = #{header_util}::set_response_header_if_absent(builder, #{http}::header::CONTENT_LENGTH, content_length);
            """,
            *codegenScope,
        )
    }

    private fun serverRenderHttpResponseCode(defaultCode: Int) =
        writable {
            check(defaultCode in 100..999) {
                """
                Smithy library lied to us. According to https://smithy.io/2.0/spec/http-bindings.html#http-trait,
                "The provided value SHOULD be between 100 and 599, and it MUST be between 100 and 999".
                """
                    .replace("\n", "")
                    .trimIndent()
            }
            rustTemplate(
                """
                let http_status: u16 = $defaultCode;
                builder = builder.status(http_status);
                """,
                *codegenScope,
            )
        }

    private fun serverRenderResponseCodeBinding(
        binding: HttpBindingDescriptor,
        /** This is the status code to fall back on if the member shape bound with `@httpResponseCode` is not
         * `@required` and the user did not provide a value for it at runtime. **/
        fallbackStatusCode: Int,
    ): Writable {
        check(binding.location == HttpLocation.RESPONSE_CODE)

        return writable {
            val memberName = symbolProvider.toMemberName(binding.member)
            withBlock("let status = output.$memberName", ";") {
                if (symbolProvider.toSymbol(binding.member).isOptional()) {
                    rust(".unwrap_or($fallbackStatusCode)")
                }
            }
            rustTemplate(
                """
                let http_status: u16 = status.try_into().map_err(#{ResponseRejection}::InvalidHttpStatusCode)?;
                builder = builder.status(http_status);
                """,
                *codegenScope,
            )
        }
    }

    private fun RustWriter.serverRenderShapeParser(
        operationShape: OperationShape,
        inputShape: StructureShape,
        bindings: List<HttpBindingDescriptor>,
    ) {
        val structuredDataParser = protocol.structuredDataParser()
        Attribute.AllowUnusedMut.render(this)
        rust(
            "let mut input = #T::default();",
            inputShape.serverBuilderSymbol(codegenContext),
        )
        Attribute.AllowUnusedVariables.render(this)
        rustTemplate(
            """
            let #{RequestParts} { uri, headers, body, .. } = #{Request}::try_from(request)?.into_parts();
            """,
            *preludeScope,
            "ParseError" to RuntimeType.smithyHttp(runtimeConfig).resolve("header::ParseError"),
            "Request" to RuntimeType.smithyRuntimeApi(runtimeConfig).resolve("http::Request"),
            "RequestParts" to
                RuntimeType.smithyRuntimeApi(runtimeConfig).resolve("http::RequestParts"),
        )
        val parser = structuredDataParser.serverInputParser(operationShape)

        // Skip body parsing if this operation has an event stream with initial request data
        // In that case, the non-event-stream members will be parsed from the initial-request message
        val hasEventStreamWithInitialRequest = httpBindingResolver.handlesEventStreamInitialRequest(operationShape)

        if (parser != null && !hasEventStreamWithInitialRequest) {
            // `null` is only returned by Smithy when there are no members, but we know there's at least one, since
            // there's something to parse (i.e. `parser != null`), so `!!` is safe here.
            val expectedRequestContentType =
                httpBindingResolver.requestContentType(operationShape)!!
            rustTemplate("let bytes = #{Hyper}::body::to_bytes(body).await?;", *codegenScope)
            // Note that the server is being very lenient here. We're accepting an empty body for when there is modeled
            // operation input; we simply parse it as empty operation input.
            // This behavior applies to all protocols. This might seem like a bug, but it isn't. There's protocol tests
            // that assert that the server should be lenient and accept both empty payloads and no payload
            // when there is modeled input:
            //
            // * [restJson1]: clients omit the payload altogether when the input is empty! So services must accept this.
            // * [rpcv2Cbor]: services must accept no payload or empty CBOR map for operations with modeled input.
            //
            // For the AWS JSON 1.x protocols, services are lenient in the case when there is no modeled input:
            //
            // * [awsJson1_0]: services must accept no payload or empty JSON document payload for operations with no modeled input
            // * [awsJson1_1]: services must accept no payload or empty JSON document payload for operations with no modeled input
            //
            // However, it's true that there are no tests pinning server behavior when there is _empty_ input. There's
            // a [consultation with Smithy] to remedy this. Until that gets resolved, in the meantime, we are being lenient.
            //
            // [restJson1]: https://github.com/smithy-lang/smithy/blob/main/smithy-aws-protocol-tests/model/restJson1/empty-input-output.smithy#L22
            // [awsJson1_0]: https://github.com/smithy-lang/smithy/blob/main/smithy-aws-protocol-tests/model/awsJson1_0/empty-input-output.smithy
            // [awsJson1_1]: https://github.com/smithy-lang/smithy/blob/main/smithy-aws-protocol-tests/model/awsJson1_1/empty-operation.smithy
            // [rpcv2Cbor]: https://github.com/smithy-lang/smithy/blob/main/smithy-protocol-tests/model/rpcv2Cbor/empty-input-output.smithy
            // [consultation with Smithy]: https://github.com/smithy-lang/smithy/issues/2327
            rustBlock("if !bytes.is_empty()") {
                rustTemplate(
                    """
                    #{SmithyHttpServer}::protocol::content_type_header_classifier_smithy(
                        &headers,
                        Some("$expectedRequestContentType"),
                    )?;
                    input = #{parser}(bytes.as_ref(), input)?;
                    """,
                    *codegenScope,
                    "parser" to parser,
                )
            }
        }

        for (binding in bindings) {
            val member = binding.member
            val parsedValue =
                serverRenderBindingParser(
                    binding,
                    operationShape,
                    httpBindingGenerator(operationShape),
                    structuredDataParser,
                )
            val valueToSet =
                if (symbolProvider.toSymbol(binding.member).isOptional()) {
                    "Some(value)"
                } else {
                    "value"
                }
            if (parsedValue != null) {
                rustTemplate(
                    """
                    if let Some(value) = #{ParsedValue:W} {
                        input = input.${member.setterName()}($valueToSet)
                    }
                    """,
                    "ParsedValue" to parsedValue,
                )
            }
        }

        serverRenderUriPathParser(this, operationShape)
        serverRenderQueryStringParser(this, operationShape)

        // If there's no modeled operation input, some protocols require that `Content-Type` header not be present.
        val noInputs = !OperationNormalizer.hadUserModeledOperationInput(operationShape, model)
        if (noInputs && protocol.serverContentTypeCheckNoModeledInput()) {
            rustTemplate(
                """
                #{SmithyHttpServer}::protocol::content_type_header_classifier_smithy(&headers, None)?;
                """,
                *codegenScope,
            )
        }

        // TODO(https://github.com/smithy-lang/smithy-rs/issues/3723): we should inject a check here that asserts that
        //  the body contents are valid when there is empty operation input or no operation input.

        val err =
            if (ServerBuilderGenerator.hasFallibleBuilder(
                    inputShape,
                    model,
                    symbolProvider,
                    takeInUnconstrainedTypes = true,
                )
            ) {
                "?"
            } else {
                ""
            }
        rustTemplate("input.build()$err", *codegenScope)
    }

    private fun serverRenderBindingParser(
        binding: HttpBindingDescriptor,
        operationShape: OperationShape,
        httpBindingGenerator: ServerRequestBindingGenerator,
        structuredDataParser: StructuredDataParserGenerator,
    ): Writable? {
        return when (binding.location) {
            HttpLocation.HEADER ->
                writable { serverRenderHeaderParser(this, binding, operationShape) }

            HttpLocation.PREFIX_HEADERS ->
                writable { serverRenderPrefixHeadersParser(this, binding, operationShape) }

            HttpLocation.PAYLOAD -> {
                val structureShapeHandler: RustWriter.(String) -> Unit = { body ->
                    rust("#T($body)", structuredDataParser.payloadParser(binding.member))
                }
                val deserializer =
                    httpBindingGenerator.generateDeserializePayloadFn(
                        binding,
                        structuredHandler = structureShapeHandler,
                    )
                return writable {
                    if (binding.member.isStreaming(model)) {
                        if (binding.member.isEventStream(model)) {
                            val parser = structuredDataParser.serverInputParser(operationShape)
                            val parseInitialRequest =
                                if (parser != null &&
                                    httpBindingResolver.handlesEventStreamInitialRequest(
                                        operationShape,
                                    )
                                ) {
                                    writable {
                                        rustTemplate(
                                            """
                                            input = #{parser}(_initial_event.payload(), input)?;
                                            """,
                                            "parser" to parser,
                                        )
                                    }
                                } else {
                                    writable { }
                                }
                            // TODO(https://github.com/smithy-lang/smithy-rs/issues/4343): The error
                            //   returned below is not actually accessible to the caller because it has
                            //   already started reading from the event stream at the time the error was sent.
                            rustTemplate(
                                """
                                {
                                    let mut receiver = #{Deserializer}(&mut body.into().into_inner())?;
                                    if let Some(_initial_event) = receiver
                                        .try_recv_initial(#{InitialMessageType}::Request)
                                        .await
                                        .map_err(
                                            |ev_error| #{RequestRejection}::ConstraintViolation(
                                                #{AllowUselessConversion}
                                                format!("{ev_error}").into()
                                            )
                                        )? {
                                        #{parseInitialRequest}
                                    }
                                    Some(receiver)
                                }
                                """,
                                "Deserializer" to deserializer,
                                "InitialMessageType" to
                                    RuntimeType.smithyHttp(runtimeConfig)
                                        .resolve("event_stream::InitialMessageType"),
                                "parseInitialRequest" to parseInitialRequest,
                                "AllowUselessConversion" to Attribute.AllowClippyUselessConversion.writable(),
                                *codegenScope,
                            )
                        } else {
                            rustTemplate(
                                """
                                {
                                    Some(#{Deserializer}(&mut body.into().into_inner())?)
                                }
                                """,
                                "Deserializer" to deserializer,
                                *codegenScope,
                            )
                        }
                    } else {
                        // This checks for the expected `Content-Type` header if the `@httpPayload` trait is present, as dictated by
                        // the core Smithy library, which _does not_ require deserializing the payload.
                        // If no members have `@httpPayload`, the expected `Content-Type` header as dictated _by the protocol_ is
                        // checked later on for non-streaming operations, in `serverRenderShapeParser`.
                        // Both checks require buffering the entire payload, since the check must only be performed if the payload is
                        // not empty.
                        val verifyRequestContentTypeHeader =
                            writable {
                                operationShape
                                    .inputShape(model)
                                    .members()
                                    .find { it.hasTrait<HttpPayloadTrait>() }
                                    ?.let { payload ->
                                        val target = model.expectShape(payload.target)
                                        if (!target.isBlobShape || target.hasTrait<MediaTypeTrait>()
                                        ) {
                                            // `null` is only returned by Smithy when there are no members, but we know there's at least
                                            // the one with `@httpPayload`, so `!!` is safe here.
                                            val expectedRequestContentType =
                                                httpBindingResolver.requestContentType(operationShape)!!
                                            rustTemplate(
                                                """
                                                if !bytes.is_empty() {
                                                    #{SmithyHttpServer}::protocol::content_type_header_classifier_smithy(
                                                        &headers,
                                                        Some("$expectedRequestContentType"),
                                                    )?;
                                                }
                                                """,
                                                *codegenScope,
                                            )
                                        }
                                    }
                            }
                        rustTemplate(
                            """
                            {
                                let bytes = #{Hyper}::body::to_bytes(body).await?;
                                #{VerifyRequestContentTypeHeader:W}
                                #{Deserializer}(&bytes)?
                            }
                            """,
                            "Deserializer" to deserializer,
                            "VerifyRequestContentTypeHeader" to verifyRequestContentTypeHeader,
                            *codegenScope,
                        )
                    }
                }
            }

            HttpLocation.DOCUMENT, HttpLocation.LABEL, HttpLocation.QUERY, HttpLocation.QUERY_PARAMS -> {
                // All of these are handled separately.
                null
            }

            else -> {
                logger.warning("[rust-server-codegen] ${operationShape.id}: request parsing does not currently support ${binding.location} bindings")
                null
            }
        }
    }

    private fun serverRenderUriPathParser(
        writer: RustWriter,
        operationShape: OperationShape,
    ) {
        val pathBindings =
            httpBindingResolver.requestBindings(operationShape).filter {
                it.location == HttpLocation.LABEL
            }
        if (pathBindings.isEmpty()) {
            return
        }
        val httpTrait = httpBindingResolver.httpTrait(operationShape)
        val greedyLabelIndex = httpTrait.uri.segments.indexOfFirst { it.isGreedyLabel }
        val segments =
            if (greedyLabelIndex >= 0) {
                httpTrait.uri.segments.slice(0 until (greedyLabelIndex + 1))
            } else {
                httpTrait.uri.segments
            }
        val restAfterGreedyLabel =
            if (greedyLabelIndex >= 0) {
                httpTrait
                    .uri
                    .segments
                    .slice((greedyLabelIndex + 1) until httpTrait.uri.segments.size)
                    .joinToString(prefix = "/", separator = "/")
            } else {
                ""
            }
        val labeledNames =
            segments
                .mapIndexed { index, segment ->
                    if (segment.isLabel) {
                        "m$index"
                    } else {
                        "_"
                    }
                }
                .joinToString(
                    prefix = (if (segments.size > 1) "(" else ""),
                    separator = ",",
                    postfix = (if (segments.size > 1) ")" else ""),
                )
        val nomParser =
            segments
                .map { segment ->
                    if (segment.isGreedyLabel) {
                        "#{Nom}::combinator::rest::<_, #{Nom}::error::Error<&str>>"
                    } else if (segment.isLabel) {
                        """#{Nom}::branch::alt::<_, _, #{Nom}::error::Error<&str>, _>((#{Nom}::bytes::complete::take_until("/"), #{Nom}::combinator::rest))"""
                    } else {
                        """#{Nom}::bytes::complete::tag::<_, _, #{Nom}::error::Error<&str>>("${segment.content}")"""
                    }
                }
                .joinToString(
                    // TODO(https://github.com/smithy-lang/smithy-rs/issues/1289): Note we're limited to 21 labels because of `tuple`.
                    prefix = if (segments.size > 1) "#{Nom}::sequence::tuple::<_, _, #{Nom}::error::Error<&str>, _>((" else "",
                    postfix = if (segments.size > 1) "))" else "",
                    transform = { parser ->
                        """
                        #{Nom}::sequence::preceded(#{Nom}::bytes::complete::tag("/"),  $parser)
                        """.trimIndent()
                    },
                )
        with(writer) {
            rustTemplate("let input_string = uri.path();")
            if (greedyLabelIndex >= 0 && greedyLabelIndex + 1 < httpTrait.uri.segments.size) {
                rustTemplate(
                    """
                    if !input_string.ends_with("$restAfterGreedyLabel") {
                        return Err(#{RequestRejection}::UriPatternGreedyLabelPostfixNotFound);
                    }
                    let input_string = &input_string[..(input_string.len() - "$restAfterGreedyLabel".len())];
                    """.trimIndent(),
                    *codegenScope,
                )
            }
            rustTemplate(
                """
                let (input_string, $labeledNames) = $nomParser(input_string)?;
                debug_assert_eq!("", input_string);
                """.trimIndent(),
                *codegenScope,
            )
            segments.forEachIndexed { index, segment ->
                val binding = pathBindings.find { it.memberName == segment.content }
                if (binding != null && segment.isLabel) {
                    val deserializer = generateParseStrFn(binding, true)
                    rustTemplate(
                        """
                        input = input.${binding.member.setterName()}(
                            #{deserializer}(m$index)?
                        );
                        """,
                        *codegenScope,
                        "deserializer" to deserializer,
                    )
                }
            }
        }
    }

    // The `httpQueryParams` trait can be applied to structure members that target:
    //     * a map of string,
    //     * a map of list of string; or
    //     * a map of set of string.
    enum class QueryParamsTargetMapValueType {
        STRING,
        LIST,
        SET,
    }

    private fun queryParamsTargetMapValueType(targetMapValue: Shape): QueryParamsTargetMapValueType =
        if (targetMapValue.isStringShape) {
            QueryParamsTargetMapValueType.STRING
        } else if (targetMapValue.isListShape) {
            QueryParamsTargetMapValueType.LIST
        } else if (targetMapValue.isSetShape) {
            QueryParamsTargetMapValueType.SET
        } else {
            throw ExpectationNotMetException(
                """
                @httpQueryParams trait applied to non-supported target $targetMapValue of type ${targetMapValue.type}
                """.trimIndent(),
                targetMapValue.sourceLocation,
            )
        }

    private fun serverRenderQueryStringParser(
        writer: RustWriter,
        operationShape: OperationShape,
    ) {
        val queryBindings =
            httpBindingResolver.requestBindings(operationShape).filter {
                it.location == HttpLocation.QUERY
            }
        // Only a single structure member can be bound to `httpQueryParams`, hence `find`.
        val queryParamsBinding =
            httpBindingResolver.requestBindings(operationShape).find {
                it.location == HttpLocation.QUERY_PARAMS
            }
        if (queryBindings.isEmpty() && queryParamsBinding == null) {
            return
        }

        fun HttpBindingDescriptor.queryParamsBindingTargetMapValueType(): QueryParamsTargetMapValueType {
            check(this.location == HttpLocation.QUERY_PARAMS)
            val queryParamsTarget = model.expectShape(this.member.target, MapShape::class.java)
            return queryParamsTargetMapValueType(model.expectShape(queryParamsTarget.value.target))
        }

        with(writer) {
            rustTemplate(
                """
                let query_string = uri.query().unwrap_or("");
                let pairs = #{FormUrlEncoded}::parse(query_string.as_bytes());
                """.trimIndent(),
                *codegenScope,
            )

            if (queryParamsBinding != null) {
                val target =
                    model.expectShape(queryParamsBinding.member.target, MapShape::class.java)
                val hasConstrainedTarget = target.canReachConstrainedShape(model, symbolProvider)
                // TODO(https://github.com/smithy-lang/smithy-rs/issues/1401) Here we only check the target shape;
                //  constraint traits on member shapes are not implemented yet.
                val targetSymbol = unconstrainedShapeSymbolProvider.toSymbol(target)
                withBlock("let mut query_params: #T = ", ";", targetSymbol) {
                    conditionalBlock("#T(", ")", conditional = hasConstrainedTarget, targetSymbol) {
                        rust("#T::new()", RuntimeType.HashMap)
                    }
                }
            }
            val (queryBindingsTargetingCollection, queryBindingsTargetingSimple) =
                queryBindings.partition {
                    model.expectShape(it.member.target) is CollectionShape
                }
            queryBindingsTargetingSimple.forEach {
                rust("let mut ${symbolProvider.toMemberName(it.member)}_seen = false;")
            }
            queryBindingsTargetingCollection.forEach {
                rust("let mut ${symbolProvider.toMemberName(it.member)} = Vec::new();")
            }

            rustBlock("for (k, v) in pairs") {
                queryBindingsTargetingSimple.forEach {
                    val deserializer = generateParseStrFn(it, false)
                    val memberName = symbolProvider.toMemberName(it.member)
                    rustTemplate(
                        """
                        if !${memberName}_seen && k == "${it.locationName}" {
                            input = input.${it.member.setterName()}(
                                #{deserializer}(&v)?
                            );
                            ${memberName}_seen = true;
                        }
                        """.trimIndent(),
                        "deserializer" to deserializer,
                    )
                }
                queryBindingsTargetingCollection.forEachIndexed { idx, it ->
                    rustBlock("${if (idx > 0) "else " else ""}if k == ${it.locationName.dq()}") {
                        val targetCollectionShape =
                            model.expectShape(it.member.target, CollectionShape::class.java)
                        val memberShape = model.expectShape(targetCollectionShape.member.target)

                        when {
                            memberShape.isStringShape -> {
                                if (queryParamsBinding != null) {
                                    // If there's an `@httpQueryParams` binding, it will want to consume the parsed data
                                    // too further down, so we need to clone it.
                                    rust("let v = v.clone().into_owned();")
                                } else {
                                    rust("let v = v.into_owned();")
                                }
                            }

                            memberShape.isTimestampShape -> {
                                val index = HttpBindingIndex.of(model)
                                val timestampFormat =
                                    index.determineTimestampFormat(
                                        it.member,
                                        it.location,
                                        protocol.defaultTimestampFormat,
                                    )
                                val timestampFormatType =
                                    RuntimeType.parseTimestampFormat(
                                        CodegenTarget.SERVER,
                                        runtimeConfig,
                                        timestampFormat,
                                    )
                                rustTemplate(
                                    """
                                    let v = #{DateTime}::from_str(&v, #{format})?
                                    """.trimIndent(),
                                    *codegenScope,
                                    "format" to timestampFormatType,
                                )
                                for (customization in customizations) {
                                    customization.section(
                                        ServerHttpBoundProtocolSection.AfterTimestampDeserializedMember(
                                            it.member,
                                        ),
                                    )(this)
                                }
                                rust(";")
                            }

                            else -> { // Number or boolean.
                                rust(
                                    """
                                    let v = <_ as #T>::parse_smithy_primitive(&v)?;
                                    """.trimIndent(),
                                    RuntimeType.smithyTypes(runtimeConfig)
                                        .resolve("primitive::Parse"),
                                )
                            }
                        }
                        rust("${symbolProvider.toMemberName(it.member)}.push(v);")
                    }
                }

                if (queryParamsBinding != null) {
                    val target = model.expectShape(queryParamsBinding.member.target, MapShape::class.java)
                    // TODO(https://github.com/smithy-lang/smithy-rs/issues/1401) Here we only check the target shape;
                    //  constraint traits on member shapes are not implemented yet.
                    val hasConstrainedTarget =
                        target.canReachConstrainedShape(model, symbolProvider)
                    when (queryParamsBinding.queryParamsBindingTargetMapValueType()) {
                        QueryParamsTargetMapValueType.STRING -> {
                            rust("query_params.${if (hasConstrainedTarget) "0." else ""}entry(String::from(k)).or_insert_with(|| String::from(v));")
                        }

                        QueryParamsTargetMapValueType.LIST, QueryParamsTargetMapValueType.SET -> {
                            if (hasConstrainedTarget) {
                                val collectionShape =
                                    model.expectShape(target.value.target, CollectionShape::class.java)
                                val collectionSymbol =
                                    unconstrainedShapeSymbolProvider.toSymbol(collectionShape)
                                rust(
                                    // `or_insert_with` instead of `or_insert` to avoid the allocation when the entry is
                                    // not empty.
                                    """
                                    let entry = query_params.0.entry(String::from(k)).or_insert_with(|| #T(std::vec::Vec::new()));
                                    entry.0.push(String::from(v));
                                    """,
                                    collectionSymbol,
                                )
                            } else {
                                rust(
                                    """
                                    let entry = query_params.entry(String::from(k)).or_default();
                                    entry.push(String::from(v));
                                    """,
                                )
                            }
                        }
                    }
                }
            }
            if (queryParamsBinding != null) {
                val isOptional =
                    unconstrainedShapeSymbolProvider
                        .toSymbol(queryParamsBinding.member)
                        .isOptional()
                withBlock("input = input.${queryParamsBinding.member.setterName()}(", ");") {
                    conditionalBlock("Some(", ")", conditional = isOptional) {
                        write("query_params")
                    }
                }
            }
            queryBindingsTargetingCollection.forEach { binding ->
                // TODO(https://github.com/smithy-lang/smithy-rs/issues/1401) Constraint traits on member shapes are not
                //  implemented yet.
                val hasConstrainedTarget =
                    model.expectShape(binding.member.target, CollectionShape::class.java)
                        .canReachConstrainedShape(model, symbolProvider)
                val memberName = unconstrainedShapeSymbolProvider.toMemberName(binding.member)
                val isOptional =
                    unconstrainedShapeSymbolProvider.toSymbol(binding.member).isOptional()
                rustBlock("if !$memberName.is_empty()") {
                    withBlock(
                        "input = input.${
                            binding.member.setterName()
                        }(",
                        ");",
                    ) {
                        conditionalBlock("Some(", ")", conditional = isOptional) {
                            conditionalBlock(
                                "#T(",
                                ")",
                                conditional = hasConstrainedTarget,
                                unconstrainedShapeSymbolProvider.toSymbol(binding.member).mapRustType {
                                    it.stripOuter<RustType.Option>()
                                },
                            ) {
                                write(memberName)
                            }
                        }
                    }
                }
            }
        }
    }

    private fun serverRenderHeaderParser(
        writer: RustWriter,
        binding: HttpBindingDescriptor,
        operationShape: OperationShape,
    ) {
        val deserializer = httpBindingGenerator(operationShape).generateDeserializeHeaderFn(binding)
        writer.rustTemplate(
            """
            #{deserializer}(&headers)?
            """.trimIndent(),
            "deserializer" to deserializer,
            *codegenScope,
        )
    }

    private fun serverRenderPrefixHeadersParser(
        writer: RustWriter,
        binding: HttpBindingDescriptor,
        operationShape: OperationShape,
    ) {
        check(binding.location == HttpLocation.PREFIX_HEADERS)

        val deserializer =
            httpBindingGenerator(operationShape).generateDeserializePrefixHeadersFn(binding)
        writer.rustTemplate(
            """
            #{deserializer}(&headers)?
            """.trimIndent(),
            "deserializer" to deserializer,
            *codegenScope,
        )
    }

    private fun generateParseStrFn(
        binding: HttpBindingDescriptor,
        percentDecoding: Boolean,
    ): RuntimeType {
        val output = unconstrainedShapeSymbolProvider.toSymbol(binding.member)
        return protocolFunctions.deserializeFn(binding.member) { fnName ->
            rustBlockTemplate(
                "pub fn $fnName(value: &str) -> std::result::Result<#{O}, #{RequestRejection}>",
                *codegenScope,
                "O" to output,
            ) {
                val target = model.expectShape(binding.member.target)

                when {
                    target.isStringShape -> {
                        if (percentDecoding) {
                            rustTemplate(
                                """
                                let value = #{PercentEncoding}::percent_decode_str(value).decode_utf8()?.into_owned();
                                """,
                                *codegenScope,
                            )
                        } else {
                            rust("let value = value.to_owned();")
                        }
                    }

                    target.isTimestampShape -> {
                        val index = HttpBindingIndex.of(model)
                        val timestampFormat =
                            index.determineTimestampFormat(
                                binding.member,
                                binding.location,
                                protocol.defaultTimestampFormat,
                            )
                        val timestampFormatType =
                            RuntimeType.parseTimestampFormat(CodegenTarget.SERVER, runtimeConfig, timestampFormat)

                        if (percentDecoding) {
                            rustTemplate(
                                """
                                let value = #{PercentEncoding}::percent_decode_str(value).decode_utf8()?;
                                let value = #{DateTime}::from_str(value.as_ref(), #{format})?
                                """,
                                *codegenScope,
                                "format" to timestampFormatType,
                            )
                        } else {
                            rustTemplate(
                                """
                                let value = #{DateTime}::from_str(value, #{format})?
                                """,
                                *codegenScope,
                                "format" to timestampFormatType,
                            )
                        }
                        for (customization in customizations) {
                            customization.section(
                                ServerHttpBoundProtocolSection.AfterTimestampDeserializedMember(
                                    binding.member,
                                ),
                            )(this)
                        }
                        rust(";")
                    }

                    else -> {
                        check(target is NumberShape || target is BooleanShape)
                        rustTemplate(
                            """
                            let value = <_ as #{PrimitiveParse}>::parse_smithy_primitive(value)?;
                            """,
                            "PrimitiveParse" to
                                RuntimeType.smithyTypes(runtimeConfig)
                                    .resolve("primitive::Parse"),
                        )
                    }
                }
                rust("Ok(${symbolProvider.wrapOptional(binding.member, "value")})")
            }
        }
    }

    private fun streamingBodyTraitBounds(operationShape: OperationShape) =
        if (operationShape.inputShape(model).hasStreamingMember(model)) {
            "\n B: Into<#{SmithyTypes}::byte_stream::ByteStream>,"
        } else {
            ""
        }

    private fun httpBindingGenerator(operationShape: OperationShape) =
        ServerRequestBindingGenerator(protocol, codegenContext, operationShape, additionalHttpBindingCustomizations)
}

private fun eventStreamWithInitialResponse(
    codegenContext: ServerCodegenContext,
    protocol: ServerProtocol,
    params: EventStreamBodyParams,
): Writable {
    return if (codegenContext.settings.codegenConfig.sendEventStreamInitialResponse) {
        val initialResponseGenerator =
            params.eventStreamMarshallerGenerator.renderInitialResponseGenerator(params.payloadContentType)

        writable {
            rustTemplate(
                """
                {
                    use #{futures_util}::StreamExt;
                    let payload = #{initial_response_payload};
                    let initial_message = #{initial_response_generator}(payload);
                    let mut buffer = #{Vec}::new();
                    #{write_message_to}(&initial_message, &mut buffer)
                        .expect("Failed to write initial message");
                    let initial_message_stream = futures_util::stream::iter(vec![Ok(buffer.into())]);
                    let adapter = #{message_stream_adaptor};
                    initial_message_stream.chain(adapter)
                }
                """,
                *preludeScope,
                "futures_util" to ServerCargoDependency.FuturesUtil.toType(),
                "initial_response_payload" to initialResponsePayload(codegenContext, protocol, params),
                "message_stream_adaptor" to messageStreamAdaptor(params.outerName, params.memberName),
                "initial_response_generator" to initialResponseGenerator,
                "write_message_to" to
                    RuntimeType.smithyEventStream(codegenContext.runtimeConfig)
                        .resolve("frame::write_message_to"),
            )
        }
    } else {
        messageStreamAdaptor(params.outerName, params.memberName)
    }
}

private fun initialResponsePayload(
    codegenContext: ServerCodegenContext,
    protocol: ServerProtocol,
    params: EventStreamBodyParams,
): Writable {
    return if (protocol.httpBindingResolver.handlesEventStreamInitialResponse(params.operationShape)) {
        val serializer = protocol.structuredDataSerializer().operationOutputSerializer(params.operationShape)!!
        writable {
            rustTemplate(
                "#{Bytes}::from(#{serializer}(&output)?)",
                "serializer" to serializer,
                "Bytes" to RuntimeType.Bytes,
            )
        }
    } else {
        val outputShape = params.operationShape.outputShape(codegenContext.model)
        val emptyPayloadFn = protocol.structuredDataSerializer().unsetStructure(outputShape)
        writable {
            rustTemplate(
                "#{Bytes}::from(#{empty_payload_fn}())",
                "Bytes" to RuntimeType.Bytes,
                "empty_payload_fn" to emptyPayloadFn,
            )
        }
    }
}

private fun messageStreamAdaptor(
    outerName: String,
    memberName: String,
) = writable {
    rust("$outerName.$memberName.into_body_stream(marshaller, error_marshaller, signer)")
}
