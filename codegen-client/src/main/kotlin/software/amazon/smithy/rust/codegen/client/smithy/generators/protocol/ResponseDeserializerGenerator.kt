/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.protocol

import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
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
import software.amazon.smithy.rust.codegen.core.smithy.generators.setterName
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolFunctions
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.EventStreamUnmarshallerGenerator
import software.amazon.smithy.rust.codegen.core.smithy.transformers.operationErrors
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.errorMessageMember
import software.amazon.smithy.rust.codegen.core.util.findStreamingMember
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

                fn deserialize_nonstreaming_with_config(&self, response: &#{HttpResponse}, _cfg: &#{ConfigBag}) -> #{OutputOrError} {
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
        val outputShape = operationShape.outputShape(model)
        val streamingMember = outputShape.findStreamingMember(model)!!
        val streamingTarget = model.expectShape(streamingMember.target)
        val isBlobStreaming = streamingTarget is BlobShape
        val isEventStream = streamingTarget is UnionShape

        if (schemaExclusive && isBlobStreaming) {
            deserializeStreamingBlobSchema(operationShape, customizations)
        } else if (schemaExclusive && isEventStream) {
            deserializeStreamingEventStreamSchema(operationShape, customizations)
        } else {
            // Non-schema-exclusive: use legacy parser
            deserializeStreamingLegacy(operationShape, customizations)
        }
    }

    /** Schema-serde path for streaming blob responses.
     *
     * Strategy (mirrors the legacy path):
     * 1. Create a deserializer from the response body (while it's still available).
     * 2. Call deserialize_with_response to read header-bound members. The streaming
     *    blob member gets set to an empty ByteStream placeholder.
     * 3. Swap the body out of the response (take ownership).
     * 4. Replace the placeholder with the real ByteStream wrapping the swapped body.
     */
    private fun RustWriter.deserializeStreamingBlobSchema(
        operationShape: OperationShape,
        customizations: List<OperationCustomization>,
    ) {
        val successCode = httpBindingResolver.httpTrait(operationShape).code
        val outputShape = operationShape.outputShape(model)
        val outputSymbol = symbolProvider.toSymbol(outputShape)
        val errorSymbol = symbolProvider.symbolForOperationError(operationShape)
        val streamingMember = outputShape.findStreamingMember(model)!!
        val memberName = symbolProvider.toMemberName(streamingMember)
        val operationName = symbolProvider.toSymbol(operationShape).name

        rustTemplate(
            """
            fn deserialize_streaming_with_config(&self, response: &mut #{HttpResponse}, _cfg: &#{ConfigBag}) -> #{Option}<#{OutputOrError}> {
                ##[allow(unused_mut)]
                let mut force_error = false;
                #{BeforeParseResponse}

                // If this is an error, defer to the non-streaming parser
                if (!response.status().is_success() && response.status().as_u16() != $successCode) || force_error {
                    return #{None};
                }

                let result = (|| -> ::std::result::Result<#{ConcreteOutput}, #{E}> {
                    // Read header-bound members while the body is still in the response.
                    // deserialize_with_response sets the streaming blob member to a placeholder.
                    let _response_headers = response.headers();
                    let mut output = {
                        let protocol = _cfg.load::<#{SharedClientProtocol}>()
                            .expect("a SharedClientProtocol is required");
                        let mut deser = protocol.deserialize_response(response, $operationName::OUTPUT_SCHEMA, _cfg)
                            .map_err(#{E}::unhandled)?;
                        #{ConcreteOutput}::deserialize_with_response(
                            &mut *deser, _response_headers, response.status().as_u16(), &[],
                        ).map_err(#{E}::unhandled)?
                    };
                    // Run MutateOutput so service-specific accessors (e.g. S3's
                    // request-id reader that checks both `x-amz-request-id` and
                    // `x-amzn-requestid`) populate synthetic members like
                    // `_request_id` after deserialize_with_response. The schema-serde
                    // gating in BaseRequestIdDecorator emits direct field access here
                    // because `output` is the built Output struct.
                    #{MutateOutput}
                    // Now take ownership of the body and replace the placeholder
                    let mut body = #{SdkBody}::taken();
                    std::mem::swap(&mut body, response.body_mut());
                    output.$memberName = #{ByteStream}::new(body);
                    #{Ok}(output)
                })();

                #{Some}(#{type_erase_result}(result))
            }
            """,
            *codegenScope,
            "ConcreteOutput" to outputSymbol,
            "E" to errorSymbol,
            "ByteStream" to RuntimeType.byteStream(runtimeConfig),
            "BeforeParseResponse" to
                writable {
                    writeCustomizations(customizations, OperationSection.BeforeParseResponse(customizations, "response", "force_error", body = null))
                },
            "MutateOutput" to
                writable {
                    writeCustomizations(
                        customizations,
                        OperationSection.MutateOutput(customizations, operationShape, "_response_headers"),
                    )
                },
        )
    }

    /** Schema-serde path for event stream responses (hybrid).
     *
     * Creates the output builder, sets the event stream member via
     * `set_<memberName>(Some(receiver))`, then builds. `EventStreamUnmarshallerGenerator`
     * (invoked here with `useSchemaSerde = true`) emits a schema-serde unmarshaller
     * that uses `self.protocol.payload_codec()` to decode each event frame, and also
     * handles initial-response data for RPC protocols (which arrives via the first
     * event frame, not the HTTP body).
     *
     * Unlike the non-streaming schema path, `deserialize_with_response` is not
     * used because it would call `builder.build()` internally — and for
     * @required streaming members the build fails before the event stream
     * member can be populated.
     */
    private fun RustWriter.deserializeStreamingEventStreamSchema(
        operationShape: OperationShape,
        customizations: List<OperationCustomization>,
    ) {
        val successCode = httpBindingResolver.httpTrait(operationShape).code
        val outputShape = operationShape.outputShape(model)
        val outputSymbol = symbolProvider.toSymbol(outputShape)
        val errorSymbol = symbolProvider.symbolForOperationError(operationShape)
        val streamingMember = outputShape.findStreamingMember(model)!!
        val unionTarget = model.expectShape(streamingMember.target, UnionShape::class.java)

        val unmarshallerCtor =
            EventStreamUnmarshallerGenerator(
                protocol,
                codegenContext,
                operationShape,
                unionTarget,
                useSchemaSerde = true,
            ).render()

        rustTemplate(
            """
            fn deserialize_streaming_with_config(&self, response: &mut #{HttpResponse}, _cfg: &#{ConfigBag}) -> #{Option}<#{OutputOrError}> {
                ##[allow(unused_mut)]
                let mut force_error = false;
                #{BeforeParseResponse}

                // If this is an error, defer to the non-streaming parser
                if (!response.status().is_success() && response.status().as_u16() != $successCode) || force_error {
                    return #{None};
                }

                let result = (|| -> ::std::result::Result<#{ConcreteOutput}, #{E}> {
                    // Swap body out — becomes the event stream receiver.
                    let body = std::mem::replace(response.body_mut(), #{SdkBody}::taken());
                    // Bind headers by reference after the body swap so `MutateOutput`
                    // customizations (e.g., the AWS SDK request-id decorator) can read
                    // `x-amzn-requestid` without cloning. `response.headers()` is still
                    // valid — swapping the body does not invalidate headers.
                    let _response_headers = response.headers();
                    let protocol = _cfg.load::<#{SharedClientProtocol}>()
                        .expect("a SharedClientProtocol is required")
                        .clone();
                    let unmarshaller = #{unmarshaller}(protocol);
                    let receiver = #{EventReceiver}::new(#{Receiver}::new(unmarshaller, body));
                    let output = #{BuilderSymbol}::default();
                    // `mut` is required because `MutateOutput` customizations (e.g.,
                    // the AWS SDK request-id decorator) call builder setters that take
                    // `&mut self` (like `_set_request_id`). Marked `allow(unused_mut)`
                    // because the customization block is empty for non-AWS services.
                    ##[allow(unused_mut)]
                    let mut output = output.${streamingMember.setterName()}(#{Some}(receiver));
                    // Emit MutateOutput customizations (populates synthetic members like
                    // `_request_id` from `_response_headers`). `deserialize_with_response`
                    // is not usable here because it calls `builder.build()` internally,
                    // which would fail for `@required` streaming members. The legacy
                    // streaming path emits the same customizations via
                    // `ProtocolParserGenerator.renderShapeParser`.
                    #{MutateOutput}
                    // Build via finalizeBuilder — applies error correction so @required
                    // non-event-stream members are populated with defaults. For RPC
                    // protocols with initial-response, the fluent builder re-populates
                    // those members from the first event frame via into_builder.
                    let output = #{finalizeBuilder};
                    #{Ok}(output)
                })();

                #{Some}(#{type_erase_result}(result))
            }
            """,
            *codegenScope,
            "ConcreteOutput" to outputSymbol,
            "E" to errorSymbol,
            "BuilderSymbol" to symbolProvider.symbolForBuilder(outputShape),
            "EventReceiver" to RuntimeType.eventReceiver(runtimeConfig),
            "Receiver" to RuntimeType.eventStreamReceiver(runtimeConfig),
            "unmarshaller" to unmarshallerCtor,
            "MutateOutput" to
                writable {
                    writeCustomizations(
                        customizations,
                        OperationSection.MutateOutput(customizations, operationShape, "_response_headers"),
                    )
                },
            "finalizeBuilder" to
                codegenContext.builderInstantiator().finalizeBuilder(
                    "output",
                    outputShape,
                    mapErr = writable { rustTemplate("#{E}::unhandled", "E" to errorSymbol) },
                ),
            "BeforeParseResponse" to
                writable {
                    writeCustomizations(customizations, OperationSection.BeforeParseResponse(customizations, "response", "force_error", body = null))
                },
        )
    }

    /** Legacy path for streaming responses (event streams and non-schema-exclusive). */
    private fun RustWriter.deserializeStreamingLegacy(
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
            // `body` is used by per-error `deserialize_with_response` calls; `headers` and
            // `status` are referenced by request-id-applying customizations through the
            // `PopulateErrorMetadataExtras` contract that `renderSchemaErrorParsing` declares
            // (it lists `status` and `headers` as the in-scope names). Bind all three so the
            // scope matches the non-streaming error path (`deserializeNonStreamingSchemaOnly`)
            // and a customization that reads `status` compiles uniformly on both paths.
            //
            // Any of the three can be unused depending on the operation shape: streaming
            // operations with zero modeled errors don't reference `body`; protocol-test models
            // without request-id customizations don't reference `headers`/`status`. The bindings
            // stay unconditional and are annotated to suppress the warning, mirroring the
            // `#[allow(unused_mut)]` idiom used elsewhere in this generator.
            rustTemplate(
                """
                // For streaming operations, we only hit this case if its an error
                ##[allow(unused_variables)]
                let body = response.body().bytes().expect("body loaded");
                ##[allow(unused_variables)]
                let headers = response.headers();
                ##[allow(unused_variables)]
                let status = response.status().as_u16();
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
            // Load body and headers BEFORE BeforeParseResponse so customizations
            // (e.g., S3's `body_is_error` check that detects errors returned with
            // HTTP 200) can inspect them. The legacy non-streaming path also
            // loads `body` before firing this hook.
            let body = response.body().bytes().expect("body loaded");
            let headers = response.headers();
            ##[allow(unused_mut)]
            let mut force_error = false;
            #{BeforeParseResponse}
            if !success && status != $successCode || force_error {
            """,
            *codegenScope,
            "BeforeParseResponse" to
                writable {
                    writeCustomizations(customizations, OperationSection.BeforeParseResponse(customizations, "response", "force_error", "body"))
                },
        )
        renderSchemaErrorParsing(operationShape, customizations)

        // Always use deserialize_with_response — it handles both HTTP-bound members
        // (headers, status code) and body members. When there are no HTTP bindings,
        // it trivially delegates to deserialize(). After deserialize_with_response,
        // run `MutateOutput` customizations so service-specific header readers
        // (e.g. S3's request-id decorator that checks `x-amz-request-id` /
        // `x-amzn-requestid` with fallback) populate synthetic members like
        // `_request_id`. This mirrors the streaming path and the legacy
        // non-streaming path; without it, S3 responses leave `_request_id` as
        // `None` because the schema-serde synthetic-member read uses only the
        // default `x-amzn-requestid` header.
        rustTemplate(
            """
            } else {
                let protocol = _cfg.load::<#{SharedClientProtocol}>()
                    .expect("a SharedClientProtocol is required");
                let mut deser = protocol.deserialize_response(response, $operationName::OUTPUT_SCHEMA, _cfg)
                    .map_err(|e| #{OrchestratorError}::other(#{BoxError}::from(e)))?;
                // body and headers are already in scope from the top of the function;
                // alias `headers` as `_response_headers` so MutateOutput
                // customizations have a stable name to read from.
                let _response_headers = headers;
                ##[allow(unused_mut)]
                let mut output = #{ConcreteOutput}::deserialize_with_response(
                    &mut *deser,
                    _response_headers,
                    response.status().into(),
                    body,
                ).map_err(|e| #{OrchestratorError}::other(#{BoxError}::from(e)))?;
                #{MutateOutput}
                #{Ok}(#{Output}::erase(output))
            }
            """,
            *codegenScope,
            "BoxError" to RuntimeType.boxError(runtimeConfig),
            "ConcreteOutput" to outputSymbol,
            "MutateOutput" to
                writable {
                    writeCustomizations(
                        customizations,
                        OperationSection.MutateOutput(customizations, operationShape, "_response_headers"),
                    )
                },
        )
    }

    /**
     * Renders schema-based error parsing code.
     * Assumes `response`, `body`, and `_cfg` are in scope.
     * Emits a complete expression that returns `OutputOrError`.
     *
     * Calls the protocol's [`ClientProtocolInner::parse_error_metadata`] to
     * extract `code` / `message` / `request_id` from the wire envelope, then
     * matches on the resolved code and per-variant calls
     * [`ClientProtocolInner::deserialize_error_response`] to obtain a body
     * deserializer positioned wherever the variant's members live (e.g.
     * inside `<Error>` for restXml).
     */
    private fun RustWriter.renderSchemaErrorParsing(
        operationShape: OperationShape,
        customizations: List<OperationCustomization>,
    ) {
        val errorSymbol = symbolProvider.symbolForOperationError(operationShape)
        val errors = operationShape.operationErrors(model)

        // Both `parse_error_metadata` and (per-variant)
        // `deserialize_error_response` are trait methods on the protocol, so
        // the protocol must be in scope before extracting the metadata
        // builder. The `parseHttpErrorMetadata` free-function call and the
        // protocol-specific `errorBodyContents` shim that the legacy schema
        // path used are eliminated.
        rustTemplate(
            """
            let protocol = _cfg.load::<#{SharedClientProtocol}>()
                .expect("a SharedClientProtocol is required");
            ##[allow(unused_mut)]
            let mut generic_builder = protocol.parse_error_metadata(response, _cfg)
                .map_err(|e| #{OrchestratorError}::other(#{BoxError}::from(e)))?;
            #{PopulateErrorMetadataExtras}
            let generic = generic_builder.build();
            """,
            *codegenScope,
            "BoxError" to RuntimeType.boxError(runtimeConfig),
            "PopulateErrorMetadataExtras" to
                writable {
                    writeCustomizations(
                        customizations,
                        OperationSection.PopulateErrorMetadataExtras(customizations, "generic_builder", "status", "headers", "body"),
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
                // The protocol decides where the error body deserializer is
                // positioned: for envelope-less protocols (awsJson, restJson1)
                // it's the response body root; for restXml it's inside
                // `<Error>`. Generated code is uniform across protocols.
                rustTemplate(
                    """
                    let mut tmp = match protocol.deserialize_error_response(response, _cfg)
                        .and_then(|mut deser| #{ErrorType}::deserialize_with_response(&mut *deser, response.headers(), response.status().into(), body))
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
