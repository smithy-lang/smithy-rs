/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.protocols

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.knowledge.HttpBinding
import software.amazon.smithy.model.knowledge.HttpBindingIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.model.traits.HttpTrait
import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.conditionalBlock
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.withBlock
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.HttpProtocolGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.builderSymbol
import software.amazon.smithy.rust.codegen.smithy.generators.error.errorSymbol
import software.amazon.smithy.rust.codegen.smithy.generators.http.ResponseBindingGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.setterName
import software.amazon.smithy.rust.codegen.smithy.isOptional
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.hasStreamingMember
import software.amazon.smithy.rust.codegen.util.isStreaming
import software.amazon.smithy.rust.codegen.util.outputShape
import software.amazon.smithy.rust.codegen.util.toSnakeCase

interface Protocol {
    fun structuredDataParser(operationShape: OperationShape): StructuredDataParserGenerator

    /**
     fn parse_generic(response: &Response<Bytes>) -> smithy_types::error::Generic
     **/
    fun parseGenericError(operationShape: OperationShape): RuntimeType
}

class HttpTraitProtocolGenerator(
    private val protocolConfig: ProtocolConfig,
    private val protocol: Protocol,
) : HttpProtocolGenerator(protocolConfig) {
    private val symbolProvider = protocolConfig.symbolProvider
    private val model = protocolConfig.model
    private val runtimeConfig = protocolConfig.runtimeConfig
    private val httpIndex = HttpBindingIndex.of(model)

    private val codegenScope = arrayOf(
        "ParseStrict" to RuntimeType.parseStrict(runtimeConfig),
        "Response" to RuntimeType.Http("Response"),
        "Bytes" to RuntimeType.Bytes,
        "SdkBody" to RuntimeType.sdkBody(runtimeConfig)
    )

    override fun RustWriter.body(self: String, operationShape: OperationShape): BodyMetadata {
        rustTemplate("#{SdkBody}::from(\"\")", *codegenScope)
        return BodyMetadata(takesOwnership = false)
    }

    override fun traitImplementations(operationWriter: RustWriter, operationShape: OperationShape) {
        val outputSymbol = symbolProvider.toSymbol(operationShape.outputShape(model))
        val operationName = symbolProvider.toSymbol(operationShape).name
        val httpTrait = operationShape.expectTrait(HttpTrait::class.java)

        // For streaming response bodies, we need to generate a different implementation of the parse traits.
        // These will first offer the streaming input to the parser & potentially read the body into memory
        // if an error occurred or if the streaming parser indicates that it needs the full data to proceed.
        if (operationShape.outputShape(model).hasStreamingMember(model)) {
            with(operationWriter) {
                renderStreamingTraits(operationName, httpTrait, outputSymbol, operationShape)
            }
        } else {
            with(operationWriter) {
                renderNonStreamingTraits(operationName, httpTrait, outputSymbol, operationShape)
            }
        }
    }

    private fun RustWriter.renderNonStreamingTraits(
        operationName: String?,
        httpTrait: HttpTrait,
        outputSymbol: Symbol,
        operationShape: OperationShape
    ) {
        rustTemplate(
            """
                impl #{ParseStrict} for $operationName {
                    type Output = Result<#{O}, #{E}>;
                    fn parse(&self, response: &#{Response}<#{Bytes}>) -> Self::Output {
                         if !response.status().is_success() && response.status().as_u16() != ${httpTrait.code} {
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
        httpTrait: HttpTrait,
        outputSymbol: Symbol,
        operationShape: OperationShape
    ) {
        rustTemplate(
            """
                    impl #{ParseResponse}<#{SdkBody}> for $operationName {
                        type Output = Result<#{O}, #{E}>;
                        fn parse_unloaded(&self, response: &mut http::Response<#{SdkBody}>) -> Option<Self::Output> {
                            // This is an error, defer to the non-streaming parser
                            if !response.status().is_success() && response.status().as_u16() != ${httpTrait.code} {
                                return None;
                            }
                            Some(#{parse_streaming_response}(response))
                        }
                        fn parse_loaded(&self, response: &http::Response<#{Bytes}>) -> Self::Output {
                            // if streaming, we only hit this case if its an error
                            #{parse_error}(response)
                        }
                    }
                """,
            "O" to outputSymbol,
            "E" to operationShape.errorSymbol(symbolProvider),
            "parse_streaming_response" to parseStreamingResponse(operationShape),
            "parse_error" to parseError(operationShape)
        )
    }

    private fun parseError(operationShape: OperationShape): RuntimeType {
        val fnName = "parse_${operationShape.id.name.toSnakeCase()}_error"
        val outputShape = operationShape.outputShape(model)
        val outputSymbol = symbolProvider.toSymbol(outputShape)
        val errorSymbol = operationShape.errorSymbol(symbolProvider)
        return RuntimeType.forInlineFun(fnName, "operation_deser") {
            Attribute.Custom("allow(clippy::unnecessary_wraps)").render(it)
            it.rustBlockTemplate(
                "pub fn $fnName(response: &#{Response}<#{Bytes}>) -> Result<#{O}, #{E}>",
                *codegenScope,
                "O" to outputSymbol,
                "E" to errorSymbol
            ) {

                rust(
                    "let generic = #T(&response).map_err(#T::unhandled)?;",
                    protocol.parseGenericError(operationShape),
                    errorSymbol
                )
                if (operationShape.errors.isNotEmpty()) {
                    rustTemplate(
                        """
                        let error_code = match generic.code() {
                            Some(code) => code,
                            None => return Err(#{error_symbol}::unhandled(generic))
                        };""",
                        "error_symbol" to errorSymbol,
                    )
                    withBlock("Err(match error_code {", "})") {
                        operationShape.errors.forEach { error ->
                            val errorShape = model.expectShape(error, StructureShape::class.java)
                            val variantName = symbolProvider.toSymbol(model.expectShape(error)).name
                            withBlock(
                                "${error.name.dq()} => #1T { meta: generic, kind: #1TKind::$variantName({",
                                "})},",
                                errorSymbol
                            ) {
                                renderShapeParser(
                                    operationShape,
                                    errorShape,
                                    httpIndex.getResponseBindings(errorShape),
                                    errorSymbol
                                )
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
        return RuntimeType.forInlineFun(fnName, "operation_deser") {
            Attribute.Custom("allow(clippy::unnecessary_wraps)").render(it)
            it.rustBlockTemplate(
                "pub fn $fnName(response: &mut #{Response}<#{SdkBody}>) -> Result<#{O}, #{E}>",
                *codegenScope,
                "O" to outputSymbol,
                "E" to errorSymbol
            ) {
                renderShapeParser(operationShape, outputShape, httpIndex.getResponseBindings(outputShape), errorSymbol)
            }
        }
    }

    private fun parseResponse(operationShape: OperationShape): RuntimeType {
        val fnName = "parse_${operationShape.id.name.toSnakeCase()}_response"
        val outputShape = operationShape.outputShape(model)
        val outputSymbol = symbolProvider.toSymbol(outputShape)
        val errorSymbol = operationShape.errorSymbol(symbolProvider)
        return RuntimeType.forInlineFun(fnName, "operation_deser") {
            Attribute.Custom("allow(clippy::unnecessary_wraps)").render(it)
            it.rustBlockTemplate(
                "pub fn $fnName(response: &#{Response}<#{Bytes}>) -> Result<#{O}, #{E}>",
                *codegenScope,
                "O" to outputSymbol,
                "E" to errorSymbol
            ) {
                withBlock("Ok({", "})") {
                    renderShapeParser(
                        operationShape,
                        outputShape,
                        httpIndex.getResponseBindings(operationShape),
                        errorSymbol
                    )
                }
            }
        }
    }

    override fun fromResponseImpl(implBlockWriter: RustWriter, operationShape: OperationShape) {
        fromResponseFun(implBlockWriter, operationShape) {
            rust("let _ = response;")
            rust("todo!()")
        }
    }

    override fun toHttpRequestImpl(
        implBlockWriter: RustWriter,
        operationShape: OperationShape,
        inputShape: StructureShape
    ) {
        httpBuilderFun(implBlockWriter) {
            rust("todo!()")
        }
    }

    private fun RustWriter.renderShapeParser(
        operationShape: OperationShape,
        outputShape: StructureShape,
        bindings: Map<String, HttpBinding>,
        errorSymbol: RuntimeType,
    ) {
        val httpBindingGenerator = ResponseBindingGenerator(protocolConfig, operationShape)
        val structuredDataParser = protocol.structuredDataParser(operationShape)
        Attribute.AllowUnusedMut.render(this)
        rust("let mut output = #T::default();", outputShape.builderSymbol(symbolProvider))
        // avoid non-usage warnings for response
        rust("let _ = response;")
        if (outputShape.id == operationShape.output.get() && !outputShape.hasStreamingMember(model)) {
            structuredDataParser.operationParser(operationShape)?.also { parser ->
                rust(
                    "output = #T(response.body().as_ref(), output).map_err(#T::unhandled)?;",
                    parser,
                    errorSymbol
                )
            }
        } else {
            check(outputShape.hasTrait(ErrorTrait::class.java)) { "should only be called on outputs or errors" }
            rust(
                "output = #T(response.body().as_ref(), output).map_err(#T::unhandled)?;",
                structuredDataParser.errorParser(outputShape),
                errorSymbol
            )
        }
        outputShape.members().forEach { member ->
            val parsedValue = renderBindingParser(
                bindings[member.memberName]!!,
                operationShape,
                httpBindingGenerator,
                structuredDataParser
            )
            if (parsedValue != null) {
                withBlock("output = output.${member.setterName()}(", ");") {
                    parsedValue(this)
                }
            }
        }

        val err = if (StructureGenerator.fallibleBuilder(outputShape, symbolProvider)) {
            ".map_err(|s|${format(errorSymbol)}::unhandled(s))?"
        } else ""
        rust("output.build()$err")
    }

    /**
     * Generate a parser & a parsed value converter for each output member of `operationShape`
     *
     * Returns a map with key = memberName, value = parsedValue
     */
    private fun renderBindingParser(
        binding: HttpBinding,
        operationShape: OperationShape,
        httpBindingGenerator: ResponseBindingGenerator,
        structuredDataParser: StructuredDataParserGenerator,
    ): Writable? {
        val errorSymbol = operationShape.errorSymbol(symbolProvider)
        val member = binding.member
        return when (binding.location) {
            HttpBinding.Location.HEADER -> writable {
                val fnName = httpBindingGenerator.generateDeserializeHeaderFn(binding)
                rust(
                    """
                        #T(response.headers())
                            .map_err(|_|#T::unhandled("Failed to parse ${member.memberName} from header `${binding.locationName}"))?
                        """,
                    fnName, errorSymbol
                )
            }
            HttpBinding.Location.DOCUMENT -> {
                // document is handled separately
                null
            }
            HttpBinding.Location.PAYLOAD -> {
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
                    binding,
                    errorSymbol,
                    docHandler = docShapeHandler,
                    structuredHandler = structureShapeHandler
                )
                return if (binding.member.isStreaming(model)) {
                    writable { rust("#T(response.body_mut())?", deserializer) }
                } else {
                    writable { rust("#T(response.body().as_ref())?", deserializer) }
                }
            }
            HttpBinding.Location.RESPONSE_CODE -> writable {
                conditionalBlock("Some(", ")", symbolProvider.toSymbol(member).isOptional()) {
                    rust("response.status().as_u16() as _")
                }
            }
            HttpBinding.Location.PREFIX_HEADERS -> {
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
                TODO("Unexpected binding location: ${binding.location}")
            }
        }
    }
}
/*
class AwsRestJsonGenerator(
    private val protocolConfig: ProtocolConfig
) : HttpProtocolGenerator(protocolConfig) {
    private val logger = Logger.getLogger(javaClass.name)
    private val symbolProvider = protocolConfig.symbolProvider
    private val runtimeConfig = protocolConfig.runtimeConfig
    private val httpIndex = HttpBindingIndex.of(protocolConfig.model)
    private val requestBuilder = RuntimeType.Http("request::Builder")
    private val sdkBody = RuntimeType.sdkBody(runtimeConfig)
    private val model = protocolConfig.model

    override fun traitImplementations(operationWriter: RustWriter, operationShape: OperationShape) {
        val outputSymbol = symbolProvider.toSymbol(operationShape.outputShape(model))
        val operationName = symbolProvider.toSymbol(operationShape).name
        // restJson1 requires all operations to use the HTTP trait
        val httpTrait = operationShape.expectTrait(HttpTrait::class.java)

        // For streaming response bodies, we need to generate a different implementation of the parse traits.
        // These will first offer the streaming input to the parser & potentially read the body into memory
        // if an error occurred or if the streaming parser indicates that it needs the full data to proceed.
        if (operationShape.outputShape(model).hasStreamingMember(model)) {
            renderStreamingTraits(operationWriter, operationName, httpTrait, outputSymbol, operationShape)
        } else {
            renderNonStreamingTraits(operationWriter, operationName, httpTrait, outputSymbol, operationShape)
        }
    }

    private fun renderNonStreamingTraits(
        operationWriter: RustWriter,
        operationName: String,
        httpTrait: HttpTrait,
        outputSymbol: Symbol,
        operationShape: OperationShape
    ) {
        operationWriter.rustTemplate(
            // strict (as in "not lazy") is the opposite of streaming
            """
                impl #{ParseStrict} for $operationName {
                    type Output = Result<#{O}, #{E}>;
                    fn parse(&self, response: &#{Response}<#{Bytes}>) -> Self::Output {
                         if #{json_errors}::is_error(&response) && response.status().as_u16() != ${httpTrait.code} {
                            self.parse_error(response)
                         } else {
                            self.parse_response(response)
                         }
                    }
                }""",
            "ParseStrict" to RuntimeType.parseStrict(symbolProvider.config().runtimeConfig),
            "O" to outputSymbol,
            "E" to operationShape.errorSymbol(symbolProvider),
            "Response" to RuntimeType.Http("Response"),
            "Bytes" to RuntimeType.Bytes,
            "json_errors" to RuntimeType.awsJsonErrors(runtimeConfig)
        )
    }

    private fun renderStreamingTraits(
        operationWriter: RustWriter,
        operationName: String,
        httpTrait: HttpTrait,
        outputSymbol: Symbol,
        operationShape: OperationShape
    ) {
        operationWriter.rustTemplate(
            """
                    impl #{ParseResponse}<#{SdkBody}> for $operationName {
                        type Output = Result<#{O}, #{E}>;
                        fn parse_unloaded(&self, response: &mut http::Response<#{SdkBody}>) -> Option<Self::Output> {
                            // This is an error, defer to the non-streaming parser
                            if #{json_errors}::is_error(&response) && response.status().as_u16() != ${httpTrait.code} {
                                return None;
                            }
                            Some(self.parse_response(response))
                        }
                        fn parse_loaded(&self, response: &http::Response<#{Bytes}>) -> Self::Output {
                            // if streaming, we only hit this case if its an error
                            self.parse_error(response)
                        }
                    }
                """,
            "ParseResponse" to RuntimeType.parseResponse(runtimeConfig),
            "O" to outputSymbol,
            "E" to operationShape.errorSymbol(symbolProvider),
            "SdkBody" to sdkBody,
            "Response" to RuntimeType.Http("Response"),
            "Bytes" to RuntimeType.Bytes,
            "json_errors" to RuntimeType.awsJsonErrors(runtimeConfig)
        )
    }

    override fun fromResponseImpl(implBlockWriter: RustWriter, operationShape: OperationShape) {
        val outputShape = operationShape.outputShape(model)
        val bodyId = outputShape.expectTrait(SyntheticOutputTrait::class.java).body
        val bodyShape = bodyId?.let { model.expectShape(bodyId, StructureShape::class.java) }
        val errorSymbol = operationShape.errorSymbol(symbolProvider)
        val jsonErrors = RuntimeType.awsJsonErrors(runtimeConfig)

        /* Render two functions:
            - An error parser `self.parse_error`
            - A happy-path parser: `Self::parse_response`
         */
        implBlockWriter.renderParseError(jsonErrors, operationShape, errorSymbol)
        fromResponseFun(implBlockWriter, operationShape) {
            withBlock("Ok({", "})") {
                renderShapeParser(
                    operationShape,
                    outputShape,
                    bodyShape,
                    httpIndex.getResponseBindings(operationShape),
                    errorSymbol
                )
            }
        }
    }

    private fun RustWriter.renderParseError(
        jsonErrors: RuntimeType,
        operationShape: OperationShape,
        errorSymbol: RuntimeType
    ) {
        rustBlock(
            "fn parse_error(&self, response: &http::Response<#T>) -> Result<#T, #T>",
            RuntimeType.Bytes,
            symbolProvider.toSymbol(operationShape.outputShape(model)),
            errorSymbol
        ) {
            rustTemplate(
                """
                        let body = #{sj}::from_slice(response.body().as_ref())
                            .unwrap_or_else(|_|#{sj}::json!({}));
                        let generic = #{aws_json_errors}::parse_generic_error(&response, &body);
                        """,
                "aws_json_errors" to jsonErrors, "sj" to RuntimeType.SJ
            )
            if (operationShape.errors.isNotEmpty()) {
                rustTemplate(
                    """

                        let error_code = match generic.code() {
                            Some(code) => code,
                            None => return Err(#{error_symbol}::unhandled(generic))
                        };""",
                    "error_symbol" to errorSymbol
                )
                withBlock("Err(match error_code {", "})") {
                    // approx:
                    /*
                            match error_code {
                                "Code1" => deserialize<Code1>(body),
                                "Code2" => deserialize<Code2>(body)
                            }
                         */
                    parseErrorVariants(operationShape, errorSymbol)
                }
            } else {
                rust("Err(#T::generic(generic))", errorSymbol)
            }
        }
    }

    /**
     * Generate a parser for [outputShape] given [bindings].
     *
     * The generated code is an expression with a return type of Result<[outputShape], [errorSymbol]> and can be
     * used for either error shapes or output shapes.
     */
    private fun RustWriter.renderShapeParser(
        operationShape: OperationShape,
        outputShape: StructureShape,
        bodyShape: StructureShape?,
        bindings: Map<String, HttpBinding>,
        errorSymbol: RuntimeType,
    ) {
        val httpBindingGenerator = ResponseBindingGenerator(protocolConfig, operationShape)
        Attribute.AllowUnusedMut.render(this)
        rust("let mut output = #T::default();", outputShape.builderSymbol(symbolProvider))
        // avoid non-usage warnings for response
        rust("let _ = response;")
        if (bodyShape != null && bindings.isNotEmpty()) {
            rustTemplate(
                """
                    let body_slice = response.body().as_ref();

                    let parsed_body: #{body} = if body_slice.is_empty() {
                        // To enable JSON parsing to succeed, replace an empty body
                        // with an empty JSON body. If a member was required, it will fail slightly later
                        // during the operation construction phase.
                        #{from_slice}(b"{}").map_err(#{err_symbol}::unhandled)?
                    } else {
                        #{from_slice}(response.body().as_ref()).map_err(#{err_symbol}::unhandled)?
                    };
                """,
                "body" to symbolProvider.toSymbol(bodyShape),
                "from_slice" to RuntimeType.SerdeJson("from_slice"),
                "err_symbol" to errorSymbol
            )
        }
        outputShape.members().forEach { member ->
            val parsedValue = renderBindingParser(
                bindings[member.memberName]!!,
                operationShape,
                httpBindingGenerator,
                bodyShape
            )
            withBlock("output = output.${member.setterName()}(", ");") {
                parsedValue(this)
            }
        }

        val err = if (StructureGenerator.fallibleBuilder(outputShape, symbolProvider)) {
            ".map_err(|s|${format(errorSymbol)}::unhandled(s))?"
        } else ""
        rust("output.build()$err")
    }

    private fun RustWriter.parseErrorVariants(
        operationShape: OperationShape,
        errorSymbol: RuntimeType,
    ) {
        operationShape.errors.forEach { error ->
            val variantName = symbolProvider.toSymbol(model.expectShape(error)).name
            val shape = model.expectShape(error, StructureShape::class.java)
            withBlock(
                "${error.name.dq()} => #1T { meta: generic, kind: #1TKind::$variantName({",
                "})},",
                errorSymbol
            ) {
                renderShapeParser(
                    operationShape = operationShape,
                    outputShape = shape,
                    bindings = httpIndex.getResponseBindings(shape),
                    bodyShape = shape,
                    errorSymbol = errorSymbol
                )
            }
        }
        write("_ => #T::generic(generic)", errorSymbol)
    }

    /**
     * Generate a parser & a parsed value converter for each output member of `operationShape`
     *
     * Returns a map with key = memberName, value = parsedValue
     */
    private fun renderBindingParser(
        binding: HttpBinding,
        operationShape: OperationShape,
        httpBindingGenerator: ResponseBindingGenerator,
        bodyShape: StructureShape?
    ): Writable {
        val errorSymbol = operationShape.errorSymbol(symbolProvider)
        val member = binding.member
        return when (binding.location) {
            HttpBinding.Location.HEADER -> writable {
                val fnName = httpBindingGenerator.generateDeserializeHeaderFn(binding)
                rust(
                    """
                        #T(response.headers())
                            .map_err(|_|#T::unhandled("Failed to parse ${member.memberName} from header `${binding.locationName}"))?
                        """,
                    fnName, errorSymbol
                )
            }
            HttpBinding.Location.DOCUMENT -> {
                check(bodyShape != null) {
                    "$bodyShape was null but a member specified document bindings. This is a bug."
                }
                // When there is a subset of fields present as the body of the response, we will create a variable
                // named `parsed_body`. Copy the field from parsed_body into the builder

                writable { rust("parsed_body.${symbolProvider.toMemberName(member)}") }
            }
            HttpBinding.Location.PAYLOAD -> {
                val docShapeHandler: RustWriter.(String) -> Unit = { body ->
                    rustTemplate(
                        """
                            #{serde_json}::from_slice::<#{doc_json}::DeserDoc>($body).map(|d|d.0).map_err(#{error_symbol}::unhandled)
                        """,
                        "doc_json" to RuntimeType.DocJson,
                        "serde_json" to CargoDependency.SerdeJson.asType(),
                        "error_symbol" to errorSymbol
                    )
                }
                val structureShapeHandler: RustWriter.(String) -> Unit = { body ->
                    rust("#T($body).map_err(#T::unhandled)", RuntimeType.SerdeJson("from_slice"), errorSymbol)
                }
                val deserializer = httpBindingGenerator.generateDeserializePayloadFn(
                    binding,
                    errorSymbol,
                    docHandler = docShapeHandler,
                    structuredHandler = structureShapeHandler
                )
                return if (binding.member.isStreaming(model)) {
                    writable { rust("#T(response.body_mut())?", deserializer) }
                } else {
                    writable { rust("#T(response.body().as_ref())?", deserializer) }
                }
            }
            HttpBinding.Location.RESPONSE_CODE -> writable {
                conditionalBlock("Some(", ")", symbolProvider.toSymbol(member).isOptional()) {
                    rust("response.status().as_u16() as _")
                }
            }
            HttpBinding.Location.PREFIX_HEADERS -> {
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
                logger.warning("Unhandled response binding type: ${binding.location}")
                TODO("Unexpected binding location: ${binding.location}")
            }
        }
    }

    private fun RustWriter.serializeViaSyntheticBody(
        self: String,
        inputShape: StructureShape,
        inputBody: StructureShape
    ) {
        val fnName = "synth_body_${inputBody.id.name.toSnakeCase()}"
        val bodySer = RuntimeType.forInlineFun(fnName, "operation_ser") {
            it.rustBlock(
                "pub fn $fnName(input: &#T) -> Result<#T, #T>",
                symbolProvider.toSymbol(inputShape),
                RuntimeType.sdkBody(runtimeConfig),
                runtimeConfig.operationBuildError()
            ) {
                withBlock("let body = ", ";") {
                    rustBlock("#T", symbolProvider.toSymbol(inputBody)) {
                        for (member in inputBody.members()) {
                            val name = protocolConfig.symbolProvider.toMemberName(member)
                            write("$name: &input.$name,")
                        }
                    }
                }
                rustTemplate(
                    """#{serde_json}::to_vec(&body)
                                  .map(#{SdkBody}::from)
                                  .map_err(|err|#{BuildError}::SerializationError(err.into()))""",
                    "serde_json" to CargoDependency.SerdeJson.asType(),
                    "BuildError" to runtimeConfig.operationBuildError(),
                    "SdkBody" to sdkBody
                )
            }
        }
        rust("#T(&$self)?", bodySer)
    }

    override fun RustWriter.body(self: String, operationShape: OperationShape): BodyMetadata {
        val inputShape = operationShape.inputShape(model)
        val inputBody = inputShape.expectTrait(SyntheticInputTrait::class.java).body?.let {
            model.expectShape(
                it,
                StructureShape::class.java
            )
        }
        if (inputBody != null) {
            serializeViaSyntheticBody(self, inputShape, inputBody)
            return BodyMetadata(takesOwnership = false)
        }
        val bindings = httpIndex.getRequestBindings(operationShape).toList()
        val payloadMemberName: String? =
            bindings.firstOrNull { (_, binding) -> binding.location == HttpBinding.Location.PAYLOAD }?.first
        if (payloadMemberName == null) {
            rustTemplate("""#{SdkBody}::from("")""", "SdkBody" to sdkBody)
            return BodyMetadata(takesOwnership = false)
        } else {
            val member = inputShape.expectMember(payloadMemberName)
            return serializeViaPayload(member)
        }
    }

    private fun RustWriter.serializeViaPayload(member: MemberShape): BodyMetadata {
        val fnName = "ser_payload_${member.container.name.toSnakeCase()}"
        val targetShape = model.expectShape(member.target)
        val bodyMetadata: BodyMetadata = RustWriter.root().renderPayload(targetShape, "payload")
        val ref = when (bodyMetadata.takesOwnership) {
            true -> ""
            false -> "&"
        }
        val serializer = RuntimeType.forInlineFun(fnName, "operation_ser") {
            it.rustBlock(
                "pub fn $fnName(payload: $ref#T) -> Result<#T, #T>",
                symbolProvider.toSymbol(member),
                sdkBody,
                runtimeConfig.operationBuildError()
            ) {
                // If this targets a member & the member is None, return an empty vec
                val ref = when (bodyMetadata.takesOwnership) {
                    false -> ".as_ref()"
                    true -> ""
                }

                if (symbolProvider.toSymbol(member).isOptional()) {
                    rustTemplate(
                        """
                        let payload = match payload$ref {
                            Some(t) => t,
                            None => return Ok(#{SdkBody}::from(""))
                        };""",
                        "SdkBody" to sdkBody
                    )
                }
                // When the body is a streaming blob it _literally_ is a SdkBody already
                // mute this clippy warning to make the codegen a little simpler
                Attribute.Custom("allow(clippy::useless_conversion)").render(this)
                withBlock("Ok(#T::from(", "))", sdkBody) {
                    renderPayload(targetShape, "payload")
                }
            }
        }
        rust("#T($ref self.${symbolProvider.toMemberName(member)})?", serializer)
        return bodyMetadata
    }

    private fun RustWriter.renderPayload(
        targetShape: Shape,
        payloadName: String,
    ): BodyMetadata {
        val serdeToVec = RuntimeType.SerdeJson("to_vec")
        return when (targetShape) {
            // Write the raw string to the payload
            is StringShape -> {
                if (targetShape.hasTrait(EnumTrait::class.java)) {
                    rust("$payloadName.as_str()")
                } else {
                    rust("""$payloadName.to_string()""")
                }
                BodyMetadata(takesOwnership = false)
            }

            // This works for streaming & non streaming blobs because they both have `into_inner()` which
            // can be converted into an SDK body!
            is BlobShape -> {
                // Write the raw blob to the payload
                rust("$payloadName.into_inner()")
                BodyMetadata(takesOwnership = true)
            }
            is StructureShape, is UnionShape -> {
                // JSON serialize the structure or union targetted
                rust(
                    """#T(&$payloadName).map_err(|err|#T::SerializationError(err.into()))?""",
                    serdeToVec, runtimeConfig.operationBuildError()
                )
                BodyMetadata(takesOwnership = false)
            }
            is DocumentShape -> {
                rustTemplate(
                    """#{to_vec}(&#{doc_json}::SerDoc(&$payloadName)).map_err(|err|#{BuildError}::SerializationError(err.into()))?""",
                    "to_vec" to serdeToVec,
                    "doc_json" to RuntimeType.DocJson,
                    "BuildError" to runtimeConfig.operationBuildError()
                )
                BodyMetadata(takesOwnership = false)
            }
            else -> TODO("Unexpected payload target type")
        }
    }

    override fun toHttpRequestImpl(
        implBlockWriter: RustWriter,
        operationShape: OperationShape,
        inputShape: StructureShape
    ) {
        val httpTrait = operationShape.expectTrait(HttpTrait::class.java)

        val httpBindingGenerator = RequestBindingGenerator(
            model,
            symbolProvider,
            runtimeConfig,
            implBlockWriter,
            operationShape,
            inputShape,
            httpTrait
        )
        val contentType =
            httpIndex.determineRequestContentType(operationShape, "application/json").orElse("application/json")
        httpBindingGenerator.renderUpdateHttpBuilder(implBlockWriter)
        httpBuilderFun(implBlockWriter) {
            rust(
                """
            let builder = #T::new();
            let builder = builder.header("Content-Type", ${contentType.dq()});
            self.update_http_builder(builder)
            """,
                requestBuilder
            )
        }
    }
}
*/
