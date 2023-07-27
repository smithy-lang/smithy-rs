/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.protocol

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.customize.writeCustomizations
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolFunctions
import software.amazon.smithy.rust.codegen.core.util.hasStreamingMember
import software.amazon.smithy.rust.codegen.core.util.outputShape

class ResponseDeserializerGenerator(
    codegenContext: ClientCodegenContext,
    protocol: Protocol,
) {
    private val symbolProvider = codegenContext.symbolProvider
    private val model = codegenContext.model
    private val runtimeConfig = codegenContext.runtimeConfig
    private val httpBindingResolver = protocol.httpBindingResolver
    private val parserGenerator = ProtocolParserGenerator(codegenContext, protocol)

    private val codegenScope by lazy {
        val interceptorContext =
            CargoDependency.smithyRuntimeApi(runtimeConfig).toType().resolve("client::interceptors::context")
        val orchestrator =
            CargoDependency.smithyRuntimeApi(runtimeConfig).toType().resolve("client::orchestrator")
        arrayOf(
            *preludeScope,
            "Error" to interceptorContext.resolve("Error"),
            "HttpResponse" to orchestrator.resolve("HttpResponse"),
            "Instrument" to CargoDependency.Tracing.toType().resolve("Instrument"),
            "Output" to interceptorContext.resolve("Output"),
            "OutputOrError" to interceptorContext.resolve("OutputOrError"),
            "OrchestratorError" to orchestrator.resolve("OrchestratorError"),
            "ResponseDeserializer" to RuntimeType.smithyRuntimeApi(runtimeConfig).resolve("client::ser_de::ResponseDeserializer"),
            "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
            "SdkError" to RuntimeType.sdkError(runtimeConfig),
            "debug_span" to RuntimeType.Tracing.resolve("debug_span"),
            "type_erase_result" to typeEraseResult(),
        )
    }

    fun render(writer: RustWriter, operationShape: OperationShape, customizations: List<OperationCustomization>) {
        val outputSymbol = symbolProvider.toSymbol(operationShape.outputShape(model))
        val operationName = symbolProvider.toSymbol(operationShape).name
        val streaming = operationShape.outputShape(model).hasStreamingMember(model)

        writer.rustTemplate(
            """
            ##[derive(Debug)]
            struct ${operationName}ResponseDeserializer;
            impl #{ResponseDeserializer} for ${operationName}ResponseDeserializer {
                #{deserialize_streaming}

                fn deserialize_nonstreaming(&self, response: &#{HttpResponse}) -> #{OutputOrError} {
                    #{deserialize_nonstreaming}
                }
            }
            """,
            *codegenScope,
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
            fn deserialize_streaming(&self, response: &mut #{HttpResponse}) -> #{Option}<#{OutputOrError}> {
                #{BeforeParseResponse}

                // If this is an error, defer to the non-streaming parser
                if !response.status().is_success() && response.status().as_u16() != $successCode {
                    return #{None};
                }
                #{Some}(#{type_erase_result}(#{parse_streaming_response}(response)))
            }
            """,
            *codegenScope,
            "parse_streaming_response" to parserGenerator.parseStreamingResponseFn(operationShape, false, customizations),
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
            *codegenScope,
            "parse_error" to parserGenerator.parseErrorFn(operationShape, customizations),
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
            *codegenScope,
            "parse_error" to parserGenerator.parseErrorFn(operationShape, customizations),
            "parse_response" to parserGenerator.parseResponseFn(operationShape, false, customizations),
            "BeforeParseResponse" to writable {
                writeCustomizations(customizations, OperationSection.BeforeParseResponse(customizations, "response"))
            },
        )
    }

    private fun typeEraseResult(): RuntimeType = ProtocolFunctions.crossOperationFn("type_erase_result") { fnName ->
        rustTemplate(
            """
            pub(crate) fn $fnName<O, E>(result: #{Result}<O, E>) -> #{Result}<#{Output}, #{OrchestratorError}<#{Error}>>
            where
                O: ::std::fmt::Debug + #{Send} + #{Sync} + 'static,
                E: ::std::error::Error + std::fmt::Debug + #{Send} + #{Sync} + 'static,
            {
                result.map(|output| #{Output}::erase(output))
                    .map_err(|error| #{Error}::erase(error))
                    .map_err(#{Into}::into)
            }
            """,
            *codegenScope,
        )
    }
}
