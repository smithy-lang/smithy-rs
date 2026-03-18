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
//! #[derive(Debug)]
//! struct MyProtocol {
//!     codec: MyJsonCodec,
//! }
//!
//! impl ClientProtocol for MyProtocol {
//!     fn protocol_id(&self) -> &ShapeId { &MY_PROTOCOL_ID }
//!
//!     fn serialize_request(
//!         &self,
//!         input: &dyn SerializableStruct,
//!         input_schema: &Schema,
//!         endpoint: &str,
//!         cfg: &ConfigBag,
//!     ) -> Result<aws_smithy_runtime_api::http::Request, SerdeError> {
//!         todo!()
//!     }
//!
//!     fn deserialize_response<'a>(
//!         &self,
//!         response: &'a aws_smithy_runtime_api::http::Response,
//!         output_schema: &Schema,
//!         cfg: &ConfigBag,
//!     ) -> Result<Box<dyn ShapeDeserializer + 'a>, SerdeError> {
//!         todo!()
//!     }
//!
//!     fn update_endpoint(
//!         &self,
//!         request: &mut aws_smithy_runtime_api::http::Request,
//!         endpoint: &str,
//!     ) -> Result<(), SerdeError> {
//!         todo!()
//!     }
//! }
//! ```

use crate::serde::{SerdeError, SerializableStruct, ShapeDeserializer};
use crate::{Schema, ShapeId};
use aws_smithy_types::config_bag::ConfigBag;

// Implementation note. We hardcode aws_smithy_runtime_api::http::{Request | Response} for the types here.
// Some other SDKs (mostly very new ones like smithy-java and smithy-python) have separated their transport
// layer into another abstraction, so you could plug in something like MQTT instead of HTTP. It is likely
// too late for us to do that given how deeply the assumptions about using HTTP are baked into the SDK. But
// if we want to keep that option open we could make Request/Response associated types and maintain the object
// safety of this trait.

/// An object-safe client protocol for serializing requests and deserializing responses.
///
/// Each Smithy protocol (e.g., `aws.protocols#restJson1`, `smithy.protocols#rpcv2Cbor`)
/// is represented by an implementation of this trait. Protocols combine one or more
/// codecs and serializers to produce protocol-specific request messages and parse
/// response messages.
///
/// # Lifecycle
///
/// `ClientProtocol` instances are immutable and thread-safe. They are typically created
/// once and shared across all requests for a client. Serializers and deserializers are
/// created per-request internally.
pub trait ClientProtocol: Send + Sync + std::fmt::Debug {
    /// Returns the Smithy shape ID of this protocol.
    ///
    /// This enables runtime protocol selection and differentiation. For example,
    /// `aws.protocols#restJson1` or `smithy.protocols#rpcv2Cbor`.
    fn protocol_id(&self) -> &ShapeId;

    /// Serializes an operation input into an HTTP request.
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
    ) -> Result<aws_smithy_runtime_api::http::Request, SerdeError>;

    /// Deserializes an HTTP response, returning a boxed [`ShapeDeserializer`].
    ///
    /// The caller uses the returned deserializer with a generated builder's
    /// consumer pattern to construct the typed operation output. The protocol handles
    /// transport framing (e.g., extracting the HTTP body) and returns a deserializer
    /// positioned to read the output shape.
    ///
    /// # Arguments
    ///
    /// * `response` - The HTTP response to deserialize.
    /// * `output_schema` - Schema describing the operation's output shape.
    /// * `cfg` - The config bag containing request-scoped configuration.
    fn deserialize_response<'a>(
        &self,
        response: &'a aws_smithy_runtime_api::http::Response,
        output_schema: &Schema,
        cfg: &ConfigBag,
    ) -> Result<Box<dyn ShapeDeserializer + 'a>, SerdeError>;

    /// Updates a previously serialized request with a new endpoint.
    ///
    /// This is required by the Smithy Reference Architecture (SRA) to support
    /// interceptors that modify the endpoint after initial serialization.
    fn update_endpoint(
        &self,
        request: &mut aws_smithy_runtime_api::http::Request,
        endpoint: &str,
    ) -> Result<(), SerdeError>;
}

/// A shared, type-erased client protocol stored in a [`ConfigBag`].
///
/// This wraps an `Arc<dyn ClientProtocol>` so it can be stored
/// and retrieved from the config bag for runtime protocol selection.
#[derive(Clone, Debug)]
pub struct SharedClientProtocol {
    inner: std::sync::Arc<dyn ClientProtocol>,
}

impl SharedClientProtocol {
    /// Creates a new shared protocol from any `ClientProtocol` implementation.
    pub fn new(protocol: impl ClientProtocol + 'static) -> Self {
        Self {
            inner: std::sync::Arc::new(protocol),
        }
    }
}

impl std::ops::Deref for SharedClientProtocol {
    type Target = dyn ClientProtocol;

    fn deref(&self) -> &Self::Target {
        &*self.inner
    }
}

impl aws_smithy_types::config_bag::Storable for SharedClientProtocol {
    type Storer = aws_smithy_types::config_bag::StoreReplace<Self>;
}
