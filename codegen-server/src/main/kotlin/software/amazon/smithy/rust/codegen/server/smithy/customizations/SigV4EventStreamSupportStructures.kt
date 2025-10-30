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
            "SigV4Unmarshaller" to sigV4Unmarshaller(runtimeConfig),
            "extract_signed_message" to extractSignedMessage(runtimeConfig),
        )

    /**
     * Wraps an event stream Receiver type to handle SigV4 signed messages.
     * Transforms: Receiver<T, E> -> Receiver<SignedEvent<T>, SignedEventError<E>>
     */
    fun wrapInEventStreamSigV4(
        symbol: Symbol,
        runtimeConfig: RuntimeConfig,
    ): Symbol {
        val signedEvent = signedEvent(runtimeConfig)
        val signedEventError = signedEventError(runtimeConfig)
        return symbol.mapRustType(signedEvent, signedEventError) { rustType ->
            // Expect Application(Receiver, [T, E])
            if (rustType is RustType.Application && rustType.name == "Receiver" && rustType.args.size == 2) {
                val eventType = rustType.args[0]
                val errorType = rustType.args[1]

                // Create SignedEvent<T> and SignedEventError<E>
                val wrappedEventType =
                    RustType.Application(
                        signedEvent.toSymbol().rustType(),
                        listOf(eventType),
                    )
                val wrappedErrorType =
                    RustType.Application(
                        signedEventError.toSymbol().rustType(),
                        listOf(errorType),
                    )

                // Create new Receiver<SignedEvent<T>, SignedEventError<E>>
                RustType.Application(
                    rustType.type,
                    listOf(wrappedEventType, wrappedErrorType),
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
                """,
                "EventStreamError" to CargoDependency.smithyEventStream(runtimeConfig).toType().resolve("error::Error"),
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

    private fun sigV4Unmarshaller(runtimeConfig: RuntimeConfig): RuntimeType =
        RuntimeType.forInlineFun("SigV4Unmarshaller", supportModule) {
            rustTemplate(
                """
                /// Unmarshaller wrapper that handles SigV4 signed event stream messages
                ##[derive(Debug)]
                pub struct SigV4Unmarshaller<T> {
                    inner: T,
                }

                impl<T> SigV4Unmarshaller<T> {
                    pub fn new(inner: T) -> Self {
                        Self { inner }
                    }
                }

                impl<T> #{UnmarshallMessage} for SigV4Unmarshaller<T>
                where
                    T: #{UnmarshallMessage},
                {
                    type Output = #{SignedEvent}<T::Output>;
                    type Error = #{SignedEventError}<T::Error>;

                    fn unmarshall(&self, message: &#{Message}) -> #{Result}<#{UnmarshalledMessage}<Self::Output, Self::Error>, #{EventStreamError}> {
                        // First, try to extract the signed message
                        match #{extract_signed_message}(message) {
                            Ok(MaybeSignedMessage::Signed { message: inner_message, signature }) => {
                                // Process the inner message with the base unmarshaller
                                match self.inner.unmarshall(&inner_message) {
                                    Ok(unmarshalled) => match unmarshalled {
                                        #{UnmarshalledMessage}::Event(event) => {
                                            Ok(#{UnmarshalledMessage}::Event(#{SignedEvent} {
                                                message: event,
                                                signature: Some(signature),
                                            }))
                                        }
                                        #{UnmarshalledMessage}::Error(err) => {
                                            Ok(#{UnmarshalledMessage}::Error(#{SignedEventError}::Event(err)))
                                        }
                                    },
                                    Err(err) => Err(err),
                                }
                            }
                            Ok(MaybeSignedMessage::Unsigned) => {
                                // Process unsigned message directly
                                match self.inner.unmarshall(message) {
                                    Ok(unmarshalled) => match unmarshalled {
                                        #{UnmarshalledMessage}::Event(event) => {
                                            Ok(#{UnmarshalledMessage}::Event(#{SignedEvent} {
                                                message: event,
                                                signature: None,
                                            }))
                                        }
                                        #{UnmarshalledMessage}::Error(err) => {
                                            Ok(#{UnmarshalledMessage}::Error(#{SignedEventError}::Event(err)))
                                        }
                                    },
                                    Err(err) => Err(err),
                                }
                            }
                            Err(extraction_err) => Ok(#{UnmarshalledMessage}::Error(#{SignedEventError}::InvalidSignedEvent(extraction_err))),
                        }
                    }
                }
                """,
                "UnmarshallMessage" to
                    CargoDependency.smithyEventStream(runtimeConfig).toType()
                        .resolve("frame::UnmarshallMessage"),
                "UnmarshalledMessage" to
                    CargoDependency.smithyEventStream(runtimeConfig).toType()
                        .resolve("frame::UnmarshalledMessage"),
                "Message" to CargoDependency.smithyTypes(runtimeConfig).toType().resolve("event_stream::Message"),
                "EventStreamError" to CargoDependency.smithyEventStream(runtimeConfig).toType().resolve("error::Error"),
                "SignedEvent" to signedEvent(runtimeConfig),
                "SignedEventError" to signedEventError(runtimeConfig),
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
