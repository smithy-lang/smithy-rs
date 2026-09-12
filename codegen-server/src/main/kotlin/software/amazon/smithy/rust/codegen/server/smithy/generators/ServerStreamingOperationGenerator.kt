/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.smithy.mapRustType
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.EventStreamUnmarshallerGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.EventStreamErrorMarshallerGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.EventStreamMarshallerGenerator
import software.amazon.smithy.rust.codegen.core.util.findStreamingMember
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.isEventStream
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.canReachConstrainedShape
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocol

/**
 * Renders the `StreamingOperationShape` implementation of an operation whose input or output carries an
 * event stream or a streaming blob, under the `schemaSerde` setting.
 *
 * Both halves work through the erased `SharedServerProtocol` handed in at runtime: the generated
 * marshallers and unmarshallers ask it for the payload codec and the event media type, and
 * `initial_messages_in_frames()` decides whether the non-stream members travel in `initial-request` and
 * `initial-response` frames. Nothing rendered here names a protocol.
 *
 * The half that does not stream is written in terms of the collected path: `DeserializableShape` for the
 * input, `ServerProtocol::serialize_response` for the output.
 */
class ServerStreamingOperationGenerator(
    private val codegenContext: ServerCodegenContext,
    private val protocol: ServerProtocol,
    private val operationShape: OperationShape,
    private val validationExceptionConversionGenerator: ValidationExceptionConversionGenerator,
) {
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val runtimeConfig = codegenContext.runtimeConfig
    private val smithyHttpServer = ServerCargoDependency.smithyHttpServer(runtimeConfig).toType()
    private val smithySchema = RuntimeType.smithySchema(runtimeConfig)
    private val smithyHttp = RuntimeType.smithyHttp(runtimeConfig)
    private val smithyEventStream = RuntimeType.smithyEventStream(runtimeConfig)
    private val serdeCustomization = ServerEventStreamSerdeCustomization(codegenContext)
    private val inputShape = operationShape.inputShape(model)
    private val outputShape = operationShape.outputShape(model)
    private val operationName = symbolProvider.toSymbol(operationShape).name
    private val codegenScope =
        arrayOf(
            *preludeScope,
            "Bytes" to RuntimeType.Bytes,
            "ByteStream" to RuntimeType.byteStream(runtimeConfig),
            "DeserializableShape" to smithyHttpServer.resolve("schema::DeserializableShape"),
            "DeserializeError" to smithyHttpServer.resolve("schema::DeserializeError"),
            "EventStreamSender" to smithyHttp.resolve("event_stream::EventStreamSender"),
            "FuturesStreamCompatByteStream" to smithyHttp.resolve("futures_stream_adapter::FuturesStreamCompatByteStream"),
            "InitialMessageType" to smithyHttp.resolve("event_stream::InitialMessageType"),
            "NoOpSigner" to smithyEventStream.resolve("frame::NoOpSigner"),
            "PayloadSerializer" to smithySchema.resolve("codec::PayloadSerializer"),
            "Response" to smithyHttpServer.resolve("response::Response"),
            "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
            "SerdeError" to smithySchema.resolve("serde::SerdeError"),
            "SharedServerProtocol" to smithyHttpServer.resolve("schema::SharedServerProtocol"),
            "ShapeDeserializer" to smithySchema.resolve("serde::ShapeDeserializer"),
            "ShapeSerializer" to smithySchema.resolve("serde::ShapeSerializer"),
            "SchemaOperationShape" to smithyHttpServer.resolve("operation::SchemaOperationShape"),
            "StreamingInputFuture" to smithyHttpServer.resolve("operation::StreamingInputFuture"),
            "StreamingOperationShape" to smithyHttpServer.resolve("operation::StreamingOperationShape"),
            "boxed" to smithyHttpServer.resolve("body::boxed"),
            "empty" to smithyHttpServer.resolve("body::empty"),
            "futures_util" to ServerCargoDependency.FuturesUtil.toType(),
            "http_body" to CargoDependency.HttpBody1x.toType(),
            "http_body_util" to CargoDependency.HttpBodyUtil01x.toType(),
            "internal_server_error" to smithyHttpServer.resolve("operation::empty_internal_server_error"),
            "ready" to RuntimeType.std.resolve("future::ready"),
            "replace" to RuntimeType.std.resolve("mem::replace"),
            "tracing" to RuntimeType.Tracing,
            "wrap_stream" to smithyHttpServer.resolve("body::wrap_stream"),
            "EventOrInitial" to smithyHttp.resolve("event_stream::EventOrInitial"),
            "EventOrInitialMarshaller" to smithyHttp.resolve("event_stream::EventOrInitialMarshaller"),
            "Message" to RuntimeType.smithyTypes(runtimeConfig).resolve("event_stream::Message"),
            "Header" to RuntimeType.smithyTypes(runtimeConfig).resolve("event_stream::Header"),
            "HeaderValue" to RuntimeType.smithyTypes(runtimeConfig).resolve("event_stream::HeaderValue"),
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
                    let unmarshaller = #{unmarshaller}(protocol.clone());
                    let mut streaming = <#{Receiver}>::new(unmarshaller, body);
                    let capability = protocol.event_stream().ok_or_else(||
                        #{DeserializeError}::Serde(#{SerdeError}::custom("protocol does not support event streams")))?;
                    if capability.initial_messages_in_frames() {
                        match streaming.try_recv_initial(#{InitialMessageType}::Request).await {
                            #{Ok}(#{Some}(initial)) => {
                                let mut deser = capability.payload_codec().create_deserializer(&initial.payload()[..]);
                                #{Input}::${ServerSchemaDeserializeGenerator.INTO_FUNCTION_NAME}(&mut builder, &mut *deser)?;
                            }
                            #{Ok}(#{None}) => {}
                            #{Err}(err) => {
                                return #{Err}(#{DeserializeError}::Serde(#{SerdeError}::custom(format!(
                                    "failed to read the initial-request frame: {err}"
                                ))));
                            }
                        }
                    }
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
            val payloadContentType =
                protocol.httpBindingResolver.eventStreamMessageContentType(member)
                    ?: throw CodegenException("event streams must set a content type")
            val marshallerGenerator = marshallerGenerator(unionShape, payloadContentType)
            // Whether an `initial-response` frame precedes the events is a service-level codegen setting, as
            // on the legacy path; whether the protocol frames initial messages at all is the protocol's answer
            // at runtime.
            val sendInitialResponse = codegenContext.settings.codegenConfig.alwaysSendEventStreamInitialResponse
            rustTemplate(
                """
                let marshaller = #{marshaller}(protocol.clone());
                let error_marshaller = #{error_marshaller}(protocol.clone());
                let signer = #{NoOpSigner} {};
                let #{Some}(capability) = protocol.event_stream() else {
                    #{tracing}::error!("protocol does not support event streams");
                    return #{internal_server_error}();
                };
                if capability.initial_messages_in_frames() && $sendInitialResponse {
                    use #{futures_util}::StreamExt;
                    let payload = {
                        let mut ser = capability.payload_codec().create_serializer();
                        if let #{Err}(err) = #{ShapeSerializer}::write_struct(&mut *ser, <Self as #{SchemaOperationShape}>::SCHEMA.output(), &output) {
                            #{tracing}::error!(error = %err, "failed to serialize the initial-response frame");
                            return #{internal_server_error}();
                        }
                        #{Bytes}::from(#{PayloadSerializer}::finish_boxed(ser))
                    };
                    let initial_message = #{Message}::new_from_parts(
                        vec![
                            #{Header}::new(":message-type", #{HeaderValue}::String("event".into())),
                            #{Header}::new(":event-type", #{HeaderValue}::String("initial-response".into())),
                            #{Header}::new(":content-type", #{HeaderValue}::String(capability.event_stream_media_type().to_string().into())),
                        ],
                        payload,
                    );
                    let initial = #{futures_util}::stream::iter([#{Ok}(#{EventOrInitial}::InitialMessage(initial_message))]);
                    let events = streaming.into_inner().map(|event| event.map(#{EventOrInitial}::Event));
                    let sender = #{EventStreamSender}::from(initial.chain(events));
                    let adapter = sender.into_body_stream(#{EventOrInitialMarshaller}::new(marshaller), error_marshaller, signer);
                    #{boxed}(#{http_body_util}::StreamBody::new(adapter))
                } else {
                    let adapter = streaming.into_body_stream(marshaller, error_marshaller, signer);
                    #{boxed}(#{http_body_util}::StreamBody::new(adapter))
                }
                """,
                *codegenScope,
                "marshaller" to marshallerGenerator.render(),
                "error_marshaller" to errorMarshaller(unionShape, payloadContentType),
            )
        }

    // ---- the schema-mode marshallers and unmarshallers ----

    private fun marshallerGenerator(
        unionShape: UnionShape,
        payloadContentType: String,
    ) = EventStreamMarshallerGenerator(
        model,
        CodegenTarget.SERVER,
        runtimeConfig,
        symbolProvider,
        unionShape,
        protocol.structuredDataSerializer(),
        payloadContentType,
        serdeCustomization = serdeCustomization,
    )

    private fun errorMarshaller(
        unionShape: UnionShape,
        payloadContentType: String,
    ): RuntimeType =
        EventStreamErrorMarshallerGenerator(
            model,
            CodegenTarget.SERVER,
            runtimeConfig,
            symbolProvider,
            unionShape,
            protocol.structuredDataSerializer(),
            payloadContentType,
            serdeCustomization = serdeCustomization,
        ).render()

    private fun unmarshaller(unionShape: UnionShape): RuntimeType =
        EventStreamUnmarshallerGenerator(
            protocol,
            codegenContext,
            operationShape,
            unionShape,
            serdeCustomization = serdeCustomization,
        ).render()
}
