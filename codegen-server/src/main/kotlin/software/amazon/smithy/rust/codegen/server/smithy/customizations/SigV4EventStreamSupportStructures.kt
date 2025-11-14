/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.customizations

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.mapRustType
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.util.PANIC

object SigV4EventStreamSupportStructures {
    private val supportModule = RustModule.private("sigv4_event_stream")

    fun codegenScope(runtimeConfig: RuntimeConfig) =
        arrayOf(
            "SignatureInfo" to signatureInfo(),
            "ExtractionError" to extractionError(runtimeConfig),
            "SignedEventError" to signedEventError(runtimeConfig),
            "SignedEvent" to signedEvent(runtimeConfig),
            "SigV4Receiver" to sigV4Receiver(runtimeConfig),
            "extract_signed_message" to extractSignedMessage(runtimeConfig),
        )

    /**
     * Wraps an event stream Receiver type with SigV4Receiver.
     * Transforms: Receiver<T, E> -> SigV4Receiver<T, E>
     */
    fun wrapInEventStreamSigV4(
        symbol: Symbol,
        runtimeConfig: RuntimeConfig,
    ): Symbol {
        val sigV4Receiver = sigV4Receiver(runtimeConfig)
        return symbol.mapRustType(sigV4Receiver) { rustType ->
            // Expect Application(Receiver, [T, E])
            if (rustType is RustType.Application && rustType.name == "Receiver" && rustType.args.size == 2) {
                val eventType = rustType.args[0]
                val errorType = rustType.args[1]

                // Create SigV4Receiver<T, E>
                RustType.Application(
                    sigV4Receiver.toSymbol().rustType(),
                    listOf(eventType, errorType),
                )
            } else {
                PANIC("Called wrap in EventStreamSigV4 on ${symbol.rustType()} which was not an event stream receiver")
            }
        }
    }

    private fun signatureInfo(): RuntimeType =
        RuntimeType.forInlineFun("SignatureInfo", supportModule) {
            rustTemplate(
                """
                /// Information extracted from a signed event stream message
                ##[non_exhaustive]
                ##[derive(Debug, Clone)]
                pub struct SignatureInfo {
                    /// The chunk signature bytes from the `:chunk-signature` header
                    pub chunk_signature: Vec<u8>,
                    /// The timestamp from the `:date` header
                    pub timestamp: #{SystemTime},
                }
                """,
                "SystemTime" to RuntimeType.std.resolve("time::SystemTime"),
            )
        }

    private fun extractionError(runtimeConfig: RuntimeConfig): RuntimeType =
        RuntimeType.forInlineFun("ExtractionError", supportModule) {
            rustTemplate(
                """
                /// Error type for signed message extraction operations
                ##[non_exhaustive]
                ##[derive(Debug)]
                pub enum ExtractionError {
                    /// The payload could not be decoded as a valid message
                    ##[non_exhaustive]
                    InvalidPayload {
                        error: #{EventStreamError},
                    },
                    /// The timestamp header is missing or has an invalid format
                    ##[non_exhaustive]
                    InvalidTimestamp,
                }

                impl #{Display} for ExtractionError {
                    fn fmt(&self, f: &mut #{Formatter}<'_>) -> #{fmt_Result} {
                        match self {
                            ExtractionError::InvalidPayload { error } => {
                                write!(f, "invalid payload: {}", error)
                            }
                            ExtractionError::InvalidTimestamp => {
                                write!(f, "invalid or missing timestamp header")
                            }
                        }
                    }
                }

                impl #{Error} for ExtractionError {
                    fn source(&self) -> #{Option}<&(dyn #{Error} + 'static)> {
                        match self {
                            ExtractionError::InvalidPayload { error } => #{Some}(error),
                            ExtractionError::InvalidTimestamp => #{None},
                        }
                    }
                }
                """,
                "EventStreamError" to CargoDependency.smithyEventStream(runtimeConfig).toType().resolve("error::Error"),
                "Display" to RuntimeType.Display,
                "Formatter" to RuntimeType.std.resolve("fmt::Formatter"),
                "fmt_Result" to RuntimeType.std.resolve("fmt::Result"),
                "Error" to RuntimeType.StdError,
                *RuntimeType.preludeScope,
            )
        }

    fun signedEventError(runtimeConfig: RuntimeConfig): RuntimeType =
        RuntimeType.forInlineFun("SignedEventError", supportModule) {
            rustTemplate(
                """
                /// Error wrapper for signed event stream errors
                ##[derive(Debug)]
                pub enum SignedEventError<E> {
                    /// Error from the underlying event stream
                    Event(E),
                    /// Error extracting signed message
                    InvalidSignedEvent(#{ExtractionError}),
                }

                impl<E> From<E> for SignedEventError<E> {
                    fn from(err: E) -> Self {
                        SignedEventError::Event(err)
                    }
                }
                """,
                "ExtractionError" to extractionError(runtimeConfig),
            )
        }

    fun signedEvent(runtimeConfig: RuntimeConfig): RuntimeType =
        RuntimeType.forInlineFun("SignedEvent", supportModule) {
            rustTemplate(
                """
                /// Wrapper for event stream messages that may be signed
                ##[derive(Debug)]
                pub struct SignedEvent<T> {
                    /// The actual event message
                    pub message: T,
                    /// Signature information if the message was signed
                    pub signature: #{Option}<#{SignatureInfo}>,
                }
                """,
                "Option" to RuntimeType.std.resolve("option::Option"),
                "SignatureInfo" to signatureInfo(),
            )
        }

    private fun sigV4Receiver(runtimeConfig: RuntimeConfig): RuntimeType =
        RuntimeType.forInlineFun("SigV4Receiver", supportModule) {
            rustTemplate(
                """
                /// Receiver wrapper that handles SigV4 signed event stream messages
                ##[derive(Debug)]
                pub struct SigV4Receiver<T, E> {
                    inner: #{Receiver}<T, E>,
                    initial_signature: #{Option}<#{SignatureInfo}>,
                }

                impl<T, E> SigV4Receiver<T, E> {
                    pub fn new(
                        unmarshaller: impl #{UnmarshallMessage}<Output = T, Error = E> + #{Send} + #{Sync} + 'static,
                        body: #{SdkBody},
                    ) -> Self {
                        Self {
                            inner: #{Receiver}::new(unmarshaller, body),
                            initial_signature: None,
                        }
                    }

                    /// Get the signature from the initial message, if it was signed
                    pub fn initial_signature(&self) -> #{Option}<&#{SignatureInfo}> {
                        self.initial_signature.as_ref()
                    }

                    /// Try to receive an initial message of the given type.
                    /// Handles SigV4-wrapped messages by extracting the inner message first.
                    pub async fn try_recv_initial(
                        &mut self,
                        message_type: #{event_stream}::InitialMessageType,
                    ) -> #{Result}<#{Option}<#{Message}>, #{SdkError}<E, #{RawMessage}>>
                    where
                        E: std::error::Error + 'static,
                    {
                        let result = self
                            .inner
                            .try_recv_initial_with_preprocessor(message_type, |message| {
                                match #{extract_signed_message}(&message) {
                                    #{Ok}(MaybeSignedMessage::Signed { message: inner, signature }) => {
                                        #{Ok}((inner, #{Some}(signature)))
                                    }
                                    #{Ok}(MaybeSignedMessage::Unsigned) => #{Ok}((message, #{None})),
                                    #{Err}(err) => #{Err}(#{ResponseError}::builder().raw(#{RawMessage}::Decoded(message)).source(err).build()),
                                }
                            })
                            .await?;
                        match result {
                            #{Some}((message, signature)) => {
                                self.initial_signature = signature;
                                #{Ok}(#{Some}(message))
                            }
                            #{None} => #{Ok}(#{None}),
                        }
                    }

                    /// Receive the next event from the stream
                    pub async fn recv(&mut self) -> #{Result}<#{Option}<#{SignedEvent}<T>>, #{SdkError}<#{SignedEventError}<E>, #{RawMessage}>>
                    where
                        E: std::error::Error + 'static,
                    {
                        match self.inner.recv().await.map_err(|e| e.map_service_error(#{SignedEventError}::Event))? {
                            #{Some}(event) => {
                                // Wrap in SignedEvent with no signature (signatures only on initial message)
                                #{Ok}(#{Some}(#{SignedEvent} {
                                    message: event,
                                    signature: #{None},
                                }))
                            }
                            #{None} => #{Ok}(#{None}),
                        }
                    }
                }
                """,
                "Receiver" to RuntimeType.eventStreamReceiver(runtimeConfig),
                "event_stream" to RuntimeType.smithyHttp(runtimeConfig).resolve("event_stream"),
                "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
                "Message" to CargoDependency.smithyTypes(runtimeConfig).toType().resolve("event_stream::Message"),
                "RawMessage" to CargoDependency.smithyTypes(runtimeConfig).toType().resolve("event_stream::RawMessage"),
                "SdkError" to RuntimeType.sdkError(runtimeConfig),
                "ResponseError" to RuntimeType.smithyRuntimeApiClient(runtimeConfig).resolve("client::result::ResponseError"),
                "UnmarshallMessage" to
                    CargoDependency.smithyEventStream(runtimeConfig).toType()
                        .resolve("frame::UnmarshallMessage"),
                "SignedEvent" to signedEvent(runtimeConfig),
                "SignedEventError" to signedEventError(runtimeConfig),
                "SignatureInfo" to signatureInfo(),
                "extract_signed_message" to extractSignedMessage(runtimeConfig),
                *RuntimeType.preludeScope,
            )
        }

    private fun extractSignedMessage(runtimeConfig: RuntimeConfig): RuntimeType =
        RuntimeType.forInlineFun("extract_signed_message", supportModule) {
            rustTemplate(
                """
                /// Result of extracting a potentially signed message
                ##[derive(Debug)]
                pub enum MaybeSignedMessage {
                    /// Message was signed and has been extracted
                    Signed {
                        /// The inner message that was signed
                        message: #{Message},
                        /// Signature information from the outer message
                        signature: #{SignatureInfo},
                    },
                    /// Message was not signed (no `:chunk-signature` header present)
                    Unsigned,
                }

                /// Extracts the inner message from a potentially signed event stream message.
                pub fn extract_signed_message(message: &#{Message}) -> #{Result}<MaybeSignedMessage, #{ExtractionError}> {
                    // Check if message has chunk signature
                    let mut chunk_signature = None;
                    let mut timestamp = None;

                    for header in message.headers() {
                        match header.name().as_str() {
                            ":chunk-signature" => {
                                if let #{HeaderValue}::ByteArray(bytes) = header.value() {
                                    chunk_signature = Some(bytes.as_ref().to_vec());
                                }
                            }
                            ":date" => {
                                if let #{HeaderValue}::Timestamp(ts) = header.value() {
                                    timestamp = Some(
                                        #{SystemTime}::try_from(*ts)
                                            .map_err(|_err| #{ExtractionError}::InvalidTimestamp)?,
                                    );
                                } else {
                                    return Err(#{ExtractionError}::InvalidTimestamp);
                                }
                            }
                            _ => {}
                        }
                    }

                    let Some(chunk_signature) = chunk_signature else {
                        return Ok(MaybeSignedMessage::Unsigned);
                    };

                    let Some(timestamp) = timestamp else {
                        return Err(#{ExtractionError}::InvalidTimestamp);
                    };

                    // Extract inner message
                    let cursor = #{Cursor}::new(message.payload());
                    let inner_message = #{read_message_from}(cursor)
                        .map_err(|err| #{ExtractionError}::InvalidPayload { error: err })?;

                    Ok(MaybeSignedMessage::Signed {
                        message: inner_message,
                        signature: #{SignatureInfo} {
                            chunk_signature,
                            timestamp,
                        },
                    })
                }
                """,
                "Message" to CargoDependency.smithyTypes(runtimeConfig).toType().resolve("event_stream::Message"),
                "HeaderValue" to
                    CargoDependency.smithyTypes(runtimeConfig).toType()
                        .resolve("event_stream::HeaderValue"),
                "SystemTime" to RuntimeType.std.resolve("time::SystemTime"),
                "Cursor" to RuntimeType.std.resolve("io::Cursor"),
                "read_message_from" to
                    CargoDependency.smithyEventStream(runtimeConfig).toType()
                        .resolve("frame::read_message_from"),
                "SignatureInfo" to signatureInfo(),
                "ExtractionError" to extractionError(runtimeConfig),
                *RuntimeType.preludeScope,
            )
        }
}
