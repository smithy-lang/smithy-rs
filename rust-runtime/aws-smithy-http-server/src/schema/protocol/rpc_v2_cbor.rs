/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_cbor::codec::CborCodec;
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeDeserializer};
use aws_smithy_schema::{shape_id, ShapeId};

use crate::protocol::rpc_v2_cbor::rejection::RequestRejection;
use crate::protocol::rpc_v2_cbor::runtime_error::RuntimeError;
use crate::protocol::rpc_v2_cbor::{RpcV2Cbor, RpcV2CborProtocol};
use crate::response::{IntoResponse, Response};
use crate::schema::{DeserializeError, HttpModeledError};

use super::discriminator::WithTypeFirst;
use super::response::{
    log_serialize_failure, serialize_rpc_modeled_error_response, stamp_error_extension, stamp_validation_extension,
};
use super::rpc::RpcProtocolProvider;
use super::{CompiledOperation, RpcOperationState, ServerProtocol, ServerRequest};

static PROTOCOL_ID: ShapeId<'static> = shape_id!("smithy.protocols", "rpcv2Cbor");
const CONTENT_TYPE: &str = "application/cbor";

/// The `smithy-protocol` and `Accept` request headers are validated by the router; responses
/// carry `smithy-protocol: rpc-v2-cbor`.
fn with_protocol_header(mut response: Response) -> Response {
    response.headers_mut().insert(
        http::HeaderName::from_static("smithy-protocol"),
        http::HeaderValue::from_static("rpc-v2-cbor"),
    );
    response
}

impl RpcProtocolProvider for RpcV2CborProtocol {
    type RpcCodec = CborCodec;

    fn rpc_protocol(&self) -> &super::rpc::RpcProtocol<CborCodec> {
        &self.inner
    }
}

impl ServerProtocol for RpcV2CborProtocol {
    type Codec = CborCodec;
    type OperationState = RpcOperationState;

    fn protocol_id(&self) -> &'static ShapeId<'static> {
        &PROTOCOL_ID
    }

    fn codec(&self) -> &CborCodec {
        self.inner.codec()
    }

    fn reads_request_body(&self, operation: &CompiledOperation<RpcOperationState>) -> bool {
        operation.state().reads_body()
    }

    fn deserialize_request<'a>(
        &'a self,
        operation: &'a CompiledOperation<RpcOperationState>,
        request: &'a ServerRequest,
    ) -> Result<Box<dyn ShapeDeserializer + 'a>, DeserializeError> {
        self.inner
            .deserialize_request(operation.state(), operation.schema().input(), request)
    }

    fn serialize_response(
        &self,
        operation: &CompiledOperation<RpcOperationState>,
        output: &dyn SerializableStruct,
    ) -> Response {
        self.inner
            .serialize_response(operation.schema(), output)
            .map(with_protocol_header)
            .unwrap_or_else(serialization_failure)
    }

    fn serialize_error(&self, error: &dyn HttpModeledError) -> Response {
        let schema = error.schema();
        let framed = WithTypeFirst {
            type_value: schema.shape_id().as_str(),
            inner: error,
        };
        serialize_rpc_modeled_error_response(self.codec(), schema, &framed, error.status_code(), CONTENT_TYPE)
            .map(with_protocol_header)
            .map(|response| stamp_error_extension(response, schema.shape_id().shape_name()))
            .unwrap_or_else(serialization_failure)
    }

    /// rpcv2Cbor's `From<RequestRejection>` collapses every transport failure into a 400
    /// `Serialization` (body `0xa0`, no `__type` — upstream #3716 — and no `smithy-protocol`
    /// header). A constraint violation serializes the modeled validation error — `__type` first,
    /// full shape ID — but through the response engine directly rather than
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
                let framed = WithTypeFirst {
                    type_value: schema.shape_id().as_str(),
                    inner: &*err,
                };
                serialize_rpc_modeled_error_response(self.codec(), schema, &framed, err.status_code(), CONTENT_TYPE)
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
