/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The server-side schema deserialization seam.
//!
//! [`DeserializableShape`] is implemented by generated operation input
//! structures. The implementation is the ONE uniform walker — indistinguishable
//! from any nested structure's: it drives
//! [`ShapeDeserializer::read_struct`] into the shape's internal (unconstrained)
//! builder and calls `build()`. Constraint validation does not move: the walker
//! validates nothing; `build()` and the constrained-newtype `TryFrom`s enforce
//! `@required`/`@length`/`@range`/`@pattern`/`@enum`, producing today's
//! `ConstraintViolation` values.
//!
//! Two error channels surface through [`DeserializeError`]:
//!
//! - **Wire-level failures** (bad document, type mismatch, unknown union
//!   variant, unparseable header) → [`DeserializeError::Serde`] → the
//!   protocol's malformed-request rejection → protocol 4xx.
//! - **Constraint violations** → [`DeserializeError::ConstraintViolation`]: the
//!   generated protocol-free `From<ConstraintViolation>` conversion builds the
//!   modeled validation error (default `ValidationException` or a
//!   decorator-customized shape), boxed as `dyn HttpModeledError`. It is
//!   serialized ONCE, at the protocol boundary, via
//!   `ServerProtocol::serialize_error`.

use aws_smithy_schema::serde::{SerdeError, ShapeDeserializer};

use crate::modeled_error::HttpModeledError;

/// Error type returned by [`DeserializableShape::deserialize`].
#[derive(Debug)]
pub enum DeserializeError {
    /// A wire-level deserialization failure: the request bytes did not match
    /// the schema (bad document, type mismatch, unknown union variant,
    /// unparseable header/label/query value). Becomes the protocol's
    /// malformed-request rejection.
    Serde(SerdeError),
    /// The request parsed but violated modeled constraints. Carries the
    /// modeled validation error, built by the generated
    /// `From<ConstraintViolation>` conversion. Serialized once at the
    /// protocol boundary via `ServerProtocol::serialize_error`.
    ConstraintViolation(Box<dyn HttpModeledError + Send>),
}

impl std::fmt::Display for DeserializeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Serde(err) => write!(f, "failed to deserialize request: {err}"),
            Self::ConstraintViolation(err) => {
                write!(f, "request does not adhere to modeled constraints: {err}")
            }
        }
    }
}

impl std::error::Error for DeserializeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Serde(err) => Some(err),
            Self::ConstraintViolation(_) => None,
        }
    }
}

impl From<SerdeError> for DeserializeError {
    fn from(err: SerdeError) -> Self {
        Self::Serde(err)
    }
}

/// Implemented by generated operation input structures: the schema-driven
/// deserialization walker, the target of
/// `ServerProtocol::deserialize_request`.
///
/// Server semantics (deliberately NOT the client's `deserialize()` pattern):
/// no error correction, no defaulting, no `Unknown` union arms — an unknown
/// `:event-type` or union variant is an error. The walker reads into the
/// internal builder; `build()` does all constraint enforcement.
pub trait DeserializableShape: Sized {
    /// Deserializes this shape by driving `deserializer`.
    fn deserialize(deserializer: &mut dyn ShapeDeserializer) -> Result<Self, DeserializeError>;
}
