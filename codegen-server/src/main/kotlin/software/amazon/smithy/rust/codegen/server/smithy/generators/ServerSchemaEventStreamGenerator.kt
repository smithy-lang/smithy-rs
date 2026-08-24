/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.protocols.shapeModuleName
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticEventStreamUnionTrait
import software.amazon.smithy.rust.codegen.core.util.expectTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.canReachConstrainedShape

/**
 * Generates the schema-driven event-stream frame serde for one stream union
 * (plan Step 4.8, the decided `Marshaller<P>`/`Unmarshaller<P>` design):
 *
 * - `{Union}Marshaller<P>` / `{Union}ErrorMarshaller<P>`: marshal outgoing
 *   events / modeled stream errors into frames. Generic over
 *   `P: EventStreamProtocol`; the only facts baked in are the `:event-type` /
 *   `:exception-type` strings (union member names) and which structure each
 *   variant maps to. Frame layout (`@eventHeader` members → frame headers,
 *   `@eventPayload` / body members → payload via `P::codec()`,
 *   `:content-type` from `P::EVENT_PAYLOAD_CONTENT_TYPE`) is interpreted at
 *   runtime by `aws_smithy_http_server::protocol::event_bindings` off the
 *   event structure's schema.
 * - `{Union}Unmarshaller<P>`: unmarshals incoming frames by driving the event
 *   structure's schema walker with the runtime frame composite. Server
 *   semantics: an unknown `:event-type` is an error (no `Unknown` arm), and
 *   event structures `build()` at unmarshal time — a constraint violation in
 *   a frame is a stream error (the legacy path could not even compile
 *   constrained events, assumptions register A1).
 */
class ServerSchemaEventStreamGenerator(
    private val codegenContext: ServerCodegenContext,
    private val writer: RustWriter,
    private val shape: UnionShape,
) {
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val runtimeConfig = codegenContext.runtimeConfig
    private val unionSymbol = symbolProvider.toSymbol(shape)

    private val smithyHttpServer = ServerCargoDependency.smithyHttpServer(runtimeConfig).toType()
    private val eventBindings = smithyHttpServer.resolve("protocol::event_bindings")
    private val eventStreamProtocol =
        smithyHttpServer.resolve("protocol::server_protocol::EventStreamProtocol")
    private val serverProtocol =
        smithyHttpServer.resolve("protocol::server_protocol::ServerProtocol")
    private val smithyEventStream = RuntimeType.smithyEventStream(runtimeConfig)
    private val smithyHttp = RuntimeType.smithyHttp(runtimeConfig)

    private val errorMembers =
        shape.expectTrait<SyntheticEventStreamUnionTrait>().errorMembers

    /** The sender/receiver error type: the generated stream-error enum, or the framework's. */
    private fun streamErrorType(): String =
        if (errorMembers.isEmpty()) {
            "::aws_smithy_http::event_stream::MessageStreamError"
        } else {
            symbolProvider.symbolForEventStreamError(shape).fullName
        }

    private fun deserFnPath(target: StructureShape): String =
        "crate::schema_serde::${symbolProvider.shapeModuleName(codegenContext.serviceShape, target)}::" +
            "deser_${symbolProvider.toSymbol(target).name.toSnakeCase()}"

    /** Renders the marshaller + error marshaller (output direction). */
    fun renderMarshallers() {
        val name = "${unionSymbol.name}Marshaller"
        val arms =
            writable {
                shape.members().forEach { member ->
                    val variantName = symbolProvider.toMemberName(member)
                    val target = model.expectShape(member.target, StructureShape::class.java)
                    val targetSymbol = symbolProvider.toSymbol(target)
                    rustTemplate(
                        """
                        Self::Input::$variantName(inner) => #{event_bindings}::marshall_event(
                            P::codec(),
                            "event",
                            ":event-type",
                            "${member.memberName}",
                            P::EVENT_PAYLOAD_CONTENT_TYPE,
                            ${targetSymbol.fullName}::SCHEMA,
                            &inner,
                        ),
                        """,
                        "event_bindings" to eventBindings,
                    )
                }
            }
        writer.rustTemplate(
            """
            /// Schema-driven event marshaller for [`${unionSymbol.name}`](#{Union}),
            /// generic over the protocol (plan Step 4.8).
            ##[non_exhaustive]
            pub struct $name<P> {
                _marker: ::std::marker::PhantomData<fn() -> P>,
            }

            impl<P> ::std::fmt::Debug for $name<P> {
                fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                    f.debug_struct("$name").finish()
                }
            }

            impl<P> $name<P> {
                /// Creates a new marshaller.
                pub fn new() -> Self {
                    Self { _marker: ::std::marker::PhantomData }
                }
            }

            impl<P> ::std::default::Default for $name<P> {
                fn default() -> Self {
                    Self::new()
                }
            }

            impl<P: #{EventStreamProtocol}> #{MarshallMessage} for $name<P> {
                type Input = #{Union};
                fn marshall(
                    &self,
                    input: Self::Input,
                ) -> ::std::result::Result<#{Message}, #{EventStreamErrorType}> {
                    match input {
                        #{arms:W}
                    }
                }
            }
            """,
            "Union" to unionSymbol,
            "EventStreamProtocol" to eventStreamProtocol,
            "MarshallMessage" to smithyEventStream.resolve("frame::MarshallMessage"),
            "Message" to RuntimeType.smithyTypes(runtimeConfig).resolve("event_stream::Message"),
            "EventStreamErrorType" to smithyEventStream.resolve("error::Error"),
            "arms" to arms,
        )
        renderErrorMarshaller()
    }

    private fun renderErrorMarshaller() {
        val name = "${unionSymbol.name}ErrorMarshaller"
        val body =
            writable {
                if (errorMembers.isEmpty()) {
                    // No modeled stream errors: the send channel's error type is the
                    // framework's `MessageStreamError`; mirror the legacy frame shape
                    // (bare `:message-type: exception`, empty payload).
                    rustTemplate(
                        """
                        let _ = input;
                        let headers = vec![#{Header}::new(
                            ":message-type",
                            #{HeaderValue}::String("exception".into()),
                        )];
                        Ok(#{Message}::new_from_parts(headers, ::std::vec::Vec::new()))
                        """,
                        "Header" to RuntimeType.smithyTypes(runtimeConfig).resolve("event_stream::Header"),
                        "HeaderValue" to
                            RuntimeType.smithyTypes(runtimeConfig).resolve("event_stream::HeaderValue"),
                        "Message" to RuntimeType.smithyTypes(runtimeConfig).resolve("event_stream::Message"),
                    )
                } else {
                    val errorEnum = symbolProvider.symbolForEventStreamError(shape)
                    val arms =
                        writable {
                            errorMembers.forEach { member ->
                                val target = model.expectShape(member.target, StructureShape::class.java)
                                val targetSymbol = symbolProvider.toSymbol(target)
                                rustTemplate(
                                    """
                                    ${errorEnum.fullName}::${targetSymbol.name}(inner) => #{event_bindings}::marshall_event(
                                        P::codec(),
                                        "exception",
                                        ":exception-type",
                                        "${member.memberName}",
                                        P::EVENT_PAYLOAD_CONTENT_TYPE,
                                        ${targetSymbol.fullName}::SCHEMA,
                                        &inner,
                                    ),
                                    """,
                                    "event_bindings" to eventBindings,
                                )
                            }
                        }
                    rustTemplate(
                        """
                        match input {
                            #{arms:W}
                        }
                        """,
                        "arms" to arms,
                    )
                }
            }
        writer.rustTemplate(
            """
            /// Schema-driven stream-error marshaller for [`${unionSymbol.name}`](#{Union}).
            ##[non_exhaustive]
            pub struct $name<P> {
                _marker: ::std::marker::PhantomData<fn() -> P>,
            }

            impl<P> ::std::fmt::Debug for $name<P> {
                fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                    f.debug_struct("$name").finish()
                }
            }

            impl<P> $name<P> {
                /// Creates a new marshaller.
                pub fn new() -> Self {
                    Self { _marker: ::std::marker::PhantomData }
                }
            }

            impl<P> ::std::default::Default for $name<P> {
                fn default() -> Self {
                    Self::new()
                }
            }

            impl<P: #{EventStreamProtocol}> #{MarshallMessage} for $name<P> {
                type Input = ${streamErrorType()};
                fn marshall(
                    &self,
                    input: Self::Input,
                ) -> ::std::result::Result<#{Message}, #{EventStreamErrorType}> {
                    #{body:W}
                }
            }
            """,
            "Union" to unionSymbol,
            "EventStreamProtocol" to eventStreamProtocol,
            "MarshallMessage" to smithyEventStream.resolve("frame::MarshallMessage"),
            "Message" to RuntimeType.smithyTypes(runtimeConfig).resolve("event_stream::Message"),
            "EventStreamErrorType" to smithyEventStream.resolve("error::Error"),
            "body" to body,
        )
    }

    /** Renders the unmarshaller (input direction). */
    fun renderUnmarshaller() {
        val name = "${unionSymbol.name}Unmarshaller"
        val arms =
            writable {
                shape.members().forEach { member ->
                    val variantName = symbolProvider.toMemberName(member)
                    val target = model.expectShape(member.target, StructureShape::class.java)
                    val fallible = target.canReachConstrainedShape(model, symbolProvider)
                    // The walker produces the parse symbol; constrained events run
                    // their `build()` here — a constraint violation in a frame is a
                    // stream error, strict server semantics.
                    val buildExpr =
                        if (fallible) {
                            """
                            let value = parsed
                                .build()
                                .map_err(|err| #{EventStreamErrorType}::unmarshalling(format!("{err}")))?;
                            """
                        } else {
                            "let value = parsed;"
                        }
                    rustTemplate(
                        """
                        "${member.memberName}" => {
                            let mut deserializer = #{event_bindings}::EventFrameDeserializer::new(P::codec(), message);
                            let parsed = ${deserFnPath(target)}(&mut deserializer)
                                .map_err(|err| #{EventStreamErrorType}::unmarshalling(format!("{err}")))?;
                            $buildExpr
                            Ok(#{UnmarshalledMessage}::Event(#{Union}::$variantName(value)))
                        }
                        """,
                        "event_bindings" to eventBindings,
                        "EventStreamErrorType" to smithyEventStream.resolve("error::Error"),
                        "UnmarshalledMessage" to smithyEventStream.resolve("frame::UnmarshalledMessage"),
                        "Union" to unionSymbol,
                    )
                }
            }
        // Modeled stream errors sent BY THE CLIENT unmarshal into the stream
        // error enum (the receiver surfaces them as service errors), mirroring
        // the legacy generated unmarshallers.
        val errorArms =
            writable {
                errorMembers.forEach { member ->
                    val target = model.expectShape(member.target, StructureShape::class.java)
                    val targetSymbol = symbolProvider.toSymbol(target)
                    val errorEnum = symbolProvider.symbolForEventStreamError(shape)
                    val fallible = target.canReachConstrainedShape(model, symbolProvider)
                    val buildExpr =
                        if (fallible) {
                            """
                            let value = parsed
                                .build()
                                .map_err(|err| #{EventStreamErrorType}::unmarshalling(format!("{err}")))?;
                            """
                        } else {
                            "let value = parsed;"
                        }
                    rustTemplate(
                        """
                        "${member.memberName}" => {
                            let mut deserializer = #{event_bindings}::EventFrameDeserializer::new(P::codec(), message);
                            let parsed = ${deserFnPath(target)}(&mut deserializer)
                                .map_err(|err| #{EventStreamErrorType}::unmarshalling(format!("{err}")))?;
                            $buildExpr
                            Ok(#{UnmarshalledMessage}::Error(${errorEnum.fullName}::${targetSymbol.name}(value)))
                        }
                        """,
                        "event_bindings" to eventBindings,
                        "EventStreamErrorType" to smithyEventStream.resolve("error::Error"),
                        "UnmarshalledMessage" to smithyEventStream.resolve("frame::UnmarshalledMessage"),
                    )
                }
            }
        writer.rustTemplate(
            """
            /// Schema-driven event unmarshaller for [`${unionSymbol.name}`](#{Union}),
            /// generic over the protocol (plan Step 4.8). Server semantics: an
            /// unknown `:event-type` is an error — there is no `Unknown` arm.
            ##[non_exhaustive]
            pub struct $name<P> {
                _marker: ::std::marker::PhantomData<fn() -> P>,
            }

            impl<P> ::std::fmt::Debug for $name<P> {
                fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                    f.debug_struct("$name").finish()
                }
            }

            impl<P> $name<P> {
                /// Creates a new unmarshaller.
                pub fn new() -> Self {
                    Self { _marker: ::std::marker::PhantomData }
                }
            }

            impl<P> ::std::default::Default for $name<P> {
                fn default() -> Self {
                    Self::new()
                }
            }

            impl<P: #{EventStreamProtocol}> #{UnmarshallMessage} for $name<P> {
                type Output = #{Union};
                type Error = ${streamErrorType()};
                fn unmarshall(
                    &self,
                    message: &#{Message},
                ) -> ::std::result::Result<#{UnmarshalledMessage}<Self::Output, Self::Error>, #{EventStreamErrorType}> {
                    let response_headers = #{parse_response_headers}(message)?;
                    match response_headers.message_type.as_str() {
                        "event" => match response_headers.smithy_type.as_str() {
                            #{arms:W}
                            _unknown_variant => Err(#{EventStreamErrorType}::unmarshalling(
                                format!("unrecognized :event-type: {_unknown_variant}"),
                            )),
                        },
                        "exception" => match response_headers.smithy_type.as_str() {
                            #{error_arms:W}
                            _unknown_exception => Err(#{EventStreamErrorType}::unmarshalling(
                                format!("unrecognized exception: {_unknown_exception}"),
                            )),
                        },
                        value => Err(#{EventStreamErrorType}::unmarshalling(
                            format!("unrecognized :message-type: {value}"),
                        )),
                    }
                }
            }
            """,
            "Union" to unionSymbol,
            "EventStreamProtocol" to eventStreamProtocol,
            "UnmarshallMessage" to smithyEventStream.resolve("frame::UnmarshallMessage"),
            "UnmarshalledMessage" to smithyEventStream.resolve("frame::UnmarshalledMessage"),
            "Message" to RuntimeType.smithyTypes(runtimeConfig).resolve("event_stream::Message"),
            "EventStreamErrorType" to smithyEventStream.resolve("error::Error"),
            "parse_response_headers" to smithyEventStream.resolve("smithy::parse_response_headers"),
            "arms" to arms,
            "error_arms" to errorArms,
        )
    }

    companion object {
        /** True when [shape] is an event-stream union (normalizer-tagged). */
        fun isEventStreamUnion(shape: software.amazon.smithy.model.shapes.Shape): Boolean =
            shape is UnionShape && shape.hasTrait<SyntheticEventStreamUnionTrait>()
    }
}
