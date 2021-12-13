/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.knowledge.HttpBindingIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.model.traits.HttpErrorTrait
import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.withBlock
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.builderSymbol
import software.amazon.smithy.rust.codegen.smithy.generators.error.errorSymbol
import software.amazon.smithy.rust.codegen.smithy.generators.http.ResponseBindingGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.MakeOperationGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolTraitImplGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.setterName
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpBindingDescriptor
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpBoundProtocolBodyGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpLocation
import software.amazon.smithy.rust.codegen.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.expectTrait
import software.amazon.smithy.rust.codegen.util.getTrait
import software.amazon.smithy.rust.codegen.util.hasStreamingMember
import software.amazon.smithy.rust.codegen.util.inputShape
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
        const val OPERATION_ERROR_WRAPPER_SUFFIX = "OperationErrorWrapper"

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
        "DateTime" to RuntimeType.DateTime(runtimeConfig),
        "HttpBody" to CargoDependency.HttpBody.asType(),
        "Hyper" to CargoDependency.Hyper.asType(),
        "LazyStatic" to CargoDependency.LazyStatic.asType(),
        "PercentEncoding" to CargoDependency.PercentEncoding.asType(),
        "Regex" to CargoDependency.Regex.asType(),
        "SmithyHttpServer" to CargoDependency.SmithyHttpServer(runtimeConfig).asType(),
        "SmithyRejection" to ServerHttpProtocolGenerator.smithyRejection(runtimeConfig),
        "http" to RuntimeType.http,
    )

    override fun generateTraitImpls(operationWriter: RustWriter, operationShape: OperationShape) {
        val inputSymbol = symbolProvider.toSymbol(operationShape.inputShape(model))
        val outputSymbol = symbolProvider.toSymbol(operationShape.outputShape(model))

        operationWriter.renderTraits(inputSymbol, outputSymbol, operationShape)
    }

    /*
     * Generation of `FromRequest` and `IntoResponse`. They are currently only implemented for non-streaming request
     * and response bodies, that is, models without streaming traits
     * (https://awslabs.github.io/smithy/1.0/spec/core/stream-traits.html).
     * For non-streaming request bodies, we require the HTTP body to be fully read in memory before parsing or
     * deserialization. From a server perspective we need a way to parse an HTTP request from `Bytes` and serialize
     * an HTTP response to `Bytes`.
     * TODO Add support for streaming.
     * These traits are the public entrypoint of the ser/de logic of the `aws-smithy-http-server` server.
     */
    private fun RustWriter.renderTraits(
        inputSymbol: Symbol,
        outputSymbol: Symbol,
        operationShape: OperationShape
    ) {
        val operationName = symbolProvider.toSymbol(operationShape).name
        // Implement Axum `FromRequest` trait for input types.
        val inputName = "${operationName}${ServerHttpProtocolGenerator.OPERATION_INPUT_WRAPPER_SUFFIX}"
        val httpExtensions = setHttpExtensions(operationShape)

        val fromRequest = if (operationShape.inputShape(model).hasStreamingMember(model)) {
            // For streaming request bodies, we need to generate a different implementation of the `FromRequest` trait.
            // It will first offer the streaming input to the parser and potentially read the body into memory
            // if an error occurred or if the streaming parser indicates that it needs the full data to proceed.
            """
            async fn from_request(_req: &mut #{AxumCore}::extract::RequestParts<B>) -> Result<Self, Self::Rejection> {
                $httpExtensions
                todo!("Streaming support for input shapes is not yet supported in `smithy-rs`")
            }
            """.trimIndent()
        } else {
            """
            async fn from_request(req: &mut #{AxumCore}::extract::RequestParts<B>) -> Result<Self, Self::Rejection> {
                $httpExtensions
                Ok($inputName(#{parse_request}(req).await?))
            }
            """.trimIndent()
        }
        rustTemplate(
            """
            pub struct $inputName(pub #{I});
            ##[#{AsyncTrait}::async_trait]
            impl<B> #{AxumCore}::extract::FromRequest<B> for $inputName
            where
                B: #{SmithyHttpServer}::HttpBody + Send,
                B::Data: Send,
                B::Error: Into<#{SmithyHttpServer}::BoxError>,
                #{SmithyRejection}: From<<B as #{SmithyHttpServer}::HttpBody>::Error>
            {
                type Rejection = #{SmithyRejection};
                $fromRequest
            }
            """.trimIndent(),
            *codegenScope,
            "I" to inputSymbol,
            "parse_request" to serverParseRequest(operationShape)
        )

        // Implement Axum `IntoResponse` for output types.
        val outputName = "${operationName}${ServerHttpProtocolGenerator.OPERATION_OUTPUT_WRAPPER_SUFFIX}"
        val errorSymbol = operationShape.errorSymbol(symbolProvider)

        // For streaming response bodies, we need to generate a different implementation of the `IntoResponse` trait.
        // The body type will have to be a `StreamBody`. The service implementer will return a `Stream` from their handler.
        val intoResponseStreaming = "todo!(\"Streaming support for output shapes is not yet supported in `smithy-rs`\")"
        if (operationShape.errors.isNotEmpty()) {
            val intoResponseImpl = if (operationShape.outputShape(model).hasStreamingMember(model)) {
                intoResponseStreaming
            } else {
                """
                match self {
                    Self::Output(o) => {
                        match #{serialize_response}(&o) {
                            Ok(response) => response,
                            Err(e) => {
                                let mut response = #{http}::Response::builder().body(#{SmithyHttpServer}::body::to_boxed(e.to_string())).expect("unable to build response from output");
                                response.extensions_mut().insert(#{SmithyHttpServer}::ExtensionFrameworkError(e.to_string()));
                                response
                            }
                        }
                    },
                    Self::Error(err) => {
                        match #{serialize_error}(&err) {
                            Ok(mut response) => {
                                response.extensions_mut().insert(aws_smithy_http_server::ExtensionUserError(err.name()));
                                response
                            },
                            Err(e) => {
                                let mut response = #{http}::Response::builder().body(#{SmithyHttpServer}::body::to_boxed(e.to_string())).expect("unable to build response from error");
                                response.extensions_mut().insert(#{SmithyHttpServer}::ExtensionFrameworkError(e.to_string()));
                                response
                            }
                        }
                    }
                }
                """.trimIndent()
            }
            // The output of fallible operations is a `Result` which we convert into an isomorphic `enum` type we control
            // that can in turn be converted into a response.
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
            val handleSerializeOutput = if (operationShape.outputShape(model).hasStreamingMember(model)) {
                intoResponseStreaming
            } else {
                """
                match #{serialize_response}(&self.0) {
                    Ok(response) => response,
                    Err(e) => #{http}::Response::builder().body(#{SmithyHttpServer}::body::to_boxed(e.to_string())).expect("unable to build response from output")
                }
                """.trimIndent()
            }
            // The output of non-fallible operations is a model type which we convert into a "wrapper" unit `struct` type
            // we control that can in turn be converted into a response.
            rustTemplate(
                """
                pub struct $outputName(pub #{O});
                ##[#{AsyncTrait}::async_trait]
                impl #{AxumCore}::response::IntoResponse for $outputName {
                    fn into_response(self) -> #{AxumCore}::response::Response {
                        $handleSerializeOutput
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
     * Set `http::Extensions` for the current request. They can be used later for things like metrics, logging, etc..
     */
    private fun setHttpExtensions(operationShape: OperationShape): String {
        val namespace = operationShape.id.getNamespace()
        val operationName = symbolProvider.toSymbol(operationShape).name
        // TODO: remove when we support streaming types
        val requestPrefix = if (operationShape.inputShape(model).hasStreamingMember(model)) { "_" } else { "" }
        return """
            let extensions = ${requestPrefix}req.extensions_mut().ok_or(#{SmithyHttpServer}::rejection::ExtensionsAlreadyExtracted)?;
            extensions.insert(#{SmithyHttpServer}::ExtensionNamespace(${namespace.dq()}));
            extensions.insert(#{SmithyHttpServer}::ExtensionOperationName(${operationName.dq()}));
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
                    B: #{SmithyHttpServer}::HttpBody + Send,
                    B::Data: Send,
                    B::Error: Into<#{SmithyHttpServer}::BoxError>,
                    #{SmithyRejection}: From<<B as #{SmithyHttpServer}::HttpBody>::Error>
                """,
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
            it.rustBlockTemplate(
                "pub fn $fnName(output: &#{O}) -> std::result::Result<#{AxumCore}::response::Response, #{SmithyRejection}>",
                *codegenScope,
                "O" to outputSymbol,
            ) {
                withBlock("Ok({", "})") {
                    serverRenderShapeResponseSerializer(
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
                    serverRenderShapeErrorSerializer(
                        operationShape,
                        errorSymbol,
                    )
                }
            }
        }
    }

    private fun RustWriter.serverRenderShapeErrorSerializer(
        operationShape: OperationShape,
        errorSymbol: RuntimeType,
    ) {
        val operationName = symbolProvider.toSymbol(operationShape).name
        val structuredDataSerializer = protocol.structuredDataSerializer(operationShape)
        rustTemplate("let response: #{AxumCore}::response::Response;", *codegenScope)
        withBlock("match error {", "};") {
            operationShape.errors.forEach {
                val variantShape = model.expectShape(it, StructureShape::class.java)
                val errorTrait = variantShape.expectTrait<ErrorTrait>()
                val variantSymbol = symbolProvider.toSymbol(variantShape)
                val data = safeName("var")
                val serializerSymbol = structuredDataSerializer.serverErrorSerializer(it)
                rustBlock("#T::${variantSymbol.name}($data) =>", errorSymbol) {
                    rust(
                        """
                        let payload = #T($data)?;
                        """,
                        serializerSymbol
                    )
                    val bindings = httpBindingResolver.errorResponseBindings(it)
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
                        response = #{http}::Response::builder().status($status).body(#{SmithyHttpServer}::body::to_boxed(payload))?;
                        """,
                        *codegenScope
                    )
                }
            }
        }
        rust("response")
    }

    private fun RustWriter.serverRenderShapeResponseSerializer(
        operationShape: OperationShape,
        bindings: List<HttpBindingDescriptor>,
    ) {
        val structuredDataSerializer = protocol.structuredDataSerializer(operationShape)
        structuredDataSerializer.serverOutputSerializer(operationShape).also { serializer ->
            rust(
                "let payload = #T(output)?;",
                serializer
            )
        }
        // avoid non-usage warnings for response
        Attribute.AllowUnusedMut.render(this)
        rustTemplate("let mut response = #{http}::Response::builder();", *codegenScope)
        for (binding in bindings) {
            val serializedValue = serverRenderBindingSerializer(binding, operationShape)
            if (serializedValue != null) {
                serializedValue(this)
            }
        }
        rustTemplate(
            """
            response.body(#{SmithyHttpServer}::body::to_boxed(payload))?
            """,
            *codegenScope,
        )
    }

    private fun serverRenderBindingSerializer(
        binding: HttpBindingDescriptor,
        operationShape: OperationShape,
    ): Writable? {
        val operationName = symbolProvider.toSymbol(operationShape).name
        val member = binding.member
        return when (binding.location) {
            HttpLocation.HEADER, HttpLocation.PREFIX_HEADERS, HttpLocation.PAYLOAD -> {
                logger.warning("[rust-server-codegen] $operationName: response serialization does not currently support ${binding.location} bindings")
                null
            }
            HttpLocation.DOCUMENT -> {
                // document is handled separately
                null
            }
            HttpLocation.RESPONSE_CODE -> writable {
                val memberName = symbolProvider.toMemberName(member)
                rustTemplate(
                    """
                    let status = output.$memberName
                        .ok_or_else(|| #{SmithyHttpServer}::rejection::Serialize::from(${(memberName + " missing or empty").dq()}))?;
                    let http_status: u16 = std::convert::TryFrom::<i32>::try_from(status)
                        .map_err(|_| #{SmithyHttpServer}::rejection::Serialize::from(${("invalid status code").dq()}))?;
                    """.trimIndent(),
                    *codegenScope,
                )
                rust("let response = response.status(http_status);")
            }
            else -> {
                TODO("Unexpected binding location: ${binding.location}")
            }
        }
    }

    private fun RustWriter.serverRenderShapeParser(
        operationShape: OperationShape,
        inputShape: StructureShape,
        bindings: List<HttpBindingDescriptor>,
    ) {
        val structuredDataParser = protocol.structuredDataParser(operationShape)
        Attribute.AllowUnusedMut.render(this)
        rust("let mut input = #T::default();", inputShape.builderSymbol(symbolProvider))
        val parser = structuredDataParser.serverInputParser(operationShape)
        if (parser != null) {
            rustTemplate(
                """
                let body = request.take_body().ok_or(#{SmithyHttpServer}::rejection::BodyAlreadyExtracted)?;
                let bytes = #{Hyper}::body::to_bytes(body).await?;
                if !bytes.is_empty() {
                    #{SmithyHttpServer}::protocols::check_json_content_type(request)?;
                    input = #{parser}(bytes.as_ref(), input)?;
                }
                """,
                *codegenScope,
                "parser" to parser,
            )
        }
        for (binding in bindings) {
            val member = binding.member
            val parsedValue = serverRenderBindingParser(binding, operationShape)
            if (parsedValue != null) {
                withBlock("input = input.${member.setterName()}(", ");") {
                    parsedValue(this)
                }
            }
        }
        serverRenderUriPathParser(this, operationShape)

        val err = if (StructureGenerator.fallibleBuilder(inputShape, symbolProvider)) {
            "?"
        } else ""
        rustTemplate("input.build()$err", *codegenScope)
    }

    private fun serverRenderBindingParser(
        binding: HttpBindingDescriptor,
        operationShape: OperationShape,
    ): Writable? {
        val operationName = symbolProvider.toSymbol(operationShape).name
        return when (val location = binding.location) {
            HttpLocation.HEADER -> writable { serverRenderHeaderParser(this, binding, operationShape) }
            HttpLocation.LABEL -> {
                null
            }
            HttpLocation.DOCUMENT -> {
                null
            }
            else -> {
                logger.warning("[rust-server-codegen] $operationName: request parsing does not currently support $location bindings")
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
        val pattern = StringBuilder()
        val httpTrait = httpBindingResolver.httpTrait(operationShape)
        httpTrait.uri.segments.forEach {
            pattern.append("/")
            if (it.isLabel) {
                pattern.append("(?P<${it.content}>")
                if (it.isGreedyLabel) {
                    pattern.append(".+")
                } else {
                    pattern.append("[^/]+")
                }
                pattern.append(")")
            } else {
                pattern.append(it.content)
            }
        }
        val errorShape = operationShape.errorSymbol(symbolProvider)
        with(writer) {
            rustTemplate(
                """
                #{LazyStatic}::lazy_static! {
                    static ref RE: #{Regex}::Regex = #{Regex}::Regex::new("$pattern").unwrap();
                }
                """.trimIndent(),
                *codegenScope,
            )
            rustBlock("if let Some(captures) = RE.captures(request.uri().path())") {
                pathBindings.forEach {
                    val deserializer = generateParseLabelFn(it)
                    rustTemplate(
                        """
                        if let Some(m) = captures.name("${it.locationName}") {
                            input = input.${it.member.setterName()}(
                                #{deserializer}(m.as_str())?
                            );
                        }
                        """.trimIndent(),
                        "deserializer" to deserializer,
                        "E" to errorShape,
                    )
                }
            }
        }
    }

    private fun serverRenderHeaderParser(writer: RustWriter, binding: HttpBindingDescriptor, operationShape: OperationShape) {
        val httpBindingGenerator =
            ResponseBindingGenerator(
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

    private fun generateParseLabelFn(binding: HttpBindingDescriptor): RuntimeType {
        check(binding.location == HttpLocation.LABEL)
        val target = model.expectShape(binding.member.target)
        return when {
            target.isStringShape -> generateParseLabelStringFn(binding)
            target.isTimestampShape -> generateParseLabelTimestampFn(binding)
            else -> generateParseLabelPrimitiveFn(binding)
        }
    }

    private fun generateParseLabelStringFn(binding: HttpBindingDescriptor): RuntimeType {
        val output = symbolProvider.toSymbol(binding.member)
        val fnName = generateParseLabelFnName(binding)
        return RuntimeType.forInlineFun(fnName, operationDeserModule) { writer ->
            writer.rustBlockTemplate(
                "pub fn $fnName(value: &str) -> std::result::Result<#{O}, #{SmithyRejection}>",
                *codegenScope,
                "O" to output,
            ) {
                rustTemplate(
                    """
                    let value = #{PercentEncoding}::percent_decode_str(value).decode_utf8()?;
                    Ok(Some(value.into_owned()))
                    """.trimIndent(),
                    *codegenScope,
                )
            }
        }
    }

    private fun generateParseLabelTimestampFn(binding: HttpBindingDescriptor): RuntimeType {
        val output = symbolProvider.toSymbol(binding.member)
        val fnName = generateParseLabelFnName(binding)
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

    private fun generateParseLabelPrimitiveFn(binding: HttpBindingDescriptor): RuntimeType {
        val output = symbolProvider.toSymbol(binding.member)
        val fnName = generateParseLabelFnName(binding)
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

    private fun generateParseLabelFnName(binding: HttpBindingDescriptor): String {
        val containerName = binding.member.container.name.toSnakeCase()
        val memberName = binding.memberName.toSnakeCase()
        return "parse_label_${containerName}_$memberName"
    }
}
