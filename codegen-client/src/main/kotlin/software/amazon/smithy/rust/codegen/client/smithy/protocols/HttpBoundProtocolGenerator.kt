/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.protocols

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.rust.codegen.client.smithy.generators.http.ResponseBindingGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.ClientProtocolGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.MakeOperationGenerator
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.assignment
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.core.smithy.customize.writeCustomizations
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.builderSymbol
import software.amazon.smithy.rust.codegen.core.smithy.generators.error.errorSymbol
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolTraitImplGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.setterName
import software.amazon.smithy.rust.codegen.core.smithy.protocols.HttpBindingDescriptor
import software.amazon.smithy.rust.codegen.core.smithy.protocols.HttpBoundProtocolPayloadGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.HttpLocation
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.StructuredDataParserGenerator
import software.amazon.smithy.rust.codegen.core.smithy.transformers.operationErrors
import software.amazon.smithy.rust.codegen.core.util.UNREACHABLE
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.errorMessageMember
import software.amazon.smithy.rust.codegen.core.util.hasStreamingMember
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.isStreaming
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase

class HttpBoundProtocolGenerator(
    codegenContext: CodegenContext,
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

class HttpBoundProtocolTraitImplGenerator(
    private val codegenContext: CodegenContext,
    private val protocol: Protocol,
) : ProtocolTraitImplGenerator {
    private val symbolProvider = codegenContext.symbolProvider
    private val model = codegenContext.model
    private val runtimeConfig = codegenContext.runtimeConfig
    private val httpBindingResolver = protocol.httpBindingResolver
    private val operationDeserModule = RustModule.private("operation_deser")

    private val codegenScope = arrayOf(
        "ParseStrict" to RuntimeType.parseStrictResponse(runtimeConfig),
        "ParseResponse" to RuntimeType.parseHttpResponse(runtimeConfig),
        "http" to RuntimeType.Http,
        "operation" to RuntimeType.operationModule(runtimeConfig),
        "Bytes" to RuntimeType.Bytes,
    )

    override fun generateTraitImpls(
        operationWriter: RustWriter,
        operationShape: OperationShape,
        customizations: List<OperationCustomization>,
    ) {
        val outputSymbol = symbolProvider.toSymbol(operationShape.outputShape(model))
        val operationName = symbolProvider.toSymbol(operationShape).name

        // For streaming response bodies, we need to generate a different implementation of the parse traits.
        // These will first offer the streaming input to the parser & potentially read the body into memory
        // if an error occurred or if the streaming parser indicates that it needs the full data to proceed.
        if (operationShape.outputShape(model).hasStreamingMember(model)) {
            with(operationWriter) {
                renderStreamingTraits(operationName, outputSymbol, operationShape, customizations)
            }
        } else {
            with(operationWriter) {
                renderNonStreamingTraits(operationName, outputSymbol, operationShape, customizations)
            }
        }
    }

    private fun RustWriter.renderNonStreamingTraits(
        operationName: String?,
        outputSymbol: Symbol,
        operationShape: OperationShape,
        customizations: List<OperationCustomization>,
    ) {
        val successCode = httpBindingResolver.httpTrait(operationShape).code
        rustTemplate(
            """
            impl #{ParseStrict} for $operationName {
                type Output = std::result::Result<#{O}, #{E}>;
                fn parse(&self, response: &#{http}::Response<#{Bytes}>) -> Self::Output {
                     if !response.status().is_success() && response.status().as_u16() != $successCode {
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
            "parse_response" to parseResponse(operationShape, customizations),
        )
    }

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
                    // This is an error, defer to the non-streaming parser
                    if !response.http().status().is_success() && response.http().status().as_u16() != $successCode {
                        return None;
                    }
                    Some(#{parse_streaming_response}(response))
                }
                fn parse_loaded(&self, response: &#{http}::Response<#{Bytes}>) -> Self::Output {
                    // if streaming, we only hit this case if its an error
                    #{parse_error}(response)
                }
            }
            """,
            "O" to outputSymbol,
            "E" to operationShape.errorSymbol(symbolProvider),
            "parse_streaming_response" to parseStreamingResponse(operationShape, customizations),
            "parse_error" to parseError(operationShape),
            *codegenScope,
        )
    }

    private fun parseError(operationShape: OperationShape): RuntimeType {
        val fnName = "parse_${operationShape.id.name.toSnakeCase()}_error"
        val outputShape = operationShape.outputShape(model)
        val outputSymbol = symbolProvider.toSymbol(outputShape)
        val errorSymbol = operationShape.errorSymbol(symbolProvider)
        return RuntimeType.forInlineFun(fnName, operationDeserModule) {
            Attribute.AllowClippyUnnecessaryWraps.render(this)
            rustBlockTemplate(
                "pub fn $fnName(response: &#{http}::Response<#{Bytes}>) -> std::result::Result<#{O}, #{E}>",
                *codegenScope,
                "O" to outputSymbol,
                "E" to errorSymbol,
            ) {
                rust(
                    "let generic = #T(response).map_err(#T::unhandled)?;",
                    protocol.parseHttpGenericError(operationShape),
                    errorSymbol,
                )
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
                                "$errorCode => #1T { meta: generic, kind: #1TKind::$variantName({",
                                "})},",
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
                                            listOf(),
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
        val fnName = "parse_${operationShape.id.name.toSnakeCase()}"
        val outputShape = operationShape.outputShape(model)
        val outputSymbol = symbolProvider.toSymbol(outputShape)
        val errorSymbol = operationShape.errorSymbol(symbolProvider)
        return RuntimeType.forInlineFun(fnName, operationDeserModule) {
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
        val fnName = "parse_${operationShape.id.name.toSnakeCase()}_response"
        val outputShape = operationShape.outputShape(model)
        val outputSymbol = symbolProvider.toSymbol(outputShape)
        val errorSymbol = operationShape.errorSymbol(symbolProvider)
        return RuntimeType.forInlineFun(fnName, operationDeserModule) {
            Attribute.AllowClippyUnnecessaryWraps.render(this)
            rustBlockTemplate(
                "pub fn $fnName(response: &#{http}::Response<#{Bytes}>) -> std::result::Result<#{O}, #{E}>",
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
        errorSymbol: RuntimeType,
        customizations: List<OperationCustomization>,
    ) {
        val httpBindingGenerator = ResponseBindingGenerator(protocol, codegenContext, operationShape)
        val structuredDataParser = protocol.structuredDataParser(operationShape)
        Attribute.AllowUnusedMut.render(this)
        rust("let mut output = #T::default();", outputShape.builderSymbol(symbolProvider))
        // avoid non-usage warnings for response
        rust("let _ = response;")
        if (outputShape.id == operationShape.output.get()) {
            structuredDataParser.operationParser(operationShape)?.also { parser ->
                rust(
                    "output = #T(response.body().as_ref(), output).map_err(#T::unhandled)?;",
                    parser,
                    errorSymbol,
                )
            }
        } else {
            check(outputShape.hasTrait<ErrorTrait>()) { "should only be called on outputs or errors $outputShape" }
            structuredDataParser.errorParser(outputShape)?.also { parser ->
                rust(
                    "output = #T(response.body().as_ref(), output).map_err(#T::unhandled)?;",
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
        } else ""

        writeCustomizations(customizations, OperationSection.MutateOutput(customizations, operationShape))

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
        val errorSymbol = operationShape.errorSymbol(symbolProvider)
        val member = binding.member
        return when (binding.location) {
            HttpLocation.HEADER -> writable {
                val fnName = httpBindingGenerator.generateDeserializeHeaderFn(binding)
                rust(
                    """
                    #T(response.headers())
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
                    writable { rust("Some(#T(response.body_mut())?)", deserializer) }
                } else {
                    writable { rust("#T(response.body().as_ref())?", deserializer) }
                }
            }
            HttpLocation.RESPONSE_CODE -> writable {
                rust("Some(response.status().as_u16() as _)")
            }
            HttpLocation.PREFIX_HEADERS -> {
                val sym = httpBindingGenerator.generateDeserializePrefixHeaderFn(binding)
                writable {
                    rustTemplate(
                        """
                        #{deser}(response.headers())
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
