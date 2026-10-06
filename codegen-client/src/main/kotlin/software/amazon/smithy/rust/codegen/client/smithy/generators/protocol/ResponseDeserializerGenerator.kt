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
            // Generated clients ask for the trait version they were generated against (`v1`), so a
            // later 1.x release of aws-smithy-schema can add a protocol trait version and adapt.
            "SchemaProtocol" to RuntimeType.smithySchema(runtimeConfig).resolve("protocol::SchemaProtocol"),
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
     * Builder-first, and the ordering is a borrow-checking requirement rather than a style
     * preference. Both the response deserializer and the borrowed headers hold an immutable borrow
     * of the response, and taking the live body needs a mutable one, so:
     *
     * 1. create the builder;
     * 2. in an inner scope, create the protocol's response deserializer and populate the builder
     *    from it — the composite omits the streaming payload member, leaving it for step 4 — then
     *    run `MutateOutput` while the headers are still borrowed;
     * 3. let that scope end, dropping every immutable borrow;
     * 4. swap the live body out and set it on the builder as a `ByteStream`;
     * 5. finalize, which is where required-member correction runs.
     *
     * The stream is set after member population rather than before because the composite never
     * invokes the consumer for a streaming payload member, so there is nothing to protect it from;
     * setting it afterward keeps the borrow scopes as small as possible.
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
                    ##[allow(unused_mut)]
                    let mut output = <#{BuilderSymbol}>::default();
                    {
                        let _response_headers = response.headers();
                        let protocol = #{SchemaProtocol}::from_config_bag(_cfg).and_then(#{SchemaProtocol}::v1).map_err(#{E}::unhandled)?;
                        let mut deser = protocol.deserialize_response(response, $operationName::OUTPUT_SCHEMA, _cfg)
                            .map_err(#{E}::unhandled)?;
                        output.deserialize_members(&mut *deser).map_err(#{E}::unhandled)?;
                        #{MutateOutput}
                    }
                    // Every immutable borrow of the response has been dropped, so the live body can
                    // be taken. The streaming member was left unset above.
                    let mut body = #{SdkBody}::taken();
                    std::mem::swap(&mut body, response.body_mut());
                    let output = output.${streamingMember.setterName()}(#{Some}(#{ByteStream}::new(body)));
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
            "ByteStream" to RuntimeType.byteStream(runtimeConfig),
            "BeforeParseResponse" to
                writable {
                    writeCustomizations(customizations, OperationSection.BeforeParseResponse(customizations, "response", "force_error", body = null))
                },
            "finalizeBuilder" to
                codegenContext.builderInstantiator().finalizeBuilder(
                    "output",
                    outputShape,
                    mapErr = writable { rustTemplate("#{E}::unhandled", "E" to errorSymbol) },
                ),
            "MutateOutput" to
                writable {
                    writeCustomizations(
                        customizations,
                        OperationSection.MutateOutput(customizations, operationShape, "_response_headers"),
                    )
                },
        )
    }

    /** Schema-serde path for event stream responses.
     *
     * Builder-first, in this order:
     *
     * 1. swap the live body out of the response; it becomes the event receiver;
     * 2. set the receiver on the output builder;
     * 3. create the protocol's response deserializer over the now-bodyless response and populate
     *    the builder's remaining members from it;
     * 4. run `MutateOutput` and finalize, which is where required-member correction runs.
     *
     * Step 3 is what lets a REST protocol populate modeled response headers and status on an
     * event-stream output, which this path previously omitted entirely. It cannot disturb the
     * receiver: the composite never invokes the consumer for the `@streaming` payload member, and
     * the generated arm for a streaming member only skips. For an RPC protocol the deserializer is
     * body-only over an empty body, which every built-in codec reads as an empty structure, so the
     * step is a no-op there and initial-response members still come from the first event frame via
     * the unmarshaller emitted by `EventStreamUnmarshallerGenerator` (`useSchemaSerde = true`).
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
        val operationName = symbolProvider.toSymbol(operationShape).name
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
                    let protocol = #{SchemaProtocol}::from_config_bag(_cfg).and_then(#{SchemaProtocol}::v1).map_err(#{E}::unhandled)?;
                    let unmarshaller = #{unmarshaller}(protocol.clone());
                    let receiver = #{EventReceiver}::new(#{Receiver}::new(unmarshaller, body));
                    let mut output = #{BuilderSymbol}::default().${streamingMember.setterName()}(#{Some}(receiver));
                    // The body is gone, so the protocol reads only headers and status.
                    let mut deser = protocol
                        .deserialize_response(response, $operationName::OUTPUT_SCHEMA, _cfg)
                        .map_err(#{E}::unhandled)?;
                    output.deserialize_members(&mut *deser).map_err(#{E}::unhandled)?;
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
            fn deserialize_streaming_with_config(&self, response: &mut #{HttpResponse}, _cfg: &#{ConfigBag}) -> #{Option}<#{OutputOrError}> {
                ##[allow(unused_mut)]
                let mut force_error = false;
                #{BeforeParseResponse}

                // If this is an error, defer to the non-streaming parser
                if (!response.status().is_success() && response.status().as_u16() != $successCode) || force_error {
                    return #{None};
                }
                #{Some}(#{type_erase_result}(#{parse_streaming_response}(response, _cfg)))
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
            // `body` is read by error-metadata parsing and customizations; `headers` and
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
                #{type_erase_result}(#{parse_error}(response.status().as_u16(), response.headers(), body, _cfg))
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
            deserializeNonStreamingSchemaOnly(operationShape, operationName, customizations, successCode)
        } else {
            deserializeNonStreamingLegacy(operationShape, customizations, successCode)
        }
    }

    /** Schema-only: schema path for success, schema-based error deserialization. */
    private fun RustWriter.deserializeNonStreamingSchemaOnly(
        operationShape: OperationShape,
        operationName: String,
        customizations: List<OperationCustomization>,
        successCode: Int,
    ) {
        val outputShape = operationShape.outputShape(model)
        // `body` is only read by `BeforeParseResponse` customizations and by the
        // `PopulateErrorMetadataExtras` contract that `renderSchemaErrorParsing` declares; the
        // success path no longer takes it, because the selected protocol reads the body from the
        // response itself. An operation with no modeled errors and no body-inspecting
        // customization therefore leaves it unused, hence `allow(unused_variables)`. This
        // rationale lives here rather than in the template because a generated comment is
        // emitted once per operation (0.42% of SSM's source when it was generated).
        rustTemplate(
            """
            let (success, status) = (response.status().is_success(), response.status().as_u16());
            // Load body and headers BEFORE BeforeParseResponse so customizations
            // (e.g., S3's `body_is_error` check that detects errors returned with
            // HTTP 200) can inspect them. The legacy non-streaming path also
            // loads `body` before firing this hook.
            ##[allow(unused_variables)]
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

        // Populate the output BUILDER from the protocol's response deserializer, then run
        // `MutateOutput` customizations, then finalize. Builder-first ordering matters for two
        // reasons. The selected protocol decides which members it can supply — a REST protocol
        // reads `@httpHeader` / `@httpPrefixHeaders` / `@httpResponseCode` / `@httpPayload`
        // members from the response itself, a body-only protocol reads them from its document —
        // so no shape-specific response method or ConfigBag argument is chosen here. And service
        // customizations that populate unmodeled members (e.g. the AWS request-id decorator
        // reading `x-amz-request-id` / `x-amzn-requestid`) run against the builder while the
        // response headers are still borrowed, so required-member error correction is applied
        // exactly once, at finalization, after every source has contributed.
        rustTemplate(
            """
            } else {
                let protocol = #{SchemaProtocol}::from_config_bag(_cfg).and_then(#{SchemaProtocol}::v1)
                    .map_err(|e| #{OrchestratorError}::other(#{BoxError}::from(e)))?;
                let mut deser = protocol.deserialize_response(response, $operationName::OUTPUT_SCHEMA, _cfg)
                    .map_err(|e| #{OrchestratorError}::other(#{BoxError}::from(e)))?;
                // `headers` is already in scope from the top of the function; alias it as
                // `_response_headers` so MutateOutput customizations have a stable name to
                // read from.
                let _response_headers = headers;
                // `mut` is required because `deserialize_members` and the `MutateOutput`
                // builder setters take `&mut self`. Marked `allow(unused_mut)` because an
                // empty output has no members and no customizations to run.
                ##[allow(unused_mut)]
                let mut output = <#{BuilderSymbol}>::default();
                output.deserialize_members(&mut *deser)
                    .map_err(|e| #{OrchestratorError}::other(#{BoxError}::from(e)))?;
                #{MutateOutput}
                let output = #{finalizeBuilder};
                #{Ok}(#{Output}::erase(output))
            }
            """,
            *codegenScope,
            "BoxError" to RuntimeType.boxError(runtimeConfig),
            "BuilderSymbol" to symbolProvider.symbolForBuilder(outputShape),
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
                    mapErr =
                        writable {
                            rustTemplate(
                                "|e| #{OrchestratorError}::other(#{BoxError}::from(e))",
                                *codegenScope,
                                "BoxError" to RuntimeType.boxError(runtimeConfig),
                            )
                        },
                ),
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
            let protocol = #{SchemaProtocol}::from_config_bag(_cfg).and_then(#{SchemaProtocol}::v1)
                .map_err(|e| #{OrchestratorError}::other(#{BoxError}::from(e)))?;
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
                val errorMessageMember = errorShape.errorMessageMember()

                rustTemplate("$errorCode => #{error_symbol}::$variantName({", "error_symbol" to errorSymbol)
                // The protocol decides where the error body deserializer is
                // positioned: for envelope-less protocols (awsJson, restJson1)
                // it's the response body root; for restXml it's inside
                // `<Error>`. Generated code is uniform across protocols.
                //
                // `deserialize_error_response` returns an error-mode deserializer,
                // which tolerates an empty body: a modeled error carrying only
                // HTTP bindings (an S3 `HEAD` failure, for example) still
                // populates them without the body codec being asked to parse
                // nothing. That replaces the empty-body short circuit the
                // generated response method used to perform.
                rustTemplate(
                    """
                    let mut deser = protocol.deserialize_error_response(response, _cfg)
                        .map_err(|e| #{OrchestratorError}::other(#{BoxError}::from(e)))?;
                    let mut tmp = <#{ErrorBuilder}>::default();
                    tmp.deserialize_members(&mut *deser)
                        .map_err(|e| #{OrchestratorError}::other(#{BoxError}::from(e)))?;
                    """,
                    *codegenScope,
                    "BoxError" to RuntimeType.boxError(runtimeConfig),
                    "ErrorBuilder" to symbolProvider.symbolForBuilder(errorShape),
                )
                if (errorMessageMember != null) {
                    val symbol = symbolProvider.toSymbol(errorMessageMember)
                    if (symbol.isOptional()) {
                        // The fallback now applies to the builder rather than to a
                        // built error, so finalization still sees a complete shape.
                        rust(
                            """
                            if tmp.message.is_none() {
                                tmp.message = _error_message;
                            }
                            """,
                        )
                    }
                }
                // `meta` is a private builder field, so it is set through the
                // generated consuming setter rather than directly.
                rust("let tmp = tmp.meta(generic);")
                rustTemplate(
                    "#{finalizeErrorBuilder}",
                    "finalizeErrorBuilder" to
                        codegenContext.builderInstantiator().finalizeBuilder(
                            "tmp",
                            errorShape,
                            mapErr =
                                writable {
                                    rustTemplate(
                                        "|e| #{OrchestratorError}::other(#{BoxError}::from(e))",
                                        *codegenScope,
                                        "BoxError" to RuntimeType.boxError(runtimeConfig),
                                    )
                                },
                        ),
                )
                rust("}),")
            }
            rustTemplate(
                """
                _ => {
                    // Registry-backed, operation-scoped reification of an error whose
                    // code this operation does not model directly: the operation's own
                    // error registry is consulted first, then the lookup widens to the
                    // service-wide error registry. On a hit, the reified error is attached
                    // as the source of the returned unhandled error while its metadata
                    // (code, message, request id) is preserved; on a miss the generic
                    // error is returned unchanged.
                    match protocol
                        .deserialize_error_response(response, _cfg)
                        .ok()
                        .and_then(|mut deser| {
                            #{reify_error}(
                                ${errorSymbol.namespace}::error_registry::REGISTRY
                                    .or(&crate::error_type_registry::REGISTRY),
                                error_code,
                                &mut *deser,
                            )
                        }) {
                        #{Some}(source) => <#{error_symbol} as #{CreateUnhandledError}>::create_unhandled_error(source, #{Some}(generic)),
                        #{None} => #{error_symbol}::generic(generic),
                    }
                }
                """,
                *codegenScope,
                "error_symbol" to errorSymbol,
                "reify_error" to RuntimeType.smithySchema(runtimeConfig).resolve("registry::reify_error"),
                "CreateUnhandledError" to
                    RuntimeType.smithyRuntimeApiClient(runtimeConfig).resolve("client::result::CreateUnhandledError"),
            )
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
                #{parse_error}(status, headers, body, _cfg)
            } else {
                #{parse_response}(status, headers, body, _cfg)
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
