/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_json::codec::JsonCodec;
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeDeserializer};
use aws_smithy_schema::{shape_id, ShapeId};

use crate::protocol::rest_json_1::rejection::RequestRejection;
use crate::protocol::rest_json_1::runtime_error::RuntimeError;
use crate::protocol::rest_json_1::{RestJson1, RestJson1Protocol};
use crate::response::{IntoResponse, Response};
use crate::schema::{DeserializeError, HttpModeledError};

use super::response::{
    log_serialize_failure, serialize_rest_modeled_error_response, stamp_error_extension, stamp_validation_extension,
};
use super::rest::RestProtocolProvider;
use super::{CompiledOperation, RestOperationState, ServerProtocol, ServerRequest};

static PROTOCOL_ID: ShapeId<'static> = shape_id!("aws.protocols", "restJson1");
const CONTENT_TYPE: &str = "application/json";

impl RestProtocolProvider for RestJson1Protocol {
    type RestCodec = JsonCodec;

    fn rest_protocol(&self) -> &super::rest::RestProtocol<JsonCodec> {
        &self.inner
    }
}

impl ServerProtocol for RestJson1Protocol {
    type Codec = JsonCodec;
    type OperationState = RestOperationState;

    fn protocol_id(&self) -> &'static ShapeId<'static> {
        &PROTOCOL_ID
    }

    fn codec(&self) -> &JsonCodec {
        self.inner.codec()
    }

    fn reads_request_body(&self, operation: &CompiledOperation<RestOperationState>) -> bool {
        operation.state().reads_body()
    }

    fn deserialize_request<'a>(
        &'a self,
        operation: &'a CompiledOperation<RestOperationState>,
        request: &'a ServerRequest,
    ) -> Result<Box<dyn ShapeDeserializer + 'a>, DeserializeError> {
        self.inner.deserialize_request(operation.state(), request)
    }

    fn serialize_response(
        &self,
        operation: &CompiledOperation<RestOperationState>,
        output: &dyn SerializableStruct,
    ) -> Response {
        self.inner
            .serialize_response(operation.schema(), operation.state(), output)
            .unwrap_or_else(serialization_failure)
    }

    fn serialize_error(&self, error: &dyn HttpModeledError) -> Response {
        let schema = error.schema();
        let name = schema.shape_id().shape_name();
        let result =
            serialize_rest_modeled_error_response(self.codec(), schema, error, error.status_code(), CONTENT_TYPE);
        match result {
            Ok(mut response) => {
                // The discriminator travels in the header, as the shape name only.
                if let Ok(value) = http::HeaderValue::try_from(name) {
                    response
                        .headers_mut()
                        .insert(http::HeaderName::from_static("x-amzn-errortype"), value);
                }
                stamp_error_extension(response, name)
            }
            Err(err) => serialization_failure(err),
        }
    }

    /// restJson1 is the only protocol that keeps `Accept` and `Content-Type` failures distinct:
    /// 406 and 415, per its `From<RequestRejection>` mapping. Transport failures answer with the
    /// `RuntimeError` responses; a constraint violation with the modeled validation error.
    fn serialize_rejection(&self, err: DeserializeError) -> Response {
        match err {
            DeserializeError::Serde(err) => {
                IntoResponse::<RestJson1>::into_response(RuntimeError::Serialization(crate::Error::new(err)))
            }
            DeserializeError::UnsupportedMediaType(reason) => IntoResponse::<RestJson1>::into_response(
                RuntimeError::from(RequestRejection::MissingContentType(*reason)),
            ),
            DeserializeError::NotAcceptable => {
                IntoResponse::<RestJson1>::into_response(RuntimeError::from(RequestRejection::NotAcceptable))
            }
            DeserializeError::ConstraintViolation(err) => stamp_validation_extension(self.serialize_error(&*err)),
        }
    }
}

fn serialization_failure(err: SerdeError) -> Response {
    log_serialize_failure(&err);
    IntoResponse::<RestJson1>::into_response(RuntimeError::Serialization(crate::Error::new(err)))
}
