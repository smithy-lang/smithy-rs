/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.protocols

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.generators.SensitiveIndex
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.ClientProtocolGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.MakeOperationGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.ProtocolParserGenerator
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.core.smithy.customize.writeCustomizations
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolPayloadGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.HttpBoundProtocolPayloadGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolFunctions
import software.amazon.smithy.rust.codegen.core.util.hasStreamingMember
import software.amazon.smithy.rust.codegen.core.util.outputShape

// TODO(enableNewSmithyRuntime): Delete this class when cleaning up `enableNewSmithyRuntime`
class HttpBoundProtocolGenerator(
    codegenContext: ClientCodegenContext,
    protocol: Protocol,
    bodyGenerator: ProtocolPayloadGenerator = HttpBoundProtocolPayloadGenerator(codegenContext, protocol),
) : ClientProtocolGenerator(
    codegenContext,
    protocol,
    MakeOperationGenerator(
        codegenContext,
        protocol,
        bodyGenerator,
        public = true,
        includeDefaultPayloadHeaders = true,
    ),
    bodyGenerator,
    HttpBoundProtocolTraitImplGenerator(codegenContext, protocol),
)

// TODO(enableNewSmithyRuntime): Delete this class when cleaning up `enableNewSmithyRuntime`
open class HttpBoundProtocolTraitImplGenerator(
    codegenContext: ClientCodegenContext,
    protocol: Protocol,
) {
    private val symbolProvider = codegenContext.symbolProvider
    private val model = codegenContext.model
    private val runtimeConfig = codegenContext.runtimeConfig
    private val httpBindingResolver = protocol.httpBindingResolver
    private val protocolFunctions = ProtocolFunctions(codegenContext)
    private val parserGenerator = ProtocolParserGenerator(codegenContext, protocol)

    private val codegenScope = arrayOf(
        "ParseStrict" to RuntimeType.parseStrictResponse(runtimeConfig),
        "ParseResponse" to RuntimeType.parseHttpResponse(runtimeConfig),
        "http" to RuntimeType.Http,
        "operation" to RuntimeType.operationModule(runtimeConfig),
        "Bytes" to RuntimeType.Bytes,
        "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
    )

    private val sensitiveIndex = SensitiveIndex.of(model)

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
    }

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
            "parse_error" to parserGenerator.parseErrorFn(operationShape, customizations),
            "parse_response" to parserGenerator.parseResponseFn(operationShape, customizations),
            "BeforeParseResponse" to writable {
                writeCustomizations(customizations, OperationSection.BeforeParseResponse(customizations, "response"))
            },
        )
        val sensitive = writable {
            if (sensitiveIndex.hasSensitiveOutput(operationShape)) {
                rust("fn sensitive(&self) -> bool { true }")
            }
        }
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
                #{sensitive}
            }""",
            *codegenScope,
            *localScope,
            "sensitive" to sensitive,
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
            "parse_streaming_response" to parseStreamingResponse(operationShape, customizations),
            "parse_error" to parserGenerator.parseErrorFn(operationShape, customizations),
            "BeforeParseResponse" to writable {
                writeCustomizations(customizations, OperationSection.BeforeParseResponse(customizations, "response"))
            },
            *codegenScope,
        )
    }

    private fun parseStreamingResponse(
        operationShape: OperationShape,
        customizations: List<OperationCustomization>,
    ): RuntimeType {
        val outputShape = operationShape.outputShape(model)
        val outputSymbol = symbolProvider.toSymbol(outputShape)
        val errorSymbol = symbolProvider.symbolForOperationError(operationShape)
        return protocolFunctions.deserializeFn(operationShape, fnNameSuffix = "op_response") { fnName ->
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
                    #{parse_streaming_response}(response, &properties)
                    """,
                    "parse_streaming_response" to parserGenerator.parseStreamingResponseFn(
                        operationShape,
                        true,
                        customizations,
                    ),
                )
            }
        }
    }
}
