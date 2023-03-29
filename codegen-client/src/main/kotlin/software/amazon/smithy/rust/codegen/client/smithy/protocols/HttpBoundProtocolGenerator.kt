/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.protocols

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.generators.http.ResponseBindingGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.ClientProtocolGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.MakeOperationGenerator
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.assignment
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.core.smithy.customize.writeCustomizations
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.setterName
import software.amazon.smithy.rust.codegen.core.smithy.protocols.HttpBindingDescriptor
import software.amazon.smithy.rust.codegen.core.smithy.protocols.HttpBoundProtocolPayloadGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.HttpLocation
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolFunctions
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.StructuredDataParserGenerator
import software.amazon.smithy.rust.codegen.core.smithy.transformers.operationErrors
import software.amazon.smithy.rust.codegen.core.util.UNREACHABLE
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.errorMessageMember
import software.amazon.smithy.rust.codegen.core.util.hasStreamingMember
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.isStreaming
import software.amazon.smithy.rust.codegen.core.util.outputShape

class HttpBoundProtocolGenerator(
    codegenContext: ClientCodegenContext,
    protocol: Protocol,
) : ClientProtocolGenerator(
    codegenContext,
    protocol,
    MakeOperationGenerator(
        codegenContext,
        protocol,
        HttpBoundProtocolPayloadGenerator(codegenContext, protocol),
        public = true,
        includeDefaultPayloadHeaders = true,
    ),
    HttpBoundProtocolTraitImplGenerator(codegenContext, protocol),
)

open class HttpBoundProtocolTraitImplGenerator(
    private val codegenContext: ClientCodegenContext,
    private val protocol: Protocol,
) {
    private val symbolProvider = codegenContext.symbolProvider
    private val model = codegenContext.model
    private val runtimeConfig = codegenContext.runtimeConfig
    private val httpBindingResolver = protocol.httpBindingResolver
    private val protocolFunctions = ProtocolFunctions(codegenContext)

    private val codegenScope = arrayOf(
        "ParseStrict" to RuntimeType.parseStrictResponse(runtimeConfig),
        "ParseResponse" to RuntimeType.parseHttpResponse(runtimeConfig),
        "http" to RuntimeType.Http,
        "operation" to RuntimeType.operationModule(runtimeConfig),
        "Bytes" to RuntimeType.Bytes,
        "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
    )
    private val orchestratorCodegenScope by lazy {
        val interceptorContext =
            CargoDependency.smithyRuntimeApi(runtimeConfig).toType().resolve("client::interceptors::context")
        val orchestrator =
            CargoDependency.smithyRuntimeApi(runtimeConfig).toType().resolve("client::orchestrator")
        arrayOf(
            "Error" to interceptorContext.resolve("Error"),
            "HttpResponse" to orchestrator.resolve("HttpResponse"),
            "Instrument" to CargoDependency.Tracing.toType().resolve("Instrument"),
            "Output" to interceptorContext.resolve("Output"),
            "OutputOrError" to interceptorContext.resolve("OutputOrError"),
            "ResponseDeserializer" to orchestrator.resolve("ResponseDeserializer"),
            "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
            "SdkError" to RuntimeType.sdkError(runtimeConfig),
            "TypedBox" to CargoDependency.smithyRuntimeApi(runtimeConfig).toType().resolve("type_erasure::TypedBox"),
            "debug_span" to RuntimeType.Tracing.resolve("debug_span"),
            "type_erase_result" to typeEraseResult(),
        )
    }

    open fun generateTraitImpls(
        operationWriter: RustWriter,
        operationShape: OperationShape,
        customizations: List<OperationCustomization>,
    ) {
        val outputSymbol = symbolProvider.toSymbol(operationShape.outputShape(model))
        val operationName = symbolProvider.toSymbol(operationShape).name

        // For streaming response bodies, we need to generate a different implementation of the parse traits.
        // These will first offer the streaming input to the parser & potentially read the body into memory
        // if an error occurred or if the streaming parser indicates that it needs the full data to proceed.
        val streaming = operationShape.outputShape(model).hasStreamingMember(model)
        if (streaming) {
            operationWriter.renderStreamingTraits(operationName, outputSymbol, operationShape, customizations)
        } else {
            operationWriter.renderNonStreamingTraits(operationName, outputSymbol, operationShape, customizations)
        }

        if (codegenContext.settings.codegenConfig.enableNewSmithyRuntime) {
            operationWriter.renderRuntimeTraits(operationName, outputSymbol, operationShape, customizations, streaming)
        }
    }

    private fun typeEraseResult(): RuntimeType = ProtocolFunctions.crossOperationFn("type_erase_result") { fnName ->
        rustTemplate(
            """
            pub(crate) fn $fnName<O, E>(result: Result<O, E>) -> Result<#{Output}, #{Error}>
            where
                O: Send + Sync + 'static,
                E: Send + Sync + 'static,
            {
                result.map(|output| #{TypedBox}::new(output).erase())
                    .map_err(|error| #{TypedBox}::new(error).erase())
            }
            """,
            *orchestratorCodegenScope,
        )
    }

    private fun RustWriter.renderRuntimeTraits(
        operationName: String?,
        outputSymbol: Symbol,
        operationShape: OperationShape,
        customizations: List<OperationCustomization>,
        streaming: Boolean,
    ) {
        rustTemplate(
            """
            impl #{ResponseDeserializer} for $operationName {
                #{deserialize_streaming}

                fn deserialize_nonstreaming(&self, response: &#{HttpResponse}) -> #{OutputOrError} {
                    #{deserialize_nonstreaming}
                }
            }
            """,
            *orchestratorCodegenScope,
            "O" to outputSymbol,
            "E" to symbolProvider.symbolForOperationError(operationShape),
            "deserialize_streaming" to writable {
                if (streaming) {
                    deserializeStreaming(operationShape, customizations)
                }
            },
            "deserialize_nonstreaming" to writable {
                when (streaming) {
                    true -> deserializeStreamingError(operationShape, customizations)
                    else -> deserializeNonStreaming(operationShape, customizations)
                }
            },
        )
    }

    private fun RustWriter.deserializeStreaming(
        operationShape: OperationShape,
        customizations: List<OperationCustomization>,
    ) {
        val successCode = httpBindingResolver.httpTrait(operationShape).code
        rustTemplate(
            """
            fn deserialize_streaming(&self, response: &mut #{HttpResponse}) -> Option<#{OutputOrError}> {
                #{BeforeParseResponse}

                // If this is an error, defer to the non-streaming parser
                if !response.status().is_success() && response.status().as_u16() != $successCode {
                    return None;
                }
                Some(#{type_erase_result}(#{parse_streaming_response}(response)))
            }
            """,
            *orchestratorCodegenScope,
            "parse_streaming_response" to parseStreamingResponse(operationShape, customizations),
            "BeforeParseResponse" to writable {
                writeCustomizations(customizations, OperationSection.BeforeParseResponse(customizations, "response"))
            },
        )
    }

    private fun RustWriter.deserializeStreamingError(
        operationShape: OperationShape,
        customizations: List<OperationCustomization>,
    ) {
        rustTemplate(
            """
            // For streaming operations, we only hit this case if its an error
            let body = response.body().bytes().expect("body loaded");
            #{type_erase_result}(#{parse_error}(response.status().as_u16(), response.headers(), body))
            """,
            *orchestratorCodegenScope,
            "parse_error" to parseError(operationShape, customizations),
        )
    }

    private fun RustWriter.deserializeNonStreaming(
        operationShape: OperationShape,
        customizations: List<OperationCustomization>,
    ) {
        val successCode = httpBindingResolver.httpTrait(operationShape).code
        rustTemplate(
            """
            let (success, status) = (response.status().is_success(), response.status().as_u16());
            let headers = response.headers();
            let body = response.body().bytes().expect("body loaded");
             #{BeforeParseResponse}
            let parse_result = if !success && status != $successCode {
                #{parse_error}(status, headers, body)
            } else {
                #{parse_response}(status, headers, body)
            };
            #{type_erase_result}(parse_result)
            """,
            *orchestratorCodegenScope,
            "parse_error" to parseError(operationShape, customizations),
            "parse_response" to parseResponse(operationShape, customizations),
            "BeforeParseResponse" to writable {
                writeCustomizations(customizations, OperationSection.BeforeParseResponse(customizations, "response"))
            },
        )
    }

    // TODO(enableNewSmithyRuntime): Delete this when cleaning up `enableNewSmithyRuntime`
    private fun RustWriter.renderNonStreamingTraits(
        operationName: String?,
        outputSymbol: Symbol,
        operationShape: OperationShape,
        customizations: List<OperationCustomization>,
    ) {
        val successCode = httpBindingResolver.httpTrait(operationShape).code
        val localScope = arrayOf(
            "O" to outputSymbol,
            "E" to symbolProvider.symbolForOperationError(operationShape),
            "parse_error" to parseError(operationShape, customizations),
            "parse_response" to parseResponse(operationShape, customizations),
            "BeforeParseResponse" to writable {
                writeCustomizations(customizations, OperationSection.BeforeParseResponse(customizations, "response"))
            },
        )
        rustTemplate(
            """
            impl #{ParseStrict} for $operationName {
                type Output = std::result::Result<#{O}, #{E}>;
                fn parse(&self, response: &#{http}::Response<#{Bytes}>) -> Self::Output {
                     let (success, status) = (response.status().is_success(), response.status().as_u16());
                     let headers = response.headers();
                     let body = response.body().as_ref();
                     #{BeforeParseResponse}
                     if !success && status != $successCode {
                        #{parse_error}(status, headers, body)
                     } else {
                        #{parse_response}(status, headers, body)
                     }
                }
            }""",
            *codegenScope,
            *localScope,
        )
    }

    // TODO(enableNewSmithyRuntime): Delete this when cleaning up `enableNewSmithyRuntime`
    private fun RustWriter.renderStreamingTraits(
        operationName: String,
        outputSymbol: Symbol,
        operationShape: OperationShape,
        customizations: List<OperationCustomization>,
    ) {
        val successCode = httpBindingResolver.httpTrait(operationShape).code
        rustTemplate(
            """
            impl #{ParseResponse} for $operationName {
                type Output = std::result::Result<#{O}, #{E}>;
                fn parse_unloaded(&self, response: &mut #{operation}::Response) -> Option<Self::Output> {
                     #{BeforeParseResponse}
                    // This is an error, defer to the non-streaming parser
                    if !response.http().status().is_success() && response.http().status().as_u16() != $successCode {
                        return None;
                    }
                    Some(#{parse_streaming_response}(response))
                }
                fn parse_loaded(&self, response: &#{http}::Response<#{Bytes}>) -> Self::Output {
                    // if streaming, we only hit this case if its an error
                    #{parse_error}(response.status().as_u16(), response.headers(), response.body().as_ref())
                }
            }
            """,
            "O" to outputSymbol,
            "E" to symbolProvider.symbolForOperationError(operationShape),
            "parse_streaming_response" to parseStreamingResponseNoRt(operationShape, customizations),
            "parse_error" to parseError(operationShape, customizations),
            "BeforeParseResponse" to writable {
                writeCustomizations(customizations, OperationSection.BeforeParseResponse(customizations, "response"))
            },
            *codegenScope,
        )
    }

    private fun parseError(operationShape: OperationShape, customizations: List<OperationCustomization>): RuntimeType {
        val outputShape = operationShape.outputShape(model)
        val outputSymbol = symbolProvider.toSymbol(outputShape)
        val errorSymbol = symbolProvider.symbolForOperationError(operationShape)
        return protocolFunctions.deserializeFn(operationShape, fnNameSuffix = "http_error") { fnName ->
            Attribute.AllowClippyUnnecessaryWraps.render(this)
            rustBlockTemplate(
                "pub fn $fnName(_response_status: u16, _response_headers: &#{http}::header::HeaderMap, _response_body: &[u8]) -> std::result::Result<#{O}, #{E}>",
                *codegenScope,
                "O" to outputSymbol,
                "E" to errorSymbol,
            ) {
                Attribute.AllowUnusedMut.render(this)
                rust(
                    "let mut generic_builder = #T(_response_status, _response_headers, _response_body).map_err(#T::unhandled)?;",
                    protocol.parseHttpErrorMetadata(operationShape),
                    errorSymbol,
                )
                writeCustomizations(
                    customizations,
                    OperationSection.PopulateErrorMetadataExtras(
                        customizations,
                        "generic_builder",
                        "_response_status",
                        "_response_headers",
                    ),
                )
                rust("let generic = generic_builder.build();")
                if (operationShape.operationErrors(model).isNotEmpty()) {
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
                        val errors = operationShape.operationErrors(model)
                        errors.forEach { error ->
                            val errorShape = model.expectShape(error.id, StructureShape::class.java)
                            val variantName = symbolProvider.toSymbol(model.expectShape(error.id)).name
                            val errorCode = httpBindingResolver.errorCode(errorShape).dq()
                            withBlock(
                                "$errorCode => #1T::$variantName({",
                                "}),",
                                errorSymbol,
                            ) {
                                Attribute.AllowUnusedMut.render(this)
                                assignment("mut tmp") {
                                    rustBlock("") {
                                        renderShapeParser(
                                            operationShape,
                                            errorShape,
                                            httpBindingResolver.errorResponseBindings(errorShape),
                                            errorSymbol,
                                            listOf(object : OperationCustomization() {
                                                override fun section(section: OperationSection): Writable = writable {
                                                    if (section is OperationSection.MutateOutput) {
                                                        rust("let output = output.meta(generic);")
                                                    }
                                                }
                                            },
                                            ),
                                        )
                                    }
                                }
                                if (errorShape.errorMessageMember() != null) {
                                    rust(
                                        """
                                        if tmp.message.is_none() {
                                            tmp.message = _error_message;
                                        }
                                        """,
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

    private fun parseStreamingResponse(operationShape: OperationShape, customizations: List<OperationCustomization>): RuntimeType {
        val outputShape = operationShape.outputShape(model)
        val outputSymbol = symbolProvider.toSymbol(outputShape)
        val errorSymbol = symbolProvider.symbolForOperationError(operationShape)
        return protocolFunctions.deserializeFn(operationShape, fnNameSuffix = "http_response") { fnName ->
            Attribute.AllowClippyUnnecessaryWraps.render(this)
            rustBlockTemplate(
                "pub fn $fnName(response: &mut #{http}::Response<#{SdkBody}>) -> std::result::Result<#{O}, #{E}>",
                *codegenScope,
                "O" to outputSymbol,
                "E" to errorSymbol,
            ) {
                rustTemplate(
                    """
                    let mut _response_body = #{SdkBody}::taken();
                    std::mem::swap(&mut _response_body, response.body_mut());
                    let _response_body = &mut _response_body;

                    let _response_status = response.status().as_u16();
                    let _response_headers = response.headers();
                    """,
                    *codegenScope,
                )
                withBlock("Ok({", "})") {
                    renderShapeParser(
                        operationShape,
                        outputShape,
                        httpBindingResolver.responseBindings(operationShape),
                        errorSymbol,
                        customizations,
                    )
                }
            }
        }
    }

    // TODO(enableNewSmithyRuntime): Delete this when cleaning up `enableNewSmithyRuntime`
    private fun parseStreamingResponseNoRt(operationShape: OperationShape, customizations: List<OperationCustomization>): RuntimeType {
        val outputShape = operationShape.outputShape(model)
        val outputSymbol = symbolProvider.toSymbol(outputShape)
        val errorSymbol = symbolProvider.symbolForOperationError(operationShape)
        return protocolFunctions.deserializeFn(operationShape, fnNameSuffix = "http_response_") { fnName ->
            Attribute.AllowClippyUnnecessaryWraps.render(this)
            rustBlockTemplate(
                "pub fn $fnName(op_response: &mut #{operation}::Response) -> std::result::Result<#{O}, #{E}>",
                *codegenScope,
                "O" to outputSymbol,
                "E" to errorSymbol,
            ) {
                // Not all implementations will use the property bag, but some will
                Attribute.AllowUnusedVariables.render(this)
                rust("let (response, properties) = op_response.parts_mut();")
                rustTemplate(
                    """
                    let mut _response_body = #{SdkBody}::taken();
                    std::mem::swap(&mut _response_body, response.body_mut());
                    let _response_body = &mut _response_body;

                    let _response_status = response.status().as_u16();
                    let _response_headers = response.headers();
                    """,
                    *codegenScope,
                )
                withBlock("Ok({", "})") {
                    renderShapeParser(
                        operationShape,
                        outputShape,
                        httpBindingResolver.responseBindings(operationShape),
                        errorSymbol,
                        customizations,
                    )
                }
            }
        }
    }

    private fun parseResponse(operationShape: OperationShape, customizations: List<OperationCustomization>): RuntimeType {
        val outputShape = operationShape.outputShape(model)
        val outputSymbol = symbolProvider.toSymbol(outputShape)
        val errorSymbol = symbolProvider.symbolForOperationError(operationShape)
        return protocolFunctions.deserializeFn(operationShape, fnNameSuffix = "http_response") { fnName ->
            Attribute.AllowClippyUnnecessaryWraps.render(this)
            rustBlockTemplate(
                "pub fn $fnName(_response_status: u16, _response_headers: &#{http}::header::HeaderMap, _response_body: &[u8]) -> std::result::Result<#{O}, #{E}>",
                *codegenScope,
                "O" to outputSymbol,
                "E" to errorSymbol,
            ) {
                withBlock("Ok({", "})") {
                    renderShapeParser(
                        operationShape,
                        outputShape,
                        httpBindingResolver.responseBindings(operationShape),
                        errorSymbol,
                        customizations,
                    )
                }
            }
        }
    }

    private fun RustWriter.renderShapeParser(
        operationShape: OperationShape,
        outputShape: StructureShape,
        bindings: List<HttpBindingDescriptor>,
        errorSymbol: Symbol,
        customizations: List<OperationCustomization>,
    ) {
        val httpBindingGenerator = ResponseBindingGenerator(protocol, codegenContext, operationShape)
        val structuredDataParser = protocol.structuredDataParser(operationShape)
        Attribute.AllowUnusedMut.render(this)
        rust("let mut output = #T::default();", symbolProvider.symbolForBuilder(outputShape))
        if (outputShape.id == operationShape.output.get()) {
            structuredDataParser.operationParser(operationShape)?.also { parser ->
                rust(
                    "output = #T(_response_body, output).map_err(#T::unhandled)?;",
                    parser,
                    errorSymbol,
                )
            }
        } else {
            check(outputShape.hasTrait<ErrorTrait>()) { "should only be called on outputs or errors $outputShape" }
            structuredDataParser.errorParser(outputShape)?.also { parser ->
                rust(
                    "output = #T(_response_body, output).map_err(#T::unhandled)?;",
                    parser, errorSymbol,
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

        val err = if (BuilderGenerator.hasFallibleBuilder(outputShape, symbolProvider)) {
            ".map_err(${format(errorSymbol)}::unhandled)?"
        } else {
            ""
        }

        writeCustomizations(
            customizations,
            OperationSection.MutateOutput(customizations, operationShape, "_response_headers"),
        )

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
        val errorSymbol = symbolProvider.symbolForOperationError(operationShape)
        val member = binding.member
        return when (binding.location) {
            HttpLocation.HEADER -> writable {
                val fnName = httpBindingGenerator.generateDeserializeHeaderFn(binding)
                rust(
                    """
                    #T(_response_headers)
                        .map_err(|_|#T::unhandled("Failed to parse ${member.memberName} from header `${binding.locationName}"))?
                    """,
                    fnName, errorSymbol,
                )
            }
            HttpLocation.DOCUMENT -> {
                // document is handled separately
                null
            }
            HttpLocation.PAYLOAD -> {
                val payloadParser: RustWriter.(String) -> Unit = { body ->
                    rust("#T($body).map_err(#T::unhandled)", structuredDataParser.payloadParser(member), errorSymbol)
                }
                val deserializer = httpBindingGenerator.generateDeserializePayloadFn(
                    binding,
                    errorSymbol,
                    payloadParser = payloadParser,
                )
                return if (binding.member.isStreaming(model)) {
                    writable { rust("Some(#T(_response_body)?)", deserializer) }
                } else {
                    writable { rust("#T(_response_body)?", deserializer) }
                }
            }
            HttpLocation.RESPONSE_CODE -> writable {
                rust("Some(_response_status as _)")
            }
            HttpLocation.PREFIX_HEADERS -> {
                val sym = httpBindingGenerator.generateDeserializePrefixHeaderFn(binding)
                writable {
                    rustTemplate(
                        """
                        #{deser}(_response_headers)
                             .map_err(|_|
                                #{err}::unhandled("Failed to parse ${member.memberName} from prefix header `${binding.locationName}")
                             )?
                        """,
                        "deser" to sym, "err" to errorSymbol,
                    )
                }
            }
            else -> {
                UNREACHABLE("Unexpected binding location: ${binding.location}")
            }
        }
    }
}
