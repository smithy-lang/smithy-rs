/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.protocol

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customizations.SchemaSerdeAllowlist
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.customize.writeCustomizations
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolFunctions
import software.amazon.smithy.rust.codegen.core.smithy.transformers.operationErrors
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.errorMessageMember
import software.amazon.smithy.rust.codegen.core.util.hasStreamingMember
import software.amazon.smithy.rust.codegen.core.util.outputShape

class ResponseDeserializerGenerator(
    private val codegenContext: ClientCodegenContext,
    private val protocol: Protocol,
) {
    private val symbolProvider = codegenContext.symbolProvider
    private val model = codegenContext.model
    private val runtimeConfig = codegenContext.runtimeConfig
    private val httpBindingResolver = protocol.httpBindingResolver
    private val parserGenerator = ProtocolParserGenerator(codegenContext, protocol)
    private val schemaExclusive = SchemaSerdeAllowlist.usesSchemaSerdeExclusively(codegenContext)

    private val codegenScope by lazy {
        val interceptorContext =
            CargoDependency.smithyRuntimeApiClient(runtimeConfig).toType().resolve("client::interceptors::context")
        val orchestrator =
            CargoDependency.smithyRuntimeApiClient(runtimeConfig).toType().resolve("client::orchestrator")
        arrayOf(
            *preludeScope,
            "ConfigBag" to RuntimeType.configBag(runtimeConfig),
            "Error" to interceptorContext.resolve("Error"),
            "HttpResponse" to orchestrator.resolve("HttpResponse"),
            "Instrument" to CargoDependency.Tracing.toType().resolve("Instrument"),
            "Output" to interceptorContext.resolve("Output"),
            "OutputOrError" to interceptorContext.resolve("OutputOrError"),
            "OrchestratorError" to orchestrator.resolve("OrchestratorError"),
            "DeserializeResponse" to RuntimeType.smithyRuntimeApiClient(runtimeConfig).resolve("client::ser_de::DeserializeResponse"),
            "SharedClientProtocol" to RuntimeType.smithySchema(runtimeConfig).resolve("protocol::SharedClientProtocol"),
            "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
            "SdkError" to RuntimeType.sdkError(runtimeConfig),
            "debug_span" to RuntimeType.Tracing.resolve("debug_span"),
            "type_erase_result" to typeEraseResult(),
        )
    }

    fun render(
        writer: RustWriter,
        operationShape: OperationShape,
        customizations: List<OperationCustomization>,
    ) {
        val outputSymbol = symbolProvider.toSymbol(operationShape.outputShape(model))
        val operationName = symbolProvider.toSymbol(operationShape).name
        val streaming = operationShape.outputShape(model).hasStreamingMember(model)

        writer.rustTemplate(
            """
            ##[derive(Debug)]
            struct ${operationName}ResponseDeserializer;
            impl #{DeserializeResponse} for ${operationName}ResponseDeserializer {
                #{deserialize_streaming}

                fn deserialize_nonstreaming(&self, response: &#{HttpResponse}, _cfg: &#{ConfigBag}) -> #{OutputOrError} {
                    #{deserialize_nonstreaming}
                }
            }
            """,
            *codegenScope,
            "O" to outputSymbol,
            "E" to symbolProvider.symbolForOperationError(operationShape),
            "deserialize_streaming" to
                writable {
                    if (streaming) {
                        deserializeStreaming(operationShape, customizations)
                    }
                },
            "deserialize_nonstreaming" to
                writable {
                    when (streaming) {
                        true -> deserializeStreamingError(operationShape, customizations)
                        else -> deserializeNonStreaming(operationShape, operationName, outputSymbol, customizations)
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
                ##[allow(unused_mut)]
                let mut force_error = false;
                #{BeforeParseResponse}

                // If this is an error, defer to the non-streaming parser
                if (!response.status().is_success() && response.status().as_u16() != $successCode) || force_error {
                    return #{None};
                }
                #{Some}(#{type_erase_result}(#{parse_streaming_response}(response)))
            }
            """,
            *codegenScope,
            "parse_streaming_response" to parserGenerator.parseStreamingResponseFn(operationShape, customizations),
            "BeforeParseResponse" to
                writable {
                    writeCustomizations(customizations, OperationSection.BeforeParseResponse(customizations, "response", "force_error", body = null))
                },
        )
    }

    private fun RustWriter.deserializeStreamingError(
        operationShape: OperationShape,
        customizations: List<OperationCustomization>,
    ) {
        if (schemaExclusive) {
            rustTemplate(
                """
                // For streaming operations, we only hit this case if its an error
                let body = response.body().bytes().expect("body loaded");
                let status = response.status().as_u16();
                let headers = response.headers();
                """,
                *codegenScope,
            )
            renderSchemaErrorParsing(operationShape, customizations)
        } else {
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
    }

    private fun RustWriter.deserializeNonStreaming(
        operationShape: OperationShape,
        operationName: String,
        outputSymbol: software.amazon.smithy.codegen.core.Symbol,
        customizations: List<OperationCustomization>,
    ) {
        val successCode = httpBindingResolver.httpTrait(operationShape).code
        if (schemaExclusive) {
            deserializeNonStreamingSchemaOnly(operationShape, operationName, outputSymbol, customizations, successCode)
        } else {
            deserializeNonStreamingLegacy(operationShape, customizations, successCode)
        }
    }

    /** Schema-only: schema path for success, schema-based error deserialization. */
    private fun RustWriter.deserializeNonStreamingSchemaOnly(
        operationShape: OperationShape,
        operationName: String,
        outputSymbol: software.amazon.smithy.codegen.core.Symbol,
        customizations: List<OperationCustomization>,
        successCode: Int,
    ) {
        rustTemplate(
            """
            let (success, status) = (response.status().is_success(), response.status().as_u16());
            ##[allow(unused_mut)]
            let mut force_error = false;
            #{BeforeParseResponse}
            if !success && status != $successCode || force_error {
                let headers = response.headers();
                let body = response.body().bytes().expect("body loaded");
            """,
            *codegenScope,
            "BeforeParseResponse" to
                writable {
                    writeCustomizations(customizations, OperationSection.BeforeParseResponse(customizations, "response", "force_error", body = null))
                },
        )
        renderSchemaErrorParsing(operationShape, customizations)

        // Always use deserialize_with_response — it handles both HTTP-bound members
        // (headers, status code) and body members. When there are no HTTP bindings,
        // it trivially delegates to deserialize(). This ensures synthetic members
        // (e.g., request_id from x-amzn-requestid) are always read from headers.
        rustTemplate(
            """
            } else {
                let protocol = _cfg.load::<#{SharedClientProtocol}>()
                    .expect("a SharedClientProtocol is required");
                let mut deser = protocol.deserialize_response(response, $operationName::OUTPUT_SCHEMA, _cfg)
                    .map_err(|e| #{OrchestratorError}::other(#{BoxError}::from(e)))?;
                let output = #{ConcreteOutput}::deserialize_with_response(
                    &mut *deser,
                    response.headers(),
                    response.status().into(),
                ).map_err(|e| #{OrchestratorError}::other(#{BoxError}::from(e)))?;
                #{Ok}(#{Output}::erase(output))
            }
            """,
            *codegenScope,
            "BoxError" to RuntimeType.boxError(runtimeConfig),
            "ConcreteOutput" to outputSymbol,
        )
    }

    /**
     * Renders schema-based error parsing code.
     * Assumes `status`, `headers`, `body`, and `_cfg` are in scope.
     * Emits a complete expression that returns `OutputOrError`.
     */
    private fun RustWriter.renderSchemaErrorParsing(
        operationShape: OperationShape,
        customizations: List<OperationCustomization>,
    ) {
        val errorSymbol = symbolProvider.symbolForOperationError(operationShape)
        val errors = operationShape.operationErrors(model)

        rustTemplate(
            """
            ##[allow(unused_mut)]
            let mut generic_builder = #{parse_http_error_metadata}(status, headers, body)
                .map_err(|e| #{OrchestratorError}::other(#{BoxError}::from(e)))?;
            #{PopulateErrorMetadataExtras}
            let generic = generic_builder.build();
            """,
            *codegenScope,
            "BoxError" to RuntimeType.boxError(runtimeConfig),
            "parse_http_error_metadata" to protocol.parseHttpErrorMetadata(operationShape),
            "PopulateErrorMetadataExtras" to
                writable {
                    writeCustomizations(
                        customizations,
                        OperationSection.PopulateErrorMetadataExtras(customizations, "generic_builder", "status", "headers"),
                    )
                },
        )

        if (errors.isNotEmpty()) {
            rustTemplate(
                """
                let error_code = match generic.code() {
                    #{Some}(code) => code,
                    #{None} => return #{Err}(#{OrchestratorError}::other(#{BoxError}::from(#{error_symbol}::unhandled(generic)))),
                };
                let _error_message = generic.message().map(|msg| msg.to_owned());
                let protocol = _cfg.load::<#{SharedClientProtocol}>()
                    .expect("a SharedClientProtocol is required");
                """,
                *codegenScope,
                "BoxError" to RuntimeType.boxError(runtimeConfig),
                "error_symbol" to errorSymbol,
            )
            rustTemplate("let err = match error_code {")
            for (error in errors) {
                val errorShape = model.expectShape(error.id, StructureShape::class.java)
                val variantName = symbolProvider.toSymbol(errorShape).name
                val errorCode = httpBindingResolver.errorCode(errorShape).dq()
                val errorType = symbolProvider.toSymbol(errorShape)
                val errorMessageMember = errorShape.errorMessageMember()

                rustTemplate("$errorCode => #{error_symbol}::$variantName({", "error_symbol" to errorSymbol)
                rustTemplate(
                    """
                    let mut tmp = match protocol.deserialize_response(response, #{ErrorType}::SCHEMA, _cfg)
                        .and_then(|mut deser| #{ErrorType}::deserialize(&mut *deser))
                    {
                        #{Ok}(val) => val,
                        #{Err}(e) => return #{Err}(#{OrchestratorError}::other(#{BoxError}::from(e))),
                    };
                    tmp.meta = generic;
                    """,
                    *codegenScope,
                    "BoxError" to RuntimeType.boxError(runtimeConfig),
                    "error_symbol" to errorSymbol,
                    "ErrorType" to errorType,
                )
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
                rust("}),")
            }
            rustTemplate("_ => #{error_symbol}::generic(generic)", "error_symbol" to errorSymbol)
            rustTemplate(
                """
                };
                #{Err}(#{OrchestratorError}::operation(#{Error}::erase(err)))
                """,
                *codegenScope,
            )
        } else {
            rustTemplate(
                "#{Err}(#{OrchestratorError}::operation(#{Error}::erase(#{error_symbol}::generic(generic))))",
                *codegenScope,
                "error_symbol" to errorSymbol,
            )
        }
    }

    /** Legacy path: old codegen only, no schema-based deserialization. */
    private fun RustWriter.deserializeNonStreamingLegacy(
        operationShape: OperationShape,
        customizations: List<OperationCustomization>,
        successCode: Int,
    ) {
        rustTemplate(
            """
            let (success, status) = (response.status().is_success(), response.status().as_u16());
            let headers = response.headers();
            let body = response.body().bytes().expect("body loaded");
            ##[allow(unused_mut)]
            let mut force_error = false;
            #{BeforeParseResponse}
            let parse_result = if !success && status != $successCode || force_error {
                #{parse_error}(status, headers, body)
            } else {
                #{parse_response}(status, headers, body)
            };
            #{type_erase_result}(parse_result)
            """,
            *codegenScope,
            "parse_error" to parserGenerator.parseErrorFn(operationShape, customizations),
            "parse_response" to parserGenerator.parseResponseFn(operationShape, customizations),
            "BeforeParseResponse" to
                writable {
                    writeCustomizations(customizations, OperationSection.BeforeParseResponse(customizations, "response", "force_error", "body"))
                },
        )
    }

    private fun typeEraseResult(): RuntimeType =
        ProtocolFunctions.crossOperationFn("type_erase_result") { fnName ->
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
