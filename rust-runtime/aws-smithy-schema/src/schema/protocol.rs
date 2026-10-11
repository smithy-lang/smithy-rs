/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Client protocol traits for protocol-agnostic request serialization and response deserialization.
//!
//! [`ClientProtocolInner`] is the trait implementors write. It carries associated
//! `Request` / `Response` types and allows transport-agnostic protocols (the SEP calls
//! this out as a requirement).
//!
//! [`ClientProtocol`] is the object-safe view that callers use through `dyn`. It's
//! parameterized over concrete request/response types (defaulted to HTTP) so a
//! [`SharedClientProtocol`] can be configured and swapped at runtime.
//!
//! A configured protocol is stored in the [`ConfigBag`] as a [`ConfiguredProtocol`], a
//! version-stable wrapper owned by `aws-smithy-runtime-api`, rather than as a type from this
//! crate. Build one with [`SharedClientProtocol::configured`]. Generated clients recover it by
//! downcasting the [`ConfiguredProtocol`] to a [`SchemaProtocol`] and calling
//! [`SchemaProtocol::v1`]; see [`SchemaProtocol`] for how the protocol trait can evolve within 1.x.
//!
//! A blanket impl (`impl<P: ClientProtocolInner> ClientProtocol<P::Request, P::Response> for P`)
//! means implementors only write `ClientProtocolInner`; the object-safe view comes for
//! free. This mirrors the [`Codec`](crate::codec::Codec) / [`DynCodec`](crate::codec::DynCodec)
//! pair in the codec module — the same "static-dispatch inner trait + object-safe sibling"
//! pattern.
//!
//! # Implementing a custom protocol
//!
//! Third parties can create custom protocols and use them with any client without
//! modifying a code generator.
//!
//! ```ignore
//! use aws_smithy_schema::protocol::{apply_http_endpoint, ClientProtocolInner};
//! use aws_smithy_schema::{Schema, ShapeId};
//! use aws_smithy_schema::serde::SerializableStruct;
//!
//! #[derive(Debug)]
//! struct MyProtocol {
//!     codec: MyJsonCodec,
//! }
//!
//! impl ClientProtocolInner for MyProtocol {
//!     type Request = aws_smithy_runtime_api::http::Request;
//!     type Response = aws_smithy_runtime_api::http::Response;
//!
//!     fn protocol_id(&self) -> &ShapeId<'static> { &MY_PROTOCOL_ID }
//!
//!     fn serialize_request(
//!         &self,
//!         input: &dyn SerializableStruct,
//!         input_schema: &Schema<'_>,
//!         endpoint: &str,
//!         cfg: &ConfigBag,
//!     ) -> Result<Self::Request, SerdeError> {
//!         todo!()
//!     }
//!
//!     fn deserialize_response<'a>(
//!         &self,
//!         response: &'a Self::Response,
//!         output_schema: &Schema<'_>,
//!         cfg: &'a ConfigBag,
//!     ) -> Result<Box<dyn ShapeDeserializer + 'a>, SerdeError> {
//!         todo!()
//!     }
//!
//!     fn update_endpoint(
//!         &self,
//!         request: &mut Self::Request,
//!         endpoint: &aws_smithy_types::endpoint::Endpoint,
//!         cfg: &ConfigBag,
//!     ) -> Result<(), SerdeError> {
//!         apply_http_endpoint(request, endpoint, cfg)
//!     }
//! }
//! ```

use crate::serde::{SerdeError, SerializableStruct, ShapeDeserializer};
use crate::{Schema, ShapeId};
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::protocol::{ClientProtocolSlot, ProtocolHandle};
use aws_smithy_runtime_api::client::versioned_config::{
    ConfigPayloadFor, ConfigSlotError, RepresentationId,
};
use aws_smithy_types::config_bag::ConfigBag;
use aws_smithy_types::endpoint::Endpoint;
use aws_smithy_types::error::metadata::{Builder as ErrorMetadataBuilder, ErrorMetadata};

/// Re-exported from `aws-smithy-runtime-api`, which owns the type so that its identity does not
/// depend on this crate's major version.
pub use aws_smithy_runtime_api::client::protocol::ConfiguredProtocol;

/// Statically-dispatched client protocol trait — the one implementors write.
///
/// `Request` and `Response` are associated types so a protocol can target any transport
/// (HTTP, MQTT, Unix-socket, in-memory, …). For the common HTTP case, set both to
/// `aws_smithy_runtime_api::http::Request` / `Response`.
///
/// Callers who need to store a protocol behind `dyn` (e.g., in a [`ConfigBag`] for
/// runtime swapping) should use the object-safe [`ClientProtocol`] trait instead.
/// Every `ClientProtocolInner` is automatically a
/// `ClientProtocol<Self::Request, Self::Response>` via a blanket impl, so implementors
/// never write `ClientProtocol` manually.
///
/// See [`apply_http_endpoint`] for the canonical HTTP implementation of
/// `update_endpoint`.
///
/// # Lifecycle
///
/// Instances are immutable and thread-safe. They are typically created once and
/// shared across all requests for a client.
pub trait ClientProtocolInner: Send + Sync + std::fmt::Debug {
    /// The protocol's request message type (e.g., `http::Request`).
    type Request;

    /// The protocol's response message type (e.g., `http::Response`).
    type Response;

    /// Returns the Smithy shape ID of this protocol.
    fn protocol_id(&self) -> &ShapeId<'static>;

    /// Serializes an operation input into a request message.
    ///
    /// # The protocol owns its own wire format
    ///
    /// Anything this protocol puts on the wire that *the protocol alone determines* must be
    /// resolved here, not emitted by codegen. A client generated for one protocol can be pointed at
    /// another with `Config::builder().protocol(..)`, so anything codegen bakes in based on the
    /// generated protocol is wrong after a swap: left behind when this protocol is swapped out, and
    /// missing when it is swapped in. Concretely, an implementor owns:
    ///
    /// - **Framing headers.** rpcv2Cbor sets `smithy-protocol` and `accept`; awsJson sets
    ///   `X-Amz-Target`. Codegen's schema path deliberately emits none of these.
    /// - **The request path**, per the note on `endpoint` below.
    /// - **Model facts the protocol needs but a caller cannot know**, read from `cfg`:
    ///   [`ServiceShapeName`], [`ServiceVersion`], [`ServiceXmlNamespace`]. Generated clients store
    ///   these regardless of which protocol they were generated for, precisely so a swapped-in
    ///   protocol can find them. Keep an explicit builder as an override for what the model cannot
    ///   express -- a target prefix that is not the service shape name, for instance.
    ///
    /// Headers determined by the *service* rather than the protocol -- `x-amzn-query-mode`, from
    /// `@awsQueryCompatible` -- are outside this rule and stay in codegen, because their value does
    /// not change when the protocol does.
    ///
    /// # `endpoint` is advisory
    ///
    /// `endpoint` is a request **path** (or `""`), never a host; scheme and authority are merged
    /// later by [`apply_http_endpoint`]. It is whatever codegen computed *for the protocol the
    /// client was generated for*, so a protocol that alone determines its route must ignore it:
    ///
    /// - **Fixed route** -- awsJson and awsQuery are specified to `POST /`, so they pass `/` and
    ///   ignore the argument entirely. Forwarding it would let an rpcv2Cbor-generated client POST
    ///   to `/service/{service}/operation/{operation}`.
    /// - **Route derived from model facts** -- rpcv2Cbor computes
    ///   `/service/{service}/operation/{operation}` from `cfg`, falling back to `endpoint` only
    ///   when those facts are absent.
    /// - **Route from `@http` bindings** -- REST protocols expand the operation's `@http` template
    ///   from the schema, which is authoritative; `endpoint` is ignored. Generated REST clients
    ///   pass `""`, so this costs them nothing, and it stops an RPC route from being prefixed onto
    ///   the template after a swap. `endpoint` acts as the template only for a schema that carries
    ///   no `@http` trait at all.
    ///
    /// The shared [`HttpRpcProtocol`](crate::schema::http_protocol::HttpRpcProtocol) helper
    /// deliberately does *not* hard-code `/`; only a concrete protocol knows whether its route is
    /// constant.
    ///
    /// [`ServiceShapeName`]: aws_smithy_runtime_api::client::protocol::ServiceShapeName
    /// [`ServiceVersion`]: aws_smithy_runtime_api::client::protocol::ServiceVersion
    /// [`ServiceXmlNamespace`]: aws_smithy_runtime_api::client::protocol::ServiceXmlNamespace
    fn serialize_request(
        &self,
        input: &dyn SerializableStruct,
        input_schema: &Schema<'_>,
        endpoint: &str,
        cfg: &ConfigBag,
    ) -> Result<Self::Request, SerdeError>;

    /// Deserializes a response message, returning a boxed [`ShapeDeserializer`] that supplies
    /// every member of `output_schema` this protocol carries in the response.
    ///
    /// The protocol, not the caller, decides where each member comes from. A protocol that uses
    /// HTTP bindings reads `@httpHeader`, `@httpPrefixHeaders`, `@httpResponseCode` and
    /// `@httpPayload` members from the HTTP message and the rest from the body (see
    /// [`http_output_deserializer`](crate::http_protocol::http_output_deserializer)); a body-only
    /// protocol ignores those traits and reads every member from the body. Generated code hands
    /// the result to the output builder's member consumer and does no transport parsing itself.
    ///
    /// A `@streaming` payload member is never supplied: the caller owns the live body.
    ///
    /// `cfg` shares the returned deserializer's lifetime so a protocol can defer reading
    /// response-scoped settings until they are needed; the HTTP-binding composite reads
    /// `NonUtf8HeaderHandling` only after a header fails to parse. An implementation that does
    /// not keep `cfg` may declare it as a plain `&ConfigBag`.
    fn deserialize_response<'a>(
        &self,
        response: &'a Self::Response,
        output_schema: &Schema<'_>,
        cfg: &'a ConfigBag,
    ) -> Result<Box<dyn ShapeDeserializer + 'a>, SerdeError>;

    /// Extracts canonical error metadata (code, message, request id) from a
    /// response's wire envelope.
    ///
    /// Returns a [`Builder`](ErrorMetadataBuilder) so callers can attach
    /// per-request fields (e.g., `x-amzn-RequestId` from an HTTP header) before
    /// finalizing.
    ///
    /// Concrete protocols override this to extract their envelope-specific
    /// fields:
    /// - awsJson1.0 / awsJson1.1: `__type` from the body, `X-Amzn-Errortype`
    ///   header fallback.
    /// - restJson1: same as awsJson.
    /// - restXml (wrapped): `<ErrorResponse><Error><Code>` etc.
    /// - restXml (`@restXml(noErrorWrapping: true)`): `<Error><Code>` etc.
    /// - awsQuery / ec2Query: `<ErrorResponse><Error><Code>` etc.
    /// - rpcv2Cbor: `__type` from the CBOR map.
    ///
    /// The default implementation returns an empty
    /// [`Builder`](ErrorMetadataBuilder) — sufficient for protocols that
    /// haven't migrated to schema-driven error dispatch yet, but
    /// callers will see `Option::None` for `code()` / `message()` and treat
    /// the response as an unhandled error.
    fn parse_error_metadata(
        &self,
        response: &Self::Response,
        cfg: &ConfigBag,
    ) -> Result<ErrorMetadataBuilder, SerdeError> {
        let _ = (response, cfg);
        Ok(ErrorMetadata::builder())
    }

    /// Returns a [`ShapeDeserializer`] positioned at the body of an error
    /// response — *inside* the protocol's error envelope, where applicable.
    ///
    /// Generated error dispatch code calls this to obtain a deserializer for the modeled error's
    /// member consumer, regardless of which protocol is active at runtime. As with
    /// [`deserialize_response`](Self::deserialize_response), the protocol decides which members
    /// come from the transport and which from the body.
    ///
    /// For **body-only** protocols (awsJson1.0/1.1, awsQuery, ec2Query, rpcv2Cbor) the default
    /// implementation suffices: the body root *is* the error body, so it forwards to
    /// [`deserialize_response`](Self::deserialize_response) against
    /// [`prelude::DOCUMENT`](crate::prelude::DOCUMENT), and HTTP response bindings are correctly
    /// ignored.
    ///
    /// Envelope-bearing protocols (restXml wrapped / unwrapped, awsQuery, ec2Query) MUST
    /// override to strip the outer `<ErrorResponse>` / `<Error>` wrapper before returning the
    /// deserializer.
    ///
    /// # Protocols that use HTTP bindings must override this
    ///
    /// A protocol whose errors carry `@httpHeader`, `@httpPrefixHeaders` or
    /// `@httpResponseCode` members MUST override this and build its deserializer with
    /// [`http_error_deserializer`](crate::http_protocol::http_error_deserializer), even when its
    /// body needs no envelope positioning. The default's two properties are both accidents
    /// rather than contracts: it evaluates the *success* body-only fast path, and it does so
    /// against a schema that is not the error's. Neither is safe to depend on, because the
    /// concrete error schema is unknown until the generated error variant calls `read_struct`.
    /// Every built-in REST protocol therefore overrides this, and
    /// `the_error_path_never_takes_the_body_only_fast_path` pins the consequence.
    fn deserialize_error_response<'a>(
        &self,
        response: &'a Self::Response,
        cfg: &'a ConfigBag,
    ) -> Result<Box<dyn ShapeDeserializer + 'a>, SerdeError> {
        self.deserialize_response(response, &crate::prelude::DOCUMENT, cfg)
    }

    /// Updates a previously serialized request with a resolved endpoint.
    ///
    /// Required by SEP requirement 7. The orchestrator calls this after endpoint
    /// resolution, which happens *after* `serialize_request`.
    ///
    /// HTTP protocols should implement this as:
    /// ```ignore
    /// apply_http_endpoint(request, endpoint, cfg)
    /// ```
    /// (See [`apply_http_endpoint`].) Non-HTTP protocols implement the transport's
    /// equivalent.
    fn update_endpoint(
        &self,
        request: &mut Self::Request,
        endpoint: &Endpoint,
        cfg: &ConfigBag,
    ) -> Result<(), SerdeError>;

    /// Returns the codec used for payload (de)serialization, if any.
    ///
    /// See [`DynCodec`](crate::codec::DynCodec) for why the codec is exposed
    /// through the object-safe sibling.
    fn payload_codec(&self) -> Option<&dyn crate::codec::DynCodec> {
        None
    }

    /// The media type used to label a **structured event-stream payload**, if this
    /// protocol supports event streams — for example `application/cbor` or
    /// `application/json`.
    ///
    /// Every event-stream frame carries a `:content-type` header describing its
    /// payload. The payload itself is encoded by whichever protocol is selected at
    /// runtime, so this label has to come from that same protocol; a label baked in
    /// when the client was generated contradicts the bytes as soon as a different
    /// protocol is selected, and a peer that honours the header then decodes with
    /// the wrong codec.
    ///
    /// # Why this is on the protocol and not on [`Codec`](crate::codec::Codec)
    ///
    /// The value is protocol-determined, not format-determined, and one codec
    /// serves several protocols that disagree about it. A single `JsonCodec` backs
    /// restJson1, awsJson1_0 and awsJson1_1, whose request content types are
    /// `application/json`, `application/x-amz-json-1.0` and
    /// `application/x-amz-json-1.1` respectively. A codec-level accessor could
    /// therefore only ever return one of those, and would describe event payloads
    /// correctly only because the three JSON protocols happen to agree *there*.
    ///
    /// # What this is not
    ///
    /// This is **not** the request `Content-Type`. For awsJson the two differ, per
    /// the above. It is also not the content type of a payload whose type is fixed
    /// by the *shape* rather than the format: an `@eventPayload` blob is
    /// `application/octet-stream` and a string is `text/plain` under every
    /// protocol, so those stay with the code generator and must not be sourced
    /// from here.
    ///
    /// Returns `None` by default, meaning the protocol declares no such media type
    /// — callers must keep a fallback. The default keeps this addition
    /// non-breaking for third-party protocols, which the SEP requires be able to
    /// exist without modifying a code generator.
    fn event_stream_media_type(&self) -> Option<&str> {
        None
    }

    /// Extracts canonical error metadata from the payload of an event-stream
    /// `exception` frame.
    ///
    /// This is the event-stream counterpart of
    /// [`parse_error_metadata`](Self::parse_error_metadata), and it exists
    /// separately for a typing reason rather than a behavioral one: that method
    /// takes `&Self::Response`, an HTTP response, and an event-stream frame is not
    /// one. The parsing itself is identical — both read a protocol-specific error
    /// envelope out of a byte payload — so implementors should delegate to the same
    /// helper they use there.
    ///
    /// # Why this must be resolved by the protocol
    ///
    /// The frame's payload is encoded by whichever protocol is selected at runtime
    /// (via [`payload_codec`](Self::payload_codec)), so its error envelope must be
    /// parsed by that same protocol. A code generator cannot decide this: it knows
    /// only the protocol the client was generated for, and after a runtime protocol
    /// swap that is the wrong one. Getting it wrong is not silent — the parse fails
    /// and the caller reports an unhandled error — but it costs the error code, and
    /// with it the modeled error variant and any retry classification keyed on that
    /// code.
    ///
    /// # Scope
    ///
    /// Takes the payload only. An event-stream frame carries no HTTP headers, so
    /// there is nothing to pass for the header-borne discriminators some protocols
    /// also accept (restJson1's `x-amzn-errortype`, awsQuery-compatible's
    /// `x-amzn-query-error`); implementors should parse as though the header map
    /// were empty. The frame's own `:exception-type` header is a separate mechanism
    /// and is handled by generated dispatch code before this is called.
    ///
    /// The default returns an empty
    /// [`Builder`](ErrorMetadataBuilder), matching
    /// [`parse_error_metadata`](Self::parse_error_metadata): callers see
    /// `Option::None` for `code()` / `message()` and treat the frame as an
    /// unhandled error. The default keeps this addition non-breaking for
    /// third-party protocols, which the SEP requires be able to exist without
    /// modifying a code generator.
    ///
    /// Implementors of a protocol without event streams should leave this alone.
    fn parse_event_stream_error_metadata(
        &self,
        payload: &[u8],
    ) -> Result<ErrorMetadataBuilder, SerdeError> {
        let _ = payload;
        Ok(ErrorMetadata::builder())
    }
}

/// Object-safe view of [`ClientProtocolInner`] parameterized over concrete
/// request / response types.
///
/// This is what callers hold behind `dyn`, for example,
/// [`SharedClientProtocol`] stores `Arc<dyn ClientProtocol<Req, Res>>` so the
/// protocol can be swapped at runtime. The generic `Req` / `Res` parameters
/// default to HTTP so existing call sites remain source-compatible.
///
/// Every `ClientProtocolInner` gets `ClientProtocol` for free via a blanket
/// impl; implementors should write `ClientProtocolInner` only.
pub trait ClientProtocol<
    Req = aws_smithy_runtime_api::http::Request,
    Res = aws_smithy_runtime_api::http::Response,
>: Send + Sync + std::fmt::Debug
{
    /// Returns the Smithy shape ID of this protocol.
    fn protocol_id(&self) -> &ShapeId<'static>;

    /// Serializes an operation input into a request message.
    ///
    /// See [`ClientProtocolInner::serialize_request`] for the invariant an implementor must uphold:
    /// the protocol owns its own framing headers and request path, and `endpoint` is advisory.
    fn serialize_request(
        &self,
        input: &dyn SerializableStruct,
        input_schema: &Schema<'_>,
        endpoint: &str,
        cfg: &ConfigBag,
    ) -> Result<Req, SerdeError>;

    /// Deserializes a response message, returning a boxed [`ShapeDeserializer`].
    fn deserialize_response<'a>(
        &self,
        response: &'a Res,
        output_schema: &Schema<'_>,
        cfg: &'a ConfigBag,
    ) -> Result<Box<dyn ShapeDeserializer + 'a>, SerdeError>;

    /// Extracts canonical error metadata from a response.
    ///
    /// See [`ClientProtocolInner::parse_error_metadata`] for the contract.
    fn parse_error_metadata(
        &self,
        response: &Res,
        cfg: &ConfigBag,
    ) -> Result<ErrorMetadataBuilder, SerdeError>;

    /// Returns a [`ShapeDeserializer`] positioned at the body of an error
    /// response — inside the protocol's error envelope, where applicable.
    ///
    /// See [`ClientProtocolInner::deserialize_error_response`] for the contract.
    fn deserialize_error_response<'a>(
        &self,
        response: &'a Res,
        cfg: &'a ConfigBag,
    ) -> Result<Box<dyn ShapeDeserializer + 'a>, SerdeError>;

    /// Updates a previously serialized request with a resolved endpoint.
    fn update_endpoint(
        &self,
        request: &mut Req,
        endpoint: &Endpoint,
        cfg: &ConfigBag,
    ) -> Result<(), SerdeError>;

    /// Returns the codec used for payload (de)serialization, if any.
    fn payload_codec(&self) -> Option<&dyn crate::codec::DynCodec>;

    /// The media type used to label a structured event-stream payload, if any.
    ///
    /// See [`ClientProtocolInner::event_stream_media_type`] for why this is a
    /// protocol-level fact rather than a codec-level one.
    fn event_stream_media_type(&self) -> Option<&str>;

    /// Extracts canonical error metadata from an event-stream `exception` frame's
    /// payload.
    ///
    /// See [`ClientProtocolInner::parse_event_stream_error_metadata`] for the
    /// contract and for why the payload's envelope must be parsed by the protocol
    /// selected at runtime.
    fn parse_event_stream_error_metadata(
        &self,
        payload: &[u8],
    ) -> Result<ErrorMetadataBuilder, SerdeError>;
}

// Blanket impl: any `ClientProtocolInner` is automatically a `ClientProtocol`
// parameterized over its associated `Request` / `Response` types.
impl<P> ClientProtocol<P::Request, P::Response> for P
where
    P: ClientProtocolInner,
{
    fn protocol_id(&self) -> &ShapeId<'static> {
        <Self as ClientProtocolInner>::protocol_id(self)
    }

    fn serialize_request(
        &self,
        input: &dyn SerializableStruct,
        input_schema: &Schema<'_>,
        endpoint: &str,
        cfg: &ConfigBag,
    ) -> Result<P::Request, SerdeError> {
        <Self as ClientProtocolInner>::serialize_request(self, input, input_schema, endpoint, cfg)
    }

    fn deserialize_response<'a>(
        &self,
        response: &'a P::Response,
        output_schema: &Schema<'_>,
        cfg: &'a ConfigBag,
    ) -> Result<Box<dyn ShapeDeserializer + 'a>, SerdeError> {
        <Self as ClientProtocolInner>::deserialize_response(self, response, output_schema, cfg)
    }

    fn parse_error_metadata(
        &self,
        response: &P::Response,
        cfg: &ConfigBag,
    ) -> Result<ErrorMetadataBuilder, SerdeError> {
        <Self as ClientProtocolInner>::parse_error_metadata(self, response, cfg)
    }

    fn deserialize_error_response<'a>(
        &self,
        response: &'a P::Response,
        cfg: &'a ConfigBag,
    ) -> Result<Box<dyn ShapeDeserializer + 'a>, SerdeError> {
        <Self as ClientProtocolInner>::deserialize_error_response(self, response, cfg)
    }

    fn update_endpoint(
        &self,
        request: &mut P::Request,
        endpoint: &Endpoint,
        cfg: &ConfigBag,
    ) -> Result<(), SerdeError> {
        <Self as ClientProtocolInner>::update_endpoint(self, request, endpoint, cfg)
    }

    fn payload_codec(&self) -> Option<&dyn crate::codec::DynCodec> {
        <Self as ClientProtocolInner>::payload_codec(self)
    }

    fn event_stream_media_type(&self) -> Option<&str> {
        <Self as ClientProtocolInner>::event_stream_media_type(self)
    }

    fn parse_event_stream_error_metadata(
        &self,
        payload: &[u8],
    ) -> Result<ErrorMetadataBuilder, SerdeError> {
        <Self as ClientProtocolInner>::parse_event_stream_error_metadata(self, payload)
    }
}

/// Applies a resolved endpoint to an HTTP request.
///
/// This is the canonical HTTP implementation of
/// [`ClientProtocolInner::update_endpoint`]. HTTP protocols should delegate to it.
///
/// Handles endpoint prefixes (for `EndpointPrefix`-enabled operations) and
/// endpoint-supplied headers.
pub fn apply_http_endpoint(
    request: &mut aws_smithy_runtime_api::http::Request,
    endpoint: &Endpoint,
    cfg: &ConfigBag,
) -> Result<(), SerdeError> {
    use std::borrow::Cow;

    let endpoint_prefix = cfg.load::<aws_smithy_runtime_api::client::endpoint::EndpointPrefix>();
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

/// A shared, type-erased client protocol.
///
/// Wraps `Arc<dyn ClientProtocol<Req, Res>>` so a protocol can be selected at runtime. To
/// configure a client with it, convert it into a [`ConfiguredProtocol`], which is what the
/// `protocol(..)` setters accept and what the [`ConfigBag`] stores.
///
/// Defaults to HTTP transport types. Only the HTTP specialization converts into a
/// [`ConfiguredProtocol`], reflecting the fact that the orchestrator is HTTP-concrete; a custom
/// transport using `SharedClientProtocol<MyReq, MyRes>` would need its own handle type.
#[derive(Debug)]
pub struct SharedClientProtocol<
    Req = aws_smithy_runtime_api::http::Request,
    Res = aws_smithy_runtime_api::http::Response,
> {
    inner: std::sync::Arc<dyn ClientProtocol<Req, Res>>,
}

// Manual `Clone` — `Arc` is cheaply cloneable regardless of whether the inner
// `Req` / `Res` types are themselves `Clone`, so this impl avoids a spurious
// `Req: Clone, Res: Clone` bound that `#[derive(Clone)]` would introduce.
impl<Req, Res> Clone for SharedClientProtocol<Req, Res> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<Req, Res> SharedClientProtocol<Req, Res>
where
    Req: 'static,
    Res: 'static,
{
    /// Creates a new shared protocol from any [`ClientProtocol<Req, Res>`] impl.
    ///
    /// In practice callers pass a concrete type that implements
    /// [`ClientProtocolInner`] — the blanket `impl<P: ClientProtocolInner>
    /// ClientProtocol<P::Request, P::Response> for P` makes every
    /// `ClientProtocolInner` automatically usable here.
    pub fn new<P>(protocol: P) -> Self
    where
        P: ClientProtocol<Req, Res> + 'static,
    {
        Self {
            inner: std::sync::Arc::new(protocol),
        }
    }
}

impl<Req, Res> std::ops::Deref for SharedClientProtocol<Req, Res> {
    type Target = dyn ClientProtocol<Req, Res>;

    fn deref(&self) -> &Self::Target {
        &*self.inner
    }
}

/// The opaque protocol handle this crate stores in a [`ConfiguredProtocol`].
///
/// The config bag is keyed by `TypeId`, so [`SharedClientProtocol`] is deliberately not `Storable`.
/// The bag entry, and the `protocol(..)` setters on `SdkConfig`, `ConfigLoader` and generated
/// configs, use the version-stable [`ConfiguredProtocol`] owned by `aws-smithy-runtime-api`, which
/// wraps a `SchemaProtocol`. Clients downcast it with
/// [`ConfiguredProtocol::downcast_ref`] and then ask for the trait version they were generated
/// against, today [`v1`](Self::v1).
///
/// # Evolving the protocol trait within 1.x
///
/// Additive changes to [`ClientProtocol`] should use default method bodies. A change that a default
/// cannot express, such as a changed signature, can ship in a minor release as a parallel trait
/// and a private variant:
///
/// ```text
/// enum SchemaProtocolKind {
///     V1(SharedClientProtocol),
///     V2(SharedClientProtocolV2), // ClientProtocolInnerV2 / ClientProtocolV2, added in 1.y
/// }
///
/// impl SchemaProtocol {
///     pub fn v1(&self) -> Result<SharedClientProtocol, ConfigSlotError> {
///         match &self.kind {
///             SchemaProtocolKind::V1(p) => Ok(p.clone()),
///             // Or an error, if a V2 protocol cannot be expressed through the V1 trait.
///             SchemaProtocolKind::V2(p) => Ok(SharedClientProtocol::new(V2AsV1(p.clone()))),
///         }
///     }
///
///     pub fn v2(&self) -> Result<SharedClientProtocolV2, ConfigSlotError> {
///         match &self.kind {
///             SchemaProtocolKind::V1(p) => Ok(SharedClientProtocolV2::new(V1AsV2(p.clone()))),
///             SchemaProtocolKind::V2(p) => Ok(p.clone()),
///         }
///     }
/// }
/// ```
///
/// Clients generated before 1.y keep calling `v1`; later ones call `v2`. Cargo builds every crate in
/// a dependency tree against the same 1.x release, so the `v1` an old client calls is the newest
/// one, which knows how to adapt a newer protocol. That is why `v1` returns an owned value and is
/// fallible even though neither is needed while `V1` is the only private variant: both let a later
/// release return an adapter, or refuse, without changing the signature old clients were compiled
/// against.
///
/// Only the HTTP specialization of [`SharedClientProtocol`] is held, matching the orchestrator's
/// HTTP-concrete wiring; a non-HTTP transport would bring its own handle type.
#[derive(Clone, Debug)]
pub struct SchemaProtocol {
    kind: SchemaProtocolKind,
}

#[derive(Clone, Debug)]
enum SchemaProtocolKind {
    V1(SharedClientProtocol),
}

impl SchemaProtocol {
    /// Returns this protocol through version 1 of the client protocol trait, [`ClientProtocol`].
    ///
    /// This is what generated clients call. It returns an owned, cheaply cloned handle and is
    /// fallible so that a later 1.x release can adapt a protocol written against a newer trait
    /// version, or reject one it cannot adapt; see the [type-level docs](Self). The error is the
    /// runtime API's [`ConfigSlotError`], whose kinds are private, so a rejection reason can be
    /// added without changing this signature.
    pub fn v1(&self) -> Result<SharedClientProtocol, ConfigSlotError> {
        match &self.kind {
            SchemaProtocolKind::V1(protocol) => Ok(protocol.clone()),
        }
    }
}

/// Identifies this crate's compatibility line and the protocol-trait API revision.
///
/// The compatibility line comes from this crate's Cargo version, so a schema 2.x handle reports
/// `aws-smithy-schema@2`. The API revision stays `1` while [`SchemaProtocol`] contains a private
/// enum over protocol-trait versions; adding a variant does not change it because its public
/// accessors adapt between variants.
impl ConfigPayloadFor<ClientProtocolSlot> for SchemaProtocol {
    const REPRESENTATION: RepresentationId = aws_smithy_runtime_api::representation_id!(1);
}

impl ProtocolHandle for SchemaProtocol {
    fn update_endpoint(
        &self,
        request: &mut aws_smithy_runtime_api::http::Request,
        endpoint: &aws_smithy_types::endpoint::Endpoint,
        cfg: &ConfigBag,
    ) -> Result<(), BoxError> {
        match &self.kind {
            SchemaProtocolKind::V1(protocol) => {
                ClientProtocol::update_endpoint(&*protocol.inner, request, endpoint, cfg)
                    .map_err(Into::into)
            }
        }
    }
}

impl From<SharedClientProtocol> for ConfiguredProtocol {
    fn from(protocol: SharedClientProtocol) -> Self {
        ConfiguredProtocol::new(SchemaProtocol {
            kind: SchemaProtocolKind::V1(protocol),
        })
    }
}

impl SharedClientProtocol {
    /// Wraps a protocol for the version-stable `protocol(..)` setters and config-bag entry.
    ///
    /// Equivalent to `ConfiguredProtocol::from(SharedClientProtocol::new(protocol))`.
    pub fn configured(protocol: impl ClientProtocol + 'static) -> ConfiguredProtocol {
        Self::new(protocol).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serde::{SerdeError, SerializableStruct, ShapeDeserializer};
    use crate::{Schema, ShapeId};
    use aws_smithy_runtime_api::http::{Request, Response, StatusCode};
    use aws_smithy_types::body::SdkBody;
    use aws_smithy_types::config_bag::{ConfigBag, Layer};
    use aws_smithy_types::endpoint::Endpoint;

    /// Minimal protocol impl that uses the HTTP apply_http_endpoint helper.
    #[derive(Debug)]
    struct StubProtocol;

    static STUB_ID: ShapeId<'static> =
        ShapeId::from_parts("test#StubProtocol", "test", "StubProtocol");

    impl ClientProtocolInner for StubProtocol {
        type Request = Request;
        type Response = Response;

        fn protocol_id(&self) -> &ShapeId<'static> {
            &STUB_ID
        }
        fn serialize_request(
            &self,
            _input: &dyn SerializableStruct,
            _input_schema: &Schema<'_>,
            _endpoint: &str,
            _cfg: &ConfigBag,
        ) -> Result<Request, SerdeError> {
            unimplemented!()
        }
        fn deserialize_response<'a>(
            &self,
            _response: &'a Response,
            _output_schema: &Schema<'_>,
            // Deliberately without `'a`: an implementation that does not keep the bag may
            // keep the pre-`'a` signature, and this pins that it still compiles.
            _cfg: &ConfigBag,
        ) -> Result<Box<dyn ShapeDeserializer + 'a>, SerdeError> {
            unimplemented!()
        }
        fn update_endpoint(
            &self,
            request: &mut Request,
            endpoint: &Endpoint,
            cfg: &ConfigBag,
        ) -> Result<(), SerdeError> {
            apply_http_endpoint(request, endpoint, cfg)
        }
    }

    fn request_with_uri(uri: &str) -> Request {
        let mut req = Request::new(SdkBody::empty());
        req.set_uri(uri).unwrap();
        req
    }

    #[test]
    fn basic_endpoint() {
        let proto = StubProtocol;
        let mut req = request_with_uri("/original/path");
        let endpoint = Endpoint::builder()
            .url("https://service.us-east-1.amazonaws.com")
            .build();
        let cfg = ConfigBag::base();

        ClientProtocolInner::update_endpoint(&proto, &mut req, &endpoint, &cfg).unwrap();
        assert_eq!(
            req.uri(),
            "https://service.us-east-1.amazonaws.com/original/path"
        );
    }

    #[test]
    fn endpoint_with_prefix() {
        let proto = StubProtocol;
        let mut req = request_with_uri("/path");
        let endpoint = Endpoint::builder()
            .url("https://service.us-east-1.amazonaws.com")
            .build();
        let mut cfg = ConfigBag::base();
        let mut layer = Layer::new("test");
        layer.store_put(
            aws_smithy_runtime_api::client::endpoint::EndpointPrefix::new("myprefix.").unwrap(),
        );
        cfg.push_shared_layer(layer.freeze());

        ClientProtocolInner::update_endpoint(&proto, &mut req, &endpoint, &cfg).unwrap();
        assert_eq!(
            req.uri(),
            "https://myprefix.service.us-east-1.amazonaws.com/path"
        );
    }

    #[test]
    fn endpoint_with_headers() {
        let proto = StubProtocol;
        let mut req = request_with_uri("/path");
        let endpoint = Endpoint::builder()
            .url("https://example.com")
            .header("x-custom", "value1")
            .header("x-custom", "value2")
            .build();
        let cfg = ConfigBag::base();

        ClientProtocolInner::update_endpoint(&proto, &mut req, &endpoint, &cfg).unwrap();
        assert_eq!(req.uri(), "https://example.com/path");
        let values: Vec<&str> = req.headers().get_all("x-custom").collect();
        assert_eq!(values, vec!["value1", "value2"]);
    }

    #[test]
    fn endpoint_with_path() {
        let proto = StubProtocol;
        let mut req = request_with_uri("/operation");
        let endpoint = Endpoint::builder().url("https://example.com/base").build();
        let cfg = ConfigBag::base();

        ClientProtocolInner::update_endpoint(&proto, &mut req, &endpoint, &cfg).unwrap();
        assert_eq!(req.uri(), "https://example.com/base/operation");
    }

    // -- Default impls for parse_error_metadata + deserialize_error_response --

    #[test]
    fn parse_error_metadata_default_returns_empty_builder() {
        let proto = StubProtocol;
        let response = Response::new(StatusCode::try_from(500).unwrap(), SdkBody::empty());
        let cfg = ConfigBag::base();

        let builder = ClientProtocolInner::parse_error_metadata(&proto, &response, &cfg).unwrap();
        let meta = builder.build();
        assert!(meta.code().is_none());
        assert!(meta.message().is_none());
    }

    /// Records the [`Schema`] id passed to `deserialize_response` so the
    /// `deserialize_error_response` default forwarding can be asserted.
    /// Captures the FQN as a `String` so the fixture isn't tied to the
    /// schema's data lifetime.
    #[derive(Debug, Default)]
    struct RecordingProtocol {
        last_schema_id: std::sync::Mutex<Option<String>>,
    }

    static REC_ID: ShapeId<'static> =
        ShapeId::from_parts("test#RecordingProtocol", "test", "RecordingProtocol");

    impl ClientProtocolInner for RecordingProtocol {
        type Request = Request;
        type Response = Response;

        fn protocol_id(&self) -> &ShapeId<'static> {
            &REC_ID
        }
        fn serialize_request(
            &self,
            _input: &dyn SerializableStruct,
            _input_schema: &Schema<'_>,
            _endpoint: &str,
            _cfg: &ConfigBag,
        ) -> Result<Request, SerdeError> {
            unimplemented!()
        }
        fn deserialize_response<'a>(
            &self,
            _response: &'a Response,
            output_schema: &Schema<'_>,
            _cfg: &'a ConfigBag,
        ) -> Result<Box<dyn ShapeDeserializer + 'a>, SerdeError> {
            *self
                .last_schema_id
                .lock()
                .expect("RecordingProtocol mutex poisoned") =
                Some(output_schema.shape_id().as_str().to_owned());
            // Return an Err so we don't have to construct a real deserializer;
            // the test only cares which schema was forwarded.
            Err(SerdeError::custom("recording stub"))
        }
        fn update_endpoint(
            &self,
            _request: &mut Request,
            _endpoint: &Endpoint,
            _cfg: &ConfigBag,
        ) -> Result<(), SerdeError> {
            unimplemented!()
        }
    }

    #[test]
    fn deserialize_error_response_default_forwards_with_prelude_document_schema() {
        let proto = RecordingProtocol::default();
        let response = Response::new(StatusCode::try_from(500).unwrap(), SdkBody::empty());
        let cfg = ConfigBag::base();

        // The default impl forwards to deserialize_response. Our recording
        // stub captures the schema and then returns an error — we don't
        // care about the result, only the schema observed.
        let _ = ClientProtocolInner::deserialize_error_response(&proto, &response, &cfg);

        let observed = proto
            .last_schema_id
            .lock()
            .expect("RecordingProtocol mutex poisoned")
            .clone()
            .expect("schema id was captured");
        assert_eq!(observed, crate::prelude::DOCUMENT.shape_id().as_str());
    }

    // -- ConfiguredProtocol integration --

    fn bag_with(protocol: ConfiguredProtocol) -> ConfigBag {
        let mut layer = Layer::new("test");
        layer.store_put(protocol);
        ConfigBag::of_layers(vec![layer])
    }

    fn schema_protocol(cfg: &ConfigBag) -> &SchemaProtocol {
        cfg.load::<ConfiguredProtocol>()
            .expect("configured")
            .downcast_ref::<SchemaProtocol>()
            .expect("this crate's representation")
    }

    #[test]
    fn configured_protocol_round_trips_through_config_bag() {
        let cfg = bag_with(SharedClientProtocol::configured(StubProtocol));
        let protocol = schema_protocol(&cfg).v1().expect("v1");
        assert_eq!("test#StubProtocol", protocol.protocol_id().as_str());
    }

    #[test]
    fn shared_client_protocol_converts_into_a_v1_protocol() {
        let configured = ConfiguredProtocol::from(SharedClientProtocol::new(StubProtocol));
        assert!(configured
            .downcast_ref::<SchemaProtocol>()
            .expect("schema protocol")
            .v1()
            .is_ok());
    }

    #[test]
    fn v1_returns_an_owned_handle_sharing_the_protocol() {
        let cfg = bag_with(SharedClientProtocol::configured(StubProtocol));
        let schema_protocol = schema_protocol(&cfg);
        let (a, b) = (schema_protocol.v1().unwrap(), schema_protocol.v1().unwrap());
        assert!(std::sync::Arc::ptr_eq(&a.inner, &b.inner));
    }

    #[test]
    fn configured_protocol_reports_this_crate_as_representation() {
        let representation = SharedClientProtocol::configured(StubProtocol).representation();
        assert_eq!("aws-smithy-schema", representation.package());
        assert_eq!(
            env!("CARGO_PKG_VERSION_MAJOR"),
            representation.compatibility_line()
        );
        assert_eq!(1, representation.api_revision());
    }

    /// Stands in for the handle type of a future major version of this crate.
    #[derive(Debug)]
    struct FutureMajorHandle;

    impl ConfigPayloadFor<ClientProtocolSlot> for FutureMajorHandle {
        const REPRESENTATION: RepresentationId =
            RepresentationId::new("aws-smithy-schema", "99", 1);
    }

    impl ProtocolHandle for FutureMajorHandle {
        fn update_endpoint(
            &self,
            _request: &mut Request,
            _endpoint: &Endpoint,
            _cfg: &ConfigBag,
        ) -> Result<(), BoxError> {
            Ok(())
        }
    }

    #[test]
    fn protocol_from_another_major_version_is_not_this_crates_payload() {
        let configured = ConfiguredProtocol::new(FutureMajorHandle);
        assert!(configured.downcast_ref::<SchemaProtocol>().is_none());
        let err = configured.unsupported_error(&[<SchemaProtocol as ConfigPayloadFor<
            ClientProtocolSlot,
        >>::REPRESENTATION]);
        assert!(err.is_unsupported());
        let message = err.to_string();
        assert!(
            message.contains("built with aws-smithy-schema@99 (api revision 1)"),
            "{message}"
        );
    }

    /// Claims this crate's representation without being `SchemaProtocol`, as a second linked copy
    /// of this crate with the same compatibility line would.
    #[derive(Debug)]
    struct DuplicateCopyHandle;

    impl ConfigPayloadFor<ClientProtocolSlot> for DuplicateCopyHandle {
        const REPRESENTATION: RepresentationId =
            <SchemaProtocol as ConfigPayloadFor<ClientProtocolSlot>>::REPRESENTATION;
    }

    impl ProtocolHandle for DuplicateCopyHandle {
        fn update_endpoint(
            &self,
            _request: &mut Request,
            _endpoint: &Endpoint,
            _cfg: &ConfigBag,
        ) -> Result<(), BoxError> {
            Ok(())
        }
    }

    #[test]
    fn duplicate_copy_of_this_crate_is_a_representation_mismatch() {
        let configured = ConfiguredProtocol::new(DuplicateCopyHandle);
        assert!(configured.downcast_ref::<SchemaProtocol>().is_none());
        let err = configured.unsupported_error(&[<SchemaProtocol as ConfigPayloadFor<
            ClientProtocolSlot,
        >>::REPRESENTATION]);
        assert!(err.is_representation_mismatch());
        assert!(!err.is_unsupported());
    }

    /// Generated clients only accept the representations listed in codegen's
    /// `ConfiguredProtocolRegistry`. Starting a new compatibility line of this crate (or a new
    /// payload API revision) changes `REPRESENTATION`, and must come with a registry entry with an
    /// aliased dependency and adapter, or an explicit decision not to support the old line.
    #[test]
    fn representation_matches_codegen_registry() {
        assert_eq!(
            <SchemaProtocol as ConfigPayloadFor<ClientProtocolSlot>>::REPRESENTATION,
            RepresentationId::new("aws-smithy-schema", "1", 1),
            "SchemaProtocol's representation changed: update ConfiguredProtocolRegistry in \
             codegen-client (ConfiguredProtocolResolver.kt), then update this test",
        );
    }

    #[test]
    fn configured_protocol_applies_endpoints_through_the_wrapped_protocol() {
        let configured = SharedClientProtocol::configured(StubProtocol);
        let mut req = request_with_uri("/path");
        let endpoint = Endpoint::builder().url("https://example.com").build();
        configured
            .update_endpoint(&mut req, &endpoint, &ConfigBag::base())
            .unwrap();
        assert_eq!(req.uri(), "https://example.com/path");
    }
}
