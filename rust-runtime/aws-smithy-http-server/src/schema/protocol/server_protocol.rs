/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::{fmt, sync::Arc};

use aws_smithy_schema::codec::DynCodec;
use aws_smithy_schema::serde::SerializableStruct;
use aws_smithy_schema::{Schema, ShapeId};

use crate::{
    body::{Body, BoxBody},
    modeled_error::HttpServerError,
};

use super::{DeserializeInputFuture, ErasedInputBuilder};

/// Runtime configuration for dynamic operation input deserialization.
#[derive(Debug, Clone, Copy)]
pub struct DeserializeInputConfig {
    /// Maximum number of non-streaming request body bytes to collect. `0`
    /// disables the limit.
    pub request_body_max_bytes: usize,
}

/// Static authoring trait for server protocols.
///
/// This is the server-side analogue of the client's `ClientProtocolInner`.
/// Implementors write this trait; the object-safe [`ServerProtocol`] view is
/// provided by a blanket impl.
pub trait ServerProtocolInner: Send + Sync + fmt::Debug {
    /// Returns the Smithy protocol shape ID.
    fn protocol_id(&self) -> &ShapeId<'static>;

    /// Returns this protocol's payload/body codec.
    fn codec(&self) -> &dyn DynCodec;

    /// Deserializes a dynamic operation input from the original HTTP request.
    ///
    /// Implementations own body collection, content-type validation, and
    /// event-stream body handling. The default implementation is intentionally
    /// absent because returning borrowed deserializers from collected bytes is
    /// not sound; concrete protocols must drive `input` inside this future.
    fn deserialize_input<'a>(
        &'a self,
        request: http::Request<Body>,
        input_schema: &'static Schema<'static>,
        config: DeserializeInputConfig,
        input: Box<dyn ErasedInputBuilder>,
    ) -> DeserializeInputFuture<'a>;

    /// Serializes a successful operation output.
    fn serialize_response(&self, schema: &Schema<'_>, output: &dyn SerializableStruct) -> http::Response<BoxBody>;

    /// Serializes an operation or framework server error.
    fn serialize_error(&self, error: &dyn HttpServerError) -> http::Response<BoxBody>;

    /// Frame-level `:content-type` for event payloads, when supported.
    fn event_payload_content_type(&self) -> Option<&'static str> {
        None
    }

    /// HTTP-level `Content-Type` for event streams, when supported.
    fn event_stream_http_content_type(&self) -> Option<&'static str> {
        None
    }

    /// Whether this protocol frames RPC initial event-stream messages.
    fn frames_initial_messages(&self) -> bool {
        false
    }
}

/// Object-safe server protocol view used by schema-driven dynamic dispatch.
///
/// This mirrors the client-side `ClientProtocol` split: concrete protocols may
/// keep static [`ServerProtocolInner`] implementations, while dynamic routing
/// and upgrade code hold a shared erased protocol object.
pub trait ServerProtocol: Send + Sync + fmt::Debug {
    /// Returns the Smithy protocol shape ID.
    fn protocol_id(&self) -> &ShapeId<'static>;

    /// Returns this protocol's payload/body codec.
    fn codec(&self) -> &dyn DynCodec;

    /// Deserializes a dynamic operation input from the original HTTP request.
    fn deserialize_input<'a>(
        &'a self,
        request: http::Request<Body>,
        input_schema: &'static Schema<'static>,
        config: DeserializeInputConfig,
        input: Box<dyn ErasedInputBuilder>,
    ) -> DeserializeInputFuture<'a>;

    /// Serializes a successful operation output.
    fn serialize_response(&self, schema: &Schema<'_>, output: &dyn SerializableStruct) -> http::Response<BoxBody>;

    /// Serializes an operation or framework server error.
    fn serialize_error(&self, error: &dyn HttpServerError) -> http::Response<BoxBody>;

    /// Frame-level `:content-type` for event payloads, when supported.
    fn event_payload_content_type(&self) -> Option<&'static str> {
        None
    }

    /// HTTP-level `Content-Type` for event streams, when supported.
    fn event_stream_http_content_type(&self) -> Option<&'static str> {
        None
    }

    /// Whether this protocol frames RPC initial event-stream messages.
    fn frames_initial_messages(&self) -> bool {
        false
    }
}

impl<P> ServerProtocol for P
where
    P: ServerProtocolInner,
{
    fn protocol_id(&self) -> &ShapeId<'static> {
        <Self as ServerProtocolInner>::protocol_id(self)
    }

    fn codec(&self) -> &dyn DynCodec {
        <Self as ServerProtocolInner>::codec(self)
    }

    fn deserialize_input<'a>(
        &'a self,
        request: http::Request<Body>,
        input_schema: &'static Schema<'static>,
        config: DeserializeInputConfig,
        input: Box<dyn ErasedInputBuilder>,
    ) -> DeserializeInputFuture<'a> {
        <Self as ServerProtocolInner>::deserialize_input(self, request, input_schema, config, input)
    }

    fn serialize_response(&self, schema: &Schema<'_>, output: &dyn SerializableStruct) -> http::Response<BoxBody> {
        <Self as ServerProtocolInner>::serialize_response(self, schema, output)
    }

    fn serialize_error(&self, error: &dyn HttpServerError) -> http::Response<BoxBody> {
        <Self as ServerProtocolInner>::serialize_error(self, error)
    }

    fn event_payload_content_type(&self) -> Option<&'static str> {
        <Self as ServerProtocolInner>::event_payload_content_type(self)
    }

    fn event_stream_http_content_type(&self) -> Option<&'static str> {
        <Self as ServerProtocolInner>::event_stream_http_content_type(self)
    }

    fn frames_initial_messages(&self) -> bool {
        <Self as ServerProtocolInner>::frames_initial_messages(self)
    }
}

/// Shared erased server protocol.
pub struct SharedServerProtocol {
    inner: Arc<dyn ServerProtocol>,
}

impl Clone for SharedServerProtocol {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl SharedServerProtocol {
    /// Creates a shared erased protocol from a concrete erased protocol value.
    pub fn new(protocol: impl ServerProtocol + 'static) -> Self {
        Self {
            inner: Arc::new(protocol),
        }
    }
}

impl std::fmt::Debug for SharedServerProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedServerProtocol")
            .field("protocol_id", &self.protocol_id())
            .finish()
    }
}

impl std::ops::Deref for SharedServerProtocol {
    type Target = dyn ServerProtocol;

    fn deref(&self) -> &Self::Target {
        &*self.inner
    }
}
