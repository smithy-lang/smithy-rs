/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.smithy.mapRustType
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.eventStreamSerdeModule
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticEventStreamUnionTrait
import software.amazon.smithy.rust.codegen.core.smithy.transformers.eventStreamErrors
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.expectTrait
import software.amazon.smithy.rust.codegen.core.util.findStreamingMember
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.isEventStream
import software.amazon.smithy.rust.codegen.core.util.isTargetUnit
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.core.util.toPascalCase
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.canReachConstrainedShape

/**
 * Renders the `StreamingOperationShape` implementation of an operation whose input or output carries an
 * event stream or a streaming blob, under the `schemaSerde` setting.
 *
 * Both halves work through the erased `SharedServerProtocol` handed in at runtime. The event frame
 * marshalling and unmarshalling itself lives in `aws-smithy-http-server`'s
 * `schema::event_stream` module, driven by the shapes' runtime schemas: it asks the protocol for the
 * payload codec and the event media type, and `initial_messages_in_frames()` decides whether the
 * non-stream members travel in `initial-request` and `initial-response` frames. Nothing rendered here
 * names a protocol.
 *
 * The generated code contributes only the pieces that need shape types:
 * - a `DeserializableEventStream` dispatch per input stream union (event name → typed walker),
 * - a `SerializableEventError` dispatch per modeled error enum (variant → `:exception-type` name),
 * - `pub type` aliases keeping the historical `*Marshaller`/`*Unmarshaller` names in
 *   `crate::event_stream_serde`.
 *
 * The half that does not stream is written in terms of the collected path: `DeserializableShape` for the
 * input, `ServerProtocol::serialize_response` for the output.
 */
class ServerStreamingOperationGenerator(
    private val codegenContext: ServerCodegenContext,
    private val operationShape: OperationShape,
    private val validationExceptionConversionGenerator: ValidationExceptionConversionGenerator,
) {
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val runtimeConfig = codegenContext.runtimeConfig
    private val smithyHttpServer = ServerCargoDependency.smithyHttpServer(runtimeConfig).toType()
    private val schemaEventStream = smithyHttpServer.resolve("schema::event_stream")
    private val smithyHttp = RuntimeType.smithyHttp(runtimeConfig)
    private val eventStreamSerdeModule = RustModule.eventStreamSerdeModule()
    private val inputShape = operationShape.inputShape(model)
    private val outputShape = operationShape.outputShape(model)
    private val operationName = symbolProvider.toSymbol(operationShape).name
    private val codegenScope =
        arrayOf(
            *preludeScope,
            "ByteStream" to RuntimeType.byteStream(runtimeConfig),
            "DeserializableShape" to smithyHttpServer.resolve("schema::DeserializableShape"),
            "DeserializeError" to smithyHttpServer.resolve("schema::DeserializeError"),
            "DeserializableEventStream" to schemaEventStream.resolve("DeserializableEventStream"),
            "EventFrame" to schemaEventStream.resolve("EventFrame"),
            "EventStreamError" to RuntimeType.smithyEventStream(runtimeConfig).resolve("error::Error"),
            "EventStreamSender" to smithyHttp.resolve("event_stream::EventStreamSender"),
            "FuturesStreamCompatByteStream" to smithyHttp.resolve("futures_stream_adapter::FuturesStreamCompatByteStream"),
            "MessageStreamError" to smithyHttp.resolve("event_stream::MessageStreamError"),
            "NoModeledEventErrorMarshaller" to schemaEventStream.resolve("NoModeledEventErrorMarshaller"),
            "Response" to smithyHttpServer.resolve("response::Response"),
            "SchemaEventErrorMarshaller" to schemaEventStream.resolve("SchemaEventErrorMarshaller"),
            "SchemaEventMarshaller" to schemaEventStream.resolve("SchemaEventMarshaller"),
            "SchemaEventUnmarshaller" to schemaEventStream.resolve("SchemaEventUnmarshaller"),
            "SchemaOperationShape" to smithyHttpServer.resolve("operation::SchemaOperationShape"),
            "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
            "SerializableEventError" to schemaEventStream.resolve("SerializableEventError"),
            "SerializableStruct" to RuntimeType.smithySchema(runtimeConfig).resolve("serde::SerializableStruct"),
            "Schema" to RuntimeType.smithySchema(runtimeConfig).resolve("Schema"),
            "ShapeDeserializer" to RuntimeType.smithySchema(runtimeConfig).resolve("serde::ShapeDeserializer"),
            "SharedServerProtocol" to smithyHttpServer.resolve("schema::SharedServerProtocol"),
            "StreamingInputFuture" to smithyHttpServer.resolve("operation::StreamingInputFuture"),
            "StreamingOperationShape" to smithyHttpServer.resolve("operation::StreamingOperationShape"),
            "apply_initial_request" to schemaEventStream.resolve("apply_initial_request"),
            "boxed" to smithyHttpServer.resolve("body::boxed"),
            "empty" to smithyHttpServer.resolve("body::empty"),
            "event_stream_response_body" to schemaEventStream.resolve("event_stream_response_body"),
            "InitialResponsePolicy" to schemaEventStream.resolve("InitialResponsePolicy"),
            "futures_util" to ServerCargoDependency.FuturesUtil.toType(),
            "http_body" to CargoDependency.HttpBody1x.toType(),
            "internal_server_error" to smithyHttpServer.resolve("operation::empty_internal_server_error"),
            "ready" to RuntimeType.std.resolve("future::ready"),
            "replace" to RuntimeType.std.resolve("mem::replace"),
            "tracing" to RuntimeType.Tracing,
            "wrap_stream" to smithyHttpServer.resolve("body::wrap_stream"),
        )

    fun render(writer: RustWriter) {
        writer.rustTemplate(
            """
            impl #{StreamingOperationShape} for $operationName {
                fn deserialize_streaming_input(
                    deserializer: &mut dyn #{ShapeDeserializer},
                    body: #{SdkBody},
                    protocol: #{SharedServerProtocol},
                ) -> #{StreamingInputFuture}<Self::Input> {
                    #{deserialize:W}
                }

                fn serialize_streaming_output(output: Self::Output, protocol: &#{SharedServerProtocol}) -> #{Response} {
                    #{serialize:W}
                }
            }
            """,
            *codegenScope,
            "deserialize" to deserializeInput(),
            "serialize" to serializeOutput(),
        )
    }

    // ---- request side ----

    private fun deserializeInput(): Writable {
        val streamingMember = inputShape.findStreamingMember(model) ?: return collectedInput()
        val builderSymbol = inputShape.serverBuilderSymbol(codegenContext)
        val inputSymbol = symbolProvider.toSymbol(inputShape)
        val field = symbolProvider.toMemberName(streamingMember)
        return writable {
            rustTemplate(
                """
                let mut builder = #{Builder}::default();
                if let #{Err}(err) = #{Input}::${ServerSchemaDeserializeGenerator.INTO_FUNCTION_NAME}(&mut builder, deserializer) {
                    return #{Box}::pin(#{ready}(#{Err}(#{DeserializeError}::from(err))));
                }
                #{Box}::pin(async move {
                    #{attach:W}
                    builder.$field = #{Some}(streaming);
                    #{finish:W}
                })
                """,
                *codegenScope,
                "Builder" to builderSymbol,
                "Input" to inputSymbol,
                "attach" to attachStreamingInput(streamingMember, inputSymbol),
                "finish" to finishInput(),
            )
        }
    }

    /** The input does not stream: the collected path reads it. */
    private fun collectedInput(): Writable =
        writable {
            rustTemplate(
                """
                let _ = (body, protocol);
                #{Box}::pin(#{ready}(<Self::Input as #{DeserializableShape}>::deserialize(deserializer)))
                """,
                *codegenScope,
            )
        }

    private fun attachStreamingInput(
        member: MemberShape,
        inputSymbol: software.amazon.smithy.codegen.core.Symbol,
    ): Writable =
        writable {
            if (member.isEventStream(model)) {
                val unionShape = model.expectShape(member.target, UnionShape::class.java)
                rustTemplate(
                    """
                    let unmarshaller = #{unmarshaller}::new(protocol.clone());
                    let mut streaming = <#{Receiver}>::new(unmarshaller, body);
                    #{apply_initial_request}(|message_type| streaming.try_recv_initial(message_type), &protocol, |deser| {
                        #{Input}::${ServerSchemaDeserializeGenerator.INTO_FUNCTION_NAME}(&mut builder, deser)
                    })
                    .await?;
                    """,
                    *codegenScope,
                    "Input" to inputSymbol,
                    "unmarshaller" to unmarshaller(unionShape),
                    "Receiver" to symbolProvider.toSymbol(member).mapRustType { it.stripOuter<RustType.Option>() },
                )
            } else {
                // A streaming blob is the body as is; the protocol has nothing to add.
                rustTemplate(
                    """
                    let _ = &protocol;
                    let streaming = #{ByteStream}::new(body);
                    """,
                    *codegenScope,
                )
            }
        }

    private fun finishInput(): Writable =
        writable {
            if (inputShape.canReachConstrainedShape(model, symbolProvider)) {
                // A constraint violation becomes the modeled validation exception, exactly as the collected path
                // and the legacy `RequestRejection` path do.
                rustTemplate(
                    """
                    builder.build().map_err(|constraint_violation| {
                        #{DeserializeError}::ConstraintViolation(
                            #{Box}::new(#{ValidationException}::from(constraint_violation)),
                        )
                    })
                    """,
                    *codegenScope,
                    "ValidationException" to validationExceptionConversionGenerator.validationExceptionSymbol(),
                )
            } else {
                rustTemplate("#{Ok}(builder.build())", *codegenScope)
            }
        }

    // ---- response side ----

    private fun serializeOutput(): Writable {
        val streamingMember = outputShape.findStreamingMember(model) ?: return collectedOutput()
        val field = symbolProvider.toMemberName(streamingMember)
        val optional = symbolProvider.toSymbol(streamingMember).isOptional()
        val take =
            writable {
                if (optional) {
                    rustTemplate("output.$field.take()")
                } else {
                    rustTemplate(
                        "#{Some}(#{replace}(&mut output.$field, #{placeholder}))",
                        "placeholder" to placeholder(streamingMember),
                        *codegenScope,
                    )
                }
            }
        return writable {
            rustTemplate(
                """
                let mut output = output;
                let body = match #{take} {
                    #{Some}(streaming) => { #{body:W} }
                    #{None} => #{empty}(),
                };
                protocol.serialize_streaming_response(<Self as #{SchemaOperationShape}>::SCHEMA.output(), &output, body)
                """,
                *codegenScope,
                "body" to streamingBody(streamingMember),
                "take" to take,
            )
        }
    }

    /** The output does not stream: the protocol serializes it in memory. */
    private fun collectedOutput(): Writable =
        writable {
            rustTemplate(
                "protocol.serialize_response(<Self as #{SchemaOperationShape}>::SCHEMA.output(), &output)",
                *codegenScope,
            )
        }

    /** A stand-in left behind in the output when its required streaming member is moved out. */
    private fun placeholder(member: MemberShape): Writable =
        writable {
            if (member.isEventStream(model)) {
                rustTemplate("#{EventStreamSender}::from(#{futures_util}::stream::empty())", *codegenScope)
            } else {
                rustTemplate("#{ByteStream}::new(#{SdkBody}::empty())", *codegenScope)
            }
        }

    private fun streamingBody(member: MemberShape): Writable =
        writable {
            if (!member.isEventStream(model)) {
                rustTemplate(
                    "#{boxed}(#{wrap_stream}(#{FuturesStreamCompatByteStream}::new(streaming)))",
                    *codegenScope,
                )
                return@writable
            }
            val unionShape = model.expectShape(member.target, UnionShape::class.java)
            // Whether an `initial-response` frame precedes the events is a service-level codegen setting, as
            // on the legacy path; whether the protocol frames initial messages at all is the protocol's answer
            // at runtime. Both are plain values handed to the runtime glue.
            val sendInitialResponse = codegenContext.settings.codegenConfig.alwaysSendEventStreamInitialResponse
            rustTemplate(
                """
                let marshaller = #{marshaller}::new(protocol.clone());
                let error_marshaller = #{error_marshaller}::new(protocol.clone());
                match #{event_stream_response_body}(
                    <Self as #{SchemaOperationShape}>::SCHEMA.output(),
                    &output,
                    streaming,
                    marshaller,
                    error_marshaller,
                    protocol,
                    #{InitialResponsePolicy}::${if (sendInitialResponse) "Send" else "Omit"},
                ) {
                    #{Ok}(body) => body,
                    #{Err}(err) => {
                        #{tracing}::error!(error = %err, "failed to serialize the event stream response");
                        return #{internal_server_error}();
                    }
                }
                """,
                *codegenScope,
                "marshaller" to marshaller(unionShape),
                "error_marshaller" to errorMarshaller(unionShape),
            )
        }

    // ---- the generated shape dispatch in `crate::event_stream_serde` ----

    /** The historical `crate::event_stream_serde::<Union>Marshaller` name, aliased to the runtime generic. */
    private fun marshaller(unionShape: UnionShape): RuntimeType {
        val unionSymbol = symbolProvider.toSymbol(unionShape)
        val name = "${unionSymbol.name.toPascalCase()}Marshaller"
        return RuntimeType.forInlineFun(name, eventStreamSerdeModule) {
            rustTemplate(
                """
                /// Marshals `${unionSymbol.name}` events into frames through the runtime-selected protocol.
                pub type $name = #{SchemaEventMarshaller}<#{Union}>;
                """,
                *codegenScope,
                "Union" to unionSymbol,
            )
        }
    }

    /**
     * The historical `crate::event_stream_serde::<Union>ErrorMarshaller` name, plus the
     * `SerializableEventError` dispatch naming each modeled error's `:exception-type`.
     */
    private fun errorMarshaller(unionShape: UnionShape): RuntimeType {
        val unionSymbol = symbolProvider.toSymbol(unionShape)
        val name = "${unionSymbol.name.toPascalCase()}ErrorMarshaller"
        if (unionShape.eventStreamErrors().isEmpty()) {
            return RuntimeType.forInlineFun(name, eventStreamSerdeModule) {
                rustTemplate(
                    """
                    /// `${unionSymbol.name}` models no errors; only an unmodeled stream failure can be marshalled.
                    pub type $name = #{NoModeledEventErrorMarshaller};
                    """,
                    *codegenScope,
                )
            }
        }
        val errorSymbol = symbolProvider.symbolForEventStreamError(unionShape)
        val errorsShape = unionShape.expectTrait<SyntheticEventStreamUnionTrait>()
        return RuntimeType.forInlineFun(name, eventStreamSerdeModule) {
            rustTemplate(
                """
                /// Marshals `${unionSymbol.name}` errors into `exception` frames through the runtime-selected protocol.
                pub type $name = #{SchemaEventErrorMarshaller}<#{OpError}>;

                impl #{SerializableEventError} for #{OpError} {
                    fn variant(&self) -> (&'static str, &#{Schema}<'_>, &dyn #{SerializableStruct}) {
                        match self {
                            #{arms:W}
                        }
                    }
                }
                """,
                *codegenScope,
                "OpError" to errorSymbol,
                "arms" to
                    writable {
                        errorsShape.errorMembers.forEach { member ->
                            val target = model.expectShape(member.target, StructureShape::class.java)
                            val targetSymbol = symbolProvider.toSymbol(target)
                            rustTemplate(
                                "Self::${targetSymbol.name}(inner) => (${member.memberName.dq()}, #{Target}::SCHEMA, inner),",
                                "Target" to targetSymbol,
                                *codegenScope,
                            )
                        }
                    },
            )
        }
    }

    /**
     * The historical `crate::event_stream_serde::<Union>Unmarshaller` name, plus the
     * `DeserializableEventStream` dispatch handing each frame to the named event struct's
     * schema-guided walker.
     */
    private fun unmarshaller(unionShape: UnionShape): RuntimeType {
        val unionSymbol = symbolProvider.toSymbol(unionShape)
        val name = "${unionSymbol.name.toPascalCase()}Unmarshaller"
        val errorSymbol =
            if (unionShape.eventStreamErrors().isEmpty()) {
                smithyHttp.resolve("event_stream::MessageStreamError").toSymbol()
            } else {
                symbolProvider.symbolForEventStreamError(unionShape)
            }
        return RuntimeType.forInlineFun(name, eventStreamSerdeModule) {
            rustTemplate(
                """
                /// Unmarshals `${unionSymbol.name}` frames through the runtime-selected protocol.
                pub type $name = #{SchemaEventUnmarshaller}<#{Union}>;

                impl #{DeserializableEventStream} for #{Union} {
                    type Error = #{OpError};

                    fn deserialize_event(
                        event_type: &str,
                        frame: &#{EventFrame}<'_>,
                    ) -> #{Result}<#{Option}<Self>, #{EventStreamError}> {
                        match event_type {
                            #{event_arms:W}
                            _ => #{Ok}(#{None}),
                        }
                    }

                    fn deserialize_error(
                        exception_type: &str,
                        frame: &#{EventFrame}<'_>,
                    ) -> #{Result}<#{Option}<Self::Error>, #{EventStreamError}> {
                        #{error_arms:W}
                    }
                }
                """,
                *codegenScope,
                "Union" to unionSymbol,
                "OpError" to errorSymbol,
                "event_arms" to eventArms(unionShape, unionSymbol),
                "error_arms" to errorArms(unionShape, errorSymbol),
            )
        }
    }

    private fun eventArms(
        unionShape: UnionShape,
        unionSymbol: software.amazon.smithy.codegen.core.Symbol,
    ): Writable =
        writable {
            unionShape.members().forEach { member ->
                val variantName = symbolProvider.toMemberName(member)
                if (member.isTargetUnit()) {
                    // Union members targeting the Smithy `Unit` type have no associated data.
                    rustTemplate(
                        "${member.memberName.dq()} => #{Ok}(#{Some}(#{Union}::$variantName)),",
                        *codegenScope,
                        "Union" to unionSymbol,
                    )
                    return@forEach
                }
                val target = model.expectShape(member.target, StructureShape::class.java)
                rustBlockTemplate("${member.memberName.dq()} =>", *codegenScope) {
                    rustTemplate(
                        """
                        let mut deser = frame.deserializer();
                        let parsed = #{Target}::${ServerSchemaDeserializeGenerator.FUNCTION_NAME}(&mut deser)
                            .map_err(|err| #{EventStreamError}::unmarshalling(format!("failed to unmarshall ${member.memberName}: {err}")))?;
                        #{finish:W}
                        #{Ok}(#{Some}(#{Union}::$variantName(parsed)))
                        """,
                        *codegenScope,
                        "Target" to symbolProvider.toSymbol(target),
                        "Union" to unionSymbol,
                        "finish" to constrainedBuild(target, "failed to unmarshall ${member.memberName} due to constraint violation: {err}"),
                    )
                }
            }
        }

    private fun errorArms(
        unionShape: UnionShape,
        errorSymbol: software.amazon.smithy.codegen.core.Symbol,
    ): Writable =
        writable {
            if (unionShape.eventStreamErrors().isEmpty()) {
                rustTemplate(
                    """
                    let _ = (exception_type, frame);
                    #{Ok}(#{None})
                    """,
                    *codegenScope,
                )
                return@writable
            }
            val errorsShape = unionShape.expectTrait<SyntheticEventStreamUnionTrait>()
            rustBlockTemplate("match exception_type", *codegenScope) {
                errorsShape.errorMembers.forEach { member ->
                    val target = model.expectShape(member.target, StructureShape::class.java)
                    rustBlockTemplate("${member.memberName.dq()} =>", *codegenScope) {
                        rustTemplate(
                            """
                            let mut deser = frame.deserializer();
                            let parsed = #{Target}::${ServerSchemaDeserializeGenerator.FUNCTION_NAME}(&mut deser)
                                .map_err(|err| #{EventStreamError}::unmarshalling(format!("failed to unmarshall exception: {err}")))?;
                            #{finish:W}
                            #{Ok}(#{Some}(#{OpError}::${target.id.name}(parsed)))
                            """,
                            *codegenScope,
                            "Target" to symbolProvider.toSymbol(target),
                            "OpError" to errorSymbol,
                            "finish" to constrainedBuild(target, "failed to unmarshall exception due to constraint violation: {err}"),
                        )
                    }
                }
                rustTemplate("_ => #{Ok}(#{None}),", *codegenScope)
            }
        }

    /**
     * The schema walker returns the unconstrained builder for shapes needing validation; `build()`
     * maps constraint violations to the historical unmarshalling error text.
     */
    private fun constrainedBuild(
        target: StructureShape,
        errorText: String,
    ): Writable =
        writable {
            if (target.canReachConstrainedShape(model, symbolProvider)) {
                rustTemplate(
                    """
                    let parsed = parsed.build()
                        .map_err(|err| #{EventStreamError}::unmarshalling(format!(${errorText.dq()})))?;
                    """,
                    *codegenScope,
                )
            }
        }
}
