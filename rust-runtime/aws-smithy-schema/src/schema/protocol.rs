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

    /// Creates a body-only deserializer from the response payload bytes.
    ///
    /// For REST protocols, response deserialization involves two distinct sources:
    /// HTTP metadata (headers, status code) and the serialized body (e.g. JSON).
    /// The `deserialize_response` method wraps both sources into a single
    /// `HttpBindingDeserializer` that routes each member to the correct source
    /// at runtime by inspecting HTTP binding traits on the schema. This runtime
    /// routing adds overhead: iterating all schema members, checking trait fields,
    /// and dispatching through `dyn ShapeDeserializer` for each header member.
    ///
    /// This method provides an alternative: it returns a raw codec deserializer
    /// for just the body. Generated code can then read HTTP-bound members
    /// directly from the response headers (with no schema iteration or trait
    /// checks) and use this deserializer only for body members. This splits the
    /// work the same way the legacy (non-schema) codegen does — headers are read
    /// inline, body is parsed by the codec — preserving the performance
    /// characteristics of the legacy approach while still using the schema-driven
    /// `ShapeDeserializer` interface for body parsing.
    fn deserialize_body<'a>(
        &self,
        body: &'a [u8],
    ) -> Result<Box<dyn ShapeDeserializer + 'a>, SerdeError>;

    /// Serializes only the body members of an operation input into an HTTP request.
    ///
    /// This is the serialization counterpart to [`deserialize_body`](Self::deserialize_body).
    /// For REST protocols, `serialize_request` routes each member through
    /// `HttpBindingSerializer` which checks HTTP binding traits at runtime to
    /// decide whether a member goes to a header, query param, URI label, or body.
    /// For operations with many HTTP-bound members, this per-member routing adds
    /// measurable overhead.
    ///
    /// This method bypasses that routing: it serializes only body members using
    /// the codec directly, constructs the URI (with `@http` trait pattern), and
    /// sets the HTTP method. Generated code then writes HTTP-bound members
    /// (headers, query params, labels) directly onto the returned request.
    ///
    /// The default implementation delegates to `serialize_request`, which is
    /// correct but slower for REST protocols with many HTTP bindings.
    fn serialize_body(
        &self,
        input: &dyn SerializableStruct,
        input_schema: &Schema,
        endpoint: &str,
        cfg: &ConfigBag,
    ) -> Result<aws_smithy_runtime_api::http::Request, SerdeError> {
        self.serialize_request(input, input_schema, endpoint, cfg)
    }

    /// Updates a previously serialized request with a new endpoint.
    ///
    /// This is required by the Smithy Reference Architecture (SRA) to support
    /// interceptors that modify the endpoint after initial serialization.
    ///
    /// The default implementation applies the endpoint URL (with prefix if present),
    /// sets the request URI, and copies any endpoint headers onto the request.
    ///
    /// Note: the default implementation here should be sufficient for most protocols.
    fn update_endpoint(
        &self,
        request: &mut aws_smithy_runtime_api::http::Request,
        endpoint: &aws_smithy_types::endpoint::Endpoint,
        cfg: &ConfigBag,
    ) -> Result<(), SerdeError> {
        use std::borrow::Cow;

        let endpoint_prefix =
            cfg.load::<aws_smithy_runtime_api::client::endpoint::EndpointPrefix>();
        let endpoint_url = match endpoint_prefix {
            None => Cow::Borrowed(endpoint.url()),
            Some(prefix) => {
                let parsed: http::Uri = endpoint
                    .url()
                    .parse()
                    .map_err(|e| SerdeError::custom(format!("invalid endpoint URI: {e}")))?;
                let scheme = parsed.scheme_str().unwrap_or_default();
                let prefix = prefix.as_str();
                let authority = parsed.authority().map(|a| a.as_str()).unwrap_or_default();
                let path_and_query = parsed
                    .path_and_query()
                    .map(|pq| pq.as_str())
                    .unwrap_or_default();
                Cow::Owned(format!("{scheme}://{prefix}{authority}{path_and_query}"))
            }
        };

        request.uri_mut().set_endpoint(&endpoint_url).map_err(|e| {
            SerdeError::custom(format!("failed to apply endpoint `{endpoint_url}`: {e}"))
        })?;

        for (header_name, header_values) in endpoint.headers() {
            request.headers_mut().remove(header_name);
            for value in header_values {
                request
                    .headers_mut()
                    .append(header_name.to_owned(), value.to_owned());
            }
        }

        Ok(())
    }
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
