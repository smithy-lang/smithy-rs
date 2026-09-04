/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The server-side protocol trait: schema-driven request deserialization and
//! output/error serialization (plan 2a).
//!
//! [`ServerProtocol`] is the erased runtime protocol object used after routing
//! has selected a protocol and operation. It is responsible for protocol
//! serialization and deserialization only; route claiming lives in
//! `routing::ProtocolRouter`.
//!
//! The three verbs mirror the client's `ClientProtocolInner`
//! (`serialize_request`↔[`deserialize_request`](ServerProtocol::deserialize_request),
//! `deserialize_response`↔[`serialize_response`](ServerProtocol::serialize_response),
//! `deserialize_error_response`↔[`serialize_error`](ServerProtocol::serialize_error)),
//! diverging where server semantics demand: no error correction, unknown
//! union variants rejected, constraint failures produce the modeled
//! validation error.
//!
//! # Error framing (frozen to legacy generated behavior)
//!
//! Wire discriminators are derived from the error shape's own full `ShapeId`
//! and emitted per-protocol exactly as today's generated serializers do:
//!
//! | Protocol   | Discriminator                                                  |
//! |------------|----------------------------------------------------------------|
//! | restJson1  | `x-amzn-errortype` header, shape **name only**; none in body   |
//! | awsJson1.0 | `__type` body member, **full** `namespace#Name`, written last  |
//! | awsJson1.1 | `__type` body member, shape **name only**, written last        |
//! | rpcv2Cbor  | `__type` body member, **full** `namespace#Name`, written first |
//! | restXml    | none                                                           |
//!
//! `@httpHeader`-bound error members are split out of the body and stamped as
//! response headers on the REST protocols, mirroring the legacy generated
//! `ser_*_headers` functions (including the skip-empty-string rule).
//! Serializers never detect errors; call sites declare them by calling
//! [`ServerProtocol::serialize_error`].

pub(crate) mod aws_json;
mod discriminator;
mod dynamic;
mod request;
mod response;
pub(crate) mod rest;
pub(crate) mod rest_json;
pub(crate) mod rest_xml;
pub(crate) mod rpc_v2_cbor;
mod server_protocol;
mod static_protocol;
#[cfg(test)]
mod tests;

pub use dynamic::{DeserializeInputFuture, ErasedInputBuilder, ErasedInputVisitor};
pub use server_protocol::{DeserializeInputConfig, ServerProtocol, ServerProtocolInner, SharedServerProtocol};
pub use static_protocol::{StaticEventStreamProtocol, StaticProtocol};
