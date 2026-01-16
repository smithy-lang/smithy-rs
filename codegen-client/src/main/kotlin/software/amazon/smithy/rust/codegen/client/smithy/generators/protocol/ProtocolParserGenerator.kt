/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.protocol

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.http.ResponseBindingGenerator
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
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
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.customize.writeCustomizations
import software.amazon.smithy.rust.codegen.core.smithy.generators.setterName
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.smithy.protocols.HttpBindingDescriptor
import software.amazon.smithy.rust.codegen.core.smithy.protocols.HttpLocation
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolFunctions
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.StructuredDataParserGenerator
import software.amazon.smithy.rust.codegen.core.smithy.transformers.operationErrors
import software.amazon.smithy.rust.codegen.core.util.UNREACHABLE
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.errorMessageMember
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.isStreaming
import software.amazon.smithy.rust.codegen.core.util.outputShape

class ProtocolParserGenerator(
    private val codegenContext: ClientCodegenContext,
    private val protocol: Protocol,
) {
    private val model = codegenContext.model
    private val httpBindingResolver = protocol.httpBindingResolver
    private val protocolFunctions = ProtocolFunctions(codegenContext)
    private val symbolProvider: RustSymbolProvider = codegenContext.symbolProvider

    private val codegenScope =
        arrayOf(
            "Bytes" to RuntimeType.Bytes,
            "Headers" to RuntimeType.headers(codegenContext.runtimeConfig),
            "Response" to RuntimeType.smithyRuntimeApi(codegenContext.runtimeConfig).resolve("http::Response"),
            "http" to RuntimeType.Http0x,
            "SdkBody" to RuntimeType.sdkBody(codegenContext.runtimeConfig),
        )

    fun parseResponseFn(
        operationShape: OperationShape,
        customizations: List<OperationCustomization>,
    ): RuntimeType {
        val outputShape = operationShape.outputShape(model)
        val outputSymbol = symbolProvider.toSymbol(outputShape)
        val errorSymbol = symbolProvider.symbolForOperationError(operationShape)
        return protocolFunctions.deserializeFn(operationShape, fnNameSuffix = "http_response") { fnName ->
            Attribute.AllowClippyUnnecessaryWraps.render(this)
            rustBlockTemplate(
                "pub fn $fnName(_response_status: u16, _response_headers: &#{Headers}, _response_body: &[u8]) -> std::result::Result<#{O}, #{E}>",
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

    fun parseErrorFn(
        operationShape: OperationShape,
        customizations: List<OperationCustomization>,
    ): RuntimeType {
        val outputShape = operationShape.outputShape(model)
        val outputSymbol = symbolProvider.toSymbol(outputShape)
        val errorSymbol = symbolProvider.symbolForOperationError(operationShape)
        return protocolFunctions.deserializeFn(operationShape, fnNameSuffix = "http_error") { fnName ->
            Attribute.AllowClippyUnnecessaryWraps.render(this)
            rustBlockTemplate(
                "pub fn $fnName(_response_status: u16, _response_headers: &#{Headers}, _response_body: &[u8]) -> std::result::Result<#{O}, #{E}>",
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
                                            listOf(
                                                object : OperationCustomization() {
                                                    override fun section(section: OperationSection): Writable =
                                                        {
                                                            if (section is OperationSection.MutateOutput) {
                                                                rust("let output = output.meta(generic);")
                                                            }
                                                        }
                                                },
                                            ),
                                        )
                                    }
                                }
                                val errorMessageMember = errorShape.errorMessageMember()
                                // If the message member is optional and wasn't set, we set a generic error message.
                                if (errorMessageMember != null) {
                                    val symbol = symbolProvider.toSymbol(errorMessageMember)
                                    if (symbol.isOptional()) {
                                        rust(
                                            """
                                            if tmp.message.is_none() {
                                                tmp.message = _error_message;
                                            }
                                            """,
                                        )
                                    }
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

    fun parseStreamingResponseFn(
        operationShape: OperationShape,
        customizations: List<OperationCustomization>,
    ): RuntimeType {
        val outputShape = operationShape.outputShape(model)
        val outputSymbol = symbolProvider.toSymbol(outputShape)
        val errorSymbol = symbolProvider.symbolForOperationError(operationShape)
        return protocolFunctions.deserializeFn(operationShape, fnNameSuffix = "http_response") { fnName ->
            Attribute.AllowClippyUnnecessaryWraps.render(this)
            rustBlockTemplate(
                "pub fn $fnName(response: &mut #{Response}) -> std::result::Result<#{O}, #{E}>",
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

    private fun RustWriter.renderShapeParser(
        operationShape: OperationShape,
        outputShape: StructureShape,
        bindings: List<HttpBindingDescriptor>,
        errorSymbol: Symbol,
        customizations: List<OperationCustomization>,
    ) {
        val httpBindingGenerator = ResponseBindingGenerator(protocol, codegenContext, operationShape)
        val structuredDataParser = protocol.structuredDataParser()
        Attribute.AllowUnusedMut.render(this)
        rust("let mut output = #T::default();", symbolProvider.symbolForBuilder(outputShape))
        if (outputShape.id == operationShape.output.get()) {
            structuredDataParser.operationParser(operationShape)?.also { parser ->
                // Don't deserialize non-event stream members for an event stream operation with RPC bound protocols,
                // as they need to be deserialized from payload in the first frame of event stream.
                // See https://smithy.io/2.0/spec/streaming.html#initial-response
                if (codegenContext.protocolImpl?.httpBindingResolver?.handlesEventStreamInitialResponse(operationShape) != true) {
                    rust(
                        "output = #T(_response_body, output).map_err(#T::unhandled)?;",
                        parser,
                        errorSymbol,
                    )
                }
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

        val mapErr =
            writable {
                rust("#T::unhandled", errorSymbol)
            }

        writeCustomizations(
            customizations,
            OperationSection.MutateOutput(customizations, operationShape, "_response_headers"),
        )
        codegenContext.builderInstantiator().finalizeBuilder("output", outputShape, mapErr)(this)
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
            HttpLocation.HEADER ->
                writable {
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
                val deserializer =
                    httpBindingGenerator.generateDeserializePayloadFn(
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
            HttpLocation.RESPONSE_CODE ->
                writable {
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
