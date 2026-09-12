/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_runtime_api::http::Headers;
use aws_smithy_schema::codec::DynCodec;
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeDeserializer};
use aws_smithy_schema::{shape_id, Schema, ShapeId};

use crate::body::BoxBody;
use crate::protocol::rpc_v2_cbor::rejection::RequestRejection;
use crate::protocol::rpc_v2_cbor::runtime_error::RuntimeError;
use crate::protocol::rpc_v2_cbor::{RpcV2Cbor, RpcV2CborProtocol};
use crate::response::{IntoResponse, Response};
use crate::schema::{DeserializeError, HttpModeledError};

use super::response::{
    log_serialize_failure, serialize_modeled_error_response, stamp_error_extension, stamp_validation_extension,
    ResponseBindings,
};
use super::{ServerEventStreamProtocol, ServerProtocol, ServerRequest};

static PROTOCOL_ID: ShapeId<'static> = shape_id!("smithy.protocols", "rpcv2Cbor");
const CONTENT_TYPE: &str = "application/cbor";
const SMITHY_PROTOCOL_HEADER: http::HeaderName = http::HeaderName::from_static("smithy-protocol");
const SMITHY_PROTOCOL_VALUE: http::HeaderValue = http::HeaderValue::from_static("rpc-v2-cbor");

/// The `smithy-protocol` and `Accept` request headers are validated by the router; responses
/// carry `smithy-protocol: rpc-v2-cbor`.
fn with_protocol_header(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(SMITHY_PROTOCOL_HEADER, SMITHY_PROTOCOL_VALUE);
    response
}

impl ServerEventStreamProtocol for RpcV2CborProtocol {
    fn payload_codec(&self) -> &dyn DynCodec {
        self.inner.codec()
    }

    fn event_stream_media_type(&self) -> &str {
        CONTENT_TYPE
    }

    fn initial_messages_in_frames(&self) -> bool {
        true
    }
}

impl ServerProtocol for RpcV2CborProtocol {
    fn protocol_id(&self) -> &'static ShapeId<'static> {
        &PROTOCOL_ID
    }

    fn event_stream(&self) -> Option<&dyn ServerEventStreamProtocol> {
        Some(self)
    }

    fn check_accept(&self, output: &Schema<'_>, headers: &Headers) -> Result<(), DeserializeError> {
        self.inner.check_accept(output, headers)
    }

    fn reads_request_body(&self, input: &Schema<'_>) -> bool {
        self.inner.reads_request_body(input)
    }

    fn deserialize_request<'a>(
        &'a self,
        input: &Schema<'_>,
        request: &'a ServerRequest,
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
        }
    }
}

fn serialization_failure(err: SerdeError) -> Response {
    log_serialize_failure(&err);
    IntoResponse::<RpcV2Cbor>::into_response(RuntimeError::Serialization(crate::Error::new(err)))
}
