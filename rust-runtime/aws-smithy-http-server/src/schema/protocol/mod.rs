/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! [`ServerProtocol`] and its implementations on the protocol markers.
//!
//! Each protocol frames modeled errors with its discriminator:
//!
//! | Protocol   | Discriminator                                            |
//! |------------|----------------------------------------------------------|
//! | restJson1  | `x-amzn-errortype` header, shape name only; none in body |
//! | awsJson1.0 | `__type` body member, full `namespace#Name`, written last |
//! | awsJson1.1 | `__type` body member, shape name only, written last      |
//! | rpcv2Cbor  | `__type` body member, full `namespace#Name`, written first |
//! | restXml    | none                                                     |
//!
//! `@httpHeader`-bound error members are split out of the body on the REST protocols.

mod aws_json;
mod discriminator;
mod request;
pub(crate) mod response;
pub(crate) mod rest;
mod rest_json_1;
mod rest_xml;
pub(crate) mod rpc;
mod rpc_v2_cbor;
#[cfg(test)]
mod tests;

use std::any::Any;
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Duration;

use aws_smithy_runtime_api::http::{Headers, Uri};
use aws_smithy_schema::codec::{Codec, DynCodec};
use aws_smithy_schema::serde::{SerializableStruct, ShapeDeserializer};
use aws_smithy_schema::{OperationSchema, ServiceSchema, ShapeId};
use bytes::Bytes;

use crate::body::{collect_body_limited, CollectBodyError, HttpBody};
use crate::response::Response;
use crate::routing::tiny_map::TinyMap;

use super::{DeserializableShape, DeserializeError, HttpModeledError};

pub use rest::RestOperationState;
pub use rpc::RpcOperationState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RequestBodyHandling {
    Unused,
    Collected,
    Streaming,
}

pub trait OperationState: Send + Sync + 'static {
    fn request_body(&self) -> RequestBodyHandling;
}

/// The canonical, transport-independent view of a collected request on the schema path.
///
/// Transports convert into this at the edge, the way `LambdaHandler` (behind the `aws-lambda`
/// feature) converts a Lambda event into an HTTP request today: an HTTP server runs
/// [`Request::try_from`](aws_smithy_runtime_api::http::Request), collects the body, and builds one
/// of these; a future transport synthesizes the same fields from its own messages. The fields are
/// public precisely so such upgrade layers can construct it.
///
/// Event-stream operations never construct a `ServerRequest`: their body must stay streaming, so
/// they will enter through a separate streaming entry point rather than
/// [`ServerProtocol::deserialize_request`].
#[derive(Debug)]
pub struct ServerRequest {
    /// The request URI.
    pub uri: Uri,
    /// The request headers. Values are valid UTF-8 by construction.
    pub headers: Headers,
    /// The collected request body. Empty when the protocol reads no body for the operation's
    /// input; see [`collect_request_body`].
    pub body: Bytes,
}

/// A model operation paired with the state derived for one protocol instance.
#[derive(Debug)]
pub struct CompiledOperation<T> {
    schema: &'static OperationSchema<'static>,
    state: T,
}

impl<T> CompiledOperation<T> {
    /// Creates a compiled operation.
    pub const fn new(schema: &'static OperationSchema<'static>, state: T) -> Self {
        Self { schema, state }
    }

    /// Returns the protocol-neutral operation schema.
    pub fn schema(&self) -> &'static OperationSchema<'static> {
        self.schema
    }

    /// Returns the state derived by the protocol for this operation.
    pub fn state(&self) -> &T {
        &self.state
    }
}

/// Builds the protocol-specific state stored beside an operation schema.
pub trait CompileOperationState<P: ?Sized>: OperationState {
    /// Compiles state for `schema`, taking service-local protocol configuration into account.
    fn compile(protocol: &P, schema: &'static OperationSchema<'static>) -> Self;
}

impl<P: rest::RestProtocolProvider> CompileOperationState<P> for RestOperationState {
    fn compile(protocol: &P, schema: &'static OperationSchema<'static>) -> Self {
        protocol.rest_protocol().compile_operation(schema)
    }
}

impl<P: rpc::RpcProtocolProvider> CompileOperationState<P> for RpcOperationState {
    fn compile(protocol: &P, schema: &'static OperationSchema<'static>) -> Self {
        protocol.rpc_protocol().compile_operation(schema)
    }
}

/// The object-safe view of a protocol-specific compiled operation.
pub trait ErasedCompiledOperation: Send + Sync + 'static {
    /// Returns the protocol-neutral operation schema.
    fn schema(&self) -> &'static OperationSchema<'static>;

    fn request_body(&self) -> RequestBodyHandling;

    /// Returns this value for checked downcasting by the selected protocol.
    fn as_any(&self) -> &dyn Any;
}

impl<T: OperationState> ErasedCompiledOperation for CompiledOperation<T> {
    fn schema(&self) -> &'static OperationSchema<'static> {
        self.schema
    }

    fn request_body(&self) -> RequestBodyHandling {
        self.state.request_body()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

const OPERATION_TABLE_CUTOFF: usize = 8;

/// A service-local protocol instance and its compiled interpretation of every operation.
#[derive(Debug)]
pub struct ProtocolRoutingTable<P: ServerProtocol> {
    protocol: Arc<P>,
    operations: TinyMap<&'static str, Arc<CompiledOperation<P::OperationState>>, OPERATION_TABLE_CUTOFF>,
}

impl<P: ServerProtocol> ProtocolRoutingTable<P> {
    /// Compiles every operation in `service` for `protocol`.
    pub fn new(protocol: P, service: &'static ServiceSchema<'static>) -> Self {
        let operations = service
            .operations()
            .iter()
            .map(|schema| (schema.shape_id().as_str(), Arc::new(protocol.compile_operation(schema))))
            .collect();
        Self {
            protocol: Arc::new(protocol),
            operations,
        }
    }

    /// Returns the service-local protocol instance.
    pub fn protocol(&self) -> &P {
        self.protocol.as_ref()
    }

    /// Returns a shared, erased protocol together with a shared compiled operation.
    pub fn select(&self, shape_id: &ShapeId<'_>) -> Option<super::SelectedProtocolOperation> {
        self.operations
            .get(shape_id.as_str())
            .map(|operation| super::SelectedProtocolOperation {
                protocol: self.protocol.clone(),
                operation: operation.clone(),
            })
    }

    /// Returns the compiled operation identified by `shape_id`.
    pub fn operation(&self, shape_id: &ShapeId<'_>) -> Option<&CompiledOperation<P::OperationState>> {
        self.operations.get(shape_id.as_str()).map(Arc::as_ref)
    }
}

/// Schema-driven serialization for one protocol, implemented on its marker type.
///
/// This is the typed surface that protocol authors implement and that single-protocol server
/// code calls. It is not object-safe; [`DynServerProtocol`] is the erased view, derived from every
/// implementation by a blanket impl. Deserialization is synchronous over an already collected body;
/// [`collect_request_body`] decides whether a body needs collecting at all.
pub trait ServerProtocol: Send + Sync + 'static {
    /// The codec for request and response bodies.
    type Codec: Codec + Send + Sync + std::fmt::Debug + 'static;

    /// State derived once for each operation used with this protocol instance.
    type OperationState: CompileOperationState<Self>;

    /// Compiles the protocol's interpretation of an operation.
    fn compile_operation(&self, schema: &'static OperationSchema<'static>) -> CompiledOperation<Self::OperationState> {
        CompiledOperation::new(schema, Self::OperationState::compile(self, schema))
    }

    /// The protocol trait's shape ID, such as `aws.protocols#restJson1`.
    fn protocol_id(&self) -> &'static ShapeId<'static>;

    /// The codec for request and response bodies.
    fn codec(&self) -> &Self::Codec;

    /// Presents `request` as a deserializer for `operation`'s input.
    ///
    /// `Accept` and `Content-Type` checks happen here; the returned deserializer resolves `@http`
    /// bindings from the request and hands body members to [`Self::Codec`].
    fn deserialize_request<'a>(
        &'a self,
        operation: &'a CompiledOperation<Self::OperationState>,
        request: &'a ServerRequest,
    ) -> Result<Box<dyn ShapeDeserializer + 'a>, DeserializeError>;

    /// Reads `operation`'s input as `T`.
    fn deserialize<'a, T: DeserializableShape>(
        &'a self,
        operation: &'a CompiledOperation<Self::OperationState>,
        request: &'a ServerRequest,
    ) -> Result<T, DeserializeError> {
        let mut deserializer = self.deserialize_request(operation, request)?;
        T::deserialize(&mut *deserializer)
    }

    /// Serializes a successful `operation` output.
    ///
    /// The status is the `@httpResponseCode` member when bound and set, else the operation's
    /// `@http` code, else `200`. A serialization failure is logged and answered with the protocol's
    /// `RuntimeError::Serialization` response.
    fn serialize_response(
        &self,
        operation: &CompiledOperation<Self::OperationState>,
        output: &dyn SerializableStruct,
    ) -> Response;

    /// Serializes a modeled error with the protocol's discriminator framing.
    fn serialize_error(&self, error: &dyn HttpModeledError) -> Response;

    /// Converts a request-deserialization failure into the protocol's response.
    ///
    /// Each protocol answers with its `RuntimeError` responses — quirks included, such as awsJson
    /// and rpcv2Cbor collapsing `Accept` and `Content-Type` failures into a plain 400. These
    /// responses are the protocol's wire contract and must not change shape.
    fn serialize_rejection(&self, err: DeserializeError) -> Response;
}

/// The erased view of a [`ServerProtocol`], usable as `dyn DynServerProtocol`.
///
/// It has no associated types: the codec is exposed as [`DynCodec`]. Every [`ServerProtocol`]
/// implements it through the blanket impl below; protocol authors never implement it directly.
pub trait DynServerProtocol: Send + Sync + 'static {
    /// The protocol trait's shape ID.
    fn protocol_id(&self) -> &'static ShapeId<'static>;

    /// The codec for request and response bodies.
    fn codec(&self) -> &dyn DynCodec;

    /// Presents `request` as a deserializer for `operation`'s input.
    fn deserialize_request<'a>(
        &'a self,
        operation: &'a dyn ErasedCompiledOperation,
        request: &'a ServerRequest,
    ) -> Result<Box<dyn ShapeDeserializer + 'a>, DeserializeError>;

    /// Serializes a successful `operation` output.
    fn serialize_response(&self, operation: &dyn ErasedCompiledOperation, output: &dyn SerializableStruct) -> Response;

    /// Serializes a modeled error with the protocol's discriminator framing.
    fn serialize_error(&self, error: &dyn HttpModeledError) -> Response;

    /// Converts a request-deserialization failure into the protocol's response.
    fn serialize_rejection(&self, err: DeserializeError) -> Response;

    /// Whether `operation` was compiled by this protocol implementation.
    fn accepts_operation(&self, operation: &dyn ErasedCompiledOperation) -> bool;
}

impl<P: ServerProtocol> DynServerProtocol for P {
    fn protocol_id(&self) -> &'static ShapeId<'static> {
        ServerProtocol::protocol_id(self)
    }

    fn codec(&self) -> &dyn DynCodec {
        ServerProtocol::codec(self)
    }

    fn deserialize_request<'a>(
        &'a self,
        operation: &'a dyn ErasedCompiledOperation,
        request: &'a ServerRequest,
    ) -> Result<Box<dyn ShapeDeserializer + 'a>, DeserializeError> {
        ServerProtocol::deserialize_request(self, downcast_operation::<P>(operation), request)
    }

    fn serialize_response(&self, operation: &dyn ErasedCompiledOperation, output: &dyn SerializableStruct) -> Response {
        ServerProtocol::serialize_response(self, downcast_operation::<P>(operation), output)
    }

    fn serialize_error(&self, error: &dyn HttpModeledError) -> Response {
        ServerProtocol::serialize_error(self, error)
    }

    fn serialize_rejection(&self, err: DeserializeError) -> Response {
        ServerProtocol::serialize_rejection(self, err)
    }

    fn accepts_operation(&self, operation: &dyn ErasedCompiledOperation) -> bool {
        operation.as_any().is::<CompiledOperation<P::OperationState>>()
    }
}

fn downcast_operation<P: ServerProtocol>(
    operation: &dyn ErasedCompiledOperation,
) -> &CompiledOperation<P::OperationState> {
    operation
        .as_any()
        .downcast_ref()
        .expect("compiled operation must belong to the selected server protocol")
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RequestBodyCollectionConfig {
    pub max_bytes: Option<NonZeroUsize>,
    pub read_timeout: Option<Duration>,
}

#[derive(Debug, Default, Clone)]
pub struct ServiceRequestBodyConfig {
    pub global: RequestBodyCollectionConfig,
    pub per_operation: HashMap<String, RequestBodyCollectionConfig>,
}

impl ServiceRequestBodyConfig {
    pub fn for_routing(&self) -> RequestBodyCollectionConfig {
        self.global
    }

    pub fn for_operation(&self, operation: &ShapeId<'_>) -> RequestBodyCollectionConfig {
        self.per_operation
            .get(operation.as_str())
            .copied()
            .unwrap_or(self.global)
    }
}

#[derive(Debug)]
pub enum RequestBodyCollectionError<E> {
    Body(E),
    TooLarge(crate::body::BodyLimitExceeded),
    Timeout { timeout: Duration },
}

impl<E: std::fmt::Display> std::fmt::Display for RequestBodyCollectionError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Body(err) => write!(f, "error reading request body: {err}"),
            Self::TooLarge(err) => err.fmt(f),
            Self::Timeout { timeout } => write!(f, "request body read timed out after {timeout:?}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for RequestBodyCollectionError<E> {}

pub async fn collect_request_body<B>(
    body: B,
    config: &RequestBodyCollectionConfig,
) -> Result<Bytes, RequestBodyCollectionError<B::Error>>
where
    B: HttpBody,
{
    let limit = config.max_bytes.map(NonZeroUsize::get).unwrap_or(0);
    let collect = async move {
        collect_body_limited(body, limit).await.map_err(|err| match err {
            CollectBodyError::Body(err) => RequestBodyCollectionError::Body(err),
            CollectBodyError::TooLarge(err) => RequestBodyCollectionError::TooLarge(err),
        })
    };
    match config.read_timeout {
        Some(timeout) => tokio::time::timeout(timeout, collect)
            .await
            .map_err(|_| RequestBodyCollectionError::Timeout { timeout })?,
        None => collect.await,
    }
}
