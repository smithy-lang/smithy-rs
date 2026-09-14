/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_schema::serde::{SerdeError, ShapeDeserializer};

use super::HttpModeledError;
use crate::rejection::MissingContentTypeReason;

/// Why an operation input could not be produced from a request.
///
/// This is the one protocol-independent failure enum on the schema path: every way a request can
/// fail to become an operation input lands here, and each protocol turns the whole enum into its
/// wire response through [`ServerProtocol::serialize_rejection`](super::ServerProtocol::serialize_rejection).
///
/// There is one variant per distinct rejection response, which is why the `Content-Type` and
/// `Accept` failures are not folded into [`Serde`](Self::Serde): protocols answer them
/// differently (restJson1 keeps 415 and 406 distinct from the 400 that `Serde` produces), and
/// the renderer matches on the variant to pick the response. They also originate in different
/// layers: [`SerdeError`] belongs to the codec in `aws-smithy-schema` and describes failures
/// reading the payload and bindings into the input shape, while the `Content-Type` and `Accept`
/// checks run in the HTTP protocol layer before the codec — the codec never sees headers, and
/// the transport-agnostic schema crate carries no HTTP semantics.
#[derive(Debug)]
#[non_exhaustive]
pub enum DeserializeError {
    /// The request could not be read as the input shape: a malformed document, a type mismatch,
    /// an unparseable header, label or query value, a body that could not be collected, or a
    /// request that could not be converted into its canonical form.
    Serde(SerdeError),
    /// The request was well-formed but violates a modeled constraint.
    ConstraintViolation(Box<dyn HttpModeledError>),
    /// The request's `Content-Type` header does not match what the operation expects.
    ///
    /// Raised by the protocol layer's header check, never by the codec; carries the expected
    /// and found mime types the 415 response is built from.
    // Boxed to keep the enum small: the reason carries two parsed mime types.
    UnsupportedMediaType(Box<MissingContentTypeReason>),
    /// The request's `Accept` header cannot accept the operation's response content type.
    ///
    /// Raised by the protocol layer's header check, never by the codec.
    NotAcceptable,
}

impl std::fmt::Display for DeserializeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Serde(err) => write!(f, "failed to deserialize request: {err}"),
            Self::ConstraintViolation(err) => {
                write!(f, "request does not adhere to modeled constraints: {err}")
            }
            Self::UnsupportedMediaType(reason) => {
                write!(f, "request has an unsupported `Content-Type`: {reason}")
            }
            Self::NotAcceptable => write!(f, "request contains an invalid value for the `Accept` header"),
        }
    }
}

impl std::error::Error for DeserializeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Serde(err) => Some(err),
            Self::ConstraintViolation(err) => Some(&**err),
            Self::UnsupportedMediaType(reason) => Some(&**reason),
            Self::NotAcceptable => None,
        }
    }
}

impl From<SerdeError> for DeserializeError {
    fn from(err: SerdeError) -> Self {
        Self::Serde(err)
    }
}

impl From<MissingContentTypeReason> for DeserializeError {
    fn from(reason: MissingContentTypeReason) -> Self {
        Self::UnsupportedMediaType(Box::new(reason))
    }
}

/// An operation input that can be read from a [`ShapeDeserializer`].
///
/// Generated inputs walk their schema into the internal builder and then `build()`, so constraint
/// validation keeps happening in one place.
pub trait DeserializableShape: Sized {
    /// Reads `Self` from `deserializer`.
    fn deserialize(deserializer: &mut dyn ShapeDeserializer) -> Result<Self, DeserializeError>;
}
