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
import software.amazon.smithy.rust.codegen.rustlang.CratesIo
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
import software.amazon.smithy.rust.codegen.smithy.transformers.errorMessageMember
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
)

/*
 * Class used to expose Rust server traits types. Is is used in [ServerHttpProtocolGenerator] and [ServerProtocolTestGenerator].
 */
class HttpServerTraits {
    fun parseHttpRequest(runtimeConfig: RuntimeConfig) = RuntimeType(
        "ParseHttpRequest",
        dependency = CargoDependency.SmithyHttpServer(runtimeConfig),
        namespace = "aws_smithy_http_server::request"
    )

    fun serializeHttpResponse(runtimeConfig: RuntimeConfig) = RuntimeType(
        "SerializeHttpResponse",
        dependency = CargoDependency.SmithyHttpServer(runtimeConfig),
        namespace = "aws_smithy_http_server::response"
    )

    fun serializeHttpError(runtimeConfig: RuntimeConfig) = RuntimeType(
        "SerializeHttpError",
        dependency = CargoDependency.SmithyHttpServer(runtimeConfig),
        namespace = "aws_smithy_http_server::response"
    )
}

/*
 * Generate all operation input parsers and output serializers for streaming and
 * non-straming types.
 */
private class ServerHttpProtocolImplGenerator(
    private val codegenContext: CodegenContext,
    private val protocol: Protocol,
) : ProtocolTraitImplGenerator {
    private val logger = Logger.getLogger(javaClass.name)
    private val symbolProvider = codegenContext.symbolProvider
    private val model = codegenContext.model
    private val errorType = RuntimeType("error", null, "crate")
    private val runtimeConfig = codegenContext.runtimeConfig
    private val httpBindingResolver = protocol.httpBindingResolver
    private val operationDeserModule = RustModule.private("operation_deser")
    private val operationSerModule = RustModule.private("operation_ser")
    private val smithyJson = CargoDependency.smithyJson(runtimeConfig).asType()
    private val httpServerTraits = HttpServerTraits()

    private val codegenScope = arrayOf(
        "ParseHttpRequest" to httpServerTraits.parseHttpRequest(runtimeConfig),
        "ParseStrictResponse" to RuntimeType.parseStrictResponse(runtimeConfig),
        "SerializeHttpResponse" to httpServerTraits.serializeHttpResponse(runtimeConfig),
        "SerializeHttpError" to httpServerTraits.serializeHttpError(runtimeConfig),
        "JsonObjectWriter" to smithyJson.member("serialize::JsonObjectWriter"),
        "http" to RuntimeType.http,
        "Bytes" to RuntimeType.Bytes,
        "Error" to errorType.member("Error"),
        "LazyStatic" to CargoDependency("lazy_static", CratesIo("1.4")).asType(),
        "Regex" to CargoDependency("regex", CratesIo("1.0")).asType(),
        "PercentEncoding" to CargoDependency("percent-encoding", CratesIo("2.1.0")).asType(),
        "SdkBody" to RuntimeType.sdkBody(runtimeConfig)
    )

    override fun generateTraitImpls(operationWriter: RustWriter, operationShape: OperationShape) {
        val inputSymbol = symbolProvider.toSymbol(operationShape.inputShape(model))
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
                renderNonStreamingTraits(operationName, inputSymbol, outputSymbol, operationShape)
            }
        }
    }

    /*
     * Generation of non-streaming traits. A non-streaming trait requires the HTTP body to be fully read in
     * memory before parsing or deserialization. From a server perspective we need a way to parse an HTTP
     * request from `Bytes` and serialize a HTTP response to `Bytes`. These traits are the public entrypoint
     * of the ser/de logic of the smithy-rs server.
     */
    private fun RustWriter.renderNonStreamingTraits(
        operationName: String?,
        inputSymbol: Symbol,
        outputSymbol: Symbol,
        operationShape: OperationShape
    ) {
        val errorSymbol = operationShape.errorSymbol(symbolProvider)
        /* Implement `ParseHttpRequest` for non streaming types. This is done by only implementing `parse_loaded` */
        rustTemplate(
            """
            impl #{ParseHttpRequest} for $operationName {
                type Input = std::result::Result<#{I}, #{Error}>;
                fn parse_unloaded(&self, _request: &mut #{http}::Request<#{SdkBody}>) -> Option<Self::Input> {
                    None
                }
                fn parse_loaded(&self, request: &#{http}::Request<#{Bytes}>) -> Self::Input {
                    #{parse_request}(request)
                }
            }""",
            *codegenScope,
            "I" to inputSymbol,
            "parse_request" to serverParseRequest(operationShape)
        )
        /* Implement `SerializeHttpResponse` for non streaming types. This is done by only implementing `serialize` */
        rustTemplate(
            """
            impl #{SerializeHttpResponse} for $operationName {
                type Output = std::result::Result<#{http}::Response<#{Bytes}>, #{Error}>;
                type Struct = #{O};
                fn serialize(&self, output: &Self::Struct) -> Self::Output {
                    #{serialize_response}(output)
                }
            }""",
            *codegenScope,
            "O" to outputSymbol,
            "serialize_response" to serverSerializeResponse(operationShape)
        )
        /* Implement `SerializeHttpError` for non streaming types. This is done by only implementing `serialize` */
        if (operationShape.errors.isNotEmpty()) {
            rustTemplate(
                """
                impl #{SerializeHttpError} for $operationName {
                    type Output = std::result::Result<#{http}::Response<#{Bytes}>, #{Error}>;
                    type Struct = #{E};
                    fn serialize(&self, error: &Self::Struct) -> Self::Output {
                        #{serialize_error}(error)
                    }
            }""",
                *codegenScope,
                "E" to errorSymbol,
                "serialize_error" to serverSerializeError(operationShape)
            )
        }
    }

    /*
     * TODO: implement streaming traits
     */
    private fun RustWriter.renderStreamingTraits(
        operationName: String,
        outputSymbol: Symbol,
        operationShape: OperationShape
    ) {
        logger.warning("[rust-server-codegen] $operationName: streaming trait is not yet implemented")
    }

    private fun serverParseRequest(operationShape: OperationShape): RuntimeType {
        val fnName = "parse_${operationShape.id.name.toSnakeCase()}_request"
        val inputShape = operationShape.inputShape(model)
        val inputSymbol = symbolProvider.toSymbol(inputShape)
        return RuntimeType.forInlineFun(fnName, operationDeserModule) {
            Attribute.Custom("allow(clippy::unnecessary_wraps)").render(it)
            it.rustBlockTemplate(
                "pub fn $fnName(request: &#{http}::Request<#{Bytes}>) -> std::result::Result<#{I}, #{Error}>",
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
                "pub fn $fnName(output: &#{O}) -> std::result::Result<#{http}::Response<#{Bytes}>, #{Error}>",
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
                "pub fn $fnName(error: &#{E}) -> std::result::Result<#{http}::Response<#{Bytes}>, #{Error}>",
                *codegenScope,
                "E" to errorSymbol
            ) {
                serverRenderShapeErrorSerializer(
                    operationShape,
                    errorSymbol,
                )
            }
        }
    }

    private fun RustWriter.serverRenderShapeErrorSerializer(
        operationShape: OperationShape,
        errorSymbol: RuntimeType,
    ) {
        val operationName = symbolProvider.toSymbol(operationShape).name
        val structuredDataSerializer = protocol.structuredDataSerializer(operationShape)
        rustTemplate("let mut response = #{http}::Response::builder();", *codegenScope)
        rust("let mut out = String::new();")
        rustTemplate("let mut object = #{JsonObjectWriter}::new(&mut out);", *codegenScope)
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
                        #T($data)?;
                        object.key(${"code".dq()}).string(${httpBindingResolver.errorCode(variantShape).dq()});
                        """.trimIndent(),
                        serializerSymbol
                    )
                    if (variantShape.errorMessageMember() != null) {
                        rust(
                            """
                            if let Some(message) = $data.message() {
                                object.key(${"message".dq()}).string(message);
                            }
                            """.trimIndent()
                        )
                    }
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
                    rust("response = response.status($status);")
                }
            }
        }
        rust("object.finish();")
        rustTemplate(
            """
            response.body(#{Bytes}::from(out))
                .map_err(#{Error}::BuildResponse)
            """.trimIndent(),
            *codegenScope
        )
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
            response.body(#{Bytes}::from(payload))
                .map_err(#{Error}::BuildResponse)?
            """.trimIndent(),
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
                        .ok_or(#{Error}::generic(${(memberName + " missing or empty").dq()}))?;
                    let http_status: u16 = #{Convert}::TryFrom::<i32>::try_from(status)
                        .map_err(|_| #{Error}::generic(${("invalid status code").dq()}))?;
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
        structuredDataParser.serverInputParser(operationShape).also { parser ->
            rust(
                "input = #T(request.body().as_ref(), input)?;",
                parser,
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
            ".map_err(#{Error}::from)?"
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
        writer.rust(
            """
            #T(request.headers())?
            """.trimIndent(),
            deserializer,
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
                "pub fn $fnName(value: &str) -> std::result::Result<#{O}, #{Error}>",
                *codegenScope,
                "O" to output,
            ) {
                rustTemplate(
                    """
                    let value = #{PercentEncoding}::percent_decode_str(value)
                        .decode_utf8()
                        .map_err(|err| #{Error}::DeserializeLabel(err.to_string()))?;
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
                "pub fn $fnName(value: &str) -> std::result::Result<#{O}, #{Error}>",
                *codegenScope,
                "O" to output,
            ) {
                rustTemplate(
                    """
                    let value = #{PercentEncoding}::percent_decode_str(value)
                        .decode_utf8()
                        .map_err(|err| #{Error}::DeserializeLabel(err.to_string()))?;
                    let value = #{DateTime}::DateTime::from_str(&value, #{format})
                        .map_err(|err| #{Error}::DeserializeLabel(err.to_string()))?;
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
                "pub fn $fnName(value: &str) -> std::result::Result<#{O}, #{Error}>",
                *codegenScope,
                "O" to output,
            ) {
                rustTemplate(
                    """
                    let value = std::str::FromStr::from_str(value)
                        .map_err(|_| #{Error}::DeserializeLabel(${"label parse error".dq()}.to_string()))?;
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
