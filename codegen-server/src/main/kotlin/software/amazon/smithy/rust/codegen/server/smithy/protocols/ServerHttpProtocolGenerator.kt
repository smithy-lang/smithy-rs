/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import software.amazon.smithy.aws.traits.protocols.RestJson1Trait
import software.amazon.smithy.aws.traits.protocols.RestXmlTrait
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.knowledge.HttpBindingIndex
import software.amazon.smithy.model.node.ExpectationNotMetException
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.model.traits.HttpErrorTrait
import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.render
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.withBlock
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerRuntimeType
import software.amazon.smithy.rust.codegen.server.smithy.generators.http.ServerRequestBindingGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.http.ServerResponseBindingGenerator
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.builderSymbol
import software.amazon.smithy.rust.codegen.smithy.generators.error.errorSymbol
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.MakeOperationGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolTraitImplGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.setterName
import software.amazon.smithy.rust.codegen.smithy.makeOptional
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpBindingDescriptor
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpBoundProtocolBodyGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpLocation
import software.amazon.smithy.rust.codegen.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.smithy.protocols.parse.StructuredDataParserGenerator
import software.amazon.smithy.rust.codegen.util.UNREACHABLE
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.expectTrait
import software.amazon.smithy.rust.codegen.util.findStreamingMember
import software.amazon.smithy.rust.codegen.util.getTrait
import software.amazon.smithy.rust.codegen.util.hasStreamingMember
import software.amazon.smithy.rust.codegen.util.inputShape
import software.amazon.smithy.rust.codegen.util.isStreaming
import software.amazon.smithy.rust.codegen.util.outputShape
import software.amazon.smithy.rust.codegen.util.toSnakeCase
import java.util.logging.Logger

/*
 * Implement operations' input parsing and output serialization. Protocols can plug their own implementations
 * and overrides by creating a protocol factory inheriting from this class and feeding it to the [ServerProtocolLoader].
 * See `ServerRestJson.kt` for more info.
 */
class ServerHttpProtocolGenerator(
    codegenContext: CodegenContext,
    protocol: Protocol,
) : ProtocolGenerator(
    codegenContext,
    protocol,
    MakeOperationGenerator(codegenContext, protocol, HttpBoundProtocolBodyGenerator(codegenContext, protocol)),
    ServerHttpProtocolImplGenerator(codegenContext, protocol),
) {
    // Define suffixes for operation input / output / error wrappers
    companion object {
        const val OPERATION_INPUT_WRAPPER_SUFFIX = "OperationInputWrapper"
        const val OPERATION_OUTPUT_WRAPPER_SUFFIX = "OperationOutputWrapper"

        fun smithyRejection(runtimeConfig: RuntimeConfig) = RuntimeType(
            "SmithyRejection",
            dependency = CargoDependency.SmithyHttpServer(runtimeConfig),
            namespace = "aws_smithy_http_server::rejection"
        )
    }
}

/*
 * Generate all operation input parsers and output serializers for streaming and
 * non-streaming types.
 */
private class ServerHttpProtocolImplGenerator(
    private val codegenContext: CodegenContext,
    private val protocol: Protocol,
) : ProtocolTraitImplGenerator {
    private val logger = Logger.getLogger(javaClass.name)
    private val symbolProvider = codegenContext.symbolProvider
    private val model = codegenContext.model
    private val runtimeConfig = codegenContext.runtimeConfig
    val httpBindingResolver = protocol.httpBindingResolver
    private val operationDeserModule = RustModule.private("operation_deser")
    private val operationSerModule = RustModule.private("operation_ser")

    private val codegenScope = arrayOf(
        "AsyncTrait" to ServerCargoDependency.AsyncTrait.asType(),
        "AxumCore" to ServerCargoDependency.AxumCore.asType(),
        "Cow" to ServerRuntimeType.Cow,
        "DateTime" to RuntimeType.DateTime(runtimeConfig),
        "HttpBody" to CargoDependency.HttpBody.asType(),
        "header_util" to CargoDependency.SmithyHttp(runtimeConfig).asType().member("header"),
        "Hyper" to CargoDependency.Hyper.asType(),
        "LazyStatic" to CargoDependency.LazyStatic.asType(),
        "Nom" to ServerCargoDependency.Nom.asType(),
        "PercentEncoding" to CargoDependency.PercentEncoding.asType(),
        "Regex" to CargoDependency.Regex.asType(),
        "SerdeUrlEncoded" to ServerCargoDependency.SerdeUrlEncoded.asType(),
        "SmithyHttp" to CargoDependency.SmithyHttp(runtimeConfig).asType(),
        "SmithyHttpServer" to CargoDependency.SmithyHttpServer(runtimeConfig).asType(),
        "SmithyRejection" to ServerHttpProtocolGenerator.smithyRejection(runtimeConfig),
        "http" to RuntimeType.http
    )

    override fun generateTraitImpls(operationWriter: RustWriter, operationShape: OperationShape) {
        val inputSymbol = symbolProvider.toSymbol(operationShape.inputShape(model))
        val outputSymbol = symbolProvider.toSymbol(operationShape.outputShape(model))

        operationWriter.renderTraits(inputSymbol, outputSymbol, operationShape)
    }

    /*
     * Generation of `FromRequest` and `IntoResponse`.
     * For non-streaming request bodies, that is, models without streaming traits
     * (https://awslabs.github.io/smithy/1.0/spec/core/stream-traits.html)
     * we require the HTTP body to be fully read in memory before parsing or deserialization.
     * From a server perspective we need a way to parse an HTTP request from `Bytes` and serialize
     * an HTTP response to `Bytes`.
     * These traits are the public entrypoint of the ser/de logic of the `aws-smithy-http-server` server.
     */
    private fun RustWriter.renderTraits(
        inputSymbol: Symbol,
        outputSymbol: Symbol,
        operationShape: OperationShape
    ) {
        val operationName = symbolProvider.toSymbol(operationShape).name
        val inputName = "${operationName}${ServerHttpProtocolGenerator.OPERATION_INPUT_WRAPPER_SUFFIX}"

        // Implement Axum `FromRequest` trait for input types.
        rustTemplate(
            """
            pub struct $inputName(pub #{I});
            ##[#{AsyncTrait}::async_trait]
            impl<B> #{AxumCore}::extract::FromRequest<B> for $inputName
            where
                B: #{SmithyHttpServer}::HttpBody + Send, ${getStreamingBodyTraitBounds(operationShape)}
                B::Data: Send,
                B::Error: Into<#{SmithyHttpServer}::BoxError>,
                #{SmithyRejection}: From<<B as #{SmithyHttpServer}::HttpBody>::Error>
            {
                type Rejection = #{SmithyRejection};
                async fn from_request(req: &mut #{AxumCore}::extract::RequestParts<B>) -> Result<Self, Self::Rejection> {
                    Ok($inputName(#{parse_request}(req).await?))
                }
            }
            """.trimIndent(),
            *codegenScope,
            "I" to inputSymbol,
            "parse_request" to serverParseRequest(operationShape)
        )

        // Implement Axum `IntoResponse` for output types.

        val outputName = "${operationName}${ServerHttpProtocolGenerator.OPERATION_OUTPUT_WRAPPER_SUFFIX}"
        val errorSymbol = operationShape.errorSymbol(symbolProvider)
        val httpExtensions = setHttpExtensions(operationShape)

        if (operationShape.errors.isNotEmpty()) {
            // The output of fallible operations is a `Result` which we convert into an
            // isomorphic `enum` type we control that can in turn be converted into a response.
            val intoResponseImpl =
                """
                let mut response = match self {
                    Self::Output(o) => {
                        match #{serialize_response}(o) {
                            Ok(response) => response,
                            Err(e) => {
                                e.into_response()
                            }
                        }
                    },
                    Self::Error(err) => {
                        match #{serialize_error}(&err) {
                            Ok(mut response) => {
                                response.extensions_mut().insert(aws_smithy_http_server::ExtensionModeledError::new(err.name()));
                                response
                            },
                            Err(e) => {
                                e.into_response()
                            }
                        }
                    }
                };
                $httpExtensions
                response
                """.trimIndent()

            rustTemplate(
                """
                pub enum $outputName {
                    Output(#{O}),
                    Error(#{E})
                }
                ##[#{AsyncTrait}::async_trait]
                impl #{AxumCore}::response::IntoResponse for $outputName {
                    fn into_response(self) -> #{AxumCore}::response::Response {
                        $intoResponseImpl
                    }
                }
                """.trimIndent(),
                *codegenScope,
                "O" to outputSymbol,
                "E" to errorSymbol,
                "serialize_response" to serverSerializeResponse(operationShape),
                "serialize_error" to serverSerializeError(operationShape)
            )
        } else {
            // The output of non-fallible operations is a model type which we convert into
            // a "wrapper" unit `struct` type we control that can in turn be converted into a response.
            val intoResponseImpl =
                """
                let mut response = match #{serialize_response}(self.0) {
                    Ok(response) => response,
                    Err(e) => e.into_response()
                };
                $httpExtensions
                response
                """.trimIndent()

            rustTemplate(
                """
                pub struct $outputName(pub #{O});
                ##[#{AsyncTrait}::async_trait]
                impl #{AxumCore}::response::IntoResponse for $outputName {
                    fn into_response(self) -> #{AxumCore}::response::Response {
                        $intoResponseImpl
                    }
                }
                """.trimIndent(),
                *codegenScope,
                "O" to outputSymbol,
                "serialize_response" to serverSerializeResponse(operationShape)
            )
        }

        // Implement conversion function to "wrap" from the model operation output types.
        if (operationShape.errors.isNotEmpty()) {
            rustTemplate(
                """
                impl From<Result<#{O}, #{E}>> for $outputName {
                    fn from(res: Result<#{O}, #{E}>) -> Self {
                        match res {
                            Ok(v) => Self::Output(v),
                            Err(e) => Self::Error(e),
                        }
                    }
                }
                """.trimIndent(),
                "O" to outputSymbol,
                "E" to errorSymbol
            )
        } else {
            rustTemplate(
                """
                impl From<#{O}> for $outputName {
                    fn from(o: #{O}) -> Self {
                        Self(o)
                    }
                }
                """.trimIndent(),
                "O" to outputSymbol
            )
        }

        // Implement conversion function to "unwrap" into the model operation input types.
        rustTemplate(
            """
            impl From<$inputName> for #{I} {
                fn from(i: $inputName) -> Self {
                    i.0
                }
            }
            """.trimIndent(),
            "I" to inputSymbol
        )
    }

    /*
     * Set `http::Extensions` for the current request. They can be used later for things like metrics, logging...
     */
    private fun setHttpExtensions(operationShape: OperationShape): String {
        val namespace = operationShape.id.namespace
        val operationName = symbolProvider.toSymbol(operationShape).name
        return """
            response.extensions_mut().insert(#{SmithyHttpServer}::ResponseExtensions::new("$namespace", "$operationName"));
        """.trimIndent()
    }

    private fun serverParseRequest(operationShape: OperationShape): RuntimeType {
        val fnName = "parse_${operationShape.id.name.toSnakeCase()}_request"
        val inputShape = operationShape.inputShape(model)
        val inputSymbol = symbolProvider.toSymbol(inputShape)
        val includedMembers = httpBindingResolver.requestMembers(operationShape, HttpLocation.DOCUMENT)
        val unusedVars = if (includedMembers.isEmpty()) "##[allow(unused_variables)] " else ""

        return RuntimeType.forInlineFun(fnName, operationDeserModule) {
            Attribute.Custom("allow(clippy::unnecessary_wraps)").render(it)
            it.rustBlockTemplate(
                """
                pub async fn $fnName<B>(
                    ${unusedVars}request: &mut #{AxumCore}::extract::RequestParts<B>
                ) -> std::result::Result<
                    #{I},
                    #{SmithyRejection}
                >
                where
                    B: #{SmithyHttpServer}::HttpBody + Send, ${getStreamingBodyTraitBounds(operationShape)}
                    B::Data: Send,
                    B::Error: Into<#{SmithyHttpServer}::BoxError>,
                    #{SmithyRejection}: From<<B as #{SmithyHttpServer}::HttpBody>::Error>
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
        val fnName = "serialize_${operationShape.id.name.toSnakeCase()}_response"
        val outputShape = operationShape.outputShape(model)
        val outputSymbol = symbolProvider.toSymbol(outputShape)
        return RuntimeType.forInlineFun(fnName, operationSerModule) {
            Attribute.Custom("allow(clippy::unnecessary_wraps)").render(it)

            // Note we only need to take ownership of the output in the case that it contains streaming members.
            // However we currently always take ownership here, but worth noting in case in the future we want
            // to generate different signatures for streaming vs non-streaming for some reason.
            it.rustBlockTemplate(
                "pub fn $fnName(output: #{O}) -> std::result::Result<#{AxumCore}::response::Response, #{SmithyRejection}>",
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
        val fnName = "serialize_${operationShape.id.name.toSnakeCase()}_error"
        val errorSymbol = operationShape.errorSymbol(symbolProvider)
        return RuntimeType.forInlineFun(fnName, operationSerModule) {
            Attribute.Custom("allow(clippy::unnecessary_wraps)").render(it)
            it.rustBlockTemplate(
                "pub fn $fnName(error: &#{E}) -> std::result::Result<#{AxumCore}::response::Response, #{SmithyRejection}>",
                *codegenScope,
                "E" to errorSymbol
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
        errorSymbol: RuntimeType,
    ) {
        val operationName = symbolProvider.toSymbol(operationShape).name
        val structuredDataSerializer = protocol.structuredDataSerializer(operationShape)
        withBlock("match error {", "}") {
            operationShape.errors.forEach {
                val variantShape = model.expectShape(it, StructureShape::class.java)
                val errorTrait = variantShape.expectTrait<ErrorTrait>()
                val variantSymbol = symbolProvider.toSymbol(variantShape)
                val serializerSymbol = structuredDataSerializer.serverErrorSerializer(it)

                rustBlock("#T::${variantSymbol.name}(output) =>", errorSymbol) {
                    rust(
                        """
                        let payload = #T(output)?;
                        """,
                        serializerSymbol
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
                        variantShape.getTrait<HttpErrorTrait>()?.let { trait -> trait.code }
                            ?: errorTrait.defaultHttpStatusCode
                    rustTemplate(
                        """
                        builder.status($status).body(#{SmithyHttpServer}::body::to_boxed(payload))?
                        """,
                        *codegenScope
                    )
                }
            }
        }
    }

    private fun RustWriter.serverRenderOutputShapeResponseSerializer(
        operationShape: OperationShape,
        bindings: List<HttpBindingDescriptor>,
    ) {
        // avoid non-usage warnings for response
        Attribute.AllowUnusedMut.render(this)
        rustTemplate("let mut builder = #{http}::Response::builder();", *codegenScope)
        serverRenderResponseHeaders(operationShape)

        for (binding in bindings) {
            val serializedValue = serverRenderBindingSerializer(binding, operationShape)
            if (serializedValue != null) {
                serializedValue(this)
            }
        }
        val streamingMember = operationShape.outputShape(model).findStreamingMember(model)
        if (streamingMember != null) {
            val memberName = symbolProvider.toMemberName(streamingMember)
            rustTemplate(
                """
                let payload = #{SmithyHttpServer}::body::Body::wrap_stream(output.$memberName);
                """,
                *codegenScope,
            )
        } else {
            val structuredDataSerializer = protocol.structuredDataSerializer(operationShape)
            structuredDataSerializer.serverOutputSerializer(operationShape)?.let { serializer ->
                rust(
                    "let payload = #T(&output)?;",
                    serializer
                )
            } ?: rust("""let payload = "";""")
        }
        rustTemplate(
            """
            builder.body(#{SmithyHttpServer}::body::to_boxed(payload))?
            """,
            *codegenScope,
        )
    }

    /**
     * Sets HTTP response headers for the operation's output shape or the operation's error shape.
     * It will generate response headers for the operation's output shape, unless [errorShape] is non-null, in which
     * case it will generate response headers for the given error shape.
     *
     * It serializes as HTTP headers shape members that are bound with `httpHeader` and `httpPrefixHeader`.
     *
     * The `Content-Type` header is also set according to the protocol and the contents of the shape to be serialized.
     */
    private fun RustWriter.serverRenderResponseHeaders(operationShape: OperationShape, errorShape: StructureShape? = null) {
        val contentType = httpBindingResolver.responseContentType(operationShape)
        if (contentType != null) {
            rustTemplate(
                """
                builder = #{header_util}::set_response_header_if_absent(
                    builder,
                    #{http}::header::CONTENT_TYPE,
                    "$contentType"
                );
                """,
                *codegenScope
            )
        }

        val bindingGenerator = ServerResponseBindingGenerator(protocol, codegenContext, operationShape)
        val addHeadersFn = bindingGenerator.generateAddHeadersFn(errorShape ?: operationShape)
        if (addHeadersFn != null) {
            // notice that we need to borrow the output only for output shapes but not for error shapes
            val outputOwnedOrBorrow = if (errorShape == null) "&output" else "output"
            rust(
                """
                builder = #{T}($outputOwnedOrBorrow, builder)?;
                """.trimIndent(),
                addHeadersFn
            )
        }
    }

    private fun serverRenderBindingSerializer(
        binding: HttpBindingDescriptor,
        operationShape: OperationShape,
    ): Writable? {
        val operationName = symbolProvider.toSymbol(operationShape).name
        val member = binding.member
        return when (binding.location) {
            HttpLocation.HEADER,
            HttpLocation.PREFIX_HEADERS,
            HttpLocation.DOCUMENT,
            HttpLocation.PAYLOAD -> {
                // All of these are handled separately.
                null
            }
            HttpLocation.RESPONSE_CODE -> writable {
                val memberName = symbolProvider.toMemberName(member)
                rustTemplate(
                    """
                    let status = output.$memberName
                        .ok_or_else(|| #{SmithyHttpServer}::rejection::Serialize::from("$memberName missing or empty"))?;
                    let http_status: u16 = std::convert::TryFrom::<i32>::try_from(status)
                        .map_err(|_| #{SmithyHttpServer}::rejection::Serialize::from("invalid status code"))?;
                    """.trimIndent(),
                    *codegenScope,
                )
                rust("builder = builder.status(http_status);")
            }
            else -> {
                UNREACHABLE("Unexpected binding location: ${binding.location}")
            }
        }
    }

    private fun RustWriter.serverRenderShapeParser(
        operationShape: OperationShape,
        inputShape: StructureShape,
        bindings: List<HttpBindingDescriptor>,
    ) {
        val httpBindingGenerator = ServerRequestBindingGenerator(protocol, codegenContext, operationShape)
        val structuredDataParser = protocol.structuredDataParser(operationShape)
        Attribute.AllowUnusedMut.render(this)
        rust("let mut input = #T::default();", inputShape.builderSymbol(symbolProvider))
        val parser = structuredDataParser.serverInputParser(operationShape)
        if (parser != null) {
            val contentTypeCheck = getContentTypeCheck()
            rustTemplate(
                """
                let body = request.take_body().ok_or(#{SmithyHttpServer}::rejection::BodyAlreadyExtracted)?;
                let bytes = #{Hyper}::body::to_bytes(body).await?;
                if !bytes.is_empty() {
                    #{SmithyHttpServer}::protocols::$contentTypeCheck(request)?;
                    input = #{parser}(bytes.as_ref(), input)?;
                }
                """,
                *codegenScope,
                "parser" to parser,
            )
        }
        for (binding in bindings) {
            val member = binding.member
            val parsedValue = serverRenderBindingParser(binding, operationShape, httpBindingGenerator, structuredDataParser)
            if (parsedValue != null) {
                withBlock("input = input.${member.setterName()}(", ");") {
                    parsedValue(this)
                }
            }
        }
        serverRenderUriPathParser(this, operationShape)
        serverRenderQueryStringParser(this, operationShape)

        val err = if (StructureGenerator.fallibleBuilder(inputShape, symbolProvider)) {
            "?"
        } else ""
        rustTemplate("input.build()$err", *codegenScope)
    }

    private fun serverRenderBindingParser(
        binding: HttpBindingDescriptor,
        operationShape: OperationShape,
        httpBindingGenerator: ServerRequestBindingGenerator,
        structuredDataParser: StructuredDataParserGenerator,
    ): Writable? {
        val errorSymbol = getDeserializeErrorSymbol(binding)
        return when (binding.location) {
            HttpLocation.HEADER -> writable { serverRenderHeaderParser(this, binding, operationShape) }
            HttpLocation.PAYLOAD -> {
                return if (binding.member.isStreaming(model)) {
                    writable {
                        rustTemplate(
                            """
                            {
                                let body = request.take_body().ok_or(#{SmithyHttpServer}::rejection::BodyAlreadyExtracted)?;
                                Some(body.into())
                            }
                            """.trimIndent(),
                            *codegenScope
                        )
                    }
                } else {
                    val structureShapeHandler: RustWriter.(String) -> Unit = { body ->
                        rust("#T($body)", structuredDataParser.payloadParser(binding.member))
                    }
                    val deserializer = httpBindingGenerator.generateDeserializePayloadFn(
                        operationShape,
                        binding,
                        errorSymbol,
                        structuredHandler = structureShapeHandler
                    )
                    writable {
                        rustTemplate(
                            """
                            {
                                let body = request.take_body().ok_or(#{SmithyHttpServer}::rejection::BodyAlreadyExtracted)?;
                                let bytes = #{Hyper}::body::to_bytes(body).await?;
                                #{Deserializer}(&bytes)?
                            }
                            """,
                            "Deserializer" to deserializer,
                            *codegenScope
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

    private fun serverRenderUriPathParser(writer: RustWriter, operationShape: OperationShape) {
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
            if (greedyLabelIndex >= 0)
                httpTrait.uri.segments.slice(0 until (greedyLabelIndex + 1))
            else
                httpTrait.uri.segments
        val restAfterGreedyLabel =
            if (greedyLabelIndex >= 0)
                httpTrait.uri.segments.slice((greedyLabelIndex + 1) until httpTrait.uri.segments.size).joinToString(prefix = "/", separator = "/")
            else
                ""
        val labeledNames = segments
            .mapIndexed { index, segment ->
                if (segment.isLabel) { "m$index" } else { "_" }
            }
            .joinToString(prefix = (if (segments.size > 1) "(" else ""), separator = ",", postfix = (if (segments.size > 1) ")" else ""))
        val nomParser = segments
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
                // TODO: tuple() is currently limited to 21 items
                prefix = if (segments.size > 1) "#{Nom}::sequence::tuple::<_, _, #{Nom}::error::Error<&str>, _>((" else "",
                postfix = if (segments.size > 1) "))" else "",
                transform = { parser ->
                    """
                    #{Nom}::sequence::preceded(#{Nom}::bytes::complete::tag("/"),  $parser)
                    """.trimIndent()
                }
            )
        with(writer) {
            rustTemplate("let input_string = request.uri().path();")
            if (greedyLabelIndex >= 0 && greedyLabelIndex + 1 < httpTrait.uri.segments.size) {
                rustTemplate(
                    """
                    if !input_string.ends_with(${restAfterGreedyLabel.dq()}) {
                        return std::result::Result::Err(#{SmithyRejection}::Deserialize(
                            aws_smithy_http_server::rejection::Deserialize::from_err(format!("Postfix not found: {}", ${restAfterGreedyLabel.dq()}))));
                    }
                    let input_string = &input_string[..(input_string.len() - ${restAfterGreedyLabel.dq()}.len())];
                    """.trimIndent(),
                    *codegenScope
                )
            }
            rustTemplate(
                """
                let (input_string, $labeledNames) = $nomParser(input_string)?;
                debug_assert_eq!("", input_string);
                """.trimIndent(),
                *codegenScope
            )
            segments
                .forEachIndexed { index, segment ->
                    val binding = pathBindings.find { it.memberName == segment.content }
                    if (binding != null && segment.isLabel) {
                        val deserializer = generateParsePercentEncodedStrFn(binding)
                        rustTemplate(
                            """
                            input = input.${binding.member.setterName()}(
                                #{deserializer}(m$index)?
                            );
                            """.trimIndent(),
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
        STRING, LIST, SET;

        fun asRustType(): RustType =
            when (this) {
                STRING -> RustType.String
                LIST -> RustType.Vec(RustType.String)
                SET -> RustType.HashSet(RustType.String)
            }
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
                @httpQueryParams trait applied to non-supported target
                $targetMapValue of type ${targetMapValue.type}
                """.trimIndent(),
                targetMapValue.sourceLocation
            )
        }

    private fun serverRenderQueryStringParser(writer: RustWriter, operationShape: OperationShape) {
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
            val queryParamsTarget = model.expectShape(this.member.target)
            val mapTarget = queryParamsTarget.asMapShape().get()
            return queryParamsTargetMapValueType(model.expectShape(mapTarget.value.target))
        }

        with(writer) {
            rustTemplate(
                """
                let query_string = request.uri().query().unwrap_or("");
                let pairs = #{SerdeUrlEncoded}::from_str::<Vec<(#{Cow}<'_, str>, #{Cow}<'_, str>)>>(query_string)?;
                """.trimIndent(),
                *codegenScope
            )

            if (queryParamsBinding != null) {
                rustTemplate(
                    "let mut query_params: #{HashMap}<String, " +
                        "${queryParamsBinding.queryParamsBindingTargetMapValueType().asRustType().render()}> = #{HashMap}::new();",
                    "HashMap" to RustType.HashMap.RuntimeType,
                )
            }
            val (queryBindingsTargettingCollection, queryBindingsTargettingSimple) =
                queryBindings.partition { model.expectShape(it.member.target) is CollectionShape }
            queryBindingsTargettingSimple.forEach {
                rust("let mut seen_${symbolProvider.toMemberName(it.member)} = false;")
            }
            queryBindingsTargettingCollection.forEach {
                rust("let mut ${symbolProvider.toMemberName(it.member)} = Vec::new();")
            }

            rustBlock("for (k, v) in pairs") {
                queryBindingsTargettingSimple.forEach {
                    val deserializer = generateParsePercentEncodedStrFn(it)
                    val memberName = symbolProvider.toMemberName(it.member)
                    rustTemplate(
                        """
                        if !seen_$memberName && k == "${it.locationName}" {
                            input = input.${it.member.setterName()}(
                                #{deserializer}(&v)?
                            );
                            seen_$memberName = true;
                        }
                        """.trimIndent(),
                        "deserializer" to deserializer
                    )
                }
                queryBindingsTargettingCollection.forEach {
                    rustBlock("if k == ${it.locationName.dq()}") {
                        val targetCollectionShape = model.expectShape(it.member.target, CollectionShape::class.java)
                        val memberShape = model.expectShape(targetCollectionShape.member.target)

                        when {
                            memberShape.isStringShape -> {
                                // `<_>::from()` is necessary to convert the `&str` into:
                                //     * the Rust enum in case the `string` shape has the `enum` trait; or
                                //     * `String` in case it doesn't.
                                rustTemplate(
                                    """
                                    let v = <_>::from(#{PercentEncoding}::percent_decode_str(&v).decode_utf8()?.as_ref());
                                    """.trimIndent(),
                                    *codegenScope
                                )
                            }
                            memberShape.isTimestampShape -> {
                                val index = HttpBindingIndex.of(model)
                                val timestampFormat =
                                    index.determineTimestampFormat(
                                        it.member,
                                        it.location,
                                        protocol.defaultTimestampFormat,
                                    )
                                val timestampFormatType = RuntimeType.TimestampFormat(runtimeConfig, timestampFormat)
                                rustTemplate(
                                    """
                                    let v = #{PercentEncoding}::percent_decode_str(&v).decode_utf8()?;
                                    let v = #{DateTime}::from_str(&v, #{format})?;
                                    """.trimIndent(),
                                    *codegenScope,
                                    "format" to timestampFormatType,
                                )
                            }
                            else -> { // Number or boolean.
                                rust(
                                    """
                                    let v = <_ as #T>::parse_smithy_primitive(&v)?;
                                    """.trimIndent(),
                                    CargoDependency.SmithyTypes(runtimeConfig).asType().member("primitive::Parse")
                                )
                            }
                        }
                        rust("${symbolProvider.toMemberName(it.member)}.push(v);")
                    }
                }

                if (queryParamsBinding != null) {
                    when (queryParamsBinding.queryParamsBindingTargetMapValueType()) {
                        QueryParamsTargetMapValueType.STRING -> {
                            rust("query_params.entry(String::from(k)).or_insert_with(|| String::from(v));")
                        } else -> {
                            rustTemplate(
                                """
                                let entry = query_params.entry(String::from(k)).or_default();
                                entry.push(String::from(v));
                                """.trimIndent()
                            )
                        }
                    }
                }
            }
            if (queryParamsBinding != null) {
                rust("input = input.${queryParamsBinding.member.setterName()}(Some(query_params));")
            }
            queryBindingsTargettingCollection.forEach {
                val memberName = symbolProvider.toMemberName(it.member)
                rustTemplate(
                    """
                    input = input.${it.member.setterName()}(
                        if $memberName.is_empty() {
                            None
                        } else {
                            Some($memberName)
                        }
                    );
                    """.trimIndent()
                )
            }
        }
    }

    private fun serverRenderHeaderParser(writer: RustWriter, binding: HttpBindingDescriptor, operationShape: OperationShape) {
        val httpBindingGenerator =
            ServerRequestBindingGenerator(
                ServerRestJson(codegenContext),
                codegenContext,
                operationShape,
            )
        val deserializer = httpBindingGenerator.generateDeserializeHeaderFn(binding)
        writer.rustTemplate(
            """
            #{deserializer}(request.headers().ok_or(#{SmithyHttpServer}::rejection::HeadersAlreadyExtracted)?)?
            """.trimIndent(),
            "deserializer" to deserializer,
            *codegenScope
        )
    }

    private fun generateParsePercentEncodedStrFn(binding: HttpBindingDescriptor): RuntimeType {
        // HTTP bindings we support that contain percent-encoded data.
        check(binding.location == HttpLocation.LABEL || binding.location == HttpLocation.QUERY)

        val target = model.expectShape(binding.member.target)
        return when {
            target.isStringShape -> generateParsePercentEncodedStrAsStringFn(binding)
            target.isTimestampShape -> generateParsePercentEncodedStrAsTimestampFn(binding)
            else -> generateParseStrAsPrimitiveFn(binding)
        }
    }

    private fun generateParsePercentEncodedStrAsStringFn(binding: HttpBindingDescriptor): RuntimeType {
        val output = symbolProvider.toSymbol(binding.member)
        val fnName = generateParseStrFnName(binding)
        return RuntimeType.forInlineFun(fnName, operationDeserModule) { writer ->
            writer.rustBlockTemplate(
                "pub fn $fnName(value: &str) -> std::result::Result<#{O}, #{SmithyRejection}>",
                *codegenScope,
                "O" to output,
            ) {
                // `<_>::from()` is necessary to convert the `&str` into:
                //     * the Rust enum in case the `string` shape has the `enum` trait; or
                //     * `String` in case it doesn't.
                rustTemplate(
                    """
                    let value = <_>::from(#{PercentEncoding}::percent_decode_str(value).decode_utf8()?.as_ref());
                    Ok(Some(value))
                    """.trimIndent(),
                    *codegenScope,
                )
            }
        }
    }

    private fun generateParsePercentEncodedStrAsTimestampFn(binding: HttpBindingDescriptor): RuntimeType {
        val output = symbolProvider.toSymbol(binding.member)
        val fnName = generateParseStrFnName(binding)
        val index = HttpBindingIndex.of(model)
        val timestampFormat =
            index.determineTimestampFormat(
                binding.member,
                binding.location,
                protocol.defaultTimestampFormat,
            )
        val timestampFormatType = RuntimeType.TimestampFormat(runtimeConfig, timestampFormat)
        return RuntimeType.forInlineFun(fnName, operationDeserModule) { writer ->
            writer.rustBlockTemplate(
                "pub fn $fnName(value: &str) -> std::result::Result<#{O}, #{SmithyRejection}>",
                *codegenScope,
                "O" to output,
            ) {
                rustTemplate(
                    """
                    let value = #{PercentEncoding}::percent_decode_str(value).decode_utf8()?;
                    let value = #{DateTime}::from_str(&value, #{format})?;
                    Ok(Some(value))
                    """.trimIndent(),
                    *codegenScope,
                    "format" to timestampFormatType,
                )
            }
        }
    }

    // TODO These functions can be replaced with the ones in https://docs.rs/aws-smithy-types/latest/aws_smithy_types/primitive/trait.Parse.html
    private fun generateParseStrAsPrimitiveFn(binding: HttpBindingDescriptor): RuntimeType {
        val output = symbolProvider.toSymbol(binding.member).makeOptional()
        val fnName = generateParseStrFnName(binding)
        return RuntimeType.forInlineFun(fnName, operationDeserModule) { writer ->
            writer.rustBlockTemplate(
                "pub fn $fnName(value: &str) -> std::result::Result<#{O}, #{SmithyRejection}>",
                *codegenScope,
                "O" to output,
            ) {
                rustTemplate(
                    """
                    let value = std::str::FromStr::from_str(value)?;
                    Ok(Some(value))
                    """.trimIndent(),
                    *codegenScope,
                )
            }
        }
    }

    private fun generateParseStrFnName(binding: HttpBindingDescriptor): String {
        val containerName = binding.member.container.name.toSnakeCase()
        val memberName = binding.memberName.toSnakeCase()
        return "parse_str_${containerName}_$memberName"
    }

    private fun getContentTypeCheck(): String {
        when (codegenContext.protocol) {
            RestJson1Trait.ID -> {
                return "check_json_content_type"
            }
            RestXmlTrait.ID -> {
                return "check_xml_content_type"
            }
            else -> {
                TODO("Protocol ${codegenContext.protocol} not supported yet")
            }
        }
    }

    private fun getDeserializeErrorSymbol(binding: HttpBindingDescriptor): RuntimeType {
        if (model.expectShape(binding.member.target) is StringShape) {
            return CargoDependency.SmithyHttpServer(runtimeConfig).asType().member("rejection").member("SmithyRejection")
        }
        when (codegenContext.protocol) {
            RestJson1Trait.ID -> {
                return CargoDependency.smithyJson(runtimeConfig).asType().member("deserialize").member("Error")
            }
            RestXmlTrait.ID -> {
                return CargoDependency.smithyXml(runtimeConfig).asType().member("decode").member("XmlError")
            }
            else -> {
                TODO("Protocol ${codegenContext.protocol} not supported yet")
            }
        }
    }

    private fun getStreamingBodyTraitBounds(operationShape: OperationShape): String {
        if (operationShape.inputShape(model).hasStreamingMember(model)) {
            return "\n B: Into<#{SmithyHttp}::byte_stream::ByteStream>,"
        } else {
            return ""
        }
    }
}
