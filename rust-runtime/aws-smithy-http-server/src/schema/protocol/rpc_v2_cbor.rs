/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::protocol::rpc_v2_cbor::SMITHY_PROTOCOL_HEADER;
use aws_smithy_runtime_api::http::Headers;
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeDeserializer};
use aws_smithy_schema::{shape_id, Schema, ShapeId};

use crate::body::BoxBody;
use crate::protocol::rpc_v2_cbor::rejection::RequestRejection;
use crate::protocol::rpc_v2_cbor::runtime_error::RuntimeError;
use crate::protocol::rpc_v2_cbor::RpcV2Cbor;
use crate::response::{IntoResponse, Response};
use crate::schema::{DeserializeError, HttpModeledError};

use super::response::{
    log_serialize_failure, serialize_modeled_error_response, stamp_error_extension, stamp_validation_extension,
    ResponseBindings,
};
use super::{BodyDirective, EventStreamFraming, MetadataRoutedProtocol, ServerProtocol};

/// Stateful schema-driven Smithy RPC v2 CBOR protocol implementation.
///
/// Non-POST requests return `405` by default. Set the boolean
/// `customizationConfig.protocols["smithy.protocols#rpcv2Cbor"].methodNotAllowedAsNotFound`
/// to `true` to return the Java server's unknown-operation `404` instead.
#[derive(Debug)]
pub struct RpcV2CborProtocol {
    pub(crate) inner:
        crate::schema::protocol::rpc::RpcProtocol<crate::schema::protocol::rpc_v2_cbor_serde::RpcV2CborSerde>,
    method_not_allowed_as_not_found: bool,
}

impl Default for RpcV2CborProtocol {
    fn default() -> Self {
        Self {
            inner: crate::schema::protocol::rpc::RpcProtocol::new(
                crate::schema::protocol::rpc_v2_cbor_serde::RpcV2CborSerde::default(),
                "application/cbor",
                None,
                crate::schema::protocol::rpc::RpcAccept::ModeledOutput,
                crate::schema::protocol::rpc::RpcStreaming::EventStreamContentType,
            ),
            method_not_allowed_as_not_found: false,
        }
    }
}

static PROTOCOL_ID: ShapeId<'static> = shape_id!("smithy.protocols", "rpcv2Cbor");
const CONTENT_TYPE: &str = "application/cbor";
const SMITHY_PROTOCOL_VALUE: http::HeaderValue = http::HeaderValue::from_static("rpc-v2-cbor");

/// The `smithy-protocol` and `Accept` request headers are validated by the router; responses
/// carry `smithy-protocol: rpc-v2-cbor`.
fn with_protocol_header(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(SMITHY_PROTOCOL_HEADER, SMITHY_PROTOCOL_VALUE);
    response
}

impl MetadataRoutedProtocol for RpcV2CborProtocol {
    fn from_build_context(
        ctx: &crate::schema::ProtocolBuildContext<'_>,
    ) -> Result<Self, crate::schema::routing::RouterBuildError> {
        Ok(Self {
            method_not_allowed_as_not_found: crate::schema::settings::get::<bool>(
                ctx.settings,
                "methodNotAllowedAsNotFound",
            )?
            .unwrap_or(false),
            ..Self::default()
        })
    }

    fn build_router(
        &self,
        ctx: crate::schema::routing::RouterBuildContext<'_>,
    ) -> Result<
        impl crate::schema::routing::MetadataProtocolRouter + 'static + use<>,
        crate::schema::routing::RouterBuildError,
    > {
        crate::schema::routing::rpc_v2_cbor_router(&ctx)
    }

    fn event_stream_framing(&self) -> Option<EventStreamFraming<'_>> {
        Some(EventStreamFraming::new(self.inner.codec(), CONTENT_TYPE).initial_messages_in_frames(true))
    }
}

impl ServerProtocol for RpcV2CborProtocol {
    fn protocol_id(&self) -> &'static ShapeId<'static> {
        &PROTOCOL_ID
    }

    fn validate_request_headers(
        &self,
        operation: &crate::schema::OperationSchema<'_>,
        headers: &Headers,
    ) -> Result<(), DeserializeError> {
        self.inner.check_accept(operation.output(), headers)
    }

    fn request_body_requirement(&self, operation: &crate::schema::OperationSchema<'_>) -> BodyDirective {
        if self.inner.reads_request_body(operation.input()) {
            BodyDirective::Collect
        } else {
            BodyDirective::Skip
        }
    }

    fn deserialize_request<'a>(
        &'a self,
        input: &Schema<'_>,
        request: &'a aws_smithy_runtime_api::http::Request<bytes::Bytes>,
    ) -> Result<Box<dyn ShapeDeserializer + 'a>, DeserializeError> {
        self.inner.deserialize_request(input, request)
    }

    fn serialize_response(&self, output: &Schema<'_>, value: &dyn SerializableStruct) -> Response {
        self.inner
            .serialize_response(output, value)
            .map(with_protocol_header)
            .unwrap_or_else(serialization_failure)
    }

    fn serialize_streaming_response(
        &self,
        output: &Schema<'_>,
        value: &dyn SerializableStruct,
        body: BoxBody,
    ) -> Response {
        self.inner
            .serialize_streaming_response(output, value, body)
            .map(with_protocol_header)
            .unwrap_or_else(serialization_failure)
    }

    fn serialize_error(&self, error: &dyn HttpModeledError) -> Response {
        let schema = error.schema();
        serialize_modeled_error_response(
            self.inner.codec(),
            schema,
            error,
            error.status_code(),
            ResponseBindings::BodyOnly,
            CONTENT_TYPE,
        )
        .map(with_protocol_header)
        .map(|response| stamp_error_extension(response, schema.shape_id().shape_name()))
        .unwrap_or_else(serialization_failure)
    }

    /// Method mismatches return the legacy server's bare `405` by default. The
    /// `methodNotAllowedAsNotFound` setting selects the Java server's unknown-operation
    /// `404` instead. Other routing rejections answer the way Coral's rpcv2 handler does.
    /// A framing violation —
    /// forbidden `x-amz-target`/`x-amzn-target` headers, a malformed rpcv2 path — is `400`
    /// with no `Content-Type`, the bare body `<MalformedHttpRequestException/>` and
    /// `Connection: close` (Coral tears the connection down on these). Every other kind is
    /// Coral's unknown-operation response, `404` with the CBOR `__type` body.
    fn serialize_routing_error(&self, err: &crate::schema::routing::RoutingError) -> Response {
        match err.kind() {
            crate::schema::routing::RoutingErrorKind::MethodNotAllowed if !self.method_not_allowed_as_not_found => {
                crate::routing::method_disallowed()
            }
            crate::schema::routing::RoutingErrorKind::MalformedRequest => http::Response::builder()
                .status(http::StatusCode::BAD_REQUEST)
                .header(http::header::CONNECTION, "close")
                .body(crate::body::to_boxed("<MalformedHttpRequestException/>\n"))
                .expect("a status and static body response is valid"),
            _ => self.serialize_error(&crate::schema::routing::RoutingError::unknown_operation()),
        }
    }

    /// rpcv2Cbor's `From<RequestRejection>` collapses every transport failure into a 400
    /// `Serialization` (body `0xa0`, no `__type`, upstream #3716, and no `smithy-protocol`
    /// header). A constraint violation serializes the modeled validation error, `__type` first,
    /// full shape ID, but through the response engine directly rather than
    /// [`Self::serialize_error`]: rejection responses must NOT carry the `smithy-protocol`
    /// header, which is reserved for handler-returned responses.
    fn serialize_rejection(&self, err: DeserializeError) -> Response {
        match err {
            DeserializeError::Serde(err) => {
                IntoResponse::<RpcV2Cbor>::into_response(RuntimeError::Serialization(crate::Error::new(err)))
            }
            DeserializeError::UnsupportedMediaType(reason) => IntoResponse::<RpcV2Cbor>::into_response(
                RuntimeError::from(RequestRejection::MissingContentType(*reason)),
            ),
            DeserializeError::NotAcceptable => {
                IntoResponse::<RpcV2Cbor>::into_response(RuntimeError::from(RequestRejection::NotAcceptable))
            }
            DeserializeError::ConstraintViolation(err) => {
                let schema = err.schema();
                serialize_modeled_error_response(
                    self.inner.codec(),
                    schema,
                    &*err,
                    err.status_code(),
                    ResponseBindings::BodyOnly,
                    CONTENT_TYPE,
                )
                .map(|response| stamp_error_extension(response, schema.shape_id().shape_name()))
                .map(stamp_validation_extension)
                .unwrap_or_else(serialization_failure)
            }
            DeserializeError::InternalFailure(err) => {
                IntoResponse::<RpcV2Cbor>::into_response(RuntimeError::InternalFailure(err))
            }
        }
    }
}

fn serialization_failure(err: SerdeError) -> Response {
    log_serialize_failure(&err);
    IntoResponse::<RpcV2Cbor>::into_response(RuntimeError::Serialization(crate::Error::new(err)))
}
