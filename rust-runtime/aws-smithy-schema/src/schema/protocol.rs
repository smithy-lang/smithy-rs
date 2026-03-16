/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Client protocol trait for protocol-agnostic request serialization and response deserialization.
//!
//! A [`ClientProtocol`] uses a combination of codecs, serializers, and deserializers to
//! serialize requests and deserialize responses for a specific Smithy protocol
//! (e.g., AWS JSON 1.0, REST JSON, REST XML, RPCv2 CBOR).
//!
//! This trait is intentionally **not** HTTP-specific. Smithy is protocol-agnostic, and
//! implementations could support HTTP, MQTT, Unix domain sockets, or in-memory transports.
//! The request and response types are associated types chosen by each protocol implementation.
//!
//! # Implementing a custom protocol
//!
//! Third parties can create custom protocols and use them with any client without
//! modifying a code generator.
//!
//! ```ignore
//! use aws_smithy_schema::protocol::ClientProtocol;
//! use aws_smithy_schema::{Schema, ShapeId};
//! use aws_smithy_schema::serde::SerializableStruct;
//!
//! struct MyProtocol {
//!     codec: MyJsonCodec,
//! }
//!
//! impl ClientProtocol for MyProtocol {
//!     type Request = MyRequest;
//!     type Response = MyResponse;
//!     type Codec = MyJsonCodec;
//!     type ResponseDeserializer<'a> = MyJsonDeserializer<'a>;
//!
//!     fn protocol_id(&self) -> &ShapeId { &MY_PROTOCOL_ID }
//!
//!     fn payload_codec(&self) -> Option<&MyJsonCodec> { Some(&self.codec) }
//!
//!     fn serialize_request(
//!         &self,
//!         input: &dyn SerializableStruct,
//!         input_schema: &Schema,
//!         endpoint: &str,
//!         cfg: &ConfigBag,
//!     ) -> Result<MyRequest, SerdeError> {
//!         todo!()
//!     }
//!
//!     fn deserialize_response<'a>(
//!         &self,
//!         response: &'a Self::Response,
//!         output_schema: &Schema,
//!         cfg: &ConfigBag,
//!     ) -> Result<Self::ResponseDeserializer<'a>, SerdeError> {
//!         todo!()
//!     }
//!
//!     fn update_endpoint(
//!         &self,
//!         request: &mut Self::Request,
//!         endpoint: &str,
//!     ) -> Result<(), SerdeError> {
//!         todo!()
//!     }
//! }
//! ```

use crate::serde::{SerdeError, SerializableStruct, ShapeDeserializer};
use crate::{Schema, ShapeId};
use aws_smithy_types::config_bag::ConfigBag;

/// A client protocol for serializing requests and deserializing responses.
///
/// Each Smithy protocol (e.g., `aws.protocols#restJson1`, `smithy.protocols#rpcv2Cbor`)
/// is represented by an implementation of this trait. Protocols combine one or more
/// codecs and serializers to produce protocol-specific request messages and parse
/// response messages.
///
/// # Protocol agnosticism
///
/// This trait does not assume HTTP or any specific transport. The `Request` and `Response`
/// associated types are defined by each implementation. For HTTP-based protocols these
/// would be HTTP request/response types; for other transports they could be anything.
///
/// # Codec access
///
/// [`payload_codec`](ClientProtocol::payload_codec) exposes the protocol's payload codec
/// so callers can serialize shapes independently of operations. The codec instance should
/// be static within the protocol, allowing access to the canonical codec settings of
/// implemented protocols (e.g., the differently configured JSON codecs of AWS JSON RPC
/// vs AWS REST JSON). Asymmetric protocols (e.g., AWS Query) that use different formats
/// for serialization and deserialization return `None`.
///
/// # Lifecycle
///
/// `ClientProtocol` instances are immutable and thread-safe. They are typically created
/// once and shared across all requests for a client. Serializers and deserializers are
/// created per-request internally.
pub trait ClientProtocol {
    /// The request message type produced by this protocol (e.g., an HTTP request).
    type Request;

    /// The response message type consumed by this protocol (e.g., an HTTP response).
    type Response;

    /// The payload codec type used by this protocol.
    ///
    /// For protocols that use a single symmetric codec (e.g., JSON for `restJson1`,
    /// CBOR for `rpcv2Cbor`), this is that codec's concrete type. For asymmetric
    /// protocols (e.g., AWS Query), this can be any type — those protocols return
    /// `None` from [`payload_codec`](ClientProtocol::payload_codec).
    type Codec;

    /// The deserializer type returned by [`deserialize_response`](ClientProtocol::deserialize_response).
    ///
    /// This is the [`ShapeDeserializer`] that callers use with a generated builder's
    /// consumer pattern to construct the typed operation output.
    type ResponseDeserializer<'a>: ShapeDeserializer
    where
        Self: 'a;

    /// Returns the Smithy shape ID of this protocol.
    ///
    /// This enables runtime protocol selection and differentiation. For example,
    /// `aws.protocols#restJson1` or `smithy.protocols#rpcv2Cbor`.
    fn protocol_id(&self) -> &ShapeId;

    /// Returns the payload codec used by this protocol, if applicable.
    ///
    /// Returns `None` for asymmetric protocols like AWS Query and EC2 Query that
    /// use different formats for serialization and deserialization, or for protocols
    /// that use multiple codecs.
    fn payload_codec(&self) -> Option<&Self::Codec>;

    /// Serializes an operation input into a protocol-specific request message.
    ///
    /// # Arguments
    ///
    /// * `input` - The operation input to serialize.
    /// * `input_schema` - Schema describing the operation's input shape.
    /// * `endpoint` - The target endpoint URI as a string.
    /// * `cfg` - The config bag containing request-scoped configuration
    ///   (e.g., service name, operation name for RPC protocols).
    fn serialize_request(
        &self,
        input: &dyn SerializableStruct,
        input_schema: &Schema,
        endpoint: &str,
        cfg: &ConfigBag,
    ) -> Result<Self::Request, SerdeError>;

    /// Deserializes a protocol-specific response message.
    ///
    /// Returns a [`ShapeDeserializer`] that the caller uses with a generated builder's
    /// consumer pattern to construct the typed operation output. The protocol handles
    /// transport framing (e.g., extracting the HTTP body) and returns a deserializer
    /// positioned to read the output shape.
    ///
    /// # Arguments
    ///
    /// * `response` - The protocol response message to deserialize.
    /// * `output_schema` - Schema describing the operation's output shape.
    /// * `cfg` - The config bag containing request-scoped configuration.
    fn deserialize_response<'a>(
        &self,
        response: &'a Self::Response,
        output_schema: &Schema,
        cfg: &ConfigBag,
    ) -> Result<Self::ResponseDeserializer<'a>, SerdeError>;

    /// Updates a previously serialized request with a new endpoint.
    ///
    /// This is required by the Smithy Reference Architecture (SRA) to support
    /// interceptors that modify the endpoint after initial serialization.
    fn update_endpoint(
        &self,
        request: &mut Self::Request,
        endpoint: &str,
    ) -> Result<(), SerdeError>;
}
