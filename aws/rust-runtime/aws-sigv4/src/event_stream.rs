/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Utilities to sign Event Stream messages.
//!
//! # Example: Signing an event stream message
//!
//! ```rust
//! use aws_sigv4::event_stream::sign_message;
//! use aws_smithy_types::event_stream::{Header, HeaderValue, Message};
//! use std::time::SystemTime;
//! use aws_credential_types::Credentials;
//! use aws_smithy_runtime_api::client::identity::Identity;
//! use aws_sigv4::sign::v4;
//!
//! // The `last_signature` argument is the previous message's signature, or
//! // the signature of the initial HTTP request if a message hasn't been signed yet.
//! let last_signature = "example298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
//!
//! let message_to_sign = Message::new(&b"example"[..]).add_header(Header::new(
//!     "some-header",
//!     HeaderValue::String("value".into()),
//! ));
//!
//! let identity = Credentials::new(
//!     "AKIDEXAMPLE",
//!     "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
//!     None,
//!     None,
//!     "hardcoded-credentials"
//! ).into();
//! let params = v4::SigningParams::builder()
//!     .identity(&identity)
//!     .region("us-east-1")
//!     .name("exampleservice")
//!     .time(SystemTime::now())
//!     .settings(())
//!     .build()
//!     .unwrap();
//!
//! // Use the returned `signature` to sign the next message.
//! let (signed, signature) = sign_message(&message_to_sign, &last_signature, &params)
//!     .expect("signing should succeed")
//!     .into_parts();
//! ```

use crate::date_time::{format_date, format_date_time, truncate_subsecs};
use crate::http_request::SigningError;
use crate::sign::v4::{calculate_signature, generate_signing_key, sha256_hex_string};
use crate::SigningOutput;
use aws_credential_types::Credentials;
use aws_smithy_eventstream::error::Error;
use aws_smithy_eventstream::frame::{read_message_from, write_headers_to, write_message_to};
use aws_smithy_eventstream::frame::{UnmarshallMessage, UnmarshalledMessage};
use aws_smithy_types::event_stream::{Header, HeaderValue, Message};
use bytes::Bytes;
use std::io::{Cursor, Write};
use std::time::SystemTime;

/// Event stream signing parameters
pub type SigningParams<'a> = crate::sign::v4::SigningParams<'a, ()>;

/// Creates a string to sign for an Event Stream message.
fn calculate_string_to_sign(
    message_payload: &[u8],
    last_signature: &str,
    time: SystemTime,
    params: &SigningParams<'_>,
) -> Vec<u8> {
    // Event Stream string to sign format is documented here:
    // https://docs.aws.amazon.com/transcribe/latest/dg/how-streaming.html
    let date_time_str = format_date_time(time);
    let date_str = format_date(time);

    let mut sts: Vec<u8> = Vec::new();
    writeln!(sts, "AWS4-HMAC-SHA256-PAYLOAD").unwrap();
    writeln!(sts, "{}", date_time_str).unwrap();
    writeln!(
        sts,
        "{}/{}/{}/aws4_request",
        date_str, params.region, params.name
    )
    .unwrap();
    writeln!(sts, "{}", last_signature).unwrap();

    let date_header = Header::new(":date", HeaderValue::Timestamp(time.into()));
    let mut date_buffer = Vec::new();
    write_headers_to(&[date_header], &mut date_buffer).unwrap();
    writeln!(sts, "{}", sha256_hex_string(&date_buffer)).unwrap();
    write!(sts, "{}", sha256_hex_string(message_payload)).unwrap();
    sts
}

/// Signs an Event Stream message with the given `credentials`.
///
/// Each message's signature incorporates the signature of the previous message (`last_signature`).
/// The very first message incorporates the signature of the top-level request
/// for both HTTP 2 and WebSocket.
pub fn sign_message<'a>(
    message: &'a Message,
    last_signature: &'a str,
    params: &'a SigningParams<'a>,
) -> Result<SigningOutput<Message>, SigningError> {
    let message_payload = {
        let mut payload = Vec::new();
        write_message_to(message, &mut payload).unwrap();
        payload
    };
    sign_payload(Some(message_payload), last_signature, params)
}

/// Returns a signed empty message
///
/// Empty signed event stream messages differ from normal signed event stream
/// in that the payload is 0-bytes rather than a nested message. There is no way
/// to create a signed empty message using [`sign_message`].
pub fn sign_empty_message<'a>(
    last_signature: &'a str,
    params: &'a SigningParams<'a>,
) -> Result<SigningOutput<Message>, SigningError> {
    sign_payload(None, last_signature, params)
}

fn sign_payload<'a>(
    message_payload: Option<Vec<u8>>,
    last_signature: &'a str,
    params: &'a SigningParams<'a>,
) -> Result<SigningOutput<Message>, SigningError> {
    // Truncate the sub-seconds up front since the timestamp written to the signed message header
    // needs to exactly match the string formatted timestamp, which doesn't include sub-seconds.
    let time = truncate_subsecs(params.time);
    let creds = params
        .identity
        .data::<Credentials>()
        .ok_or_else(SigningError::unsupported_identity_type)?;

    let signing_key =
        generate_signing_key(creds.secret_access_key(), time, params.region, params.name);
    let string_to_sign = calculate_string_to_sign(
        message_payload.as_ref().map(|v| &v[..]).unwrap_or(&[]),
        last_signature,
        time,
        params,
    );
    let signature = calculate_signature(signing_key, &string_to_sign);
    tracing::trace!(canonical_request = ?message_payload, string_to_sign = ?string_to_sign, "calculated signing parameters");

    // Generate the signed wrapper event frame
    Ok(SigningOutput::new(
        Message::new(message_payload.map(Bytes::from).unwrap_or_default())
            .add_header(Header::new(
                ":chunk-signature",
                HeaderValue::ByteArray(hex::decode(&signature).unwrap().into()),
            ))
            .add_header(Header::new(":date", HeaderValue::Timestamp(time.into()))),
        signature,
    ))
}

/// Error type for signed message extraction operations
#[derive(Debug)]
pub enum ExtractionError {
    /// The payload could not be decoded as a valid message
    InvalidPayload(Option<Error>),
    /// The timestamp header is missing or has an invalid format
    InvalidTimestamp,
}

/// Information extracted from a signed event stream message
#[derive(Debug, Clone)]
pub struct SignatureInfo {
    /// The chunk signature bytes from the `:chunk-signature` header
    pub chunk_signature: Bytes,
    /// The timestamp from the `:date` header
    pub timestamp: SystemTime,
}

/// Result of extracting a potentially signed message (raw API)
#[derive(Debug)]
pub enum MaybeSignedMessage {
    /// Message was signed and has been extracted
    Signed {
        /// The inner message that was signed
        message: Message,
        /// Signature information from the outer message
        signature: SignatureInfo,
    },
    /// Message was not signed (no `:chunk-signature` header present)
    Unsigned,
}

/// Extracts the inner message from a potentially signed event stream message.
///
/// This is the "raw" API that only handles message extraction. For a higher-level
/// API that integrates with unmarshallers, see [`SigV4Unmarshaller`].
pub fn extract_signed_message(message: &Message) -> Result<MaybeSignedMessage, ExtractionError> {
    // Check if message has chunk signature
    let mut chunk_signature = None;
    let mut timestamp = None;

    for header in message.headers() {
        match header.name().as_str() {
            ":chunk-signature" => {
                if let HeaderValue::ByteArray(bytes) = header.value() {
                    chunk_signature = Some(bytes.clone());
                }
            }
            ":date" => {
                if let HeaderValue::Timestamp(ts) = header.value() {
                    timestamp = Some(
                        SystemTime::try_from(*ts)
                            // this only occurs if the datetime is thousands of years in the future
                            .map_err(|_err| ExtractionError::InvalidTimestamp)?,
                    );
                } else {
                    return Err(ExtractionError::InvalidTimestamp);
                }
            }
            _ => {}
        }
    }

    let Some(chunk_signature) = chunk_signature else {
        return Ok(MaybeSignedMessage::Unsigned);
    };

    let Some(timestamp) = timestamp else {
        return Err(ExtractionError::InvalidTimestamp);
    };

    // Extract inner message
    let cursor = Cursor::new(message.payload());
    let inner_message =
        read_message_from(cursor).map_err(|err| ExtractionError::InvalidPayload(Some(err)))?;

    Ok(MaybeSignedMessage::Signed {
        message: inner_message,
        signature: SignatureInfo {
            chunk_signature,
            timestamp,
        },
    })
}

/// A wrapper unmarshaller that handles SigV4 signed event stream messages
#[derive(Debug)]
pub struct SigV4Unmarshaller<I> {
    inner: I,
}

impl<I> SigV4Unmarshaller<I> {
    /// Creates a new SigV4 unmarshaller wrapping the given inner unmarshaller
    pub fn new(inner: I) -> Self {
        Self { inner }
    }
}

/// An event that may or may not be signed, containing the unmarshalled message
#[derive(Debug)]
pub struct SignedEvent<T> {
    /// Signature information if the message was signed, None if unsigned
    pub signature: Option<SignatureInfo>,
    /// The unmarshalled message content
    pub message: T,
}

/// Error type for the SigV4 unmarshaller
#[derive(Debug)]
pub enum SignedEventError<E> {
    /// Error from the inner unmarshaller when parsing the message
    ParseError(E),
    /// Error when extracting the signed message (invalid signature format, etc.)
    InvalidSignedEvent(ExtractionError),
}

// Note: This would need to be implemented when we have access to the UnmarshallMessage trait
impl<I> UnmarshallMessage for SigV4Unmarshaller<I>
where
    I: UnmarshallMessage,
{
    type Output = SignedEvent<I::Output>;
    type Error = SignedEventError<I::Error>;

    fn unmarshall(
        &self,
        message: &Message,
    ) -> Result<UnmarshalledMessage<Self::Output, Self::Error>, Error> {
        let extract_result = match extract_signed_message(message) {
            Ok(result) => result,
            Err(extraction_error) => {
                return Ok(UnmarshalledMessage::Error(
                    SignedEventError::InvalidSignedEvent(extraction_error),
                ));
            }
        };

        match extract_result {
            MaybeSignedMessage::Signed { message, signature } => {
                match self.inner.unmarshall(&message)? {
                    UnmarshalledMessage::Event(inner_result) => {
                        Ok(UnmarshalledMessage::Event(SignedEvent {
                            signature: Some(signature),
                            message: inner_result,
                        }))
                    }
                    UnmarshalledMessage::Error(inner_error) => Ok(UnmarshalledMessage::Error(
                        SignedEventError::ParseError(inner_error),
                    )),
                }
            }
            MaybeSignedMessage::Unsigned => match self.inner.unmarshall(message)? {
                UnmarshalledMessage::Event(inner_result) => {
                    Ok(UnmarshalledMessage::Event(SignedEvent {
                        signature: None,
                        message: inner_result,
                    }))
                }
                UnmarshalledMessage::Error(inner_error) => Ok(UnmarshalledMessage::Error(
                    SignedEventError::ParseError(inner_error),
                )),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::event_stream::{calculate_string_to_sign, sign_message, SigningParams};
    use crate::sign::v4::sha256_hex_string;
    use aws_credential_types::Credentials;
    use aws_smithy_eventstream::frame::write_message_to;
    use aws_smithy_types::event_stream::{Header, HeaderValue, Message};
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn string_to_sign() {
        let message_to_sign = Message::new(&b"test payload"[..]).add_header(Header::new(
            "some-header",
            HeaderValue::String("value".into()),
        ));
        let mut message_payload = Vec::new();
        write_message_to(&message_to_sign, &mut message_payload).unwrap();

        let params = SigningParams {
            identity: &Credentials::for_tests().into(),
            region: "us-east-1",
            name: "testservice",
            time: (UNIX_EPOCH + Duration::new(123_456_789_u64, 1234u32)),
            settings: (),
        };

        let expected = "\
            AWS4-HMAC-SHA256-PAYLOAD\n\
            19731129T213309Z\n\
            19731129/us-east-1/testservice/aws4_request\n\
            be1f8c7d79ef8e1abc5254a2c70e4da3bfaf4f07328f527444e1fc6ea67273e2\n\
            0c0e3b3bf66b59b976181bd7d401927bbd624107303c713fd1e5f3d3c8dd1b1e\n\
            f2eba0f2e95967ee9fbc6db5e678d2fd599229c0d04b11e4fc8e0f2a02a806c6\
        ";

        let last_signature = sha256_hex_string(b"last message sts");
        assert_eq!(
            expected,
            std::str::from_utf8(&calculate_string_to_sign(
                &message_payload,
                &last_signature,
                params.time,
                &params
            ))
            .unwrap()
        );
    }

    #[test]
    fn sign() {
        let message_to_sign = Message::new(&b"test payload"[..]).add_header(Header::new(
            "some-header",
            HeaderValue::String("value".into()),
        ));
        let params = SigningParams {
            identity: &Credentials::for_tests().into(),
            region: "us-east-1",
            name: "testservice",
            time: (UNIX_EPOCH + Duration::new(123_456_789_u64, 1234u32)),
            settings: (),
        };

        let last_signature = sha256_hex_string(b"last message sts");
        let (signed, signature) = sign_message(&message_to_sign, &last_signature, &params)
            .unwrap()
            .into_parts();
        assert_eq!(":chunk-signature", signed.headers()[0].name().as_str());
        if let HeaderValue::ByteArray(bytes) = signed.headers()[0].value() {
            assert_eq!(signature, hex::encode(bytes));
        } else {
            panic!("expected byte array for :chunk-signature header");
        }
        assert_eq!(":date", signed.headers()[1].name().as_str());
        if let HeaderValue::Timestamp(value) = signed.headers()[1].value() {
            assert_eq!(123_456_789_i64, value.secs());
            // The subseconds should have been truncated off
            assert_eq!(0, value.subsec_nanos());
        } else {
            panic!("expected timestamp for :date header");
        }
    }

    #[test]
    fn sign_and_unsign_roundtrip() {
        let original_message = Message::new(&b"test payload"[..]).add_header(Header::new(
            "some-header",
            HeaderValue::String("value".into()),
        ));
        let params = SigningParams {
            identity: &Credentials::for_tests().into(),
            region: "us-east-1",
            name: "testservice",
            time: (UNIX_EPOCH + Duration::new(123_456_789_u64, 1234u32)),
            settings: (),
        };

        let last_signature = sha256_hex_string(b"last message sts");
        let (signed_message, _signature) =
            sign_message(&original_message, &last_signature, &params)
                .unwrap()
                .into_parts();

        let unsigned_message = match super::extract_signed_message(&signed_message).unwrap() {
            super::MaybeSignedMessage::Signed { message, .. } => message,
            super::MaybeSignedMessage::Unsigned => panic!("Expected signed message"),
        };

        // Verify the unsigned message matches the original
        assert_eq!(original_message.payload(), unsigned_message.payload());
        assert_eq!(
            original_message.headers().len(),
            unsigned_message.headers().len()
        );
        for (orig, unsigned) in original_message
            .headers()
            .iter()
            .zip(unsigned_message.headers().iter())
        {
            assert_eq!(orig.name(), unsigned.name());
            assert_eq!(orig.value(), unsigned.value());
        }
    }

    #[test]
    fn test_extract_signed_message() {
        let original_message = Message::new(&b"test payload"[..]).add_header(Header::new(
            "some-header",
            HeaderValue::String("value".into()),
        ));
        let params = SigningParams {
            identity: &Credentials::for_tests().into(),
            region: "us-east-1",
            name: "testservice",
            time: (UNIX_EPOCH + Duration::new(123_456_789_u64, 1234u32)),
            settings: (),
        };

        let last_signature = sha256_hex_string(b"last message sts");
        let (signed_message, _signature) =
            sign_message(&original_message, &last_signature, &params)
                .unwrap()
                .into_parts();

        // Test extracting signed message
        let result = super::extract_signed_message(&signed_message).unwrap();
        match result {
            super::MaybeSignedMessage::Signed { message, signature } => {
                assert_eq!(original_message.payload(), message.payload());
                assert_eq!(original_message.headers().len(), message.headers().len());
                assert_eq!(
                    signature
                        .timestamp
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                    123_456_789
                );
            }
            super::MaybeSignedMessage::Unsigned => panic!("Expected signed message"),
        }

        // Test unsigned message
        let unsigned_result = super::extract_signed_message(&original_message).unwrap();
        assert!(matches!(
            unsigned_result,
            super::MaybeSignedMessage::Unsigned
        ));
    }

    #[test]
    fn test_sigv4_unmarshaller() {
        use aws_smithy_eventstream::frame::{UnmarshallMessage, UnmarshalledMessage};

        // Mock unmarshaller for testing
        #[derive(Debug)]
        struct MockUnmarshaller;
        impl UnmarshallMessage for MockUnmarshaller {
            type Output = String;
            type Error = &'static str;

            fn unmarshall(
                &self,
                message: &Message,
            ) -> Result<
                UnmarshalledMessage<Self::Output, Self::Error>,
                aws_smithy_eventstream::error::Error,
            > {
                Ok(UnmarshalledMessage::Event(format!(
                    "unmarshalled: {}",
                    String::from_utf8_lossy(message.payload())
                )))
            }
        }

        let original_message = Message::new(&b"test payload"[..]);
        let params = SigningParams {
            identity: &Credentials::for_tests().into(),
            region: "us-east-1",
            name: "testservice",
            time: (UNIX_EPOCH + Duration::new(123_456_789_u64, 1234u32)),
            settings: (),
        };

        let last_signature = sha256_hex_string(b"last message sts");
        let (signed_message, _) = sign_message(&original_message, &last_signature, &params)
            .unwrap()
            .into_parts();

        let sigv4_unmarshaller = super::SigV4Unmarshaller::new(MockUnmarshaller);

        // Test signed message
        let result = sigv4_unmarshaller.unmarshall(&signed_message).unwrap();
        match result {
            UnmarshalledMessage::Event(signed_event) => {
                assert!(signed_event.signature.is_some());
                assert_eq!(signed_event.message, "unmarshalled: test payload");
            }
            UnmarshalledMessage::Error(err) => {
                panic!("Expected successful unmarshalling. got {err:?}")
            }
        }

        // Test unsigned message
        let result = sigv4_unmarshaller.unmarshall(&original_message).unwrap();
        match result {
            UnmarshalledMessage::Event(signed_event) => {
                assert!(signed_event.signature.is_none());
                assert_eq!(signed_event.message, "unmarshalled: test payload");
            }
            UnmarshalledMessage::Error(_) => panic!("Expected successful unmarshalling"),
        }
    }
}
